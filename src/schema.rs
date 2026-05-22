use anyhow::{Context, Result};
use parquet::basic::{ConvertedType, LogicalType, Repetition, TimeUnit};
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::schema::types::ColumnDescriptor;
use serde::Serialize;
use std::fs::File;
use std::io::stdout;
use std::path::Path;

pub fn format_field_type(col: &ColumnDescriptor) -> String {
    match get_logical_type_str(col) {
        Some(lt) => format!("{:?} {}", col.physical_type(), lt),
        None => format!("{:?}", col.physical_type()),
    }
}

pub fn get_logical_type_str(col: &ColumnDescriptor) -> Option<String> {
    // Try logical type first
    if let Some(lt) = col.logical_type() {
        match lt {
            LogicalType::Unknown => {} // fall through to converted type
            LogicalType::String => return Some("STRING".to_string()),
            LogicalType::Date => return Some("DATE".to_string()),
            LogicalType::Timestamp { unit, .. } => {
                let s = match unit {
                    TimeUnit::MILLIS(_) => "TIMESTAMP_MILLIS",
                    TimeUnit::MICROS(_) => "TIMESTAMP_MICROS",
                    TimeUnit::NANOS(_) => "TIMESTAMP_NANOS",
                };
                return Some(s.to_string());
            }
            LogicalType::Integer { bit_width, is_signed } => {
                return Some(format!("INT({}, {})", bit_width, is_signed));
            }
            LogicalType::Decimal { precision, scale } => {
                return Some(format!("DECIMAL(precision={}, scale={})", precision, scale));
            }
            LogicalType::List => return Some("LIST".to_string()),
            LogicalType::Map => return Some("MAP".to_string()),
            LogicalType::Enum => return Some("ENUM".to_string()),
            LogicalType::Json => return Some("JSON".to_string()),
            LogicalType::Bson => return Some("BSON".to_string()),
            LogicalType::Uuid => return Some("UUID".to_string()),
            _ => return Some(format!("{:?}", lt)),
        }
    }

    // Fall back to converted type
    let ct = col.converted_type();
    if ct != ConvertedType::NONE {
        return Some(format!("{:?}", ct));
    }

    None
}

pub fn emit_text(path: &Path) -> Result<()> {
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let reader = SerializedFileReader::new(file)?;
    let meta = reader.metadata();
    let schema_descr = meta.file_metadata().schema_descr();

    for i in 0..schema_descr.num_columns() {
        let col = schema_descr.column(i);
        match get_logical_type_str(&col) {
            Some(lt) => println!("{} {:?} {}", col.name(), col.physical_type(), lt),
            None => println!("{} {:?}", col.name(), col.physical_type()),
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct FieldJson {
    index: usize,
    name: String,
    physical_type: String,
    logical_type: Option<String>,
    repetition: String,
}

#[derive(Serialize)]
struct SchemaJson {
    file: String,
    num_rows: i64,
    num_row_groups: usize,
    created_by: Option<String>,
    fields: Vec<FieldJson>,
}

pub fn emit_json(path: &Path) -> Result<()> {
    let abs_path = std::fs::canonicalize(path)
        .with_context(|| format!("Cannot resolve path {}", path.display()))?;

    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let reader = SerializedFileReader::new(file)?;
    let meta = reader.metadata();
    let file_meta = meta.file_metadata();
    let schema_descr = file_meta.schema_descr();

    let mut fields = Vec::new();
    for i in 0..schema_descr.num_columns() {
        let col = schema_descr.column(i);
        let physical_type = format!("{:?}", col.physical_type());
        let logical_type = get_logical_type_str(&col);

        let basic_info = col.self_type().get_basic_info();
        let repetition = if basic_info.has_repetition() {
            match basic_info.repetition() {
                Repetition::REQUIRED => "REQUIRED",
                Repetition::OPTIONAL => "OPTIONAL",
                Repetition::REPEATED => "REPEATED",
            }
            .to_string()
        } else {
            "REQUIRED".to_string()
        };

        fields.push(FieldJson {
            index: i,
            name: col.name().to_string(),
            physical_type,
            logical_type,
            repetition,
        });
    }

    let schema_json = SchemaJson {
        file: abs_path.to_string_lossy().to_string(),
        num_rows: file_meta.num_rows(),
        num_row_groups: meta.num_row_groups(),
        created_by: file_meta.created_by().map(|s| s.to_string()),
        fields,
    };

    serde_json::to_writer_pretty(stdout(), &schema_json)?;
    println!();

    Ok(())
}
