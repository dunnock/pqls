use anyhow::{Context, Result};
use parquet::file::reader::{FileReader, SerializedFileReader};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::stdout;
use std::path::Path;

use crate::schema;

#[derive(Serialize)]
pub struct FieldDiff {
    pub name: String,
    #[serde(rename = "type")]
    pub type_str: String,
}

#[derive(Serialize)]
pub struct FieldChanged {
    pub name: String,
    pub from: String,
    pub to: String,
}

pub enum DiffOutcome {
    Identical,
    Different {
        added: Vec<FieldDiff>,
        removed: Vec<FieldDiff>,
        changed: Vec<FieldChanged>,
        union_order: Vec<String>,
    },
}

pub fn diff_schemas(path_a: &Path, path_b: &Path) -> Result<DiffOutcome> {
    let fields_a = read_fields(path_a)?;
    let fields_b = read_fields(path_b)?;

    let map_a: HashMap<String, String> = fields_a.iter().cloned().collect();
    let map_b: HashMap<String, String> = fields_b.iter().cloned().collect();

    let mut seen: HashSet<String> = HashSet::new();
    let mut union_order: Vec<String> = Vec::new();
    for (name, _) in &fields_a {
        union_order.push(name.clone());
        seen.insert(name.clone());
    }
    for (name, _) in &fields_b {
        if !seen.contains(name) {
            union_order.push(name.clone());
            seen.insert(name.clone());
        }
    }

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for name in &union_order {
        match (map_a.get(name), map_b.get(name)) {
            (Some(type_a), None) => {
                removed.push(FieldDiff { name: name.clone(), type_str: type_a.clone() });
            }
            (None, Some(type_b)) => {
                added.push(FieldDiff { name: name.clone(), type_str: type_b.clone() });
            }
            (Some(type_a), Some(type_b)) if type_a != type_b => {
                changed.push(FieldChanged {
                    name: name.clone(),
                    from: type_a.clone(),
                    to: type_b.clone(),
                });
            }
            _ => {}
        }
    }

    if added.is_empty() && removed.is_empty() && changed.is_empty() {
        Ok(DiffOutcome::Identical)
    } else {
        Ok(DiffOutcome::Different { added, removed, changed, union_order })
    }
}

pub fn emit_text(outcome: &DiffOutcome) {
    let DiffOutcome::Different { added, removed, changed, union_order } = outcome else {
        return;
    };

    let removed_map: HashMap<&str, &str> =
        removed.iter().map(|f| (f.name.as_str(), f.type_str.as_str())).collect();
    let added_map: HashMap<&str, &str> =
        added.iter().map(|f| (f.name.as_str(), f.type_str.as_str())).collect();
    let changed_map: HashMap<&str, (&str, &str)> =
        changed.iter().map(|f| (f.name.as_str(), (f.from.as_str(), f.to.as_str()))).collect();

    for name in union_order {
        if let Some(type_str) = removed_map.get(name.as_str()) {
            println!("- {} {}", name, type_str);
        } else if let Some(type_str) = added_map.get(name.as_str()) {
            println!("+ {} {}", name, type_str);
        } else if let Some((from, to)) = changed_map.get(name.as_str()) {
            println!("~ {} {} → {}", name, from, to);
        }
    }
}

pub fn emit_json(outcome: &DiffOutcome) -> Result<()> {
    let json = match outcome {
        DiffOutcome::Identical => serde_json::json!({"identical": true}),
        DiffOutcome::Different { added, removed, changed, .. } => {
            serde_json::json!({
                "identical": false,
                "added":   added.iter().map(|f| serde_json::json!({"name": f.name, "type": f.type_str})).collect::<Vec<_>>(),
                "removed": removed.iter().map(|f| serde_json::json!({"name": f.name, "type": f.type_str})).collect::<Vec<_>>(),
                "changed": changed.iter().map(|f| serde_json::json!({"name": f.name, "from": f.from, "to": f.to})).collect::<Vec<_>>(),
            })
        }
    };
    serde_json::to_writer(stdout(), &json)?;
    println!();
    Ok(())
}

fn read_fields(path: &Path) -> Result<Vec<(String, String)>> {
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let reader = SerializedFileReader::new(file)
        .with_context(|| format!("Cannot read parquet metadata from {}", path.display()))?;
    let meta = reader.metadata();
    let schema_descr = meta.file_metadata().schema_descr();

    let mut fields = Vec::new();
    for i in 0..schema_descr.num_columns() {
        let col = schema_descr.column(i);
        fields.push((col.name().to_string(), schema::format_field_type(&col)));
    }
    Ok(fields)
}
