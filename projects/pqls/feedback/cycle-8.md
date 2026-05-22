# Cycle-8 Dogfood Notes

Fixture: `/tmp/yellow_tripdata.parquet` — NYC taxi 2024-01 (47 MiB, 2,964,624 rows, 19 columns, pyarrow-written)

Binary: `target/release/pqls` built from branch `cycle-8` (built 2026-05-22).

---

## Pass/Fail Table

| # | Priority | Check | Command | Result | Notes |
|---|----------|-------|---------|--------|-------|
| 1 | P1-A | `--kv-meta` shows ARROW:schema field names | `pqls --kv-meta yellow_tripdata.parquet` | **PASS** | Shows `(decoded Arrow schema, 19 fields)` with all column names and types |
| 2 | P1-A | Fallback shows byte count when decode fails | `pqls --kv-meta /tmp/bad_arrow.parquet` (file with invalid ARROW:schema value) | **PASS** | Outputs `ARROW:schema	(binary, 8 bytes)` |
| 3 | P1-B | `--help` exits 0 | `pqls --help; echo $?` | **PASS** | Exits 0; full help text emitted |
| 4 | P1-B | `--version` exits 0 | `pqls --version; echo $?` | **PASS** | Exits 0; prints `pqls 0.1.0` |
| 5 | P1-B | Unknown flag exits non-zero | `pqls --bad-flag; echo $?` | **PASS** | Exits 2 with "unexpected argument" error |
| 6 | P2 | Same-file diff: `.added` is `[]` | `pqls --diff A A --json \| jq '.added'` | **PASS** | Returns `[]` |
| 7 | P2 | Same-file diff: `.identical` is `true` | same | **PASS** | Returns `true` |
| 8 | P3 | `--scan-stats` shows elapsed time | `pqls -d --scan-stats yellow_tripdata.parquet 2>&1 \| grep 'scan completed'` | **PASS** | `scan completed in 0.2s` |
| 9 | P4 | `--columns` projects NDJSON | `pqls --ndjson --columns VendorID,fare_amount --head 3 yellow_tripdata.parquet` | **PASS** | Outputs 3 NDJSON rows with exactly those two columns |
| 10 | P4 | `--columns` projects CSV | `pqls --csv --columns VendorID,fare_amount --head 3 yellow_tripdata.parquet` | **PASS** | Outputs CSV header + 3 data rows |
| 11 | P4 | Unknown column exits 2 with error | `pqls --ndjson --columns xyz yellow_tripdata.parquet; echo $?` | **FAIL** | Exits 3, not 2; error message is correct |

**Summary: 10 PASS, 1 FAIL (P4)**

---

## Raw Output for Failures

### Check 11: Unknown column exit code

```
$ ./target/release/pqls --ndjson --columns xyz /tmp/yellow_tripdata.parquet; echo $?
error: unknown column: "xyz"; valid columns: VendorID, tpep_pickup_datetime, ...
3
```

Expected exit code 2 (caller-visible usage error). Got exit code 3 (pqls internal convention for "bad argument"). The error message itself is excellent — lists all valid columns.
The spec says exit 2 for unknown columns. Code in `ndjson_dump.rs:25` and `csv_dump.rs:20` both call `std::process::exit(3)`. Either the spec should be updated to say 3, or the exit code should be changed to 2.

---

## Friction Observed This Session

1. **Unknown-column exit code mismatch.** The acceptance check expected exit 2 for an unknown column name. The code consistently uses exit 3 for "bad argument" class errors (missing file, unknown column, incompatible flags). This is internally consistent but conflicts with the dogfood spec. Recommend: either adopt exit 2 for all user-input validation errors (files, columns, flag combos) to match standard Unix convention (exit 1 = generic error, exit 2 = usage error), or update the spec to document exit 3 as the pqls convention. The current mix (clap exits 2 for unknown flags, pqls exits 3 for unknown columns) is confusing.

2. **Check 2 required a synthetic fixture.** There is no bundled parquet file with an invalid ARROW:schema entry to exercise the fallback path. Added `examples/gen_bad_arrow.rs` as a helper to generate one. Consider committing a small fixture file (`tests/fixtures/bad_arrow_meta.parquet`) so the CI unit tests can cover this path without building a generator.

3. **`--scan-stats` scan time is very fast (0.2s) on this fixture.** The NYC taxi file has embedded row-group statistics, so `--scan-stats` reads only metadata — not the full file. The timing confirms the fast path works. A fixture without embedded stats would be needed to test the full scan path.

4. **All cycle-7 P1 blockers resolved.** The Arrow IPC decode fix (strip 8-byte IPC envelope, try bare-schema path) is working correctly. `--help` exits 0. Identical-diff JSON now includes `added`/`removed`/`changed` as empty arrays. `--scan-stats` reports elapsed time.

---

## Carry-Forward Items (ranked by impact)

| Rank | Item | Impact |
|------|------|--------|
| 1 | Unify exit codes: adopt exit 2 for user-input validation errors (unknown column, bad file path) OR document exit 3 as pqls convention | Medium — scripts using `; echo $?` will get wrong value |
| 2 | Add bundled fixture with bad ARROW:schema for CI coverage of fallback path | Low — currently only verified with gen_bad_arrow helper |
| 3 | Add fixture without embedded row-group stats to test full `--scan-stats` scan path | Low — current fixture exercises fast path only |
