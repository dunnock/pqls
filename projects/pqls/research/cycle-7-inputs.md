# pqls Cycle-7 Inputs

Distilled from: `projects/pqls/feedback/cycle-6.md`  
Prepared by: retro-cycle-6 distiller  
Date: 2026-05-22

---

## 1. Source / Fixture Attribution

| Item | Value |
|------|-------|
| Fixture | `/tmp/yellow_tripdata.parquet` — NYC taxi 2024-01 |
| Size | 47 MiB, 2,964,624 rows, 3 row groups, 19 columns |
| Binary | `target/release/pqls` built from branch `cycle-6` |
| Dogfood file | `projects/pqls/feedback/cycle-6.md` |

Arrow-written file (no per-column footer statistics, triggers `compute_scan_stats` path). This is the most realistic fixture to date; friction on it is representative of real data engineer workflows.

---

## 2. Pass/Fail Summary — Cycle 6

| # | Test | Result |
|---|------|--------|
| 1 | `--kv-meta` no "failed" in output | PASS |
| 2 | `--scan-stats` integer min/max no `.0` suffix | PASS |
| 3 | `--scan-stats` includes `n_distinct=N` on every column | PASS |
| 4 | `--ndjson --head 5` emits 5 valid JSON objects, exit 0 | PASS |
| 5 | `--diff A A` emits nothing, exit 0 | PASS |
| 6 | `--diff A B` (divergent) emits `-`/`+` lines, exit 1 | PASS |
| 7 | `--diff A B --json` emits valid JSON with correct keys, exit 1 | PASS |
| 8 | `--check` valid file: silent, exit 0 | PASS |

8/8 checks pass. No regressions.

---

## 3. Regressions / Partial Passes

### Partial: `--kv-meta` Arrow schema decode (cosmetic fix only)

- **What changed in cycle 6:** The word "failed" was removed from the fallback output. Now shows `ARROW:schema	(binary, 1568 bytes)`.
- **Root cause unresolved:** The underlying base64/flatbuffer decode of the Arrow IPC schema still fails. The byte count reported (1568) is the base64 string length, not the decoded byte length — misleading.
- **Likely cause:** Base64 URL-safe vs. standard alphabet mismatch, or Arrow IPC version incompatibility in the parsing code.
- **Impact:** Agents using `--kv-meta` to read Arrow-written schema metadata see a useless fallback instead of field names. Core feature, silently non-functional for the most common writer (Arrow/Polars/Pandas).

---

## 4. Carry-Forward Friction Items

### Friction A — Empty `--help` flag descriptions (must-fix)

Every flag has an empty description in `--help` output. Users see `--scan-stats` with no explanation; `--deep`, `--diff`, `--n_distinct` are undiscoverable. The top-level `long_about` examples partially compensate but are not machine-parseable.

**Impact:** Every first-time user and every agent doing tool discovery is blocked. This is the single biggest usability gap remaining.

### Friction B — `--scan-stats` timestamp values are quoted (must-fix)

`min="2002-12-31T22:59:39Z"` — polars `AnyValue::to_string()` wraps string-typed values in double-quotes. All other types (int, float) are unquoted. A pipeline parsing `min=` and `max=` values will break on timestamp columns.

**Impact:** Breaks composability. Any script extracting stats via grep/awk will get malformed values for timestamp columns.

### Friction C — `--kv-meta` Arrow schema decode silently returns binary fallback (must-fix)

See §3. The fallback `(binary, N bytes)` is uninformative. The correct fix is to resolve the base64 decode, not just polish the fallback label.

**Impact:** `--kv-meta` is a key inspection command; agents querying Arrow-written file metadata get no useful signal.

### Friction D — `--scan-stats` no progress indicator on stderr (nice-to-fix)

47 MiB / 2.9M rows takes ~25 s with no output. The upfront warning `warning: --scan-stats reads the full file` goes to stderr, then silence. For files >100 MiB this looks broken.

**Impact:** Medium UX friction for large files. Would not block adoption but causes user anxiety and spurious process kills.

### Friction E — `--diff --json` type is a freeform compound string (nice-to-fix)

JSON output: `"type": "INT64 TIMESTAMP_MICROS"` (space-separated physical + logical type). Splitting on space is fragile: `BYTE_ARRAY STRING` has two tokens, `INT64` alone has one.

**Impact:** Agent programmatic schema comparison is error-prone. Should be `{"physical": "INT64", "logical": "TIMESTAMP_MICROS"}` or `{"physical": "INT64"}` when no logical type.

### Friction F — `--diff` all-columns-different shows full remove+add (nice-to-fix)

When schemas share no columns, every column appears as removed from A and added from B. No summary message. For wide schemas (50+ columns) the output is hard to scan.

**Impact:** Cosmetic, but confusing when comparing structurally unrelated files.

### Friction G — Partial-download error message is accurate but unfriendly (nice-to-fix)

`Parquet error: Invalid Parquet file. Corrupt footer` when the file is still downloading. The footer lives at end-of-file; a still-downloading file triggers this legitimately. The message gives no hint.

**Impact:** Edge case but common in data pipeline contexts (streaming writes, S3 partial downloads). A hint like `— file may be incomplete or still downloading` reduces confusion.

---

## 5. Prioritised Proposals for Cycle 7

Ranking: agent-friendliness > composability > coverage > polish.

---

### P1. Add flag descriptions to `--help` (agent-friendliness, must-fix)

**Problem:** Every flag description is empty. `pqls --help` is useless for discovering what `--scan-stats`, `--diff`, `--deep`, `--kv-meta` do or when to use them.

**Proposed solution:** Add `help = "..."` strings to all `#[arg(...)]` attributes in `src/main.rs` (the clap struct). Cover: what the flag does, output format, and any notable interactions (e.g. `--scan-stats` requires `--detail`, `--diff` requires two paths).

**Implementation hint:** Clap uses the `help` attribute for short one-liners shown in `--help`, and `long_help` for the extended description shown by `--help --flag`. Prioritise `help` strings first; `long_help` can be a follow-on. The existing `long_about` on the struct shows good example commands — those can be trimmed once per-flag help is present.

**Acceptance criteria:**
- `pqls --help` shows a non-empty description for every flag.
- `pqls --help` | grep -A1 '\-\-scan-stats' shows a meaningful sentence describing the flag.
- `pqls --help` | grep -A1 '\-\-diff' explains the two-path contract.
- No flag has an empty or placeholder description.

---

### P2. Fix `--scan-stats` timestamp quoting (composability, must-fix)

**Problem:** `AnyValue::to_string()` for string-typed aggregated values (timestamps formatted as strings) includes surrounding double-quotes. Output: `min="2024-01-01T00:00:00Z"`. All other types are unquoted. Inconsistent and breaks downstream `awk`/`cut` parsing.

**Proposed solution:** In `inspect.rs`, after extracting the `AnyValue` for min/max, strip surrounding quotes when the value is `AnyValue::String(_)` before formatting the output line. Use `.trim_matches('"')` on the `to_string()` result, or match on `AnyValue::String(s)` directly and use `s` without formatting.

**Implementation hint:** The existing helper that formats min/max already pattern-matches on `AnyValue` variants for integer formatting (the cycle-6 fix). Extend that match to handle `AnyValue::String(s)` → format as `s` directly without calling `to_string()`.

**Acceptance criteria:**
- `pqls -d --scan-stats yellow_tripdata.parquet` for a timestamp column shows `min=2024-01-01T00:00:00Z` (no surrounding quotes).
- Integer and float columns are unchanged.
- A script `pqls -d --scan-stats foo.parquet | grep min= | cut -d= -f2` produces a bare value for all column types.

---

### P3. Fix `--kv-meta` Arrow IPC schema decode (agent-friendliness, must-fix)

**Problem:** Arrow-written files encode schema as a base64 Arrow IPC flatbuffer under key `ARROW:schema`. The current decode falls back to `(binary, N bytes)` where N is the base64 string length (not the decoded byte count). Root cause is likely a base64 alphabet mismatch (URL-safe vs. standard) or wrong IPC version.

**Proposed solution:**
1. Use standard base64 decode (`base64::engine::general_purpose::STANDARD`), then try URL-safe (`URL_SAFE`) on failure — Arrow's Java and C++ implementations use standard; some Spark writers use URL-safe.
2. After decoding, parse the Arrow IPC schema message using the `arrow-schema` or `arrow-ipc` crate. Extract field names and data types and display them as a field list.
3. If decode still fails after both attempts, display `(binary, N decoded bytes)` where N is the byte count of the base64-decoded bytes, not the base64 string length.

**Implementation hint:** The `arrow` crate family (already transitively present via `parquet`) provides `arrow_ipc::root_as_message` for flatbuffer parsing. Field names and types can be extracted from the resulting `Schema` message. If adding a direct `arrow-ipc` dependency is undesirable, print the decoded byte length as an improvement.

**Acceptance criteria:**
- `pqls --kv-meta yellow_tripdata.parquet` shows field names for `ARROW:schema` (e.g. `VendorID: INT64, tpep_pickup_datetime: TIMESTAMP_MICROS, ...`).
- If decode fails, the fallback reports the decoded byte count (not the base64 string length).
- `--kv-meta` still works on non-Arrow parquet files (those without `ARROW:schema` key).

---

### P4. `--diff --json` structured type representation (agent-friendliness, nice-to-fix)

**Problem:** `"type": "INT64 TIMESTAMP_MICROS"` is a freeform string. Agent code splitting on space to extract physical and logical types will fail: `BYTE_ARRAY STRING` has two tokens, `INT64` has one, `INT32 INT(32, true)` has two but the second contains spaces itself.

**Proposed solution:** Replace the `"type"` string field in the JSON diff output with a structured object:

```json
{"physical": "INT64", "logical": "TIMESTAMP_MICROS"}
```

For columns with no logical type:
```json
{"physical": "BYTE_ARRAY"}
```

In `diff.rs`, change `FieldDiff.type_str: String` to `FieldDiff` with separate `physical: String` and `logical: Option<String>` fields. Apply the same split to `FieldChanged.from`/`.to`.

**Implementation hint:** `schema::format_field_type` already splits physical and logical. Extract two separate values instead of formatting into one string. `serde_json` `#[serde(skip_serializing_if = "Option::is_none")]` handles the absent `logical` case cleanly.

**Acceptance criteria:**
- `pqls --diff A.parquet B.parquet --json` emits `{"physical": "...", "logical": "..."}` (or `{"physical": "..."}`) for each field in `added`, `removed`, and `changed`.
- `jq '.added[0].type.physical'` returns a bare type name without needing string splitting.
- Text output (`--diff` without `--json`) is unchanged.

---

### P5. `--scan-stats` elapsed-time heartbeat on stderr (composability, nice-to-fix)

**Problem:** Scanning 47 MiB takes ~25 s with no output. The initial warning goes to stderr then silence; the terminal appears frozen.

**Proposed solution:** Print elapsed-time updates to stderr every N seconds (e.g. 5 s) while `compute_scan_stats` is running. Use a background thread that sleeps in a loop and writes `\r[scan-stats] elapsed: Xs` to stderr. Join the thread when the scan completes and write a final `\n`.

**Implementation hint:** A simple `std::thread::spawn` with `std::time::Instant` and `thread::sleep(Duration::from_secs(5))` is sufficient. The main thread signals completion via an `Arc<AtomicBool>`. Alternatively, print a single "scanning…" line at start and a "done in Xs" line at end — simpler and avoids the carriage-return flicker.

**Acceptance criteria:**
- `pqls -d --scan-stats large.parquet` (>10 MiB) emits at least one progress line to stderr before completing.
- Progress output goes to stderr, not stdout (does not contaminate `|` pipelines).
- When the scan is fast (<3 s), no intermediate lines are printed.

---

### P6. `--diff` short-circuit for fully disjoint schemas (polish, nice-to-fix)

**Problem:** When schemas share no columns, all columns appear as removed from A and added from B. The output is long and hard to scan; no summary is provided.

**Proposed solution:** After categorising fields, if `removed.len() == fields_a.len() && added.len() == fields_b.len() && changed.is_empty()`, prepend a summary line `! schemas share no common columns (A: N, B: M)` before the field list. In JSON mode, add `"disjoint": true` at the top level.

**Acceptance criteria:**
- `pqls --diff A.parquet B.parquet` where A and B share no column names prints the `!` summary line followed by the full `-`/`+` list.
- JSON mode includes `"disjoint": true` when schemas are fully disjoint.
- Identical and partially-overlapping schemas are unaffected.

---

### P7. Clearer partial-download / corrupt-footer error message (polish, nice-to-fix)

**Problem:** `Parquet error: Invalid Parquet file. Corrupt footer` gives no hint that the file might be incomplete. Data engineers downloading large files or working with streaming writes hit this frequently.

**Proposed solution:** In the error handler in `main.rs`, check if the error message contains `Corrupt footer` and, if so, append `— file may be incomplete or still being written` to the stderr output before exiting 2.

**Implementation hint:** `anyhow::Error`'s `to_string()` / `chain()` can be inspected for the substring. A simple `if err_str.contains("Corrupt footer")` guard in the `unwrap_or_else` closure is sufficient; no dependency changes needed.

**Acceptance criteria:**
- Reading a truncated parquet file (e.g. `head -c 1024 real.parquet > /tmp/truncated.parquet`) exits 2 with the hint in stderr.
- Reading a legitimately corrupt file still exits 2 (exit code unchanged).
- Normal successful reads are unaffected.

---

## 6. Open Design Questions for the Cycle-7 Planner

**Q1. Arrow IPC decode dependency scope (for P3).**  
The `arrow-ipc` flatbuffer parser may pull in significant transitive dependencies. Is it acceptable to add it, or should P3 be scoped to: (a) fix the base64 byte-count reporting only, and (b) decode to a field list as a stretch goal?

**Q2. `--scan-stats` progress indicator approach (for P5).**  
Two options: (a) continuous heartbeat with carriage-return overwrite (cleaner UX, more complex), or (b) single start/end pair `scanning columns…` / `done in Xs` (simpler, less informative). Which matches the UNIX aesthetic better?

**Q3. Coverage: next unimplemented use case.**  
Cycle 6 closed `--diff` and `n_distinct`. Remaining prominent gaps from the project mission:
- `--sample N` — random row sampling (agent/exploration use case)
- Partition pruning preview — show which row groups match a predicate
- Column projection in dump (`--columns` flag is present but not yet wired to NDJSON/CSV output)

Should one of these be the coverage target for cycle 7, or should cycle 7 focus entirely on quality (P1–P4)?

**Q4. `--diff --json` backwards compatibility (for P4).**  
Changing `"type"` from a string to an object is a breaking change for any consumer of the current JSON format. The tool is pre-v1 so breaking changes are acceptable, but should the new format be documented in SKILL.md before shipping?

**Q5. `--help` flag descriptions — scope.**  
Should the cycle-7 coder also add `long_help` (extended per-flag descriptions) and examples, or just `help` (one-liners)? The former is more useful for human users; the latter is sufficient for agent tool discovery.
