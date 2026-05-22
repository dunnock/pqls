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
    let mut lf = LazyFrame::scan_parquet(path, ScanArgsParquet::default())
        .with_context(|| format!("Cannot scan {}", path.display()))?;

    // Apply column projection
    if let Some(ref cols) = columns {
        let exprs: Vec<Expr> = cols.iter().map(|c| col(c.as_str())).collect();
        lf = lf.select(exprs);
    }

    // Apply head limit only when not sampling
    if let Some(n) = head {
        if sample.is_none() {
            lf = lf.limit(n as u32);
        }
    }

    let mut df = lf.collect()?;

    // Apply sampling if requested
    if let Some(n) = sample {
        let total = df.height() as u64;
        if n >= total {
            eprintln!("warning: --sample {} exceeds file row count; returning all rows", n);
        } else {
            df = df.sample_n_literal(n as usize, false, false, None)?;
        }
    }

    JsonWriter::new(stdout())
        .with_json_format(JsonFormat::JsonLines)
        .finish(&mut df)
        .context("Failed to write NDJSON")?;

    Ok(())
}
