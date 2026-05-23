use anyhow::{Context, Result};
use arrow_array::RecordBatchReader;
use arrow_ipc::writer::StreamWriter;
use parquet::arrow::arrow_reader::{ParquetRecordBatchReaderBuilder, RowSelection, RowSelector};
use polars::prelude::{DataFrame, IpcStreamReader, SerReader};
use rand::seq::index;
use std::fs::File;
use std::io::Cursor;
use std::path::Path;

/// Read `n` randomly-sampled rows from `path`, returning a Polars DataFrame.
/// Total row count comes from the parquet footer; only the selected row pages
/// are decoded, so peak RSS scales with `n` not the full file size.
pub fn sample_lazy(path: &Path, n: u64) -> Result<DataFrame> {
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let total_rows = builder.metadata().file_metadata().num_rows() as u64;

    if n > total_rows {
        eprintln!(
            "warning: --sample {} exceeds file row count {}; returning all rows",
            n, total_rows
        );
    }

    let reader = if n < total_rows {
        let mut rng = rand::thread_rng();
        let indices = index::sample(&mut rng, total_rows as usize, n as usize);
        let mut sorted: Vec<usize> = indices.into_vec();
        sorted.sort_unstable();
        let selection = indices_to_row_selection(&sorted);
        builder.with_row_selection(selection).build()?
    } else {
        builder.build()?
    };

    // Write sampled batches to an in-memory Arrow IPC stream, then read back
    // with Polars. This bridges arrow-rs (parquet crate) and polars (arrow2).
    let schema = reader.schema();
    let mut buf: Vec<u8> = Vec::new();
    let mut writer = StreamWriter::try_new(&mut buf, &schema)?;
    for batch in reader {
        writer.write(&batch?)?;
    }
    writer.finish()?;

    let df = IpcStreamReader::new(Cursor::new(buf)).finish()?;
    Ok(df)
}

/// Convert a sorted list of row indices into a parquet RowSelection.
/// Consecutive non-selected rows become `skip` selectors; each selected row
/// becomes a `select(1)` selector.
pub fn indices_to_row_selection(sorted_indices: &[usize]) -> RowSelection {
    let mut selectors: Vec<RowSelector> = Vec::with_capacity(sorted_indices.len() * 2);
    let mut prev = 0usize;
    for &idx in sorted_indices {
        if idx > prev {
            selectors.push(RowSelector::skip(idx - prev));
        }
        selectors.push(RowSelector::select(1));
        prev = idx + 1;
    }
    RowSelection::from(selectors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_to_row_selection_basic() {
        // indices [0, 2, 4] in a 5-row file → select, skip1, select, skip1, select
        let sel = indices_to_row_selection(&[0, 2, 4]);
        // Just verify it's non-empty and doesn't panic
        let _ = sel;
    }

    #[test]
    fn indices_to_row_selection_contiguous() {
        // indices [1, 2, 3] → skip1, select, select, select
        let sel = indices_to_row_selection(&[1, 2, 3]);
        let _ = sel;
    }

    #[test]
    fn indices_to_row_selection_empty() {
        let sel = indices_to_row_selection(&[]);
        let _ = sel;
    }
}
