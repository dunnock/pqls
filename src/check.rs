use anyhow::Result;
use parquet::file::reader::{FileReader, SerializedFileReader};
use std::fs::File;
use std::path::Path;

pub enum CheckOutcome {
    Valid,
    Invalid(Vec<String>),
}

pub fn check_file(path: &Path, deep: bool) -> Result<CheckOutcome> {
    let file_size = std::fs::metadata(path)?.len();
    let file = File::open(path)?;

    let reader = match SerializedFileReader::new(file) {
        Ok(r) => r,
        Err(e) => return Ok(CheckOutcome::Invalid(vec![format!("{e}")])),
    };

    let meta = reader.metadata();
    let mut errors: Vec<String> = Vec::new();

    for rg_idx in 0..meta.num_row_groups() {
        let rg = meta.row_group(rg_idx);
        for col_idx in 0..rg.num_columns() {
            let col = rg.column(col_idx);
            let col_name = col.column_descr().name().to_string();
            let data_offset = col.data_page_offset();
            let compressed_size = col.compressed_size();

            if data_offset < 0 || compressed_size < 0 {
                errors.push(format!(
                    "row_group={rg_idx} col={col_name}: negative offset or size ({data_offset}, {compressed_size})"
                ));
                continue;
            }

            if (data_offset as u64).saturating_add(compressed_size as u64) > file_size {
                errors.push(format!(
                    "row_group={rg_idx} col={col_name}: data_page_offset {data_offset} exceeds file size {file_size}"
                ));
            }

            if let Some(dict_offset) = col.dictionary_page_offset() {
                if dict_offset < 0 || dict_offset as u64 >= file_size {
                    errors.push(format!(
                        "row_group={rg_idx} col={col_name}: dictionary_page_offset {dict_offset} exceeds file size {file_size}"
                    ));
                }
            }
        }
    }

    if deep {
        for rg_idx in 0..meta.num_row_groups() {
            match reader.get_row_group(rg_idx) {
                Err(e) => {
                    errors.push(format!("row_group={rg_idx}: failed to open: {e}"));
                }
                Ok(rg_reader) => {
                    for col_idx in 0..rg_reader.num_columns() {
                        match rg_reader.get_column_page_reader(col_idx) {
                            Err(e) => {
                                let col_name = meta
                                    .row_group(rg_idx)
                                    .column(col_idx)
                                    .column_descr()
                                    .name()
                                    .to_string();
                                errors.push(format!(
                                    "row_group={rg_idx} col={col_name}: page reader error: {e}"
                                ));
                            }
                            Ok(mut pr) => loop {
                                match pr.get_next_page() {
                                    Ok(None) => break,
                                    Ok(Some(_)) => {}
                                    Err(e) => {
                                        let col_name = meta
                                            .row_group(rg_idx)
                                            .column(col_idx)
                                            .column_descr()
                                            .name()
                                            .to_string();
                                        errors.push(format!(
                                            "row_group={rg_idx} col={col_name}: page read error: {e}"
                                        ));
                                        break;
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(CheckOutcome::Valid)
    } else {
        Ok(CheckOutcome::Invalid(errors))
    }
}
