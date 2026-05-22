# pqls

A command-line tool for listing the contents and metadata of Apache Parquet files and partitioned parquet datasets, modelled on HDF5's `h5ls`.

## Install

```sh
curl -fsSL https://github.com/dunnock/pqls/releases/latest/download/install.sh | sh
```

## Usage

```
pqls [OPTIONS] <PATH>

ARGS:
  <PATH>            file or directory

OPTIONS:
  -d, --detail      per-row-group stats, per-column min/max/nulls, partition layout
  -r, --recursive   recurse into subdirectories
      --csv         dump file contents as CSV to stdout
      --head <N>    with --csv, output only first N rows (0 = all)
  -q, --quiet       suppress decorative headers (machine-readable)
  -h, --help
  -V, --version
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
- `1` — file/path error
- `2` — corrupt or invalid parquet
- `3` — bad flag combination
- `4` — internal bug (report at github.com/dunnock/pqls/issues)

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
