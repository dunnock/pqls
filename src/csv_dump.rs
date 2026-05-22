use anyhow::{Context, Result};
use polars::prelude::*;
use std::io::stdout;
use std::path::Path;

pub fn dump_csv(path: &Path, head: Option<u64>, columns: Option<Vec<String>>) -> Result<()> {
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
                std::process::exit(3);
            }
        }
        let exprs: Vec<Expr> = cols.iter().map(|c| col(c.as_str())).collect();
        lf = lf.select(exprs);
    }

    if let Some(n) = head {
        if n > 0 {
            lf = lf.limit(n as u32);
        }
    }

    let mut df = lf.collect()?;

    let schema = df.schema().clone();
    let cast_exprs: Vec<Expr> = schema
        .iter()
        .filter_map(|(name, dtype)| match dtype {
            DataType::Datetime(_, _) => Some(
                col(name.as_str())
                    .cast(DataType::Datetime(TimeUnit::Microseconds, Some("UTC".into()))),
            ),
            _ => None,
        })
        .collect();
    if !cast_exprs.is_empty() {
        df = df.lazy().with_columns(cast_exprs).collect()?;
    }

    CsvWriter::new(stdout())
        .with_datetime_format(Some("%Y-%m-%dT%H:%M:%S%.fZ".to_string()))
        .finish(&mut df)
        .context("Failed to write CSV")?;

    Ok(())
}
