use anyhow::{Context, Result};
use polars::prelude::*;
use std::io::stdout;
use std::path::Path;

pub fn dump_csv(path: &Path, head: Option<u64>, columns: Option<Vec<String>>) -> Result<()> {
    let mut lf = LazyFrame::scan_parquet(path, ScanArgsParquet::default())
        .with_context(|| format!("Cannot scan {}", path.display()))?;

    if let Some(n) = head {
        if n > 0 {
            lf = lf.limit(n as u32);
        }
    }

    let mut df = lf.collect()?;

    if let Some(cols) = columns {
        let exprs: Vec<Expr> = cols.iter().map(|c| col(c.as_str())).collect();
        df = df.lazy().select(exprs).collect()?;
    }

    CsvWriter::new(stdout())
        .finish(&mut df)
        .context("Failed to write CSV")?;

    Ok(())
}
