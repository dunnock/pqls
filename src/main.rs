mod inspect;
mod csv_dump;
mod dir_mode;
#[cfg(test)]
mod tests;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "pqls", version, about = "Inspect Apache Parquet files")]
pub struct Cli {
    /// File or directory to inspect
    pub path: PathBuf,

    /// Per-row-group stats, per-column min/max/nulls, partition layout
    #[arg(short = 'd', long)]
    pub detail: bool,

    /// Recurse into subdirectories
    #[arg(short = 'r', long)]
    pub recursive: bool,

    /// Dump file contents as CSV to stdout
    #[arg(long)]
    pub csv: bool,

    /// With --csv, output only first N rows (0 = all)
    #[arg(long, value_name = "N")]
    pub head: Option<u64>,

    /// Suppress decorative headers (machine-readable)
    #[arg(short = 'q', long)]
    pub quiet: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.csv {
        csv_dump::dump_csv(&cli.path, cli.head)?;
    } else if cli.path.is_dir() {
        dir_mode::list_directory(&cli.path, cli.detail, cli.recursive, cli.quiet)?;
    } else {
        inspect::inspect_file(&cli.path, cli.detail, cli.quiet)?;
    }

    Ok(())
}
