use polars::prelude::*;
use std::fs::File;

fn main() {
    let mut df = df![
        "id" => [1i64, 2, 3, 4, 5],
        "name" => ["alice", "bob", "carol", "dave", "eve"],
        "score" => [10.5f64, 20.0, 30.1, 40.2, 50.0],
    ]
    .unwrap();

    let path = std::env::args().nth(1).unwrap_or_else(|| "/tmp/smoke_test.parquet".to_string());
    let file = File::create(&path).unwrap();
    ParquetWriter::new(file).finish(&mut df).unwrap();
    println!("wrote {path}");

    // Also write partition directory
    let dir1 = "/tmp/smoke_dir/part=A";
    let dir2 = "/tmp/smoke_dir/part=B";
    std::fs::create_dir_all(dir1).unwrap();
    std::fs::create_dir_all(dir2).unwrap();

    let mut df1 = df!["x" => [1i64, 2]].unwrap();
    let mut df2 = df!["x" => [3i64, 4, 5]].unwrap();
    ParquetWriter::new(File::create(format!("{dir1}/data.parquet")).unwrap()).finish(&mut df1).unwrap();
    ParquetWriter::new(File::create(format!("{dir2}/data.parquet")).unwrap()).finish(&mut df2).unwrap();
    println!("wrote partition dir /tmp/smoke_dir/");
}
