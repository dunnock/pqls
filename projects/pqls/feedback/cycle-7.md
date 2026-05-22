# Cycle-7 Dogfood Notes

Fixture: `/tmp/yellow_tripdata.parquet` — NYC taxi 2024-01 (47 MiB, 2,964,624 rows, 3 row groups, 19 columns)

Binary: `target/release/pqls` built from branch `cycle-7` (built 2026-05-22 22:54).

---

## Pass/Fail Table

| # | Priority | Check | Command | Result | Notes |
|---|----------|-------|---------|--------|-------|
| 1 | P1 | `--scan-stats` help is non-empty | `pqls --help \| grep -A1 '\-\-scan-stats'` | **PASS** | Full description including "Requires -d/--detail" |
| 2 | P1 | `--diff` help explains two-path contract | `pqls --help \| grep -A1 '\-\-diff'` | **PASS** | "Requires two path arguments. Use --json for machine-readable output." |
| 3 | P1 | All flags have non-empty descriptions | `pqls --help` | **PASS** | 18 flags audited, all have descriptions |
| 4 | P2 | Timestamp min= without double-quotes | `pqls -d --scan-stats yellow_tripdata.parquet` | **PASS** | `min=2002-12-31T22:59:39Z` — no surrounding quotes |
| 5 | P3 | ARROW:schema shows field names | `pqls --kv-meta yellow_tripdata.parquet` | **FAIL** | Still shows `(binary, 1176 bytes)` — IPC parse fails after base64 decode |
| 6 | P3-fallback | Output shows decoded byte count (not base64 length) | same | **PASS** | 1176 bytes displayed (base64 string is ~1568 chars — cycle-6 bug fixed) |
| 7 | P4 | Same-file diff: `.added` empty array, `.identical` true | `pqls --diff A A --json \| jq '.added'` | **PARTIAL** | `.identical` is `true` ✓; `.added` is `null` (key absent) not `[]` |
| 8 | P4 | Different-file diff: `.added[0].type.physical` is bare string | `pqls --diff A B --json \| jq '.added[0].type.physical'` | **PASS** | Returns `"INT64"` — structured type repr working |

**Summary: 6 PASS, 1 FAIL (P3), 1 PARTIAL (P4)**

---

## Raw Output for Failures

### Check 5: P3 ARROW:schema decode

```
$ ./target/release/pqls --kv-meta /tmp/yellow_tripdata.parquet
ARROW:schema	(binary, 1176 bytes)
```

Base64 decode succeeds (1176 bytes proven by output), but `root_as_message(&bytes)` in
`kv_meta.rs:26` returns an error or `header_as_schema()` returns `None`. The flatbuffer
parse path added in cycle-7 is not matching the format the NYC taxi writer uses.

### Check 7: P4 identical diff JSON shape

```
$ ./target/release/pqls --diff /tmp/yellow_tripdata.parquet /tmp/yellow_tripdata.parquet --json
{"identical":true}
```

`jq '.added'` → `null` (key not present). The contract should be `{"identical":true,"added":[],"removed":[],"changed":[]}` so consumers can unconditionally iterate `.added[]` without null-guarding.

---

## Friction Observed This Session

1. **`--help` exits with code 2, not 0.** Running `pqls --help && echo ok` prints the help but `echo ok` never fires. Standard convention (GNU, POSIX) is exit 0 for `--help`. This is a clap default that can be fixed with `.arg_required_else_help(false)` or by catching the help-print event.

2. **P3 Arrow IPC decode still fails despite cycle-7 attempt.** The `arrow_ipc::root_as_message` path was added correctly but the NYC taxi parquet file written by pyarrow uses a bare Schema flatbuffer (no Message wrapper), not a full IPC Message envelope. The fix needs to try `root_as_schema_buffer(&bytes)` (from `arrow_ipc::convert`) as a fallback when `root_as_message` fails.

3. **Identical-diff JSON omits `added`/`removed`/`changed` keys.** When schemas match, consumers get `{"identical":true}` with no `added` key — `jq '.added // empty'` or a null-guard is required. Emitting `{"identical":true,"added":[],"removed":[],"changed":[]}` costs nothing and makes scripts simpler.

4. **`--scan-stats` warning goes to stderr but no timing feedback.** The warning "warning: --scan-stats reads the full file" appears immediately; then ~25 s of silence before output. The user cannot distinguish a slow scan from a hung process. A single elapsed-time line on stderr after completion (e.g., `scan completed in 24.8s`) would close this gap.

---

## Carry-Forward Items (ranked by impact)

| Rank | Item | Impact |
|------|------|--------|
| 1 | Fix P3: try `root_as_schema_buffer` fallback after `root_as_message` fails | High — core kv-meta feature still broken for pyarrow-written files |
| 2 | Fix P4: emit `added`/`removed`/`changed` as empty arrays in identical-diff JSON | Medium — breaks scriptable consumers without null-guard |
| 3 | Fix `--help` exit code 2 → 0 | Medium — breaks `pqls --help && …` idioms, surprising for users |
| 4 | `--scan-stats` elapsed-time line on stderr | Low — UX only |
