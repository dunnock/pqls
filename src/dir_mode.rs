use anyhow::Result;
use humansize::{format_size, BINARY};
use parquet::file::reader::{FileReader, SerializedFileReader};
use std::fs::File;
use std::path::Path;
use walkdir::WalkDir;

pub fn list_directory(root: &Path, detail: bool, recursive: bool, quiet: bool) -> Result<()> {
    let max_depth = if recursive { usize::MAX } else { 2 };

    let entries: Vec<_> = WalkDir::new(root)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .map(|ext| ext == "parquet")
                    .unwrap_or(false)
        })
        .collect();

    if entries.is_empty() {
        if !quiet {
            println!("{}: no parquet files found", root.display());
        }
        return Ok(());
    }

    let mut total_rows: i64 = 0;
    let mut total_size: u64 = 0;

    for entry in &entries {
        let path = entry.path();
        let file_size = std::fs::metadata(path)?.len();

        let file = File::open(path)?;
        let reader = SerializedFileReader::new(file)?;
        let meta = reader.metadata();
        let num_rows = meta.file_metadata().num_rows();
        let num_rgs = meta.num_row_groups();

        total_rows += num_rows;
        total_size += file_size;

        if !quiet {
            println!(
                "{}  rows={}  row_groups={}  size={}",
                path.display(),
                num_rows,
                num_rgs,
                format_size(file_size, BINARY)
            );
        } else {
            println!("{}\t{}\t{}", path.display(), num_rows, format_size(file_size, BINARY));
        }

        if detail {
            print_column_summary(path, meta, quiet)?;
        }
    }

    if !quiet {
        println!(
            "---\ntotal: {} files, {} rows, {}",
            entries.len(),
            total_rows,
            format_size(total_size, BINARY)
        );
    } else {
        println!("total\t{}\t{}", total_rows, format_size(total_size, BINARY));
    }

    Ok(())
}

fn parse_hive_partition(file_path: &Path, root: &Path) -> String {
    let parent = match file_path.parent() {
        Some(p) => p,
        None => return "(root)".to_string(),
    };
    let relative = match parent.strip_prefix(root) {
        Ok(r) => r,
        Err(_) => return "(root)".to_string(),
    };
    let parts: Vec<&str> = relative
        .components()
        .filter_map(|c| {
            let s = c.as_os_str().to_str()?;
            if s.contains('=') { Some(s) } else { None }
        })
        .collect();
    if parts.is_empty() {
        "(root)".to_string()
    } else {
        parts.join("/")
    }
}

#[derive(Default)]
struct PartitionStat {
    rows: i64,
    files: usize,
    size_bytes: u64,
}

pub fn partition_stats(root: &Path, json_mode: bool) -> Result<()> {
    let entries: Vec<_> = WalkDir::new(root)
        .max_depth(usize::MAX)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .map(|ext| ext == "parquet")
                    .unwrap_or(false)
        })
        .collect();

    if entries.is_empty() {
        eprintln!("{}: no parquet files found", root.display());
        return Ok(());
    }

    let mut groups: std::collections::BTreeMap<String, PartitionStat> =
        std::collections::BTreeMap::new();

    for entry in &entries {
        let path = entry.path();
        let partition = parse_hive_partition(path, root);
        let file_size = std::fs::metadata(path)?.len();
        let file = File::open(path)?;
        let reader = SerializedFileReader::new(file)?;
        let num_rows = reader.metadata().file_metadata().num_rows();

        let stat = groups.entry(partition).or_default();
        stat.rows += num_rows;
        stat.files += 1;
        stat.size_bytes += file_size;
    }

    if json_mode {
        let arr: Vec<serde_json::Value> = groups
            .iter()
            .map(|(k, v)| {
                serde_json::json!({
                    "partition": k,
                    "rows": v.rows,
                    "files": v.files,
                    "size_bytes": v.size_bytes,
                })
            })
            .collect();
        println!("{}", serde_json::to_string(&arr)?);
    } else {
        let max_part_len = groups.keys().map(|k| k.len()).max().unwrap_or(9).max(9);
        println!(
            "{:<width$}  {:>12}  {:>8}  size",
            "partition",
            "rows",
            "files",
            width = max_part_len
        );
        for (k, v) in &groups {
            println!(
                "{:<width$}  {:>12}  {:>8}  {}",
                k,
                v.rows,
                v.files,
                format_size(v.size_bytes, BINARY),
                width = max_part_len
            );
        }
    }

    Ok(())
}

fn print_column_summary(
    _path: &Path,
    meta: &parquet::file::metadata::ParquetMetaData,
    quiet: bool,
) -> Result<()> {
    let schema_descr = meta.file_metadata().schema_descr();
    for i in 0..schema_descr.num_columns() {
        let col = schema_descr.column(i);
        if !quiet {
            println!("    [{}] {} ({:?})", i, col.name(), col.physical_type());
        }
    }
    Ok(())
}
