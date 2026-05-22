use parquet::data_type::Int32Type;
use parquet::file::metadata::KeyValue;
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;
use std::fs::File;
use std::sync::Arc;

fn main() {
    let schema = Arc::new(parse_message_type("message schema { REQUIRED INT32 id; }").unwrap());
    let props = Arc::new(
        WriterProperties::builder()
            .set_key_value_metadata(Some(vec![KeyValue {
                key: "ARROW:schema".to_string(),
                value: Some("bm90dmFsaWQ=".to_string()), // base64("notvalid") — not a valid Arrow IPC schema
            }]))
            .build(),
    );
    let file = File::create("/tmp/bad_arrow.parquet").unwrap();
    let mut writer = SerializedFileWriter::new(file, schema, props).unwrap();
    let mut row_group = writer.next_row_group().unwrap();
    {
        let mut col_writer = row_group.next_column().unwrap().unwrap();
        col_writer
            .typed::<Int32Type>()
            .write_batch(&[1i32, 2, 3], None, None)
            .unwrap();
        col_writer.close().unwrap();
    }
    row_group.close().unwrap();
    writer.close().unwrap();
    println!("wrote /tmp/bad_arrow.parquet");
}
