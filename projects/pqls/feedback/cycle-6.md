# Cycle-6 Dogfood Notes

Fixture: `/tmp/yellow_tripdata.parquet` — NYC taxi 2024-01 (47 MiB, 2,964,624 rows, 3 row groups, 19 columns)

Binary: `target/release/pqls` built from branch `cycle-6`.

---

## Pass/Fail Table

| # | Test | Command | Result | Notes |
|---|------|---------|--------|-------|
| 1 | kv-meta decode fixed | `pqls --kv-meta yellow_tripdata.parquet` | **PASS** | No "failed" in output; shows `ARROW:schema\t(binary, 1568 bytes)` |
| 2 | scan-stats float format | `pqls -d --scan-stats yellow_tripdata.parquet` | **PASS** | Integer min/max show without `.0` (e.g. `min=-899 max=5000`) |
| 3 | n_distinct present | same command as #2 | **PASS** | `n_distinct=N` on every column |
| 4 | NDJSON head 5 | `pqls --ndjson --head 5 yellow_tripdata.parquet` | **PASS** | 5 valid JSON objects, exit 0 |
| 5 | diff identical | `pqls --diff A A` | **PASS** | No stdout, exit 0 |
| 6 | diff text (divergent) | `pqls --diff yellow_tripdata.parquet smoke_test.parquet` | **PASS** | Shows `-` removed / `+` added lines, exit 1 |
| 7 | diff JSON | `pqls --diff --json yellow_tripdata.parquet smoke_test.parquet` | **PASS** | Valid JSON with `added`, `removed`, `changed`, `identical` keys, exit 1 |
| 8 | check | `pqls --check yellow_tripdata.parquet` | **PASS** | Silent, exit 0 |

All 8 checks pass.

---

## Friction Notes

### 1. `--kv-meta` Arrow schema decode silently fails (low severity)
The Arrow IPC schema embedded in `ARROW:schema` kv-metadata falls back to `(binary, 1568 bytes)` instead of showing decoded fields. The base64 decode or flatbuffer parse is failing for this Arrow-written file. The "1568 bytes" reports the base64 string length, not the decoded bytes length — slightly misleading. The fix for the word "failed" is correct, but the root cause (why decode fails) is unresolved. Likely a base64 URL-safe vs standard mismatch, or Arrow IPC version difference.

### 2. `--scan-stats` has no progress indicator (medium friction)
Reading 2.9M rows / 47 MiB takes ~25s. There is no indicator — the terminal appears frozen until completion. The `warning: --scan-stats reads the full file` goes to stderr immediately but then silence. For larger files this would feel broken. A streaming row-count or elapsed-time heartbeat on stderr would help.

### 3. `--help` flag descriptions are empty (medium severity)
Every flag has an empty description in `--help` output. Users see `--scan-stats` with no explanation. The `long_about` examples in the top-level help partially compensate, but the per-flag descriptions are blank. This makes `--help` almost useless for understanding new flags like `--scan-stats`, `--deep`, `--diff`.

### 4. `--diff` JSON encodes type as a compound string (low severity)
The JSON diff output uses `"type": "INT64 TIMESTAMP_MICROS"` — a freeform string concatenating physical and logical types. For programmatic consumption (e.g. an agent comparing schemas), a structured representation `{"physical": "INT64", "logical": "TIMESTAMP_MICROS"}` would be easier to parse. Right now splitting on space is fragile (`BYTE_ARRAY STRING` has two tokens, `INT64` alone has one).

### 5. `--scan-stats` timestamp min/max strip timezone marker inconsistently (low severity)
Output shows `min="2002-12-31T22:59:39Z" max="2024-02-01T00:01:15Z"` — values are quoted (polars AnyValue::to_string() wraps strings in quotes). All other types are unquoted. Inconsistent presentation; a pipeline counting on string format will break on date columns.

### 6. `--diff` treats all-columns-different as full remove+add (cosmetic)
When two completely unrelated schemas are compared, every column appears as removed and re-added rather than a message like `schemas share no common columns`. Not wrong, but verbose and hard to scan for very wide schemas.

### 7. Partial-download detection (edge case, notable)
During testing, the file was still downloading when the first `--kv-meta` command was issued. It failed with `Parquet error: Invalid Parquet file. Corrupt footer` — expected, since the parquet footer lives at the end of the file. The error message is accurate but not user-friendly; something like `cannot read parquet footer — file may be incomplete` would be clearer.

---

## Carry-Forward Items (ranked by impact)

| Rank | Item | Impact |
|------|------|--------|
| 1 | Add flag descriptions to `--help` | High — every new user hits this |
| 2 | Fix Arrow schema decode in `--kv-meta` | Medium — core feature, fails silently |
| 3 | Timestamp values in scan-stats should not be quoted | Medium — breaks downstream parsing |
| 4 | `--scan-stats` progress indicator on stderr | Medium — large-file UX |
| 5 | `--diff` JSON: structured type representation | Low — agent readability |
| 6 | `--diff` cosmetic: "no shared columns" short-circuit message | Low — cosmetic |
