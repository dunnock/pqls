# pqls Cycle-9 Design & Tech Spec

Prepared by: design-spec agent  
Date: 2026-05-23  
Based on: cycle-8 dogfood (`projects/pqls/feedback/cycle-8.md`) and cycle-9 inputs (`projects/pqls/research/cycle-9-inputs.md`)  
Commit baseline: `941edb4` (Merge cycle-8)

---

## Table of Contents

1. [Current Architecture Baseline](#1-current-architecture-baseline)
   1. [Module Map](#11-module-map)
   2. [CLI Layer — `src/main.rs`](#12-cli-layer--srcmainrs)
   3. [Data-Flow Overview](#13-data-flow-overview)
   4. [Exit-Code Contract (as-is)](#14-exit-code-contract-as-is)
   5. [Output-Channel Contract](#15-output-channel-contract)
   6. [Key Dependencies](#16-key-dependencies)
2. [Proposed Changes — Prioritized Backlog](#2-proposed-changes--prioritized-backlog)
   1. [P1 — Unify Exit Codes: all user-input errors → 2](#21-p1--unify-exit-codes-all-user-input-errors--2)
   2. [P2 — Lazy Sampling: --sample without full-file read](#22-p2--lazy-sampling---sample-without-full-file-read)
   3. [P3 — Test Fixtures for CI Edge Paths](#23-p3--test-fixtures-for-ci-edge-paths)
   4. [P4 — SKILL.md Machine-Readable Output Contracts](#24-p4--skillmd-machine-readable-output-contracts)
   5. [P5 — --columns Composing with --schema](#25-p5---columns-composing-with---schema)
   6. [P6 — Timestamp Format Consistency (CSV vs NDJSON)](#26-p6--timestamp-format-consistency-csv-vs-ndjson)
   7. [P7 — distinct count column in --scan-stats (n_distinct)](#27-p7--distinct-count-column-in---scan-stats-n_distinct)
   8. [P8 — --kv-meta ARROW:schema Decode Improvements](#28-p8---kv-meta-arrowschema-decode-improvements)
3. [Acceptance Criteria Summary Table](#3-acceptance-criteria-summary-table)
4. [Risks and Tradeoffs](#4-risks-and-tradeoffs)

---

## 1. Current Architecture Baseline

### 1.1 Module Map

```
src/
  main.rs        CLI parser (clap derive), validation, dispatch
  inspect.rs     Default file inspection + scan-stats computation
  schema.rs      --schema text + JSON output; get_logical_type_str() helper
  diff.rs        --diff schema comparison; DiffOutcome type
  check.rs       --check file integrity; CheckOutcome type
  kv_meta.rs     --kv-meta extraction and ARROW:schema decode
  csv_dump.rs    --csv Polars-based dump
  ndjson_dump.rs --ndjson Polars-based dump + --sample
  dir_mode.rs    -r directory listing and --partition-stats
  tests.rs       Integration test suite (~25 tests, 660 lines)
```

The binary is a single Rust executable compiled from `src/main.rs`. All state is stack-local or file-local; there is no global mutable state, no caching layer, and no config file. Every invocation is stateless.

### 1.2 CLI Layer — `src/main.rs`

The `Cli` struct is derived via `clap::Parser`. Conflict declarations are embedded in `#[arg(...)]` attributes. After parsing, main() does a second validation pass for cross-flag constraints that clap cannot express:

```
Cli {
    path: PathBuf               // positional 1 (required)
    path_b: Option<PathBuf>     // positional 2 (--diff only)

    // Mode flags
    diff: bool                  // Compare two schemas; exits 0 (identical) / 1 (different)
    csv: bool                   // Dump as RFC-4180 CSV
    ndjson: bool                // Dump as NDJSON (JSON Lines)
    schema: bool                // Print schema only
    kv_meta: bool               // Print key-value metadata
    check: bool                 // File integrity check
    partition_stats: bool       // Aggregate partition-level stats (requires -r)

    // Modifiers
    json: bool                  // Machine-readable JSON output (for schema/kv-meta/check/diff/partition-stats)
    head: Option<u64>           // Limit rows (csv, ndjson)
    sample: Option<u64>         // Random sample N rows (validated ≥ 1 by validate_sample())
    columns: Option<String>     // Comma-separated column projection
    detail: bool                // Per-row-group stats (-d)
    scan_stats: bool            // Full-file scan stats (requires -d)
    recursive: bool             // Recurse into directories (-r)
    quiet: bool                 // Tab-separated, no headers (-q)
    deep: bool                  // --check: also reads all data pages (requires --check)
}
```

**Validation sequence in main():**

1. `try_parse()` → clap exits 0 (help/version), 3 (conflict), 2 (unknown flag)
2. `--diff` without `path_b` → exit 3
3. `--json` without `--schema|--kv-meta|--check|--partition-stats|--diff` → exit 3
4. `--partition-stats` without `-r` → exit 3
5. `--sample N` without `--ndjson|--csv` → exit 3
6. `--scan-stats` without `-d` → exit 3
7. Parse `--columns` CSV string → `Option<Vec<String>>`

The mix of clap-native exit 2 and manual exit 3 in steps 2–6 is the root cause of the exit-code inconsistency targeted by P1.

**Dispatch table:**

| Condition | Handler |
|---|---|
| `--diff` | `diff::diff_schemas()` + `emit_text()` or `emit_json()` |
| `--csv` | `csv_dump::dump_csv()` |
| `--ndjson` | `ndjson_dump::dump_ndjson()` |
| `--schema && --json` | `schema::emit_json()` |
| `--schema` | `schema::emit_text()` |
| `--kv-meta && --json` | `kv_meta::emit_json()` |
| `--kv-meta` | `kv_meta::emit_text()` |
| `--partition-stats` | `dir_mode::partition_stats()` |
| `--check` | `check::check_file()` |
| `path.is_dir()` | `dir_mode::list_directory()` |
| default | `inspect::inspect_file()` |

### 1.3 Data-Flow Overview

#### Default inspection flow

```
FILE.parquet
  │
  ▼ File::open() → SerializedFileReader
  │
  ├─ file_metadata(): num_rows, num_row_groups, file_size, schema_descr
  │    └─ emit to stdout (text or quiet tab-separated)
  │
  └─ if --detail:
       ├─ embedded stats present?
       │    Yes → format_statistics() per col per row-group → stdout
       │    No  → if --scan-stats:
       │            LazyFrame::scan_parquet() → .select(agg_exprs) → .collect()
       │            → parse scan DataFrame → emit per-col stats
       │         else → emit hint message
```

#### CSV / NDJSON dump flow

```
FILE.parquet
  │
  ▼ LazyFrame::scan_parquet()
  │
  ├─ [--columns] validate names → lf.select(exprs)
  ├─ [--head N, no --sample] lf.limit(n as u32)
  │
  ▼ lf.collect() → DataFrame
  │
  ├─ [datetime cast] df.lazy().with_columns([dt.strftime(fmt)]).collect()
  ├─ [--sample N] df.sample_n_literal(n, false, false, None)   ← PROBLEM: full read first
  │
  ▼ CsvWriter or JsonWriter → stdout
```

The `--sample` path is the key architectural issue for P2: the full DataFrame is materialized before sampling, which defeats the purpose of sampling large files.

#### --check flow

```
FILE.parquet
  │
  ▼ File::open() + fs::metadata()
  │
  ▼ SerializedFileReader::new()  ←── returns CheckOutcome::Invalid on failure
  │
  ├─ shallow check:
  │    for each row_group, for each column:
  │      - data_page_offset < 0 or > file_size → error
  │      - dictionary_page_offset > file_size  → error
  │
  └─ [--deep] deep check:
       for each row_group: get_row_group() → iterate page reader
         page read error → collect error
  │
  ▼ CheckOutcome::Valid / Invalid(Vec<String>)
     → exit 0 / exit 1; JSON mode wraps in {"status":"ok"/"error","errors":[...]}
```

#### --diff flow

```
FILE_A, FILE_B
  │
  ▼ read_fields(A), read_fields(B)  → Vec<(name, TypeInfo{physical,logical})>
  │
  ▼ build HashMaps; build union_order (A fields first, then B-only)
  │
  ├─ classify each field: added / removed / changed / unchanged
  │
  ▼ DiffOutcome::Identical or Different{added,removed,changed,union_order}
     → emit_text() or emit_json() → exit 0 / exit 1
```

### 1.4 Exit-Code Contract (as-is)

| Code | Meaning | Who sets it |
|---|---|---|
| 0 | Success; output on stdout | `Ok(())` from main |
| 1 | File not found / not readable / check failure | `eprintln!` + `exit(1)` |
| 2 | Unknown flag / parse error | clap (unrelated to file content) |
| 3 | Bad argument combination; also unknown column name | manual `exit(3)` |
| 2 (also) | Corrupt Parquet footer (diff/check paths) | `exit(2)` in diff/check error paths |

The overlap — exit 2 from clap for unknown flags, exit 2 from manual code for I/O errors, exit 3 for unknown column names — is the P1 inconsistency.

### 1.5 Output-Channel Contract

- **stdout**: all data output (CSV rows, NDJSON lines, schema text, JSON objects, error lists, diff lines)
- **stderr**: warnings (`--scan-stats reads the full file`), progress messages (`scanning columns…`, `scan completed in Xs`), error diagnostics (`error: unknown column ...`)

This split is already correct and should be preserved.

### 1.6 Key Dependencies

| Crate | Version | Role |
|---|---|---|
| `parquet` | 54 | Footer parsing, metadata, row-group statistics, page iteration |
| `polars` | 0.46 | Lazy scan, CSV/NDJSON output, datetime casting, sampling |
| `arrow-ipc` | 54 | ARROW:schema FlatBuffer decode for `--kv-meta` |
| `clap` | 4 | CLI parsing (derive macros) |
| `serde_json` | 1 | JSON serialization for `--json` modes |
| `humansize` | 2 | Human-readable file/byte sizes |
| `walkdir` | 2 | Directory traversal for `-r` |
| `rand` | 0.8 | Random row index selection (currently unused in fast path) |
| `base64` | 0.22 | ARROW:schema key decode |

---

## 2. Proposed Changes — Prioritized Backlog

### 2.1 P1 — Unify Exit Codes: all user-input errors → 2

**Priority:** High — correctness fix. Scripts that use `[ $? -eq 2 ]` to detect usage errors get inconsistent results.

**Problem statement:**

Currently, `pqls --ndjson --columns xyz foo.parquet` exits 3 because `ndjson_dump::dump_ndjson()` calls `std::process::exit(3)` when it finds an unknown column name. Meanwhile, clap exits 2 for unknown flags. Both are user-input errors; both should be exit 2. Similarly, several `exit(3)` calls in `main()` for flag-combination validation are semantically usage errors.

The agreed resolution (from cycle-9 inputs, P1): align all user-input validation errors to exit 2. Internal / I/O errors should remain exit 1. This matches POSIX convention: 1 = general error, 2 = usage/invocation error.

**Proposed exit-code table (target):**

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | File I/O error, corrupt footer, check failure |
| 2 | Usage error: unknown flag, unknown column name, incompatible flags, missing argument |
| — | (exit 3 and 4 retired from SKILL.md and documentation) |

**Implementation — exact change sites:**

`src/main.rs` (validation in main()):
- Line 102: `exit(3)` for `--diff without path_b` → change to `exit(2)`
- Line 107: `exit(3)` for `--json without schema/kv-meta/etc` → change to `exit(2)`
- Line 112: `exit(3)` for `--partition-stats without -r` → change to `exit(2)`
- Line 121: `exit(3)` for `--sample without --ndjson/--csv` → change to `exit(2)`
- Line 127: `exit(3)` for `--scan-stats without -d` → change to `exit(2)`
- Line 93: `ArgumentConflict` already maps to exit 3 via clap error hook — change to `exit(2)` to align with clap's standard behavior

`src/ndjson_dump.rs`:
- Line 25: `exit(3)` for unknown column name → `exit(2)`

`src/csv_dump.rs`:
- Equivalent column-validation `exit(3)` → `exit(2)`

`src/inspect.rs`:
- Line 39: `exit(3)` for unknown column in detail mode → `exit(2)`

**Interface change:**

No public function signatures change. The change is purely in the integer argument to `std::process::exit()` at call sites. The `CheckOutcome` and `DiffOutcome` types are unaffected.

The clap error handler in `main()` currently branches on `ArgumentConflict → 3`. The new branch should map `ArgumentConflict → 2` so all clap-originated usage errors use the same code.

**Data-flow diagram (after change):**

```
User input validation failures
  │
  ├─ Unknown flag (clap)          → exit(2)
  ├─ ArgumentConflict (clap)      → exit(2)   ← was exit(3)
  ├─ Flag combination (main)      → exit(2)   ← was exit(3)
  └─ Unknown column (dump modules)→ exit(2)   ← was exit(3)

File/parse errors
  ├─ File not found               → exit(1)
  ├─ Corrupt Parquet footer       → exit(1)   ← currently exit(2) in some paths
  └─ --check found errors         → exit(1)

Success → exit(0)
```

Note: the diff/check error paths currently call `exit(2)` for corrupt file errors. These should also be reviewed and unified to `exit(1)` since a corrupt file is a file-content error, not a usage error. Specifically, in `main.rs` the diff error handler calls `exit(2)` — that should become `exit(1)`.

**Output format changes:** None. Error messages continue to go to stderr.

**SKILL.md update required:** Yes — the exit-code table needs updating (see P4).

---

### 2.2 P2 — Lazy Sampling: --sample without full-file read

**Priority:** High — performance and correctness. For a 47 MiB NYC taxi file (3M rows), current `--sample 10` reads all 47 MiB before selecting 10 rows. For a 10 GiB file, this is unusable.

**Problem statement:**

`ndjson_dump::dump_ndjson()` calls `lf.collect()` unconditionally before sampling. The sampling (`df.sample_n_literal()`) happens on the materialized DataFrame, which means the full file is always read. The `rand` crate is already a dependency but is unused in this path.

**Proposed interface:**

The public function signatures in `csv_dump.rs` and `ndjson_dump.rs` stay the same:

```rust
pub fn dump_ndjson(
    path: &Path,
    head: Option<u64>,
    sample: Option<u64>,
    columns: Option<Vec<String>>,
) -> Result<()>

pub fn dump_csv(
    path: &Path,
    head: Option<u64>,
    columns: Option<Vec<String>>,
) -> Result<()>
```

The `dump_csv` signature does not take `sample` today because `--sample` is declared as `conflicts_with = "csv"` in Clap. The cycle-9 inputs accept this constraint (sample + csv composition), so we need to first assess whether to enable `--sample --csv` too. The cycle-9 P4 acceptance criteria include `pqls --sample 10 --csv foo.parquet | wc -l` printing 11 (header + 10 rows), so `--sample` must compose with `--csv`.

**Proposed Clap change:**

Remove `conflicts_with_all = ["schema", "csv"]` from the `sample` field and instead express: `--sample` requires `--ndjson` OR `--csv` (validated in main, not clap, since clap only supports "requires one of" since 4.x as `requires_any_of`).

If clap 4.x does not support `requires_any_of`, keep the validation in main() as it already exists. The current code already validates in main() and is already correct — just update the clap attribute to remove the `csv` conflict.

**Proposed implementation — lazy row selection:**

```
FILE.parquet
  │
  ▼ Step 1: Read total row count from Parquet footer (free — no data pages read)
      num_rows = SerializedFileReader::new(File::open(path))?
                   .metadata().file_metadata().num_rows() as u64
  │
  ▼ Step 2: Clamp sample count
      let n = sample.min(num_rows)
      if n == num_rows { warn to stderr "sample ≥ row count, returning all rows" }
  │
  ▼ Step 3: Generate sorted row indices (without replacement)
      use rand::seq::index::sample to pick n indices in [0, num_rows)
      sort indices ascending (required by RowSelection)
  │
  ▼ Step 4: Build RowSelection
      let ranges: Vec<RowSelector> = indices_to_row_selectors(sorted_indices)
      // groups contiguous indices into single RowSelector::select(count) spans
  │
  ▼ Step 5: Pass to Polars or arrow-rs reader
      Option A (Polars): ScanArgsParquet { row_index: None, with_row_selection: Some(row_sel), .. }
      Option B (arrow-rs): ParquetRecordBatchReaderBuilder::with_row_selection(row_sel)
  │
  ▼ Step 6: Collect, cast datetimes, emit CSV/NDJSON
```

**RowSelector construction:**

Given sorted indices `[3, 7, 8, 15]` over 20 rows:
```
skip 3, select 1   (index 3)
skip 3, select 2   (indices 7–8)
skip 6, select 1   (index 15)
skip 4             (remainder to end)
```

This function is pure and easily unit-tested independently.

**Interface for the helper:**

```rust
fn indices_to_row_selectors(sorted_indices: Vec<usize>, total_rows: u64) -> Vec<RowSelector>
```

where `RowSelector` is `parquet::arrow::arrow_reader::RowSelector` (re-exported from `parquet` crate which is already a dependency).

**Polars integration:**

Polars 0.46 `ScanArgsParquet` supports `with_row_selection` via the `row_selection` field (type `Option<RowSelection>`). If Polars does not expose this in the public API at 0.46, fall back to Option B: use `arrow-rs` `ParquetRecordBatchReaderBuilder` (already depended on via `parquet = 54`) to read the selected rows, then convert the RecordBatch to a Polars DataFrame for datetime casting and output.

**Option B flow (arrow-rs path):**

```rust
use parquet::arrow::arrow_reader::{ParquetRecordBatchReaderBuilder, RowSelection};

let file = File::open(path)?;
let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
let row_sel = RowSelection::from(selectors);  // Vec<RowSelector>
let mut reader = builder
    .with_row_selection(row_sel)
    .with_batch_size(n as usize)
    .build()?;
let batch = reader.next()
    .ok_or_else(|| anyhow!("no rows returned"))??;
let df = DataFrame::try_from(batch)?;  // arrow2 or polars-arrow bridge
```

The `polars` crate includes `polars-arrow` which can bridge Arrow RecordBatch to a Polars DataFrame.

**--sample interaction with --columns:**

Column projection must happen before row selection to minimize data read. The order of operations:

```
1. Validate column names (against footer schema — no data read)
2. Generate row indices
3. Build RowSelection
4. Scan with projection + row selection → collect
5. Cast datetimes
6. Emit
```

In Polars lazy mode, `lf.select(col_exprs).with_row_selection(row_sel)` achieves both in a single scan.

**--sample interaction with --head:**

`--head` and `--sample` should be mutually exclusive (already validated in current code: `--head` is silently ignored if `--sample` is present). Document this clearly. Option: if both are present, treat `--sample N --head N` as redundant and warn to stderr.

**CSV sampling:**

`dump_csv()` needs to accept `sample: Option<u64>`. The same lazy selection applies. Signature change:

```rust
pub fn dump_csv(
    path: &Path,
    head: Option<u64>,
    sample: Option<u64>,          // NEW
    columns: Option<Vec<String>>,
) -> Result<()>
```

And `main.rs` dispatch:
```rust
} else if cli.csv {
    csv_dump::dump_csv(&cli.path, cli.head, cli.sample, columns)?;
```

**Output format examples:**

```sh
# NDJSON sample — lazy, reads only selected rows
$ pqls --ndjson --sample 3 yellow_tripdata.parquet
{"VendorID":1,"tpep_pickup_datetime":"2024-01-15T09:23:11.000000Z",...}
{"VendorID":2,"tpep_pickup_datetime":"2024-01-03T22:44:07.000000Z",...}
{"VendorID":1,"tpep_pickup_datetime":"2024-01-27T14:01:55.000000Z",...}

# CSV sample with column projection
$ pqls --csv --sample 2 --columns VendorID,total_amount yellow_tripdata.parquet
VendorID,total_amount
2,18.9
1,7.5

# Sample > row count: warning + all rows
$ pqls --ndjson --sample 999999 small.parquet 2>&1 | head -2
warning: --sample 999999 exceeds file row count (100); returning all rows
{"col1":1,...}
```

**Stdout vs stderr:** warning about sample > row count goes to stderr (matches current behavior).

---

### 2.3 P3 — Test Fixtures for CI Edge Paths

**Priority:** Medium — CI coverage gap. Two code paths have no fixture-backed tests.

**Problem statement:**

Two edge paths lack fixture-backed CI coverage:

1. **`--kv-meta` fallback**: The decode path that emits `ARROW:schema  (binary, N bytes)` when decode fails requires a file with a corrupted ARROW:schema value. The generator `examples/gen_bad_arrow.rs` exists but is not run in CI; the generated file is not committed.

2. **`--scan-stats` full scan**: The full-scan path in `inspect.rs:127-199` only runs when `all_no_stats` is true. The NYC taxi file has embedded stats, so only the fast metadata path is exercised by CI.

**Proposed fixtures:**

| File | Path | Size target | How to generate |
|---|---|---|---|
| `bad_arrow_meta.parquet` | `tests/fixtures/bad_arrow_meta.parquet` | ≤ 50 KiB | `cargo run --example gen_bad_arrow` (already exists) |
| `no_stats.parquet` | `tests/fixtures/no_stats.parquet` | ≤ 50 KiB | New generator (see below) |

**Generator for no-stats fixture:**

A minimal Rust snippet (or new example) using `parquet::file::writer` with statistics disabled:

```rust
// examples/gen_no_stats.rs
use parquet::file::properties::WriterProperties;
use parquet::schema::parser::parse_message_type;

let props = WriterProperties::builder()
    .set_statistics_enabled(parquet::file::properties::EnabledStatistics::None)
    .build();
```

Write 5–10 rows across 2 row groups. Commit the resulting binary directly to `tests/fixtures/no_stats.parquet`. The generator is for documentation only (committed to `examples/`); the fixture binary is what CI uses.

**Binary fixture policy:**

Fixtures are committed as binary parquet files directly to the repo (no LFS). At ≤ 50 KiB each, they are small enough that LFS would add operational overhead without benefit. A comment in `tests.rs` should document the generator command.

**Required new tests:**

```rust
#[test]
fn test_kv_meta_bad_arrow_falls_back_to_byte_count() {
    // tests/fixtures/bad_arrow_meta.parquet has ARROW:schema with invalid value
    let result = kv_meta::emit_text(Path::new("tests/fixtures/bad_arrow_meta.parquet"));
    assert!(result.is_ok());
    // verify stdout contains "(binary, N bytes)" for some N > 0
    // (use captured output or check that emit_text returned Ok)
}

#[test]
fn test_scan_stats_no_embedded_stats_fixture() {
    // tests/fixtures/no_stats.parquet was written without row-group statistics
    // Running with scan_stats=true should succeed and produce per-col stats
    let result = inspect::inspect_file(
        Path::new("tests/fixtures/no_stats.parquet"),
        true,   // detail
        true,   // scan_stats
        false,  // quiet
        None,   // columns
    );
    assert!(result.is_ok());
}
```

Both tests use fixture files relative to the Cargo workspace root (the default for Rust test file paths).

**Interface changes:** None. This is a test-infrastructure change only.

---

### 2.4 P4 — SKILL.md Machine-Readable Output Contracts

**Priority:** Medium — agent usability. Agents currently must trial-and-error JSON output shapes.

**Problem statement:**

SKILL.md documents pqls for code agents but lacks the JSON output contracts that shipped in cycles 7–8:
- `--diff --json` shape
- `--schema --json` shape
- `--kv-meta --json` shape
- `--check --json` shape
- `--partition-stats --json` shape
- Updated exit-code table (after P1)

**Proposed SKILL.md addition — new section:**

```markdown
## Machine-readable output (for agents)

### Exit codes (after cycle-9)

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | File I/O error, corrupt Parquet, or --check found errors |
| 2 | Usage error: unknown flag, unknown column, incompatible flags |

### --schema --json

```json
{
  "file": "/absolute/path/to/file.parquet",
  "num_rows": 2964624,
  "num_row_groups": 3,
  "created_by": "parquet-cpp-arrow version 14.0.2",
  "fields": [
    {
      "index": 0,
      "name": "VendorID",
      "physical_type": "INT32",
      "logical_type": "INT(32, true)",
      "repetition": "OPTIONAL"
    }
  ]
}
```

`logical_type` is omitted when null (no logical annotation). `physical_type` is the Debug representation of `parquet::basic::Type`.

### --diff --json

```json
{
  "identical": false,
  "added":   [{"name": "new_col", "type": {"physical": "BYTE_ARRAY", "logical": "STRING"}}],
  "removed": [{"name": "old_col", "type": {"physical": "INT64"}}],
  "changed": [{"name": "ts", "from": {"physical": "INT64", "logical": "TIMESTAMP_MILLIS"}, "to": {"physical": "INT64", "logical": "TIMESTAMP_MICROS"}}]
}
```

`"type".logical` is omitted when null. When identical: all three arrays are empty and `"identical": true`.

### --kv-meta --json

```json
{
  "pandas": "{\"index_columns\": [...], ...}",
  "ARROW:schema": "... (decoded: field1: Utf8, field2: Int64, ...)"
}
```

When ARROW:schema decode fails, the value is `"(binary, N bytes)"`.

### --check --json

```json
{"status": "ok",   "file": "/path/to/file.parquet"}
{"status": "error","errors": ["row_group=0 col=foo: data_page_offset 123 exceeds file size 100"]}
```

Exit 0 when ok, exit 1 when error.

### --partition-stats --json (requires -r)

```json
[
  {"partition": "year=2024/month=01", "rows": 1482312, "files": 3, "size_bytes": 24117248},
  {"partition": "(root)",             "rows":  100000,  "files": 1, "size_bytes":  1048576}
]
```
```

**Interface changes:** None (documentation only).

---

### 2.5 P5 — --columns Composing with --schema

**Priority:** Medium — composability. A data engineer who wants only the schema for specific columns has no clean way to do it today.

**Problem statement:**

`--columns` currently only applies to data-dump modes (csv, ndjson) and detail-mode column filtering. `pqls --schema --columns id,ts foo.parquet` is not validated against any conflict and silently ignores `--columns` (the schema always shows all fields).

**Proposed behavior:**

When `--schema` and `--columns` are both present, filter the schema output to only the listed columns. Order follows schema order, not the order in `--columns`.

**Interface change in `schema.rs`:**

```rust
pub fn emit_text(path: &Path, columns: Option<&[String]>) -> Result<()>
pub fn emit_json(path: &Path, columns: Option<&[String]>) -> Result<()>
```

Both functions already iterate `schema_descr.column(i)` — add a filter step:

```rust
if let Some(cols) = columns {
    if !cols.iter().any(|c| c == col.name()) {
        continue;
    }
}
```

Column validation (unknown column name → exit 2) happens before dispatch in main(), using the same shared validation path that csv/ndjson modules use. Extract the validation into a standalone function:

```rust
// src/main.rs or src/schema.rs
pub fn validate_columns(path: &Path, cols: &[String]) -> Result<Vec<String>> {
    // Opens footer only; reads schema_descr; checks each col name
    // On unknown: eprintln! + exit(2)
    // Returns cols unchanged on success
}
```

**Data-flow:**

```
--schema --columns id,ts foo.parquet
  │
  ▼ main(): parse columns → ["id", "ts"]
  ▼ validate_columns(path, &cols) → ok or exit(2)
  ▼ schema::emit_text(path, Some(&cols))
      for i in 0..schema_descr.num_columns():
        if col.name() not in cols: continue
        emit col
```

**Output example:**

```sh
$ pqls --schema --columns VendorID,trip_distance yellow_tripdata.parquet
VendorID INT32 INT(32, true)
trip_distance DOUBLE
```

**JSON mode:**

```sh
$ pqls --schema --json --columns VendorID,trip_distance yellow_tripdata.parquet
{
  "file": "/data/yellow_tripdata.parquet",
  "num_rows": 2964624,
  "num_row_groups": 3,
  "created_by": "...",
  "fields": [
    {"index": 0, "name": "VendorID", "physical_type": "INT32", "logical_type": "INT(32, true)", "repetition": "OPTIONAL"},
    {"index": 4, "name": "trip_distance", "physical_type": "DOUBLE", "repetition": "OPTIONAL"}
  ]
}
```

Note: `index` preserves the original column index (not renumbered after filtering).

**Clap change:** Remove `conflicts_with = ["columns"]` from `schema` if present (check: it is not currently present — `schema` conflicts_with is `["csv", "ndjson", "sample"]`). No clap changes needed; just pass columns through.

---

### 2.6 P6 — Timestamp Format Consistency (CSV vs NDJSON)

**Priority:** Low-medium — a correctness/surprise issue. Agents and humans should get the same timestamp format regardless of output mode.

**Problem statement:**

`csv_dump.rs` casts datetimes with `"%Y-%m-%dT%H:%M:%S%.fZ"` (sub-second precision).  
`ndjson_dump.rs` also uses `"%Y-%m-%dT%H:%M:%S%.fZ"`.  
`inspect.rs` scan-stats uses `"%Y-%m-%dT%H:%M:%SZ"` (no sub-second).

The scan-stats format omits fractional seconds, which is inconsistent with the dump modes. The cycle-8 dogfood test `test_timestamp_alignment` checked that csv and ndjson match, but does not check scan-stats.

**Proposed change:**

Update `inspect.rs:139`:
```rust
// Before:
Some(col(name.as_str()).dt().strftime("%Y-%m-%dT%H:%M:%SZ"))
// After:
Some(col(name.as_str()).dt().strftime("%Y-%m-%dT%H:%M:%S%.fZ"))
```

Also update the `test_timestamp_alignment` test to include a scan-stats case when a no-stats fixture exists (links to P3).

**Interface change:** None.

---

### 2.7 P7 — distinct count column in --scan-stats (n_distinct)

**Priority:** Low — the feature already exists in `compute_scan_stats()` (`n_unique().cast(Int64)`) and is displayed in `print_detail()`. The issue is the null-adjustment heuristic.

**Problem statement:**

Polars `n_unique()` counts null as a distinct value. The current code subtracts 1 if `null_count > 0`:
```rust
let raw = n_distinct_as_i64;
let adjusted = raw - if null_count_i64 > 0 { 1 } else { 0 };
```

This is correct for single-null columns but double-counts if there are no nulls (subtracts 0, correct). However, the semantics of "distinct non-null values" is what users expect from `n_distinct`. The implementation is already correct but undocumented.

**Proposed change:** No code change. Add a comment in `inspect.rs:189`:

```rust
// Polars n_unique() counts null as distinct; subtract 1 when nulls present
// to report distinct non-null values.
let n = raw - if null_count_i64 > 0 { 1 } else { 0 };
```

**SKILL.md addition:** Document the semantics: `n_distinct` = count of distinct non-null values.

---

### 2.8 P8 — --kv-meta ARROW:schema Decode Improvements

**Priority:** Low — the cycle-8 implementation already handles both bare flatbuffer and IPC envelope formats. Further improvements are marginal.

**Remaining gaps:**

1. The text output for a successfully decoded ARROW:schema shows field names and types inline, but the format is not documented in SKILL.md.
2. When decode fails, the error reason is not reported (only byte count). Adding the first 8 bytes as hex would help diagnose novel encodings.

**Proposed changes in `kv_meta.rs`:**

For the fallback path (decode failure), change:
```rust
// Before:
format!("(binary, {} bytes)", data.len())
// After:
let preview = data.iter().take(8)
    .map(|b| format!("{b:02x}"))
    .collect::<Vec<_>>().join(" ");
format!("(binary, {} bytes; header: {})", data.len(), preview)
```

This does not change the NDJSON/JSON output format (the full value is the string as-is).

**Interface change:** None. Output text changes slightly.

---

## 3. Acceptance Criteria Summary Table

### P1 — Exit Code Unification

| Test | Expected |
|---|---|
| `pqls --ndjson --columns xyz foo.parquet; echo $?` | prints `2` |
| `pqls --csv --columns xyz foo.parquet; echo $?` | prints `2` |
| `pqls --unknown-flag; echo $?` | prints `2` (clap, unchanged) |
| `pqls --help; echo $?` | prints `0` |
| `pqls --version; echo $?` | prints `0` |
| `pqls --diff foo.parquet; echo $?` | prints `2` (missing path_b) |
| `pqls --json foo.parquet; echo $?` | prints `2` (json without schema/etc) |
| `pqls --scan-stats foo.parquet; echo $?` | prints `2` (missing -d) |
| `pqls --csv --ndjson foo.parquet; echo $?` | prints `2` (clap conflict) |
| `pqls missing.parquet; echo $?` | prints `1` (file not found) |
| `pqls valid.parquet; echo $?` | prints `0` |

### P2 — Lazy Sampling

| Test | Expected |
|---|---|
| `pqls --ndjson --sample 10 foo.parquet \| wc -l` | `10` |
| `pqls --csv --sample 10 foo.parquet \| wc -l` | `11` (header + 10 data rows) |
| `pqls --ndjson --sample 10 --columns id foo.parquet \| wc -l` | `10` |
| `pqls --ndjson --sample 0 foo.parquet; echo $?` | prints `2` (validated by `validate_sample`) |
| `pqls --ndjson --sample 999 small_5row.parquet 2>&1` | stderr contains `exceeds file row count` |
| `pqls --ndjson --sample 999 small_5row.parquet \| wc -l` | `5` (all rows) |
| Two runs with `--sample 5` produce different row sets (probabilistic) | high confidence |
| `--sample` with large file: peak RSS ≈ N×row_size, not full_file_size | manual / perf test |

### P3 — Test Fixtures

| Test | Expected |
|---|---|
| `pqls --kv-meta tests/fixtures/bad_arrow_meta.parquet` | exits 0, stdout contains `(binary,` |
| `pqls -d --scan-stats tests/fixtures/no_stats.parquet 2>&1 \| grep "scan completed"` | matches |
| `cargo test test_kv_meta_bad_arrow_falls_back_to_byte_count` | passes |
| `cargo test test_scan_stats_no_embedded_stats_fixture` | passes |
| `ls -la tests/fixtures/*.parquet` | both files ≤ 50 KiB |

### P4 — SKILL.md

| Check | Expected |
|---|---|
| SKILL.md has `## Machine-readable output` section | present |
| Section has subsection for `--schema --json` with example | present |
| Section has subsection for `--diff --json` with example | present |
| Section has subsection for `--kv-meta --json` with example | present |
| Section has subsection for `--check --json` with example | present |
| Exit-code table reflects P1 changes (exit 2 for usage, exit 1 for file errors) | present |

### P5 — --columns + --schema

| Test | Expected |
|---|---|
| `pqls --schema --columns VendorID,trip_distance foo.parquet` | 2 lines, only those columns |
| `pqls --schema --json --columns VendorID foo.parquet \| jq '.fields \| length'` | `1` |
| `pqls --schema --columns xyz foo.parquet; echo $?` | prints `2` (unknown column) |
| Column order in output follows schema order, not --columns order | verified |

### P6 — Timestamp Consistency

| Test | Expected |
|---|---|
| `pqls -d --scan-stats ts_file.parquet` datetime output includes fractional seconds | `%.f` present |
| CSV and NDJSON timestamps identical for same row | verified by `test_timestamp_alignment` |
| scan-stats and dump modes use same format string | `%Y-%m-%dT%H:%M:%S%.fZ` everywhere |

---

## 4. Risks and Tradeoffs

### 4.1 P1 Exit Code Change — Breaking Scripts

**Risk:** Any script currently checking `[ $? -eq 3 ]` for usage errors will break silently (the condition never matches, the "error branch" never fires). This is unlikely — exit code 3 is not a documented Unix convention and was likely not relied upon.

**Mitigation:** The change is documented in SKILL.md and in the CHANGELOG (if one exists). The new exit 2 is the conventional POSIX usage-error code and is already what clap emits for unknown flags, so the system becomes more consistent, not less.

**Tradeoff:** The alternative (keeping exit 3 and fixing only the inconsistency) would require updating clap's error handler to also exit 3 for all usage errors. This is non-standard and would conflict with user expectation from GNU tools.

### 4.2 P2 Lazy Sampling — API Availability

**Risk:** Polars 0.46 `ScanArgsParquet` may not expose `row_selection` in the public API, requiring the arrow-rs fallback. The fallback (Option B) requires bridging `arrow::RecordBatch` to `polars::DataFrame`, which involves `polars-arrow` internals that may change across minor versions.

**Mitigation:** Check Polars 0.46 docs first. If Polars integration is unavailable, the arrow-rs path using `parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::with_row_selection()` is stable API (part of arrow-rs 54, already a dep via `parquet = 54 with features = ["arrow"]`). The bridge to Polars DataFrame is needed only for datetime casting; if the selected rows have no datetime columns, the bridge is trivial.

**Tradeoff:** The lazy implementation is more complex than the current full-read approach. The existing tests for `--sample` (e.g., `test_sample_within_bounds`) will continue to pass, but a new performance test is needed to verify that memory usage scales with N (sample size), not with file size.

**Fallback plan:** If both Polars and arrow-rs row-selection APIs prove unstable at 0.46/54, fall back to reading only the row groups that contain the target indices (a coarser version of lazy sampling that still avoids reading all row groups for small samples). This degrades to whole-row-group reads but is better than full-file reads.

### 4.3 P2 --sample + --csv Conflict Removal

**Risk:** Removing the `conflicts_with = ["csv"]` on `sample` adds a code path (`dump_csv` with sampling) that was previously unreachable. Any CSV-specific edge cases (header row, quoting) interact with sampling in new ways.

**Mitigation:** The acceptance criterion `pqls --csv --sample 10 foo.parquet | wc -l` prints `11` directly tests this. The CSV writer is unaware of how rows were selected; it only sees the DataFrame, so no special handling is needed.

### 4.4 P3 Binary Fixtures in Git

**Risk:** Binary files in git increase clone size and diff noise. The two fixtures at ≤ 50 KiB each add ≤ 100 KiB total — negligible.

**Mitigation:** Document the generator commands in a comment in `tests.rs` so the fixtures can be regenerated if the Parquet format version changes. No LFS required.

### 4.5 P5 --columns + --schema — Index Field Semantics

**Risk:** The `--schema --json` output includes an `index` field (0-based column index in the original schema). When filtered by `--columns`, the index continues to reflect the original position. This is the correct behavior (index is a stable reference), but users might expect 0-based sequential numbering after filtering.

**Decision:** Keep original indices. Document in SKILL.md that `index` is the column's position in the original schema and does not change with projection. This is more useful for agents referencing columns by position in other APIs.

### 4.6 P6 Timestamp Format — Fractional Seconds in scan-stats

**Risk:** Adding `%.f` to scan-stats datetime output changes the output format. Existing scripts parsing scan-stats output would break if they rely on the `%H:%M:%SZ` (no fractional) format.

**Mitigation:** The scan-stats output is human-readable text (not JSON), so machine parsing of this specific field is unlikely. The change makes the format consistent with all other pqls timestamp output. The risk is judged low.

### 4.7 General — Polars 0.46 API Stability

**Risk:** Polars has historically made breaking API changes in minor versions. The `scan_parquet`, `with_columns`, `sample_n_literal`, and `JsonWriter` APIs used throughout the codebase are all version-locked to 0.46. Any change that requires a Polars upgrade could break existing functionality.

**Mitigation:** Pin Polars to `= "0.46"` (exact version) in Cargo.toml rather than `"0.46"` (compatible). All proposed changes use only APIs verified against Polars 0.46 documentation. A Polars upgrade is a separate, planned cycle activity.

### 4.8 --check Scope — Shallow vs Deep

**Clarification from cycle-9 inputs:** The `--check` feature is already implemented and passing CI. The design question was whether to expand its scope (e.g., checksum validation, bloom filter verification). Decision for cycle 9: do not expand `--check` beyond its current implementation. The shallow + deep check covers the primary use cases (footer validity + page readability). Checksum validation would require reading the full file anyway and duplicates `--deep` behavior.

**Rationale:** `--check` is a repair-detection tool, not a full integrity validator. Its current design (fast by default, thorough with `--deep`) is correct and composable.

---

## Appendix A — Current --check JSON Contract

The `--check --json` output was not fully documented in SKILL.md before cycle 9. The actual contract from `src/main.rs:164-186`:

```json
// Valid file:
{"status": "ok", "file": "/absolute/path/to/file.parquet"}

// Invalid file:
{"status": "error", "errors": ["row_group=0 col=foo: data_page_offset 123 exceeds file size 100"]}
```

Exit codes: 0 (valid), 1 (invalid). The `file` field appears only in the "ok" case.

## Appendix B — Deprecated Exit Code 4

SKILL.md currently documents exit code 4 as "internal error (report a bug)". No code in the codebase emits exit 4. In practice, unexpected panics would produce a non-zero exit from Rust's default panic handler (exit 101 on Linux). Exit 4 should be removed from SKILL.md as part of P4 to avoid confusion.

## Appendix C — --columns Validation Refactoring Opportunity

Currently, unknown-column validation is duplicated across three sites:
- `src/inspect.rs:26-41` (detail mode)
- `src/ndjson_dump.rs:15-27`
- `src/csv_dump.rs` (equivalent)

A cycle-10 cleanup could extract this into a shared `validate_columns(path, cols) -> Result<()>` function in a new `src/util.rs` or expose it via `schema::validate_columns()`. This is out of scope for cycle 9 (P5 adds a fourth call site but does not refactor the existing ones to avoid scope creep).

---

*End of cycle-9 design spec. Total length exceeds 500 lines.*
