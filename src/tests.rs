use polars::prelude::*;
use std::fs::File;
use std::path::PathBuf;
use tempfile::TempDir;

fn write_test_parquet(dir: &TempDir, name: &str, df: &mut DataFrame) -> PathBuf {
    let path = dir.path().join(name);
    let file = File::create(&path).unwrap();
    ParquetWriter::new(file).finish(df).unwrap();
    path
}

fn make_test_df() -> DataFrame {
    df![
        "id" => [1i64, 2, 3, 4, 5],
        "name" => ["alice", "bob", "carol", "dave", "eve"],
        "score" => [10.5f64, 20.0, 30.1, 40.2, 50.0],
    ]
    .unwrap()
}

#[test]
fn test_inspect_file_default() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    let output = capture_inspect(&path, false, false);
    assert!(output.contains("rows:"), "missing rows line: {output}");
    assert!(output.contains('5'.to_string().as_str()), "row count missing: {output}");
    assert!(output.contains("schema:"), "missing schema: {output}");
    assert!(output.contains("id"), "missing column id: {output}");
    assert!(output.contains("name"), "missing column name: {output}");
    assert!(output.contains("score"), "missing column score: {output}");
}

#[test]
fn test_csv_dump() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    let df = LazyFrame::scan_parquet(&path, ScanArgsParquet::default())
        .unwrap()
        .limit(3)
        .collect()
        .unwrap();

    let mut buf = Vec::new();
    CsvWriter::new(&mut buf).finish(&mut df.clone()).unwrap();
    let csv_text = String::from_utf8(buf).unwrap();

    let first_line = csv_text.lines().next().unwrap();
    let headers: Vec<&str> = first_line.split(',').collect();
    assert!(headers.contains(&"id"), "missing id in headers: {first_line}");
    assert!(headers.contains(&"name"), "missing name in headers: {first_line}");
    assert!(headers.contains(&"score"), "missing score in headers: {first_line}");
}

#[test]
fn test_directory_partition_discovery() {
    let dir = TempDir::new().unwrap();
    let part_a = dir.path().join("part=A");
    let part_b = dir.path().join("part=B");
    std::fs::create_dir_all(&part_a).unwrap();
    std::fs::create_dir_all(&part_b).unwrap();

    let mut df1 = df!["x" => [1i64, 2]].unwrap();
    let mut df2 = df!["x" => [3i64, 4, 5]].unwrap();

    let path1 = part_a.join("data.parquet");
    let path2 = part_b.join("data.parquet");
    ParquetWriter::new(File::create(&path1).unwrap()).finish(&mut df1).unwrap();
    ParquetWriter::new(File::create(&path2).unwrap()).finish(&mut df2).unwrap();

    let mut found_files = Vec::new();
    let mut total_rows = 0i64;

    use parquet::file::reader::{FileReader, SerializedFileReader};
    use walkdir::WalkDir;
    for entry in WalkDir::new(dir.path())
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().map(|x| x == "parquet").unwrap_or(false)
        })
    {
        let f = File::open(entry.path()).unwrap();
        let reader = SerializedFileReader::new(f).unwrap();
        let rows = reader.metadata().file_metadata().num_rows();
        total_rows += rows;
        found_files.push(entry.path().to_path_buf());
    }

    assert_eq!(found_files.len(), 2, "should find both partition files");
    assert_eq!(total_rows, 5, "total rows should be 5");
}

fn capture_inspect(path: &PathBuf, _detail: bool, quiet: bool) -> String {
    use humansize::{format_size, BINARY};
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use std::fmt::Write as FmtWrite;

    let file = File::open(path).unwrap();
    let reader = SerializedFileReader::new(file).unwrap();
    let meta = reader.metadata();
    let file_meta = meta.file_metadata();
    let file_size = std::fs::metadata(path).unwrap().len();

    let mut out = String::new();
    if !quiet {
        writeln!(out, "{}", path.display()).unwrap();
        writeln!(out, "  rows:        {}", file_meta.num_rows()).unwrap();
        writeln!(out, "  row groups:  {}", meta.num_row_groups()).unwrap();
        writeln!(out, "  file size:   {}", format_size(file_size, BINARY)).unwrap();
        writeln!(out, "  schema:").unwrap();
        let schema_descr = meta.file_metadata().schema_descr();
        for i in 0..schema_descr.num_columns() {
            let col = schema_descr.column(i);
            writeln!(out, "    [{}] {} ({:?})", i, col.name(), col.physical_type()).unwrap();
        }
    }
    out
}
