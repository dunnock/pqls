use anyhow::{Context, Result};
use parquet::file::reader::{FileReader, SerializedFileReader};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::stdout;
use std::path::Path;

use crate::schema;

#[derive(Serialize, PartialEq, Clone)]
pub struct TypeInfo {
    pub physical: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical: Option<String>,
}

impl TypeInfo {
    fn display(&self) -> String {
        match &self.logical {
            Some(l) => format!("{} {}", self.physical, l),
            None => self.physical.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct FieldDiff {
    pub name: String,
    #[serde(rename = "type")]
    pub type_info: TypeInfo,
}

#[derive(Serialize)]
pub struct FieldChanged {
    pub name: String,
    pub from: TypeInfo,
    pub to: TypeInfo,
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

    let map_a: HashMap<&str, &TypeInfo> = fields_a.iter().map(|(n, t)| (n.as_str(), t)).collect();
    let map_b: HashMap<&str, &TypeInfo> = fields_b.iter().map(|(n, t)| (n.as_str(), t)).collect();

    let mut seen: HashSet<&str> = HashSet::new();
    let mut union_order: Vec<String> = Vec::new();
    for (name, _) in &fields_a {
        if seen.insert(name.as_str()) {
            union_order.push(name.clone());
        }
    }
    for (name, _) in &fields_b {
        if seen.insert(name.as_str()) {
            union_order.push(name.clone());
        }
    }

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for name in &union_order {
        match (map_a.get(name.as_str()), map_b.get(name.as_str())) {
            (Some(ta), None) => {
                removed.push(FieldDiff {
                    name: name.clone(),
                    type_info: (*ta).clone(),
                });
            }
            (None, Some(tb)) => {
                added.push(FieldDiff {
                    name: name.clone(),
                    type_info: (*tb).clone(),
                });
            }
            (Some(ta), Some(tb)) if ta != tb => {
                changed.push(FieldChanged {
                    name: name.clone(),
                    from: (*ta).clone(),
                    to: (*tb).clone(),
                });
            }
            _ => {}
        }
    }

    if added.is_empty() && removed.is_empty() && changed.is_empty() {
        Ok(DiffOutcome::Identical)
    } else {
        Ok(DiffOutcome::Different {
            added,
            removed,
            changed,
            union_order,
        })
    }
}

pub fn emit_text(outcome: &DiffOutcome) {
    let DiffOutcome::Different {
        added,
        removed,
        changed,
        union_order,
    } = outcome
    else {
        return;
    };

    let removed_map: HashMap<&str, &TypeInfo> = removed
        .iter()
        .map(|f| (f.name.as_str(), &f.type_info))
        .collect();
    let added_map: HashMap<&str, &TypeInfo> = added
        .iter()
        .map(|f| (f.name.as_str(), &f.type_info))
        .collect();
    let changed_map: HashMap<&str, (&TypeInfo, &TypeInfo)> = changed
        .iter()
        .map(|f| (f.name.as_str(), (&f.from, &f.to)))
        .collect();

    for name in union_order {
        if let Some(t) = removed_map.get(name.as_str()) {
            println!("- {} {}", name, t.display());
        } else if let Some(t) = added_map.get(name.as_str()) {
            println!("+ {} {}", name, t.display());
        } else if let Some((from, to)) = changed_map.get(name.as_str()) {
            println!("~ {} {} → {}", name, from.display(), to.display());
        }
    }
}

pub fn emit_json(outcome: &DiffOutcome) -> Result<()> {
    let json = match outcome {
        DiffOutcome::Identical => serde_json::json!({
            "identical": true,
            "added": [],
            "removed": [],
            "changed": [],
        }),
        DiffOutcome::Different {
            added,
            removed,
            changed,
            ..
        } => {
            serde_json::json!({
                "identical": false,
                "added":   added,
                "removed": removed,
                "changed": changed,
            })
        }
    };
    serde_json::to_writer(stdout(), &json)?;
    println!();
    Ok(())
}

fn read_fields(path: &Path) -> Result<Vec<(String, TypeInfo)>> {
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let reader = SerializedFileReader::new(file)
        .with_context(|| format!("Cannot read parquet metadata from {}", path.display()))?;
    let meta = reader.metadata();
    let schema_descr = meta.file_metadata().schema_descr();

    let mut fields = Vec::new();
    for i in 0..schema_descr.num_columns() {
        let col = schema_descr.column(i);
        let physical = format!("{:?}", col.physical_type());
        let logical = schema::get_logical_type_str(&col);
        fields.push((col.name().to_string(), TypeInfo { physical, logical }));
    }
    Ok(fields)
}
