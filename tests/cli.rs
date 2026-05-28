use std::fs::File;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

fn pqls() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pqls"))
}

fn write_parquet(dir: &TempDir, name: &str) -> PathBuf {
    use polars::prelude::*;
    let path = dir.path().join(name);
    let mut df = df!["id" => [1i64, 2, 3], "val" => [10.0f64, 20.0, 30.0]].unwrap();
    ParquetWriter::new(File::create(&path).unwrap())
        .finish(&mut df)
        .unwrap();
    path
}

#[test]
fn local_file_regression() {
    let dir = TempDir::new().unwrap();
    let path = write_parquet(&dir, "test.parquet");

    let status = pqls().arg(&path).status().expect("failed to run pqls");
    assert!(status.success(), "pqls <local_file.parquet> should exit 0");
}

#[test]
fn local_dir_recursive_regression() {
    let dir = TempDir::new().unwrap();
    write_parquet(&dir, "a.parquet");

    let status = pqls()
        .arg("-r")
        .arg(dir.path())
        .status()
        .expect("failed to run pqls");
    assert!(status.success(), "pqls -r <local_dir/> should exit 0");
}

#[test]
fn s3_invalid_bucket_exits_2() {
    // Leading hyphen makes this an invalid S3 bucket name (parse-time error)
    let output = pqls()
        .arg("s3://-invalid-bucket/key.parquet")
        .output()
        .expect("failed to run pqls");

    assert_eq!(
        output.status.code(),
        Some(2),
        "invalid S3 bucket name should exit 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid") || stderr.contains("bucket"),
        "stderr should describe the invalid bucket: {stderr}"
    );
}

#[test]
fn s3_csv_flag_not_supported_exits_2() {
    let output = pqls()
        .args(["--csv", "s3://my-bucket/prefix/"])
        .output()
        .expect("failed to run pqls");

    assert_eq!(
        output.status.code(),
        Some(2),
        "--csv with S3 path should exit 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not supported") || stderr.contains("csv"),
        "stderr should mention unsupported flag: {stderr}"
    );
}
