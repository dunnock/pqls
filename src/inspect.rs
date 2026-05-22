use anyhow::{Context, Result};
use humansize::{format_size, BINARY};
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::file::statistics::Statistics;
use std::fs::File;
use std::path::Path;

pub fn inspect_file(path: &Path, detail: bool, quiet: bool) -> Result<()> {
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let reader = SerializedFileReader::new(file)?;
    let meta = reader.metadata();
    let file_meta = meta.file_metadata();

    let file_size = std::fs::metadata(path)?.len();

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
            println!("    [{}] {} ({:?})", i, col.name(), col.physical_type());
        }
    }

    if detail {
        print_detail(path, meta, quiet)?;
    }

    Ok(())
}

fn print_detail(
    _path: &Path,
    meta: &parquet::file::metadata::ParquetMetaData,
    quiet: bool,
) -> Result<()> {
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

        for col_idx in 0..rg.num_columns() {
            let col_chunk = rg.column(col_idx);
            let col_name = col_chunk.column_descr().name().to_string();

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

    Ok(())
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
