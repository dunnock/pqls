use anyhow::{Context, Result};
use arrow_ipc::{convert::fb_to_schema, root_as_schema};
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
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(value_str) {
                    if let Ok(ipc_schema) = root_as_schema(&bytes) {
                        let schema = fb_to_schema(ipc_schema);
                        println!(
                            "{}\t(decoded Arrow schema, {} fields)",
                            kv.key,
                            schema.fields().len()
                        );
                        for field in schema.fields() {
                            let nullable =
                                if field.is_nullable() { " [nullable]" } else { "" };
                            println!("  {}: {}{}", field.name(), field.data_type(), nullable);
                        }
                        continue;
                    }
                }
                println!("{}\t(binary, {} bytes, decode failed)", kv.key, value_str.len());
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
