# pqls Cycle-6 Design Spec

## 1. `pqls --diff A B` — Schema Diff

### 1.1 CLI Syntax

```
pqls --diff <PATH_A> <PATH_B> [--json]
```

`--diff` is a boolean flag. When present, exactly two positional path arguments must be supplied. The second positional arg (`PATH_B`) is optional in the struct but validated at runtime: if `--diff` is active and `PATH_B` is absent, print an error to stderr and exit 3.

**Clap struct changes:**

- Keep `path: PathBuf` as the first positional (PATH_A when `--diff` is active).
- Add `path_b: Option<PathBuf>` as the second positional (index 1), with `required = false`.
- Add `diff: bool` with `conflicts_with_all = ["csv", "ndjson", "schema", "kv_meta", "partition_stats", "check", "sample", "head", "detail", "recursive", "quiet", "columns", "scan_stats", "deep"]`.
- `--json` remains the shared JSON-output flag; it is compatible with `--diff`.

Runtime validation in `main.rs` (after `try_parse`):
```
if cli.diff && cli.path_b.is_none() {
    eprintln!("error: --diff requires two path arguments: pqls --diff A.parquet B.parquet");
    std::process::exit(3);
}
```

### 1.2 Type-String Canonicalisation

The type string for a field is determined by the same logic as `schema::get_logical_type_str`:

```
type_str = if let Some(lt) = get_logical_type_str(col) {
    format!("{:?} {}", col.physical_type(), lt)   // e.g. "INT32 INT(32, true)"
} else {
    format!("{:?}", col.physical_type())           // e.g. "INT64"
};
```

`schema::get_logical_type_str` must be made `pub` (it already is in `schema.rs`) so `diff.rs` can reuse it without duplication.

### 1.3 Field Comparison Algorithm

1. Build `fields_a: Vec<(name, type_str)>` from schema of A (column order from parquet metadata).
2. Build `fields_b: Vec<(name, type_str)>` from schema of B.
3. Build lookup maps `map_a: HashMap<name, type_str>` and `map_b: HashMap<name, type_str>`.
4. Categorise each field:
   - **removed**: present in A, absent in B → `map_a - map_b`
   - **added**: absent in A, present in B → `map_b - map_a`
   - **changed**: present in both, `type_str_a != type_str_b`
   - **unchanged**: present in both, same type_str
5. `identical = added.is_empty() && removed.is_empty() && changed.is_empty()`

Output ordering: follow the column order of the **union schema** (A columns first in their original order, then B-only columns in their original order).

### 1.4 Text Output (default)

Emit nothing when schemas are identical (quiet-on-success).

When schemas differ, emit one line per non-identical field:

```
- removed_col INT32 INT(32, true)
+ added_col BYTE_ARRAY STRING
~ changed_col INT32 → INT64
```

Prefix legend:
- `-` space then name and type from A: field exists in A, absent in B.
- `+` space then name and type from B: field absent in A, exists in B.
- `~` space then name, type from A, ` → `, type from B: field present in both but type changed.

Fields that are identical in both files produce no output line.

Full format strings:
```
"- {} {}\n"   where {} = name, type_str_a      (removed)
"+ {} {}\n"   where {} = name, type_str_b      (added)
"~ {} {} → {}\n" where args = name, type_str_a, type_str_b  (changed)
```

### 1.5 JSON Output (`--diff --json`)

When schemas are identical:
```json
{"identical": true}
```

When schemas differ:
```json
{
  "identical": false,
  "added":   [{"name": "col_name", "type": "BYTE_ARRAY STRING"}, ...],
  "removed": [{"name": "col_name", "type": "INT32"}, ...],
  "changed": [{"name": "col_name", "from": "INT32", "to": "INT64"}, ...]
}
```

All three arrays (`added`, `removed`, `changed`) are always present even when empty. Order within each array follows the union-schema ordering described in §1.3.

Emit via `serde_json::to_writer(stdout(), &result)?; println!();` (compact, not pretty-printed, for pipe-friendliness).

### 1.6 Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Schemas are identical |
| 1 | Schemas differ |
| 2 | I/O error (file not found, not a parquet file, parse failure) |
| 3 | Bad arguments (missing PATH_B, conflicting flags) |

### 1.7 Module Placement: `src/diff.rs`

Public surface:

```rust
pub enum DiffOutcome {
    Identical,
    Different {
        added:   Vec<FieldDiff>,
        removed: Vec<FieldDiff>,
        changed: Vec<FieldChanged>,
    },
}

pub struct FieldDiff {
    pub name:      String,
    pub type_str:  String,
}

pub struct FieldChanged {
    pub name: String,
    pub from: String,
    pub to:   String,
}

/// Returns Ok(DiffOutcome) or Err for I/O / parse failures.
pub fn diff_schemas(path_a: &Path, path_b: &Path) -> Result<DiffOutcome>;

pub fn emit_text(outcome: &DiffOutcome);
pub fn emit_json(outcome: &DiffOutcome) -> Result<()>;
```

`main.rs` calls `diff::diff_schemas`, then `diff::emit_text` or `diff::emit_json`, then `std::process::exit(if identical { 0 } else { 1 })`.

### 1.8 Error Contract

- File-not-found or corrupt parquet: propagate `anyhow::Error`, caught in `main.rs` with `unwrap_or_else(|e| { eprintln!("error: {e}"); std::process::exit(2); })` — same pattern as other subcommands.
- `--diff` with only one path: exit 3 (validated before dispatching to `diff.rs`).
- `--diff` with `--json` and one path: also exit 3 (path check runs first).

### 1.9 Acceptance Criteria

- [ ] `pqls --diff A.parquet A.parquet` exits 0 and emits nothing to stdout.
- [ ] `pqls --diff A.parquet B.parquet` exits 1 and emits `+`/`-`/`~` lines for each difference.
- [ ] `pqls --diff A.parquet B.parquet --json` exits 1 and emits a valid JSON object with `identical`, `added`, `removed`, `changed` keys.
- [ ] `pqls --diff A.parquet B.parquet --json` exits 0 and emits `{"identical":true}` when files share the same schema.
- [ ] `pqls --diff A.parquet` exits 3 with an error message on stderr.
- [ ] `pqls --diff --csv A.parquet B.parquet` exits 3 (flag conflict).
- [ ] `pqls --diff missing.parquet B.parquet` exits 2.
- [ ] Type strings match exactly what `pqls --schema A.parquet` emits for the same column.

---

## 2. `n_distinct` in `--scan-stats`

### 2.1 Gate

Computed automatically whenever `--scan-stats` is passed alongside `--detail`. No new flag. The existing flow in `inspect.rs::print_detail` → `compute_scan_stats` already gathers per-column aggregates; `n_distinct` is an additional aggregate in the same query.

### 2.2 Implementation: `compute_scan_stats` in `src/inspect.rs`

Add a fourth expression per column in the `agg_exprs` flat-map:

```rust
col(n.as_str())
    .n_unique()
    .cast(DataType::Int64)
    .alias(format!("{n}__n_distinct")),
```

This extends each column's aggregate triple (`__min`, `__max`, `__null`) to a quad (`__min`, `__max`, `__null`, `__n_distinct`).

### 2.3 Null Adjustment

polars `n_unique()` counts `null` as a distinct value. To report non-null distinct values:

```rust
let n_unique_raw: i64 = /* parsed from {name}__n_distinct column */;
let null_count: i64   = /* parsed from {name}__null column */;
let n_distinct = n_unique_raw - if null_count > 0 { 1 } else { 0 };
```

Invariant: `null_count >= 0` (polars guarantees this). `n_distinct` will be ≥ 0 because if all values are null, `n_unique()` = 1 and `null_count` > 0, giving 0.

### 2.4 Parsing Helper Pattern

In the print loop (after collecting the DataFrame), read `n_distinct` the same way `min`, `max`, `null` are read:

```rust
let n_distinct_str = stats_df
    .column(&format!("{name}__n_distinct"))
    .ok()
    .and_then(|s| s.get(0).ok())
    .and_then(|v| match v {
        AnyValue::Int64(n) => Some(n),
        _ => None,
    })
    .map(|raw| raw - if null_count_i64 > 0 { 1 } else { 0 })
    .map(|n| n.to_string())
    .unwrap_or_else(|| "?".to_string());
```

`null_count_i64` must be parsed as `i64` (not just formatted as string) before the subtraction. Parse it first, then format `nulls={}` and compute `n_distinct`.

### 2.5 Output Format

Append `n_distinct=N` at the end of the existing per-column scan-stats line:

```
VendorID → min=1 max=6 nulls=0 n_distinct=2
payment_type → min=1 max=4 nulls=0 n_distinct=4
store_and_fwd_flag → min=N max=Y nulls=0 n_distinct=2
tip_amount → min=0.0 max=450.0 nulls=0 n_distinct=3573
```

Full format string (replacing the existing `println!` in `inspect.rs:174`):

```rust
println!("    {} → min={} max={} nulls={} n_distinct={}",
    name, min_str, max_str, null_str, n_distinct_str);
```

### 2.6 Cost Warning

The existing stderr warning `warning: --scan-stats reads the full file` is emitted before `compute_scan_stats` is called and covers `n_unique` implicitly — no additional warning needed. The design doc already notes that high-cardinality string columns can be slow; this is a data-engineer expectation.

### 2.7 Acceptance Criteria

- [ ] `pqls -d --scan-stats sample.parquet` output includes `n_distinct=N` on every column line.
- [ ] For a column with all identical values, `n_distinct=1`.
- [ ] For a column with all-null values, `n_distinct=0`.
- [ ] For a column with 3 distinct non-null values and 2 nulls, `n_distinct=3` (null not counted).
- [ ] Existing `min=`, `max=`, `nulls=` values are unchanged by this addition.
- [ ] No new CLI flag is introduced.

---

## Cross-Cutting Notes

### Shared helper: `format_field_type`

Both `schema.rs` and the new `diff.rs` need the same canonical type string. Extract from `schema.rs`:

```rust
// src/schema.rs — already pub
pub fn format_field_type(col: &ColumnDescriptor) -> String {
    match get_logical_type_str(col) {
        Some(lt) => format!("{:?} {}", col.physical_type(), lt),
        None     => format!("{:?}", col.physical_type()),
    }
}
```

`diff.rs` calls `schema::format_field_type(col)` directly. This avoids duplicating the physical+logical formatting logic.

### Exit-code consistency

| Command | Success exit | Meaningful non-zero exits |
|---------|-------------|--------------------------|
| `--diff` identical | 0 | 1=differ, 2=I/O, 3=args |
| `--diff` differ | 1 | — |
| `--check` valid | 0 | 1=invalid, 3=args |
| all other modes | 0 | 2=I/O, 3=args |

This is consistent with the existing pattern in `main.rs`.
