use parquet::basic::Compression;
use parquet::data_type::Int64Type;
use parquet::file::properties::{EnabledStatistics, WriterProperties};
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;
use polars::prelude::*;
use std::fs::File;
use std::sync::Arc;

fn main() {
    let mut df = df![
        "id" => [1i64, 2, 3, 4, 5],
        "name" => ["alice", "bob", "carol", "dave", "eve"],
        "score" => [10.5f64, 20.0, 30.1, 40.2, 50.0],
    ]
    .unwrap();

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/smoke_test.parquet".to_string());
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
    ParquetWriter::new(File::create(format!("{dir1}/data.parquet")).unwrap())
        .finish(&mut df1)
        .unwrap();
    ParquetWriter::new(File::create(format!("{dir2}/data.parquet")).unwrap())
        .finish(&mut df2)
        .unwrap();
    println!("wrote partition dir /tmp/smoke_dir/");

    // Write a no-stats parquet for scan-stats smoke testing (simulates Arrow writer output)
    let schema = Arc::new(
        parse_message_type("message schema { REQUIRED INT64 id; REQUIRED INT64 value; }").unwrap(),
    );
    let props = Arc::new(
        WriterProperties::builder()
            .set_statistics_enabled(EnabledStatistics::None)
            .set_compression(Compression::UNCOMPRESSED)
            .build(),
    );
    let no_stats_path = "/tmp/smoke_no_stats.parquet";
    let file = File::create(no_stats_path).unwrap();
    let mut writer = SerializedFileWriter::new(file, schema, props).unwrap();
    let mut rg = writer.next_row_group().unwrap();
    {
        let mut col = rg.next_column().unwrap().unwrap();
        col.typed::<Int64Type>()
            .write_batch(&[1i64, 2, 2, 3, 3, 3], None, None)
            .unwrap();
        col.close().unwrap();
    }
    {
        let mut col = rg.next_column().unwrap().unwrap();
        col.typed::<Int64Type>()
            .write_batch(&[10i64, 20, 20, 30, 30, 30], None, None)
            .unwrap();
        col.close().unwrap();
    }
    rg.close().unwrap();
    writer.close().unwrap();
    println!("wrote {no_stats_path}");
}
