mod check;
mod csv_dump;
mod diff;
mod dir_mode;
mod inspect;
mod kv_meta;
mod ndjson_dump;
mod sample;
mod schema;
#[cfg(test)]
mod tests;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "pqls",
    version,
    about = "Inspect Apache Parquet files",
    long_about = "Inspect Apache Parquet files\n\nExamples:\n  pqls foo.parquet                       # inspect\n  pqls --schema --json foo.parquet       # JSON schema for agents\n  pqls --ndjson --sample 100 foo.parquet # 100 random rows as NDJSON\n  pqls --csv --columns id,ts foo.parquet # project two columns to CSV\n  pqls --kv-meta foo.parquet             # key-value metadata\n  pqls -r /data/events/                  # list partitioned dataset"
)]
pub struct Cli {
    #[arg(index = 1, help = "Path to a .parquet file or directory to inspect")]
    pub path: PathBuf,

    #[arg(
        index = 2,
        help = "Second .parquet file for schema diff (required by --diff)"
    )]
    pub path_b: Option<PathBuf>,

    #[arg(long, help = "Compare schemas of two files; exits 0 if identical, 1 if different. Requires two path arguments. Use --json for machine-readable output.", conflicts_with_all = ["csv", "ndjson", "schema", "kv_meta", "partition_stats", "check", "sample", "head", "detail", "recursive", "quiet", "columns", "scan_stats", "deep"])]
    pub diff: bool,

    #[arg(
        short = 'd',
        long,
        help = "Show per-row-group column statistics (min/max/nulls). Use with --scan-stats when the file has no embedded stats."
    )]
    pub detail: bool,

    #[arg(
        short = 'r',
        long,
        help = "Recurse into a directory and list all .parquet files; required by --partition-stats"
    )]
    pub recursive: bool,

    #[arg(long, help = "Dump rows as CSV to stdout. Combine with --head N to limit output or --columns to project.", conflicts_with_all = ["ndjson", "schema", "json", "kv_meta"])]
    pub csv: bool,

    #[arg(
        long,
        value_name = "N",
        help = "Limit output to the first N rows (applies to --csv and --ndjson)"
    )]
    pub head: Option<u64>,

    #[arg(
        short = 'q',
        long,
        help = "Suppress human-readable headers; emit tab-separated summary lines suitable for scripting"
    )]
    pub quiet: bool,

    #[arg(long, help = "Print schema only (column names and types). Add --json for a machine-readable JSON schema.", conflicts_with_all = ["csv", "ndjson", "sample"])]
    pub schema: bool,

    #[arg(
        long,
        help = "Emit output as JSON (works with --schema, --kv-meta, --check, --partition-stats, --diff)",
        conflicts_with = "csv"
    )]
    pub json: bool,

    #[arg(long, help = "Stream rows as newline-delimited JSON (NDJSON). Combine with --sample or --head to limit output.", conflicts_with_all = ["csv", "schema"])]
    pub ndjson: bool,

    #[arg(long, value_name = "N", value_parser = validate_sample, help = "Emit N randomly-sampled rows; requires --ndjson or --csv", conflicts_with = "schema")]
    pub sample: Option<u64>,

    #[arg(
        long,
        value_name = "COLS",
        help = "Comma-separated list of column names to project (e.g. id,ts,value)"
    )]
    pub columns: Option<String>,

    #[arg(long = "kv-meta", help = "Print Parquet key-value metadata (writer version, custom properties). Add --json for machine-readable output.", conflicts_with_all = ["csv"])]
    pub kv_meta: bool,

    #[arg(
        long = "scan-stats",
        help = "When embedded row-group stats are absent, scan the full file to compute per-column min/max/nulls/n_distinct. Requires -d/--detail."
    )]
    pub scan_stats: bool,

    #[arg(long = "partition-stats", help = "Aggregate row counts and file sizes across a Hive-partitioned directory. Requires -r/--recursive.", conflicts_with_all = ["csv", "ndjson", "schema", "kv_meta", "check"])]
    pub partition_stats: bool,

    #[arg(long, help = "Verify file integrity by reading the footer and all row groups. Exits 0 if valid, 1 on error. Add --deep to read all data pages.", conflicts_with_all = ["csv", "ndjson", "schema", "kv_meta", "partition_stats"])]
    pub check: bool,

    #[arg(
        long,
        help = "With --check: read every data page in addition to the footer (slower but catches corrupt column data)",
        requires = "check"
    )]
    pub deep: bool,
}

fn validate_columns_for_schema(path: &std::path::Path, columns: Option<&[String]>) {
    let Some(cols) = columns else { return };
    let schema_cols = schema::column_names(path).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    for c in cols {
        if !schema_cols.iter().any(|sc| sc == c) {
            eprintln!(
                "error: unknown column: \"{}\"; valid columns: {}",
                c,
                schema_cols.join(", ")
            );
            std::process::exit(2);
        }
    }
}

fn validate_sample(s: &str) -> std::result::Result<u64, String> {
    let n: u64 = s
        .parse()
        .map_err(|_| format!("'{s}' is not a valid number"))?;
    if n == 0 {
        Err("--sample N must be > 0".to_string())
    } else {
        Ok(n)
    }
}

fn main() -> Result<()> {
    let cli = Cli::try_parse().unwrap_or_else(|e| {
        let code = match e.kind() {
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => 0,
            clap::error::ErrorKind::ArgumentConflict => 2,
            _ => 2,
        };
        e.print().unwrap();
        std::process::exit(code);
    });

    if cli.diff && cli.path_b.is_none() {
        eprintln!("error: --diff requires two path arguments: pqls --diff A.parquet B.parquet");
        std::process::exit(2);
    }

    if cli.json && !cli.schema && !cli.kv_meta && !cli.check && !cli.partition_stats && !cli.diff {
        eprintln!(
            "error: --json requires --schema, --kv-meta, --check, --partition-stats, or --diff"
        );
        std::process::exit(2);
    }

    if cli.partition_stats && !cli.recursive {
        eprintln!("error: --partition-stats requires -r");
        std::process::exit(2);
    }

    if let Some(n) = cli.sample {
        if !cli.ndjson && !cli.csv {
            eprintln!(
                "error: --sample requires --ndjson or --csv; did you mean: pqls --ndjson --sample {n} FILE"
            );
            std::process::exit(2);
        }
    }

    if cli.scan_stats && !cli.detail {
        eprintln!("error: --scan-stats requires -d / --detail");
        std::process::exit(2);
    }

    let columns: Option<Vec<String>> = cli
        .columns
        .as_ref()
        .map(|s| s.split(',').map(|c| c.trim().to_string()).collect());

    if cli.diff {
        let path_b = cli.path_b.as_ref().unwrap();
        let outcome = diff::diff_schemas(&cli.path, path_b).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(1);
        });
        let identical = matches!(outcome, diff::DiffOutcome::Identical);
        if cli.json {
            diff::emit_json(&outcome).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(1);
            });
        } else {
            diff::emit_text(&outcome);
        }
        std::process::exit(if identical { 0 } else { 1 });
    } else if cli.csv {
        csv_dump::dump_csv(&cli.path, cli.head, cli.sample, columns)?;
    } else if cli.ndjson {
        ndjson_dump::dump_ndjson(&cli.path, cli.head, cli.sample, columns)?;
    } else if cli.schema && cli.json {
        validate_columns_for_schema(&cli.path, columns.as_deref());
        schema::emit_json(&cli.path, columns.as_deref())?;
    } else if cli.schema {
        validate_columns_for_schema(&cli.path, columns.as_deref());
        schema::emit_text(&cli.path, columns.as_deref())?;
    } else if cli.kv_meta && cli.json {
        kv_meta::emit_json(&cli.path)?;
    } else if cli.kv_meta {
        kv_meta::emit_text(&cli.path)?;
    } else if cli.partition_stats {
        dir_mode::partition_stats(&cli.path, cli.json)?;
    } else if cli.check {
        match check::check_file(&cli.path, cli.deep)? {
            check::CheckOutcome::Valid => {
                if cli.json {
                    println!(
                        "{}",
                        serde_json::json!({"status":"ok","file":cli.path.display().to_string()})
                    );
                }
            }
            check::CheckOutcome::Invalid(errors) => {
                if cli.json {
                    println!("{}", serde_json::json!({"status":"error","errors":errors}));
                } else {
                    for err in &errors {
                        println!("ERROR {err}");
                    }
                }
                std::process::exit(1);
            }
        }
    } else if cli.path.is_dir() {
        dir_mode::list_directory(&cli.path, cli.detail, cli.recursive, cli.quiet)?;
    } else {
        inspect::inspect_file(&cli.path, cli.detail, cli.scan_stats, cli.quiet, columns)?;
    }

    Ok(())
}
