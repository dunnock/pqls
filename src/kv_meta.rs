use anyhow::{Context, Result};
use arrow_ipc::{convert::fb_to_schema, root_as_message, root_as_schema};
use base64::Engine as _;
use parquet::file::reader::{FileReader, SerializedFileReader};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::stdout;
use std::path::Path;

pub fn emit_text(path: &Path) -> Result<()> {
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let reader = SerializedFileReader::new(file)?;
    let meta = reader.metadata();
    let file_meta = meta.file_metadata();

    if let Some(kv_list) = file_meta.key_value_metadata() {
        for kv in kv_list {
            let value_str = kv.value.as_deref().unwrap_or("");
            if kv.key == "ARROW:schema" {
                // Try STANDARD first, then URL_SAFE (some Spark writers use URL-safe encoding)
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(value_str)
                    .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(value_str));
                if let Ok(bytes) = decoded {
                    // Path 1: bare Schema flatbuffer (old Arrow IPC, pyarrow < 0.17)
                    if let Ok(fbs_schema) = root_as_schema(&bytes) {
                        let schema = fb_to_schema(fbs_schema);
                        println!(
                            "{}\t(decoded Arrow schema, {} fields)",
                            kv.key,
                            schema.fields().len()
                        );
                        for field in schema.fields() {
                            let nullable = if field.is_nullable() {
                                " [nullable]"
                            } else {
                                ""
                            };
                            println!("  {}: {}{}", field.name(), field.data_type(), nullable);
                        }
                        continue;
                    }
                    // Path 2: IPC Message envelope (pyarrow, Polars, Pandas, C++ Arrow, Spark)
                    // Modern Arrow IPC prefixes the flatbuffer with a 4-byte continuation marker
                    // (0xFFFFFFFF) and a 4-byte message size; the actual message starts at byte 8.
                    let msg_bytes: &[u8] =
                        if bytes.len() > 8 && bytes.starts_with(&[0xff, 0xff, 0xff, 0xff]) {
                            &bytes[8..]
                        } else {
                            &bytes
                        };
                    if let Ok(msg) = root_as_message(msg_bytes) {
                        if let Some(ipc_schema) = msg.header_as_schema() {
                            let schema = fb_to_schema(ipc_schema);
                            println!(
                                "{}\t(decoded Arrow schema, {} fields)",
                                kv.key,
                                schema.fields().len()
                            );
                            for field in schema.fields() {
                                let nullable = if field.is_nullable() {
                                    " [nullable]"
                                } else {
                                    ""
                                };
                                println!("  {}: {}{}", field.name(), field.data_type(), nullable);
                            }
                            continue;
                        }
                    }
                    // Fallback: undecoded binary, report decoded byte count
                    println!("{}\t(binary, {} bytes)", kv.key, bytes.len());
                } else {
                    println!("{}\t(binary, {} bytes)", kv.key, value_str.len());
                }
            } else {
                println!("{}\t{}", kv.key, value_str);
            }
        }
    }

    Ok(())
}

pub fn emit_json(path: &Path) -> Result<()> {
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let reader = SerializedFileReader::new(file)?;
    let meta = reader.metadata();
    let file_meta = meta.file_metadata();

    let mut map: BTreeMap<String, Option<String>> = BTreeMap::new();

    if let Some(kv_list) = file_meta.key_value_metadata() {
        for kv in kv_list {
            map.insert(kv.key.clone(), kv.value.clone());
        }
    }

    serde_json::to_writer_pretty(stdout(), &map)?;
    println!();

    Ok(())
}
