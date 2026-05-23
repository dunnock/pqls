use anyhow::{Context, Result};
use humansize::{format_size, BINARY};
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::file::statistics::Statistics;
use parquet::schema::types::ColumnDescriptor;
use polars::prelude::*;
use std::fs::File;
use std::path::Path;

pub fn inspect_file(
    path: &Path,
    detail: bool,
    scan_stats: bool,
    quiet: bool,
    columns: Option<Vec<String>>,
) -> Result<()> {
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let reader = SerializedFileReader::new(file)?;
    let meta = reader.metadata();
    let file_meta = meta.file_metadata();

    let file_size = std::fs::metadata(path)?.len();

    // Validate columns when they'll actually be used (detail mode only)
    if detail {
        if let Some(ref cols) = columns {
            let schema_descr = file_meta.schema_descr();
            let valid: Vec<String> = (0..schema_descr.num_columns())
                .map(|i| schema_descr.column(i).name().to_string())
                .collect();
            for c in cols {
                if !valid.contains(c) {
                    eprintln!(
                        "error: unknown column: \"{}\"; valid columns: {}",
                        c,
                        valid.join(", ")
                    );
                    std::process::exit(2);
                }
            }
        }
    }

    if !quiet {
        println!("{}", path.display());
        println!("  rows:        {}", file_meta.num_rows());
        println!("  row groups:  {}", meta.num_row_groups());
        println!("  file size:   {}", format_size(file_size, BINARY));
        println!("  schema:");
    } else {
        println!("{}\t{}\t{}", path.display(), file_meta.num_rows(), format_size(file_size, BINARY));
    }

    if !quiet {
        let schema_descr = meta.file_metadata().schema_descr();
        for i in 0..schema_descr.num_columns() {
            let col = schema_descr.column(i);
            let logical = get_logical_type_str(&col);
            let type_str = if let Some(lt) = logical {
                format!("{:?} [{}]", col.physical_type(), lt)
            } else {
                format!("{:?}", col.physical_type())
            };
            println!("    [{}] {} ({})", i, col.name(), type_str);
        }
    }

    if detail {
        print_detail(path, meta, quiet, scan_stats, columns.as_deref())?;
    }

    Ok(())
}

fn print_detail(
    path: &Path,
    meta: &parquet::file::metadata::ParquetMetaData,
    quiet: bool,
    scan_stats: bool,
    columns: Option<&[String]>,
) -> Result<()> {
    let all_no_stats = (0..meta.num_row_groups()).all(|rg_idx| {
        let rg = meta.row_group(rg_idx);
        (0..rg.num_columns()).all(|col_idx| rg.column(col_idx).statistics().is_none())
    });

    if !quiet {
        println!("  row groups:");
    }

    for rg_idx in 0..meta.num_row_groups() {
        let rg = meta.row_group(rg_idx);
        if !quiet {
            println!(
                "    row group {} — {} rows, {} compressed",
                rg_idx,
                rg.num_rows(),
                format_size(rg.compressed_size() as u64, BINARY)
            );
        }

        if !all_no_stats {
            for col_idx in 0..rg.num_columns() {
                let col_chunk = rg.column(col_idx);
                let col_name = col_chunk.column_descr().name().to_string();

                if let Some(cols) = columns {
                    if !cols.iter().any(|c| c == &col_name) {
                        continue;
                    }
                }

                let stats_str = col_chunk
                    .statistics()
                    .map(format_statistics)
                    .unwrap_or_else(|| "(no stats)".to_string());

                if !quiet {
                    println!("      {} → {}", col_name, stats_str);
                } else {
                    println!("{}\t{}\t{}", rg_idx, col_name, stats_str);
                }
            }
        }
    }

    if all_no_stats {
        if scan_stats {
            eprintln!("warning: --scan-stats reads the full file");
            eprintln!("scanning columns…");
            let t0 = std::time::Instant::now();
            let mut stats_df = compute_scan_stats(path, columns)?;
            eprintln!("scan completed in {:.1}s", t0.elapsed().as_secs_f64());
            let dt_cast: Vec<Expr> = stats_df
                .schema()
                .iter()
                .filter_map(|(name, dtype)| match dtype {
                    DataType::Datetime(_, _) => {
                        Some(col(name.as_str()).dt().strftime("%Y-%m-%dT%H:%M:%S%.fZ"))
                    }
                    _ => None,
                })
                .collect();
            if !dt_cast.is_empty() {
                stats_df = stats_df.lazy().with_columns(dt_cast).collect()?;
            }
            println!("  scan stats (file-level):");
            let schema_descr = meta.file_metadata().schema_descr();
            for i in 0..schema_descr.num_columns() {
                let col_desc = schema_descr.column(i);
                let name = col_desc.name();

                if let Some(cols) = columns {
                    if !cols.iter().any(|c| c == name) {
                        continue;
                    }
                }

                let min_str = stats_df
                    .column(&format!("{name}__min"))
                    .ok()
                    .and_then(|s| s.get(0).ok())
                    .map(|v| strip_trailing_dot_zero(anyvalue_to_string(v)))
                    .unwrap_or_else(|| "null".to_string());
                let max_str = stats_df
                    .column(&format!("{name}__max"))
                    .ok()
                    .and_then(|s| s.get(0).ok())
                    .map(|v| strip_trailing_dot_zero(anyvalue_to_string(v)))
                    .unwrap_or_else(|| "null".to_string());
                let null_count_i64 = stats_df
                    .column(&format!("{name}__null"))
                    .ok()
                    .and_then(|s| s.get(0).ok())
                    .and_then(|v| match v {
                        AnyValue::Int64(n) => Some(n),
                        _ => None,
                    })
                    .unwrap_or(0);
                let null_str = null_count_i64.to_string();
                let n_distinct_str = stats_df
                    .column(&format!("{name}__n_distinct"))
                    .ok()
                    .and_then(|s| s.get(0).ok())
                    .and_then(|v| match v {
                        AnyValue::Int64(n) => Some(n),
                        _ => None,
                    })
                    .map(|raw| raw - if null_count_i64 > 0 { 1 } else { 0 })
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".to_string());
                println!("    {} → min={} max={} nulls={} n_distinct={}",
                    name, min_str, max_str, null_str, n_distinct_str);
            }
        } else {
            println!("# note: no column statistics — file was written without stats (common with Arrow writers)");
            println!("#       run with --scan-stats to compute min/max from data (reads full file)");
        }
    }

    Ok(())
}

fn compute_scan_stats(path: &Path, columns: Option<&[String]>) -> anyhow::Result<DataFrame> {
    let mut lf = LazyFrame::scan_parquet(path, ScanArgsParquet::default())
        .with_context(|| format!("Cannot scan {}", path.display()))?;

    let schema = lf.collect_schema()?;

    let col_names: Vec<String> = schema
        .iter_names()
        .filter(|name| {
            columns.map_or(true, |cols| {
                cols.iter().any(|c| c.as_str() == name.as_str())
            })
        })
        .map(|n| n.to_string())
        .collect();

    let agg_exprs: Vec<Expr> = col_names
        .iter()
        .flat_map(|n| {
            [
                col(n.as_str()).min().alias(format!("{n}__min")),
                col(n.as_str()).max().alias(format!("{n}__max")),
                col(n.as_str())
                    .null_count()
                    .cast(DataType::Int64)
                    .alias(format!("{n}__null")),
                col(n.as_str())
                    .n_unique()
                    .cast(DataType::Int64)
                    .alias(format!("{n}__n_distinct")),
            ]
        })
        .collect();

    lf.select(agg_exprs).collect().map_err(Into::into)
}

fn get_logical_type_str(col: &ColumnDescriptor) -> Option<String> {
    use parquet::basic::{ConvertedType, LogicalType, TimeUnit};
    if let Some(lt) = col.logical_type() {
        match lt {
            LogicalType::Unknown => {}
            LogicalType::String => return Some("STRING".to_string()),
            LogicalType::Date => return Some("DATE".to_string()),
            LogicalType::Timestamp { unit, .. } => {
                let s = match unit {
                    TimeUnit::MILLIS(_) => "TIMESTAMP_MILLIS",
                    TimeUnit::MICROS(_) => "TIMESTAMP_MICROS",
                    TimeUnit::NANOS(_) => "TIMESTAMP_NANOS",
                };
                return Some(s.to_string());
            }
            LogicalType::Integer { bit_width, is_signed } => {
                return Some(format!("INT({}, {})", bit_width, is_signed));
            }
            LogicalType::Decimal { precision, scale } => {
                return Some(format!("DECIMAL(precision={}, scale={})", precision, scale));
            }
            LogicalType::List => return Some("LIST".to_string()),
            LogicalType::Map => return Some("MAP".to_string()),
            LogicalType::Enum => return Some("ENUM".to_string()),
            LogicalType::Json => return Some("JSON".to_string()),
            LogicalType::Bson => return Some("BSON".to_string()),
            LogicalType::Uuid => return Some("UUID".to_string()),
            _ => return Some(format!("{:?}", lt)),
        }
    }
    let ct = col.converted_type();
    if ct != ConvertedType::NONE {
        return Some(format!("{:?}", ct));
    }
    None
}

fn anyvalue_to_string(v: AnyValue) -> String {
    match v {
        AnyValue::String(s) => s.to_string(),
        AnyValue::StringOwned(s) => s.to_string(),
        other => other.to_string(),
    }
}

fn strip_trailing_dot_zero(s: String) -> String {
    if let Some(prefix) = s.strip_suffix(".0") {
        if prefix.parse::<i64>().is_ok() {
            return prefix.to_string();
        }
    }
    s
}

fn format_statistics(stats: &Statistics) -> String {
    match stats {
        Statistics::Int32(s) => format!(
            "min={:?} max={:?} nulls={}",
            s.min_opt(),
            s.max_opt(),
            s.null_count_opt().unwrap_or(0)
        ),
        Statistics::Int64(s) => format!(
            "min={:?} max={:?} nulls={}",
            s.min_opt(),
            s.max_opt(),
            s.null_count_opt().unwrap_or(0)
        ),
        Statistics::Float(s) => format!(
            "min={:?} max={:?} nulls={}",
            s.min_opt(),
            s.max_opt(),
            s.null_count_opt().unwrap_or(0)
        ),
        Statistics::Double(s) => format!(
            "min={:?} max={:?} nulls={}",
            s.min_opt(),
            s.max_opt(),
            s.null_count_opt().unwrap_or(0)
        ),
        Statistics::ByteArray(s) => format!(
            "min={:?} max={:?} nulls={}",
            s.min_opt().map(|v| String::from_utf8_lossy(v.data()).into_owned()),
            s.max_opt().map(|v| String::from_utf8_lossy(v.data()).into_owned()),
            s.null_count_opt().unwrap_or(0)
        ),
        Statistics::Boolean(s) => format!(
            "min={:?} max={:?} nulls={}",
            s.min_opt(),
            s.max_opt(),
            s.null_count_opt().unwrap_or(0)
        ),
        Statistics::FixedLenByteArray(s) => format!(
            "nulls={}",
            s.null_count_opt().unwrap_or(0)
        ),
        Statistics::Int96(s) => format!(
            "nulls={}",
            s.null_count_opt().unwrap_or(0)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anyvalue_to_string_no_quotes_for_strings() {
        let v = AnyValue::String("2024-01-01T00:00:00Z");
        let s = anyvalue_to_string(v);
        assert_eq!(s, "2024-01-01T00:00:00Z");
        assert!(!s.starts_with('"'), "string values must not be quoted: {s}");
    }

    #[test]
    fn anyvalue_to_string_integers_unchanged() {
        let v = AnyValue::Int64(42);
        assert_eq!(anyvalue_to_string(v), "42");
    }

    #[test]
    fn strip_trailing_dot_zero_removes_suffix() {
        assert_eq!(strip_trailing_dot_zero("42.0".into()), "42");
        assert_eq!(strip_trailing_dot_zero("3.14".into()), "3.14");
        assert_eq!(strip_trailing_dot_zero("2024-01-01T00:00:00Z".into()), "2024-01-01T00:00:00Z");
    }
}
