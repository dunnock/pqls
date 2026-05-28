# pqls

![Release](https://github.com/dunnock/pqls/actions/workflows/release.yml/badge.svg)
![Latest Release](https://img.shields.io/github/v/release/dunnock/pqls)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)

A command-line tool for listing the contents and metadata of Apache Parquet files and partitioned parquet datasets, modelled on HDF5's `h5ls`.

## Install

```sh
curl -fsSL https://github.com/dunnock/pqls/releases/latest/download/install.sh | sh
```

### Or install with cargo:

```sh
cargo install pqls
```

## Examples

**Inspect a single file:**
```sh
pqls data.parquet
```

**Detailed stats (per-column min/max/nulls):**
```sh
pqls -d data.parquet
```

**Dump as CSV:**
```sh
pqls --csv data.parquet
pqls --csv --head 100 data.parquet
```

**List a partitioned dataset:**
```sh
pqls /path/to/dataset/
pqls -d -r /path/to/dataset/
```

**Machine-readable output:**
```sh
pqls -q data.parquet
```

## CLI

```
pqls [OPTIONS] <PATH> [PATH_B]

ARGS:
  <PATH>            path to a .parquet file or directory to inspect
  [PATH_B]          second .parquet file for schema diff (required by --diff)

OPTIONS:
      --diff                compare schemas of two files; exits 0 if identical, 1 if different
  -d, --detail              show per-row-group column statistics (min/max/nulls)
  -r, --recursive           recurse into a directory and list all .parquet files
      --csv                 dump rows as CSV to stdout
      --head <N>            limit output to the first N rows (applies to --csv and --ndjson)
  -q, --quiet               suppress human-readable headers; emit tab-separated summary lines
      --schema              print schema only (column names and types)
      --json                emit output as JSON (works with --schema, --kv-meta, --check, --partition-stats, --diff)
      --ndjson              stream rows as newline-delimited JSON (NDJSON)
      --sample <N>          emit N randomly-sampled rows; requires --ndjson or --csv
      --columns <COLS>      comma-separated list of column names to project (e.g. id,ts,value)
      --kv-meta             print Parquet key-value metadata (writer version, custom properties)
      --scan-stats          scan the full file to compute per-column min/max/nulls/n_distinct; requires -d
      --partition-stats     aggregate row counts and file sizes across a Hive-partitioned directory; requires -r
      --check               verify file integrity by reading the footer and all row groups
      --deep                with --check: read every data page (slower but catches corrupt column data)
  -h, --help                print help
  -V, --version             print version
```

## Why pqls?

**Static binary.** No JVM, no Python interpreter, no `pip install`. Drop the binary on
any Linux box and it runs — sub-100ms startup on the critical path of a data pipeline.

**Composable.** Stdout is always clean (data only; warnings go to stderr). Pipe anywhere:

```sh
pqls --csv file.parquet | xsv stats
pqls --schema file.parquet | diff - expected.schema
```

**Agent-friendly.** Machine-readable `--schema --json` and `--ndjson` output let code
agents inspect schema and rows without parsing human text. See `SKILL.md` for patterns.

**One-liner install:**

```sh
curl -fsSL https://github.com/dunnock/pqls/releases/latest/download/install.sh | sh
```

**Fast:**

| Tool | Runtime | Startup | Schema dump | Stats | Pipe-composable |
|------|---------|---------|-------------|-------|-----------------|
| **pqls** | none (static) | ~50ms | `--schema --json` | `--scan-stats` | yes |
| parquet-tools | JVM | ~2s | text only | yes | no |
| DuckDB | Go binary | ~200ms | SQL only | SQL | no |
| fastparquet | Python | ~500ms | Python API | Python API | no |

## How pqls compares

| | pqls | parquet-cli (Apache) | pqrs | DuckDB |
|---|---|---|---|---|
| Static binary, no JVM/Python | **yes** | no (JAR) | yes | yes |
| `--schema --json` for agents | **yes** | no (text only) | no | via SQL |
| NDJSON rows (`--ndjson`) | **yes** | no | cat -f json | via SQL |
| Column projection (`--columns`) | **yes** | yes | no | via SQL |
| Random sampling (`--sample N`) | **yes** | no | yes | ORDER BY random() |
| Key-value metadata (`--kv-meta`) | **yes** | footer cmd | no | parquet_kv_metadata() |
| Directory / partition listing | **yes** | no | no | no |
| SKILL.md for code agents | **yes** | no | no | no |
| Composable (stdin/stdout clean) | **yes** | no | partial | no |

pqls is the only static binary in this list that produces JSON schema output and
NDJSON rows without requiring SQL. It is designed for shell pipelines and agent
tooling where DuckDB's startup time or SQL syntax is overhead.

## Agent usage

pqls is designed to be called by code agents (Claude, Codex, Cursor, etc.)
without any human at the terminal.

### Discover schema

```shell
pqls --schema --json /path/to/foo.parquet
```

Returns a JSON object — safe to parse with `jq` or Python `json.loads`.
Field `logical_type` tells you `DATE`, `TIMESTAMP_MICROS`, `DECIMAL(10,2)`, etc.

### Sample rows to understand data

```shell
pqls --ndjson --sample 50 foo.parquet
```

50 rows, one JSON object per line. Pipe to `jq` for field inspection.

### Project specific columns

```shell
pqls --ndjson --columns user_id,amount --sample 20 foo.parquet
```

### Check embedded metadata (Spark / Pandas schema)

```shell
pqls --kv-meta --json foo.parquet | jq '.["pandas"]'
```

### Composable pipeline example

```shell
# Find which files in a partitioned dataset have more than 1M rows
pqls -q --recursive /data/events/ \
  | awk -F'\t' '$2 > 1000000 { print $1 }'
```

### Exit code contract

Scripts should test `$?`:
- `0` — success, output on stdout
- `1` — file/path error or schema mismatch (with --diff)
- `2` — corrupt or invalid parquet, or bad flag combination

## Releases

Pre-built binaries are attached to every [GitHub release](https://github.com/dunnock/pqls/releases):

| Platform | Asset |
|----------|-------|
| Linux x86_64 | `pqls-linux-x86_64.tar.gz` |
| Linux aarch64 | `pqls-linux-aarch64.tar.gz` |
| macOS Intel (x86_64) | `pqls-darwin-x86_64.tar.gz` |
| macOS Apple Silicon (aarch64) | `pqls-darwin-aarch64.tar.gz` |
| Windows x86_64 | `pqls-windows-x86_64.zip` |

**One-liner install (Linux and macOS):**
```sh
curl -fsSL https://github.com/dunnock/pqls/releases/latest/download/install.sh | sh
```

**Install from crates.io:**
```sh
cargo install pqls
```
This compiles from source and works on any platform with a Rust toolchain. No runtime
dependencies — pqls is a fully static binary on Linux (musl not required; glibc is fine).

**System requirements:** none beyond a standard Linux/macOS/Windows environment.
pqls does not require hugepages, elevated privileges, or any kernel tuning.

**`CRATES_IO_TOKEN` secret (maintainers only):** the release workflow publishes to
crates.io automatically on tag push. Add the secret once in the GitHub repo settings
under *Settings → Secrets and variables → Actions* with the name `CRATES_IO_TOKEN`.

## S3 paths

pqls accepts `s3://bucket/key` and `s3://bucket/prefix/` paths directly.

```sh
# inspect schema of a single S3 object (no full download)
pqls s3://my-bucket/events/2024/data.parquet

# JSON schema for agents / pipelines
pqls --schema --json s3://my-bucket/events/2024/data.parquet

# list all .parquet files under a prefix with brief schema per file
pqls s3://my-bucket/events/2024/

# machine-readable listing
pqls --json s3://my-bucket/events/2024/
```

### AWS auth

pqls uses the standard AWS credential provider chain — no new CLI flags.
Set credentials via environment variables, `~/.aws/credentials`, an IAM
instance role, or SSO:

```sh
# env vars
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_REGION=us-east-1

# named profile
export AWS_PROFILE=my-profile

# IAM role / ECS task role / IRSA — no config needed
```

### Trade-offs

- Schema inspection uses S3 range gets (last 64 KiB) — no whole-file
  download.
- No row data access over S3 (`--csv`, `--ndjson` are not supported for
  S3 paths).
- No local cache — every `pqls s3://...` issues fresh range gets.

## Releasing

```sh
# default: minor bump (e.g. 0.5.1 → 0.6.0)
make release

# or override the bump type:
make release BUMP=patch
make release BUMP=major

# preview what would happen — no side effects:
make release-dry-run
```

**Requirements:** run from your host (not a container), on the `main` branch with a clean
working tree that is in sync with `origin/main`.

`.github/workflows/release.yml` fires on tag push, builds multi-platform binaries, creates
the GitHub release, and publishes to crates.io — you do **not** need to do any of that
manually. `make release` commits, tags, and pushes; the workflow does the rest.

Recovery helpers (normally not needed):
- `make release-resume` — push + publish if the local commit/tag exist but push previously failed
- `make publish` — re-run `cargo publish` only (tag already on GitHub)

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
