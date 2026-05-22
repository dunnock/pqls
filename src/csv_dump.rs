use anyhow::{Context, Result};
use polars::prelude::*;
use std::io::stdout;
use std::path::Path;

pub fn dump_csv(path: &Path, head: Option<u64>) -> Result<()> {
    let mut df = LazyFrame::scan_parquet(path, ScanArgsParquet::default())
        .with_context(|| format!("Cannot scan {}", path.display()))?;

    if let Some(n) = head {
        if n > 0 {
            df = df.limit(n as u32);
        }
    }

    let mut collected = df.collect()?;
    CsvWriter::new(stdout())
        .finish(&mut collected)
        .context("Failed to write CSV")?;

    Ok(())
}
