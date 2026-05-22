# pqls Cycle-9 Inputs

Distilled from: `projects/pqls/feedback/cycle-8.md`  
Prepared by: retro-cycle-8 distiller  
Date: 2026-05-22

---

## 1. Source / Fixture Attribution

| Item | Value |
|------|-------|
| Fixture | `/tmp/yellow_tripdata.parquet` — NYC taxi 2024-01 |
| Size | 47 MiB, 2,964,624 rows, 3 row groups, 19 columns |
| Binary | `target/release/pqls` built from branch `cycle-8` (2026-05-22) |
| Dogfood file | `projects/pqls/feedback/cycle-8.md` |

pyarrow-written Arrow-format file. This writer stores the schema as a bare Arrow flatbuffer Schema (no IPC Message envelope). All cycle-7 P1–P3 carry-forwards were resolved this cycle.

---

## 2. Pass/Fail Summary — Cycle 8

| # | Priority | Test | Result |
|---|----------|------|--------|
| 1 | P1-A | `--kv-meta` ARROW:schema shows field names | PASS |
| 2 | P1-A | Fallback shows byte count when decode fails | PASS |
| 3 | P1-B | `--help` exits 0 | PASS |
| 4 | P1-B | `--version` exits 0 | PASS |
| 5 | P1-B | Unknown flag exits non-zero | PASS |
| 6 | P2 | Same-file diff: `.added` is `[]` | PASS |
| 7 | P2 | Same-file diff: `.identical` is `true` | PASS |
| 8 | P3 | `--scan-stats` shows elapsed time | PASS |
| 9 | P4 | `--columns` projects NDJSON | PASS |
| 10 | P4 | `--columns` projects CSV | PASS |
| 11 | P4 | Unknown column exits 2 with error | **FAIL** |

**Summary: 10 PASS, 1 FAIL (P4)**

All cycle-7 P1–P3 blockers resolved. One P4 item partially fails: the feature works but exits 3 instead of the specified 2 for unknown column names.

---

## 3. Regressions / Partial Passes

### FAIL — P4: Unknown column exit code

**Symptom:** `pqls --ndjson --columns xyz foo.parquet` exits 3, not 2. The error message is correct and useful (lists all valid column names).

**Root cause:** `src/ndjson_dump.rs:25` and `src/csv_dump.rs:20` call `std::process::exit(3)` for "bad argument" class errors (unknown column, bad file path). Meanwhile, clap itself exits 2 for unknown flags. The mix of exit 2 (clap) and exit 3 (pqls) for user-input validation errors is internally inconsistent.

**Two resolution paths:**
1. Change `exit(3)` → `exit(2)` in `ndjson_dump.rs` and `csv_dump.rs` (and any other site that calls `exit(3)` for user-input errors) to match POSIX/GNU convention where exit 2 = usage error.
2. Alternatively, document exit 3 as pqls convention for user-input validation, and update the acceptance spec accordingly. This is only viable if all usage-error sites are consistently exit 3 (currently they are not — clap exits 2).

Recommend path 1: align all user-input validation errors to exit 2.

---

## 4. Carry-Forward Items (ranked by impact)

| Rank | Item | Impact |
|------|------|--------|
| 1 | **Unify exit codes**: change `exit(3)` → `exit(2)` for all user-input validation errors (unknown column, bad file path, incompatible flags) in `ndjson_dump.rs`, `csv_dump.rs`, and any other call sites | Medium — scripts using `; echo $?` or `|| die` will misfire on exit 3 |
| 2 | **Bundled bad-ARROW:schema fixture**: commit `tests/fixtures/bad_arrow_meta.parquet` (small file with invalid ARROW:schema value) so CI can cover the `--kv-meta` fallback path without running `examples/gen_bad_arrow.rs` | Low — CI coverage gap; currently only verified manually |
| 3 | **No-embedded-stats fixture**: add a fixture parquet file without embedded row-group statistics to test the full `--scan-stats` scan path (the current NYC taxi file has embedded stats, exercising only the fast metadata path) | Low — full scan path untested in CI |

---

## 5. Prioritised Proposals for Cycle 9

Ranking: correctness / composability > agent-friendliness > coverage.

---

### P1. Unify exit codes — 2 for all user-input validation errors

**Problem:** `pqls --ndjson --columns xyz foo.parquet` exits 3. Clap exits 2 for unknown flags. Scripts that check `[ $? -eq 2 ]` for usage errors get inconsistent results depending on which layer caught the error.

**Proposed solution:**  
Audit every `std::process::exit(N)` call site in the codebase. Change any call that signals a user-input validation error (unknown column, bad file path, incompatible flag combination) from exit 3 to exit 2. Internal / unexpected errors may keep a distinct code (e.g. 1 or 3), but those should be genuinely unexpected — not user mistakes.

Known sites at cycle-8:
- `src/ndjson_dump.rs:25`
- `src/csv_dump.rs:20`

**Acceptance criteria:**
- `pqls --ndjson --columns xyz foo.parquet; echo $?` prints `2`.
- `pqls --csv --columns xyz foo.parquet; echo $?` prints `2`.
- `pqls --bad-flag; echo $?` still prints `2` (clap, unchanged).
- `pqls --help; echo $?` still prints `0`.

---

### P2. Commit test fixtures for CI coverage of edge paths

**Problem:** Two code paths have no fixture-backed CI coverage:
1. `--kv-meta` fallback when ARROW:schema decode fails — requires a file with an invalid/truncated ARROW:schema value. Currently tested only with a locally generated file (`examples/gen_bad_arrow.rs`).
2. `--scan-stats` full scan path — requires a file with no embedded row-group statistics. The NYC taxi fixture has embedded stats, so the slow scan path is never exercised.

**Proposed solution:**
1. Generate `tests/fixtures/bad_arrow_meta.parquet` (few rows, intentionally corrupted ARROW:schema key value) and commit it. The `examples/gen_bad_arrow.rs` generator already exists; run it once to produce the fixture.
2. Generate `tests/fixtures/no_stats.parquet` (small file, written without row-group statistics) and commit it. A minimal Rust snippet using `parquet::file::writer` with stats disabled is sufficient.

Both fixtures should be ≤ 50 KiB.

**Acceptance criteria:**
- `pqls --kv-meta tests/fixtures/bad_arrow_meta.parquet` outputs `ARROW:schema	(binary, N bytes)` for some N > 0 without panicking.
- `pqls -d --scan-stats tests/fixtures/no_stats.parquet 2>&1 | grep "scan completed"` matches.
- Fixtures are committed under `tests/fixtures/` and referenced in at least one Rust integration test.

---

### P3. Update SKILL.md with current JSON contracts

**Problem:** SKILL.md documents pqls for code agents, but it does not reflect the JSON output contracts that shipped in cycle-7 and cycle-8: `--diff --json` shape (with `added`/`removed`/`changed`/`identical`), `--kv-meta` decoded field format, `--schema --json` structure. Agents must trial-and-error the output format instead of reading the contract.

**Proposed solution:**  
Extend `SKILL.md` with a "Machine-readable output" section. For each JSON-producing flag, add:
- The exact top-level keys and their types.
- A minimal example output block (≤ 10 lines).
- Exit codes for success and known error conditions.

Specifically document:
- `--diff --json`: `{"identical": bool, "added": [], "removed": [], "changed": []}`
- `--schema --json`: current output shape
- `--kv-meta` decoded format: `FIELD:value\t<decoded-type-string>` tab-separated

**Acceptance criteria:**
- SKILL.md has a "Machine-readable output" section with subsections for each JSON flag.
- Each subsection includes at least one example block.
- Exit code table covers at least: 0 (success), 2 (usage error), non-zero (internal error).

---

### P4. Coverage: `--sample N` — random row sampling

**Problem:** Agents and data engineers routinely want to peek at a representative slice of a large parquet file without reading all rows. `--head N` reads from the start; there is no random-sample path. This is the top unimplemented use case from cycle-7 Q4.

**Proposed solution:**  
Add `--sample N` flag that emits N rows sampled uniformly at random across the file. Implementation strategy:
1. Read total row count from metadata (free — already in the footer).
2. Generate N sorted random row indices using `rand::seq::index::sample`.
3. Use `ParquetRecordBatchReaderBuilder::with_row_selection` (arrow-rs) to read only those rows.

Output format follows existing `--ndjson` / `--csv` flags (i.e. `--sample N` selects rows; output format is controlled by the existing flags). Default output if no format flag is given: NDJSON (matches `--head` behaviour).

**Acceptance criteria:**
- `pqls --sample 10 foo.parquet | wc -l` prints `10`.
- `pqls --sample 10 --csv foo.parquet | wc -l` prints `11` (header + 10 rows).
- `pqls --sample 10 --columns VendorID foo.parquet` works (sample + projection compose).
- Two runs with different seeds produce different rows (probabilistic; skip in CI if flaky, test manually).
- `pqls --sample 0 foo.parquet; echo $?` exits 2 with an error (N must be ≥ 1).
- Works correctly when N > total row count (emits all rows, no crash).

---

## 6. Open Design Questions for the Cycle-9 Planner

**Q1. Exit code for internal/unexpected errors.**  
If exit 2 is adopted for all user-input errors, what exit code should genuine internal errors (IO failure, corrupt file, panic recovery) use? Options: exit 1 (generic POSIX), or keep exit 3 for internal errors to distinguish from usage errors. Recommend: exit 1 for internal errors, exit 2 for usage errors — matching the POSIX convention used by most Unix tools.

**Q2. `--sample` reproducibility / seed flag.**  
Should `--sample N` accept an optional `--seed S` for reproducible sampling? Useful for sharing a specific sample across team members, but adds surface area. Defer until P4 acceptance criteria are met; add as a stretch goal only if the implementation is trivial (it is, with `rand::SeedableRng`).

**Q3. Fixture size and format constraints.**  
The bad-ARROW:schema and no-stats fixtures need to be small (≤ 50 KiB) but representative. Confirm whether the repo already has a policy on binary fixture files (LFS vs. direct commit). If using LFS, the CI workflow must have `git lfs pull` before running tests.

**Q4. SKILL.md: agent-facing vs. human-facing sections.**  
SKILL.md currently blends human and agent audiences. The new "Machine-readable output" section is primarily for code agents. Consider a clear `## For agents` heading with the JSON contracts, separate from the `## For humans / CLI examples` section, so each audience can skip to their section.

**Q5. Partition pruning preview — cycle 9 stretch or cycle 10?**  
Partition pruning preview (show which row groups match a predicate) is the second unimplemented use case from cycle-7 Q4. It is more complex than `--sample N` (requires expression parsing and predicate pushdown). Defer to cycle 10 unless the planner judges it feasible after P1–P3 are clean.
