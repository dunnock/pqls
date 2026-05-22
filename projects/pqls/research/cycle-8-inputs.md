# pqls Cycle-8 Inputs

Distilled from: `projects/pqls/feedback/cycle-7.md`  
Prepared by: retro-cycle-7 distiller  
Date: 2026-05-22

---

## 1. Source / Fixture Attribution

| Item | Value |
|------|-------|
| Fixture | `/tmp/yellow_tripdata.parquet` — NYC taxi 2024-01 |
| Size | 47 MiB, 2,964,624 rows, 3 row groups, 19 columns |
| Binary | `target/release/pqls` built from branch `cycle-7` (2026-05-22) |
| Dogfood file | `projects/pqls/feedback/cycle-7.md` |

pyarrow-written Arrow-format file. This writer stores the schema as a bare Arrow flatbuffer Schema (no IPC Message envelope), which has consistently exposed the flatbuffer-parse path.

---

## 2. Pass/Fail Summary — Cycle 7

| # | Priority | Test | Result |
|---|----------|------|--------|
| 1 | P1 | `--scan-stats` help is non-empty | PASS |
| 2 | P1 | `--diff` help explains two-path contract | PASS |
| 3 | P1 | All flags have non-empty descriptions | PASS |
| 4 | P2 | `--scan-stats` timestamp min= without double-quotes | PASS |
| 5 | P3 | `--kv-meta` ARROW:schema shows field names | **FAIL** |
| 6 | P3-fallback | Fallback shows decoded byte count (not base64 length) | PASS |
| 7 | P4 | Same-file diff: `.added` empty array, `.identical` true | **PARTIAL** |
| 8 | P4 | Different-file diff: `.added[0].type.physical` bare string | PASS |

**Summary: 6 PASS, 1 FAIL (P3), 1 PARTIAL (P4)**

Cycle-7 cleared all P1 and P2 items. One P3 item (Arrow IPC decode) and one P4 item (identical-diff JSON shape) remain open.

---

## 3. Regressions / Partial Passes

### FAIL — P3: Arrow IPC Schema decode (`--kv-meta`)

**Symptom:** `pqls --kv-meta yellow_tripdata.parquet` still prints `ARROW:schema	(binary, 1176 bytes)` — field names are not decoded.

**What cycle-7 attempted:** Added `arrow_ipc::root_as_message(&bytes)` parse path in `src/kv_meta.rs:26`.

**Root cause:** pyarrow (and most Python/Pandas/Polars writers) serialises the Arrow schema as a *bare* `Schema` flatbuffer, not wrapped in an IPC `Message` envelope. `root_as_message` expects the envelope and returns an error / `None` on a bare Schema buffer. The fix must call `arrow_ipc::convert::try_decode_arrow_schema_from_bytes` or the equivalent `root_as_schema_buffer` path as a fallback (or primary path) when `root_as_message` fails.

**Evidence:** The 1176-byte decoded body length (correctly reported since cycle-6's fallback fix) confirms base64 decode succeeds; the parse step is the remaining gap.

### PARTIAL — P4: Identical-diff JSON shape

**Symptom:** `pqls --diff A A --json` emits `{"identical":true}`. `jq '.added'` returns `null` (key absent) instead of `[]`.

**Root cause:** The serialisation shortcut for the identical case only emits the `identical` key. Consumers must null-guard `.added // []` instead of unconditionally iterating `.added[]`.

**Impact:** Scriptable consumers need a null-guard; contracts stated in cycle-7-inputs §5 P4 acceptance criteria are not fully met.

---

## 4. Carry-Forward Friction Items

| Rank | Item | Impact |
|------|------|--------|
| 1 | **Arrow IPC decode**: try `root_as_schema_buffer` / `try_decode_arrow_schema_from_bytes` after `root_as_message` fails — core kv-meta feature still broken for pyarrow-written files | High |
| 2 | **`--help` exit code 2 → 0**: `pqls --help && echo ok` never prints `ok`; breaks standard shell idioms and CI check patterns | Medium |
| 3 | **Identical-diff JSON shape**: `{"identical":true}` omits `added`/`removed`/`changed` keys — consumers need null-guards | Medium |
| 4 | **`--scan-stats` no timing feedback**: 25 s of silence after upfront warning looks like a hang; a single elapsed-time line on stderr after completion is sufficient | Low |

---

## 5. Prioritised Proposals for Cycle 8

Ranking: composability / correctness > agent-friendliness > polish > coverage.

---

### P1-A. Fix Arrow IPC Schema decode — try bare-Schema path

**Problem:** `root_as_message` fails on pyarrow-written files because they use a bare Schema flatbuffer, not an IPC Message envelope. `--kv-meta` has been non-functional for the most common writer (Arrow/Polars/Pandas) for three cycles.

**Proposed solution:**  
In `src/kv_meta.rs`, after base64-decoding the `ARROW:schema` value, try two parse paths in order:

1. `arrow_ipc::convert::try_decode_arrow_schema_from_bytes(&bytes)` (or `Schema::decode_by_bytes` via `arrow_schema`) — handles the bare Schema flatbuffer written by pyarrow.
2. `arrow_ipc::root_as_message(&bytes)` followed by `.header_as_schema()` — handles the IPC Message envelope written by some C++ Arrow and Spark implementations.

If both fail, keep the existing `(binary, N decoded bytes)` fallback where N is the decoded (not base64) byte count.

**Format the output as:**
```
ARROW:schema    field_name: PhysType[/LogicalType], ...
```
one line per decoded schema entry, or collapsed onto one line for narrow schemas (< ~80 chars).

**Implementation hint:** `arrow_schema::Schema` (from the `arrow-schema` crate, likely already a transitive dep via `parquet`) provides `Schema::try_from(&fbs_schema)`. Alternatively, `parquet::arrow::arrow_reader::ArrowReaderBuilder` can read the schema without a separate decode step — but that reads the full file footer; prefer the lightweight flatbuffer parse.

**File:** `src/kv_meta.rs` — the decode block starting at approximately line 20–35.

**Acceptance criteria:**
- `pqls --kv-meta yellow_tripdata.parquet` shows field names (e.g. `VendorID: INT64, tpep_pickup_datetime: TIMESTAMP_MICROS, …`) for `ARROW:schema`.
- Fallback `(binary, N bytes)` still appears when both parse paths fail; N is the decoded byte count.
- `--kv-meta` works on non-Arrow parquet files (no `ARROW:schema` key).

---

### P1-B. Fix `--help` exit code (2 → 0)

**Problem:** `pqls --help` exits with code 2 (clap's default for help/version output). Standard POSIX and GNU convention is exit 0. The code-2 exit breaks `pqls --help && next-command` and confuses CI scripts that treat non-zero as failure.

**Proposed solution:**  
In `src/main.rs`, configure the clap `Command` with `.help_expected(true)` and override the help exit code via `.term_width(...)` or — more directly — catch `clap::error::ErrorKind::DisplayHelp` in the error handler and exit 0.

The simplest clap 4.x approach:
```rust
let cmd = Command::new("pqls")
    ...
    .version(...)
    .propagate_version(true);

let matches = cmd.try_get_matches().unwrap_or_else(|e| {
    if e.kind() == clap::error::ErrorKind::DisplayHelp
        || e.kind() == clap::error::ErrorKind::DisplayVersion
    {
        e.print().unwrap();
        std::process::exit(0);
    }
    e.exit(); // real errors still exit non-zero
});
```

**File:** `src/main.rs` — the argument-parsing entry point.

**Acceptance criteria:**
- `pqls --help; echo $?` prints `0`.
- `pqls --help && echo ok` prints `ok`.
- `pqls --version; echo $?` prints `0`.
- `pqls --unknown-flag; echo $?` still exits non-zero (2).

---

### P2. Fix identical-diff JSON shape

**Problem:** `pqls --diff A A --json` emits `{"identical":true}` without `added`, `removed`, or `changed` keys. Consumers must null-guard `.added // []`; the P4 acceptance criteria from cycle-7 stated `.added` should be `[]`.

**Proposed solution:**  
In `src/diff.rs` (or wherever the identical-case JSON is serialised), always emit the full struct:
```json
{"identical": true, "added": [], "removed": [], "changed": []}
```
This can be done by ensuring the `DiffOutput` struct always serialises all four fields (`#[serde(default)]` + `Vec<_>` fields default to empty vec).

**File:** `src/diff.rs` — the serialisation of `DiffOutput` for the identical case.

**Acceptance criteria:**
- `pqls --diff A A --json | jq '.added'` returns `[]` (not `null`).
- `pqls --diff A A --json | jq '.removed'` returns `[]`.
- `pqls --diff A A --json | jq '.identical'` returns `true`.
- Non-identical diff output is unchanged.

---

### P3. `--scan-stats` elapsed-time line on stderr

**Problem:** Scanning 47 MiB takes ~25 s. After the upfront `warning: --scan-stats reads the full file` on stderr, there is silence until completion. A user cannot distinguish a slow scan from a hung process.

**Proposed solution:**  
Print a single elapsed-time line to stderr after `compute_scan_stats` completes:
```
scan completed in 24.8s
```
Use `std::time::Instant::now()` before the scan call; compute elapsed in `.elapsed().as_secs_f64()` after.

Do **not** use a background thread or carriage-return ticker — the simpler start/end approach matches the UNIX aesthetic better (one diagnostic line, not a progress bar). If desired, also print `scanning columns…` to stderr before the scan starts so users see activity immediately.

**File:** `src/main.rs` or `src/inspect.rs` — the call site of `compute_scan_stats`.

**Acceptance criteria:**
- `pqls -d --scan-stats large.parquet 2>&1 | grep "scan completed"` matches on files that take >3 s.
- Output goes to stderr (does not contaminate `|` pipelines).
- Fast scans (<1 s) still emit the line (it just reads `scan completed in 0.Xs`).

---

### P4. Coverage: wire `--columns` projection to NDJSON / CSV output

**Problem:** The `--columns` flag is present in the CLI but not wired through to NDJSON (`--ndjson`) or CSV (`--csv`) output. Data engineers routinely dump a subset of columns; today they must dump all columns and post-filter with `jq` / `cut`.

**Proposed solution:**  
In `src/dump.rs` (or wherever `--ndjson` / `--csv` row emission happens), pass the `columns` vector from the parsed args to the row-group reader. With polars, this is `LazyFrame::select(cols)`. With arrow-rs, pass the column projection to `ParquetRecordBatchReaderBuilder::with_projection`.

**Acceptance criteria:**
- `pqls --ndjson --columns VendorID,fare_amount foo.parquet` emits NDJSON rows with only those two fields.
- `pqls --csv --columns VendorID,fare_amount foo.parquet` emits CSV with only those two columns (header row included).
- Unknown column names exit 2 with an informative error: `unknown column: 'xyz'`.
- `--schema` and `--scan-stats` are unaffected.

---

## 6. Open Design Questions for the Cycle-8 Planner

**Q1. Arrow IPC decode: which crate path to use?**  
Three options: (a) `arrow_schema` decode via `arrow-schema` crate (lightweight, likely already a transitive dep), (b) `arrow_ipc::convert::try_decode_arrow_schema_from_bytes` (more complete, explicit dep on `arrow-ipc`), (c) `parquet::arrow::arrow_reader::ArrowReaderBuilder::open` to extract the schema via the parquet metadata path (zero new deps, but reads footer rather than parsing the kv-meta bytes). Recommend (a) or (b); confirm which is already a transitive dep before adding new Cargo.toml entries.

**Q2. `--help` clap version specifics.**  
The fix depends on the clap version in `Cargo.toml`. In clap 4.x, `ErrorKind::DisplayHelp` is the right variant; in clap 3.x, it differs. Confirm the clap version before writing the fix.

**Q3. P3 scan-stats elapsed: print-before vs. print-after.**  
`scanning columns…` before + `done in Xs` after (two lines) vs. `scan completed in Xs` after only (one line). The former is more informative for interactive use; the latter is simpler and more script-friendly. Choose based on whether the target workflow is interactive or scripted.

**Q4. Coverage priority after `--columns` wiring.**  
After P4 (`--columns` projection), the next unimplemented use cases are:
- `--sample N` — random row sampling (agent/exploration use case)
- Partition pruning preview — show which row groups match a predicate
Both are non-trivial. Should cycle 8 add `--sample N` as a P4 stretch, or defer all new coverage until the carry-forward P1–P3 items are clean?

**Q5. SKILL.md update cadence.**  
The `--diff --json` structured type format shipped in cycle-7. SKILL.md should document the current JSON contracts so agents can consume them without trial-and-error. Should the cycle-8 coder update SKILL.md in the same pass, or is that a separate task?
