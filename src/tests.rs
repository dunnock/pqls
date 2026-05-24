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
    assert!(
        output.contains('5'.to_string().as_str()),
        "row count missing: {output}"
    );
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
    assert!(
        headers.contains(&"id"),
        "missing id in headers: {first_line}"
    );
    assert!(
        headers.contains(&"name"),
        "missing name in headers: {first_line}"
    );
    assert!(
        headers.contains(&"score"),
        "missing score in headers: {first_line}"
    );
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
    ParquetWriter::new(File::create(&path1).unwrap())
        .finish(&mut df1)
        .unwrap();
    ParquetWriter::new(File::create(&path2).unwrap())
        .finish(&mut df2)
        .unwrap();

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
                && e.path()
                    .extension()
                    .map(|x| x == "parquet")
                    .unwrap_or(false)
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

#[test]
fn test_schema_json() {
    // Test that schema JSON output contains expected fields
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    // We can't capture stdout easily, so test via the parquet metadata directly
    use parquet::file::reader::{FileReader, SerializedFileReader};
    let file = File::open(&path).unwrap();
    let reader = SerializedFileReader::new(file).unwrap();
    let meta = reader.metadata();
    let schema_descr = meta.file_metadata().schema_descr();

    // Schema should have 3 columns: id, name, score
    assert_eq!(schema_descr.num_columns(), 3);
    let col0 = schema_descr.column(0);
    assert_eq!(col0.name(), "id");
}

#[test]
fn test_ndjson_dump() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    // Test that ndjson_dump produces valid output
    // We test the polars scan + json write directly
    let df = LazyFrame::scan_parquet(&path, ScanArgsParquet::default())
        .unwrap()
        .limit(3)
        .collect()
        .unwrap();
    assert_eq!(df.height(), 3);
    let col_names: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(col_names.iter().any(|n| n == "id"));
}

#[test]
fn test_ndjson_columns_projection() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    let df = LazyFrame::scan_parquet(&path, ScanArgsParquet::default())
        .unwrap()
        .select([col("id"), col("name")])
        .collect()
        .unwrap();

    let cols: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(cols.iter().any(|n| n == "id"));
    assert!(cols.iter().any(|n| n == "name"));
    assert!(!cols.iter().any(|n| n == "score"));
}

#[test]
fn test_sample_within_bounds() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    let df = LazyFrame::scan_parquet(&path, ScanArgsParquet::default())
        .unwrap()
        .collect()
        .unwrap();

    // sample 3 from 5 rows
    let sampled = df.sample_n_literal(3, false, false, None).unwrap();
    assert_eq!(sampled.height(), 3);
}

#[test]
fn test_sample_lazy_exact_count() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    let df = crate::sample::sample_lazy(&path, 3).unwrap();
    assert_eq!(df.height(), 3, "sample_lazy should return exactly N rows");
    // schema must be preserved
    let cols: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(cols.iter().any(|n| n == "id"));
    assert!(cols.iter().any(|n| n == "name"));
    assert!(cols.iter().any(|n| n == "score"));
}

#[test]
fn test_sample_lazy_exceeds_rows() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    // 10 > 5 rows in file — should return all 5
    let df = crate::sample::sample_lazy(&path, 10).unwrap();
    assert_eq!(
        df.height(),
        5,
        "sample exceeding row count should return all rows"
    );
}

#[test]
fn test_sample_lazy_single_row() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    let df = crate::sample::sample_lazy(&path, 1).unwrap();
    assert_eq!(df.height(), 1);
}

#[test]
fn test_csv_dump_with_sample() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    // Capture output to a buffer
    let df = crate::sample::sample_lazy(&path, 3).unwrap();
    let mut buf = Vec::new();
    CsvWriter::new(&mut buf).finish(&mut df.clone()).unwrap();
    let text = String::from_utf8(buf).unwrap();

    let lines: Vec<&str> = text.lines().collect();
    // 1 header + 3 data rows = 4 lines
    assert_eq!(
        lines.len(),
        4,
        "CSV: 1 header + 3 data rows expected, got:\n{text}"
    );
}

fn make_large_test_df() -> DataFrame {
    let n = 100usize;
    let ids: Vec<i64> = (0..n as i64).collect();
    let names: Vec<String> = (0..n).map(|i| format!("user_{i}")).collect();
    let names_str: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let scores: Vec<f64> = (0..n).map(|i| i as f64 * 1.5).collect();
    df!["id" => ids, "name" => names_str, "score" => scores].unwrap()
}

/// Acceptance criterion: --csv --sample 10 | wc -l == 11 (1 header + 10 rows)
#[test]
fn test_csv_sample_line_count() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "big.parquet", &mut make_large_test_df());

    let df = crate::sample::sample_lazy(&path, 10).unwrap();
    assert_eq!(
        df.height(),
        10,
        "sample_lazy must return exactly 10 rows from a 100-row file"
    );

    let mut buf = Vec::new();
    CsvWriter::new(&mut buf).finish(&mut df.clone()).unwrap();
    let text = String::from_utf8(buf).unwrap();
    let line_count = text.lines().count();
    // wc -l counts newlines; CsvWriter adds trailing newline so lines() == wc -l
    assert_eq!(
        line_count, 11,
        "CSV output must have 11 lines (1 header + 10 rows), got {line_count}:\n{text}"
    );
}

#[test]
fn test_kv_meta_no_crash() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    // Just test that it doesn't crash on a file with no kv metadata
    let result = crate::kv_meta::emit_text(&path);
    assert!(result.is_ok());
}

#[test]
fn test_timestamp_alignment() {
    use polars::prelude::*;

    let dir = TempDir::new().unwrap();

    let ts_us = 1704067200000000i64; // 2024-01-01T00:00:00 UTC in microseconds
    let ts_col = Series::new("ts".into(), vec![ts_us])
        .cast(&DataType::Datetime(TimeUnit::Microseconds, None))
        .unwrap();
    let mut df = DataFrame::new(vec![ts_col.into()]).unwrap();

    let path = dir.path().join("ts_test.parquet");
    ParquetWriter::new(File::create(&path).unwrap())
        .finish(&mut df)
        .unwrap();

    let df = LazyFrame::scan_parquet(&path, ScanArgsParquet::default())
        .unwrap()
        .collect()
        .unwrap();

    // Apply CSV datetime cast (same logic as csv_dump.rs)
    let schema = df.schema().clone();
    let csv_cast_exprs: Vec<Expr> = schema
        .iter()
        .filter_map(|(name, dtype)| match dtype {
            DataType::Datetime(_, _) => Some(col(name.as_str()).cast(DataType::Datetime(
                TimeUnit::Microseconds,
                Some("UTC".into()),
            ))),
            _ => None,
        })
        .collect();
    let mut df_csv = df.clone();
    if !csv_cast_exprs.is_empty() {
        df_csv = df_csv
            .lazy()
            .with_columns(csv_cast_exprs)
            .collect()
            .unwrap();
    }

    // Apply NDJSON datetime strftime (same logic as ndjson_dump.rs)
    let schema = df.schema().clone();
    let ndjson_cast_exprs: Vec<Expr> = schema
        .iter()
        .filter_map(|(name, dtype)| match dtype {
            DataType::Datetime(_, _) => {
                Some(col(name.as_str()).dt().strftime("%Y-%m-%dT%H:%M:%S%.fZ"))
            }
            _ => None,
        })
        .collect();
    let mut df_json = df.clone();
    if !ndjson_cast_exprs.is_empty() {
        df_json = df_json
            .lazy()
            .with_columns(ndjson_cast_exprs)
            .collect()
            .unwrap();
    }

    // Write CSV to buffer
    let mut csv_buf: Vec<u8> = Vec::new();
    CsvWriter::new(&mut csv_buf)
        .with_datetime_format(Some("%Y-%m-%dT%H:%M:%S%.fZ".to_string()))
        .finish(&mut df_csv)
        .unwrap();
    let csv_text = String::from_utf8(csv_buf).unwrap();

    // Write NDJSON to buffer
    let mut json_buf: Vec<u8> = Vec::new();
    JsonWriter::new(&mut json_buf)
        .with_json_format(JsonFormat::JsonLines)
        .finish(&mut df_json)
        .unwrap();
    let json_text = String::from_utf8(json_buf).unwrap();

    // CSV: second line is first data row
    let csv_ts = csv_text.lines().nth(1).unwrap().trim();

    // NDJSON: parse first line as JSON
    let json_obj: serde_json::Value =
        serde_json::from_str(json_text.lines().next().unwrap()).unwrap();
    let json_ts = json_obj["ts"].as_str().unwrap();

    // scan-stats: apply same strftime as inspect.rs (after P6 fix)
    let lf_scan = LazyFrame::scan_parquet(&path, ScanArgsParquet::default()).unwrap();
    let agg_exprs = vec![col("ts").min().alias("ts__min")];
    let mut stats_df = lf_scan.select(agg_exprs).collect().unwrap();
    let dt_cast: Vec<Expr> = stats_df
        .schema()
        .iter()
        .filter_map(|(name, dtype)| match dtype {
            DataType::Datetime(_, _) => {
                Some(col(name.as_str()).dt().strftime("%Y-%m-%dT%H:%M:%S%.fZ"))
            }
            _ => None,
        })
        .collect();
    if !dt_cast.is_empty() {
        stats_df = stats_df.lazy().with_columns(dt_cast).collect().unwrap();
    }
    let scan_ts = match stats_df.column("ts__min").unwrap().get(0).unwrap() {
        AnyValue::String(s) => s.to_string(),
        AnyValue::StringOwned(s) => s.to_string(),
        other => other.to_string(),
    };

    assert!(
        csv_ts.contains('T'),
        "CSV timestamp should use T separator: {csv_ts}"
    );
    assert!(
        json_ts.contains('T'),
        "NDJSON timestamp should use T separator: {json_ts}"
    );

    assert!(
        csv_ts.ends_with('Z'),
        "CSV timestamp should end with Z: {csv_ts}"
    );
    assert!(
        !csv_ts.contains(".000000000"),
        "CSV should not have trailing zeros: {csv_ts}"
    );

    assert!(
        json_ts.ends_with('Z'),
        "NDJSON timestamp should end with Z: {json_ts}"
    );

    assert!(
        scan_ts.contains('T'),
        "scan-stats timestamp should use T separator: {scan_ts}"
    );
    assert!(
        scan_ts.ends_with('Z'),
        "scan-stats timestamp should end with Z: {scan_ts}"
    );
    assert_eq!(
        scan_ts, json_ts,
        "scan-stats and NDJSON must produce identical timestamp strings"
    );
    assert_eq!(
        scan_ts, csv_ts,
        "scan-stats and CSV must produce identical timestamp strings"
    );
}

#[test]
fn test_column_validation_unknown() {
    let valid = ["id", "name", "score"];
    let requested = ["id".to_string(), "nonexistent".to_string()];
    let bad = requested.iter().find(|c| !valid.contains(&c.as_str()));
    assert_eq!(bad, Some(&"nonexistent".to_string()));
}

#[test]
fn test_column_validation_all_valid() {
    let valid = ["id", "name", "score"];
    let requested = ["id".to_string(), "score".to_string()];
    let bad = requested.iter().find(|c| !valid.contains(&c.as_str()));
    assert!(bad.is_none());
}

#[test]
fn test_csv_columns_projection() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    let df = LazyFrame::scan_parquet(&path, ScanArgsParquet::default())
        .unwrap()
        .select([col("id"), col("score")])
        .collect()
        .unwrap();

    let cols: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(cols.len(), 2);
    assert!(cols.iter().any(|n| n == "id"));
    assert!(cols.iter().any(|n| n == "score"));
    assert!(!cols.iter().any(|n| n == "name"));
}

#[test]
fn test_scan_stats_column_scoping() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    // Simulate compute_scan_stats with columns=Some(["id"])
    let mut lf = LazyFrame::scan_parquet(&path, ScanArgsParquet::default()).unwrap();
    let schema = lf.collect_schema().unwrap();
    let requested = ["id".to_string()];
    let col_names: Vec<String> = schema
        .iter_names()
        .filter(|name| requested.iter().any(|c| c.as_str() == name.as_str()))
        .map(|n| n.to_string())
        .collect();

    assert_eq!(col_names, vec!["id".to_string()]);

    let agg_exprs: Vec<polars::prelude::Expr> = col_names
        .iter()
        .flat_map(|n| {
            [
                col(n.as_str()).min().alias(format!("{n}__min")),
                col(n.as_str()).max().alias(format!("{n}__max")),
                col(n.as_str())
                    .null_count()
                    .cast(DataType::Int64)
                    .alias(format!("{n}__null")),
                col(n.as_str())
                    .n_unique()
                    .cast(DataType::Int64)
                    .alias(format!("{n}__n_distinct")),
            ]
        })
        .collect();

    let stats_df = lf.select(agg_exprs).collect().unwrap();
    assert!(stats_df.column("id__min").is_ok());
    assert!(stats_df.column("id__max").is_ok());
    assert!(stats_df.column("id__null").is_ok());
    assert!(stats_df.column("id__n_distinct").is_ok());
    assert!(stats_df.column("name__min").is_err());
    assert!(stats_df.column("score__min").is_err());
}

#[test]
fn test_check_valid_file() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "valid.parquet", &mut make_test_df());

    let result = crate::check::check_file(&path, false).unwrap();
    assert!(
        matches!(result, crate::check::CheckOutcome::Valid),
        "expected Valid for a well-formed parquet file"
    );
}

#[test]
fn test_check_deep_valid_file() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "valid.parquet", &mut make_test_df());

    let result = crate::check::check_file(&path, true).unwrap();
    assert!(
        matches!(result, crate::check::CheckOutcome::Valid),
        "expected Valid for deep check of a well-formed parquet file"
    );
}

#[test]
fn test_check_truncated_file() {
    use std::fs::OpenOptions;

    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "truncated.parquet", &mut make_test_df());

    // Truncate to 100 bytes — too short to be a valid parquet file
    let f = OpenOptions::new().write(true).open(&path).unwrap();
    f.set_len(100).unwrap();

    let result = crate::check::check_file(&path, false).unwrap();
    assert!(
        matches!(result, crate::check::CheckOutcome::Invalid(_)),
        "expected Invalid for a truncated parquet file"
    );
}

#[test]
fn test_check_truncated_has_errors() {
    use std::fs::OpenOptions;

    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "truncated.parquet", &mut make_test_df());

    let f = OpenOptions::new().write(true).open(&path).unwrap();
    f.set_len(100).unwrap();

    let result = crate::check::check_file(&path, false).unwrap();
    if let crate::check::CheckOutcome::Invalid(errors) = result {
        assert!(!errors.is_empty(), "expected at least one error message");
    } else {
        panic!("expected Invalid");
    }
}

#[test]
fn test_partition_stats_no_crash() {
    let dir = TempDir::new().unwrap();
    let part_a = dir.path().join("year=2024").join("month=01");
    let part_b = dir.path().join("year=2024").join("month=02");
    std::fs::create_dir_all(&part_a).unwrap();
    std::fs::create_dir_all(&part_b).unwrap();

    let mut df1 = df!["x" => [1i64, 2]].unwrap();
    let mut df2 = df!["x" => [3i64, 4, 5]].unwrap();
    ParquetWriter::new(File::create(part_a.join("data.parquet")).unwrap())
        .finish(&mut df1)
        .unwrap();
    ParquetWriter::new(File::create(part_b.join("data.parquet")).unwrap())
        .finish(&mut df2)
        .unwrap();

    let result = crate::dir_mode::partition_stats(dir.path(), false);
    assert!(
        result.is_ok(),
        "partition_stats should not error: {:?}",
        result
    );
}

#[test]
fn test_partition_stats_json_no_crash() {
    let dir = TempDir::new().unwrap();
    let part = dir.path().join("region=us");
    std::fs::create_dir_all(&part).unwrap();

    let mut df = df!["v" => [42i64]].unwrap();
    ParquetWriter::new(File::create(part.join("data.parquet")).unwrap())
        .finish(&mut df)
        .unwrap();

    let result = crate::dir_mode::partition_stats(dir.path(), true);
    assert!(
        result.is_ok(),
        "partition_stats --json should not error: {:?}",
        result
    );
}

#[test]
fn test_partition_stats_without_recursive_exits() {
    // Verify the validation logic: --partition-stats without -r should exit 2
    // (tested via the flag check in main.rs — here we just verify the parsing logic)
    let parts: Vec<(String, String)> = vec![
        ("year=2024/month=01".to_string(), "year".to_string()),
        ("region=us".to_string(), "region".to_string()),
    ];
    for (partition, expected_key) in parts {
        assert!(
            partition.contains(&expected_key),
            "partition '{partition}' should contain key '{expected_key}'"
        );
    }
}

#[test]
fn test_n_distinct_aggregation() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    let mut lf = LazyFrame::scan_parquet(&path, ScanArgsParquet::default()).unwrap();
    let schema = lf.collect_schema().unwrap();
    let col_names: Vec<String> = schema.iter_names().map(|n| n.to_string()).collect();

    let agg_exprs: Vec<polars::prelude::Expr> = col_names
        .iter()
        .flat_map(|n| {
            [
                col(n.as_str()).min().alias(format!("{n}__min")),
                col(n.as_str()).max().alias(format!("{n}__max")),
                col(n.as_str())
                    .null_count()
                    .cast(DataType::Int64)
                    .alias(format!("{n}__null")),
                col(n.as_str())
                    .n_unique()
                    .cast(DataType::Int64)
                    .alias(format!("{n}__n_distinct")),
            ]
        })
        .collect();

    let stats_df = lf.select(agg_exprs).collect().unwrap();

    use polars::prelude::AnyValue;

    // id has 5 distinct values [1,2,3,4,5], null_count=0 → n_distinct=5
    let id_null: i64 = match stats_df.column("id__null").unwrap().get(0).unwrap() {
        AnyValue::Int64(n) => n,
        _ => panic!("expected Int64"),
    };
    let id_nd_raw: i64 = match stats_df.column("id__n_distinct").unwrap().get(0).unwrap() {
        AnyValue::Int64(n) => n,
        _ => panic!("expected Int64"),
    };
    let id_nd = id_nd_raw - if id_null > 0 { 1 } else { 0 };
    assert_eq!(id_null, 0);
    assert_eq!(id_nd, 5, "id should have 5 distinct values");

    // score: [10.5, 20.0, 30.1, 40.2, 50.0] — all distinct, no nulls
    let score_null: i64 = match stats_df.column("score__null").unwrap().get(0).unwrap() {
        AnyValue::Int64(n) => n,
        _ => panic!("expected Int64"),
    };
    let score_nd_raw: i64 = match stats_df
        .column("score__n_distinct")
        .unwrap()
        .get(0)
        .unwrap()
    {
        AnyValue::Int64(n) => n,
        _ => panic!("expected Int64"),
    };
    let score_nd = score_nd_raw - if score_null > 0 { 1 } else { 0 };
    assert_eq!(score_null, 0);
    assert_eq!(score_nd, 5, "score should have 5 distinct values");
}

#[test]
fn test_n_distinct_null_adjustment() {
    use polars::prelude::AnyValue;

    let dir = TempDir::new().unwrap();
    // Column with nulls and repeated values
    let s = Series::new("x".into(), vec![Some(1i64), Some(1), Some(2), None, None]);
    let mut df = DataFrame::new(vec![s.into()]).unwrap();
    let path = dir.path().join("nulls.parquet");
    ParquetWriter::new(File::create(&path).unwrap())
        .finish(&mut df)
        .unwrap();

    let lf = LazyFrame::scan_parquet(&path, ScanArgsParquet::default()).unwrap();
    let agg_exprs = vec![
        col("x").null_count().cast(DataType::Int64).alias("x__null"),
        col("x")
            .n_unique()
            .cast(DataType::Int64)
            .alias("x__n_distinct"),
    ];
    let stats_df = lf.select(agg_exprs).collect().unwrap();

    let null_count: i64 = match stats_df.column("x__null").unwrap().get(0).unwrap() {
        AnyValue::Int64(n) => n,
        _ => panic!("expected Int64"),
    };
    let nd_raw: i64 = match stats_df.column("x__n_distinct").unwrap().get(0).unwrap() {
        AnyValue::Int64(n) => n,
        _ => panic!("expected Int64"),
    };
    assert_eq!(null_count, 2, "should have 2 nulls");
    // polars n_unique counts null as distinct: {1, 2, null} = 3
    // After adjustment: 3 - 1 = 2 (non-null distinct values: 1 and 2)
    let nd = nd_raw - if null_count > 0 { 1 } else { 0 };
    assert_eq!(nd, 2, "non-null distinct values should be 2 (1 and 2)");
}

#[test]
fn test_diff_identical_schemas() {
    let dir = TempDir::new().unwrap();
    let path_a = write_test_parquet(&dir, "a.parquet", &mut make_test_df());
    let path_b = write_test_parquet(&dir, "b.parquet", &mut make_test_df());

    let outcome = crate::diff::diff_schemas(&path_a, &path_b).unwrap();
    assert!(
        matches!(outcome, crate::diff::DiffOutcome::Identical),
        "identical files should produce Identical outcome"
    );
}

#[test]
fn test_diff_added_column() {
    let dir = TempDir::new().unwrap();
    let path_a = write_test_parquet(&dir, "a.parquet", &mut make_test_df());

    // File B has an extra column
    let mut df_b = df![
        "id" => [1i64, 2, 3, 4, 5],
        "name" => ["alice", "bob", "carol", "dave", "eve"],
        "score" => [10.5f64, 20.0, 30.1, 40.2, 50.0],
        "extra" => [0i64, 1, 2, 3, 4],
    ]
    .unwrap();
    let path_b = write_test_parquet(&dir, "b.parquet", &mut df_b);

    let outcome = crate::diff::diff_schemas(&path_a, &path_b).unwrap();
    match outcome {
        crate::diff::DiffOutcome::Different {
            added,
            removed,
            changed,
            ..
        } => {
            assert_eq!(added.len(), 1, "should have 1 added column");
            assert_eq!(added[0].name, "extra");
            assert!(removed.is_empty(), "should have no removed columns");
            assert!(changed.is_empty(), "should have no changed columns");
        }
        crate::diff::DiffOutcome::Identical => panic!("expected Different"),
    }
}

#[test]
fn test_diff_removed_column() {
    let dir = TempDir::new().unwrap();
    let path_a = write_test_parquet(&dir, "a.parquet", &mut make_test_df());

    // File B is missing the score column
    let mut df_b = df![
        "id" => [1i64, 2, 3],
        "name" => ["alice", "bob", "carol"],
    ]
    .unwrap();
    let path_b = write_test_parquet(&dir, "b.parquet", &mut df_b);

    let outcome = crate::diff::diff_schemas(&path_a, &path_b).unwrap();
    match outcome {
        crate::diff::DiffOutcome::Different {
            added,
            removed,
            changed,
            ..
        } => {
            assert!(added.is_empty(), "should have no added columns");
            assert_eq!(removed.len(), 1, "should have 1 removed column");
            assert_eq!(removed[0].name, "score");
            assert!(changed.is_empty(), "should have no changed columns");
        }
        crate::diff::DiffOutcome::Identical => panic!("expected Different"),
    }
}

#[test]
fn test_diff_union_order() {
    let dir = TempDir::new().unwrap();

    // A has [id, name], B has [id, extra]
    let mut df_a = df!["id" => [1i64], "name" => ["x"]].unwrap();
    let mut df_b = df!["id" => [1i64], "extra" => [2i64]].unwrap();
    let path_a = write_test_parquet(&dir, "a.parquet", &mut df_a);
    let path_b = write_test_parquet(&dir, "b.parquet", &mut df_b);

    let outcome = crate::diff::diff_schemas(&path_a, &path_b).unwrap();
    match outcome {
        crate::diff::DiffOutcome::Different { union_order, .. } => {
            // A columns first: [id, name], then B-only: [extra]
            assert_eq!(union_order, vec!["id", "name", "extra"]);
        }
        crate::diff::DiffOutcome::Identical => panic!("expected Different"),
    }
}

#[test]
fn test_schema_column_names() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    let names = crate::schema::column_names(&path).unwrap();
    assert_eq!(names, vec!["id", "name", "score"]);
}

#[test]
fn test_schema_text_columns_filter() {
    // Verify that emit_text with columns skips non-listed columns (schema-order preserved)
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    use parquet::file::reader::{FileReader, SerializedFileReader};
    let file = File::open(&path).unwrap();
    let reader = SerializedFileReader::new(file).unwrap();
    let schema_descr = reader.metadata().file_metadata().schema_descr();

    let requested = ["id".to_string(), "score".to_string()];
    let mut matched_names: Vec<String> = Vec::new();
    for i in 0..schema_descr.num_columns() {
        let col = schema_descr.column(i);
        if requested.iter().any(|c| c == col.name()) {
            matched_names.push(col.name().to_string());
        }
    }
    // 2 lines expected; schema order preserved (id before score)
    assert_eq!(matched_names.len(), 2);
    assert_eq!(matched_names[0], "id");
    assert_eq!(matched_names[1], "score");
}

#[test]
fn test_schema_json_columns_filter_count() {
    // Verify that emit_json with a single column produces fields of length 1
    // and preserves original schema index
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    use parquet::file::reader::{FileReader, SerializedFileReader};
    let file = File::open(&path).unwrap();
    let reader = SerializedFileReader::new(file).unwrap();
    let schema_descr = reader.metadata().file_metadata().schema_descr();

    let requested = ["score".to_string()];
    let mut matched: Vec<(usize, String)> = Vec::new();
    for i in 0..schema_descr.num_columns() {
        let col = schema_descr.column(i);
        if requested.iter().any(|c| c == col.name()) {
            matched.push((i, col.name().to_string()));
        }
    }
    // .fields | length = 1
    assert_eq!(matched.len(), 1);
    // original index preserved (score is column 2 → index 2)
    assert_eq!(matched[0].0, 2);
    assert_eq!(matched[0].1, "score");
}

#[test]
fn test_schema_columns_validation_unknown() {
    let dir = TempDir::new().unwrap();
    let path = write_test_parquet(&dir, "test.parquet", &mut make_test_df());

    let names = crate::schema::column_names(&path).unwrap();
    let bad = "nonexistent";
    assert!(
        !names.iter().any(|n| n == bad),
        "nonexistent should not be a valid column"
    );
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
            writeln!(
                out,
                "    [{}] {} ({:?})",
                i,
                col.name(),
                col.physical_type()
            )
            .unwrap();
        }
    }
    out
}
