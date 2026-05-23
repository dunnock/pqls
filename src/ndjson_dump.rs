use anyhow::{Context, Result};
use polars::prelude::*;
use std::io::stdout;
use std::path::Path;

pub fn dump_ndjson(
    path: &Path,
    head: Option<u64>,
    sample: Option<u64>,
    columns: Option<Vec<String>>,
) -> Result<()> {
    let mut df = if let Some(n) = sample {
        let mut df = crate::sample::sample_lazy(path, n)?;
        if let Some(ref cols) = columns {
            let valid: Vec<String> =
                df.get_column_names().iter().map(|s| s.to_string()).collect();
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
            let col_strs: Vec<&str> = cols.iter().map(|s| s.as_str()).collect();
            df = df
                .select(col_strs)
                .with_context(|| format!("Failed to project columns for {}", path.display()))?;
        }
        df
    } else {
        let mut lf = LazyFrame::scan_parquet(path, ScanArgsParquet::default())
            .with_context(|| format!("Cannot scan {}", path.display()))?;

        if let Some(ref cols) = columns {
            let schema = lf.collect_schema()?;
            let valid: Vec<&str> = schema.iter_names().map(|n| n.as_str()).collect();
            for c in cols {
                if !valid.contains(&c.as_str()) {
                    eprintln!(
                        "error: unknown column: \"{}\"; valid columns: {}",
                        c,
                        valid.join(", ")
                    );
                    std::process::exit(2);
                }
            }
            let exprs: Vec<Expr> = cols.iter().map(|c| col(c.as_str())).collect();
            lf = lf.select(exprs);
        }

        if let Some(n) = head {
            lf = lf.limit(n as u32);
        }

        lf.collect()?
    };

    let schema = df.schema().clone();
    let cast_exprs: Vec<Expr> = schema
        .iter()
        .filter_map(|(name, dtype)| match dtype {
            DataType::Datetime(_, _) => Some(col(name.as_str()).dt().strftime("%Y-%m-%dT%H:%M:%S%.fZ")),
            _ => None,
        })
        .collect();
    if !cast_exprs.is_empty() {
        df = df.lazy().with_columns(cast_exprs).collect()?;
    }

    JsonWriter::new(stdout())
        .with_json_format(JsonFormat::JsonLines)
        .finish(&mut df)
        .context("Failed to write NDJSON")?;

    Ok(())
}
