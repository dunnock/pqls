# pqls — Agent Skill File

## When to use

Reach for pqls to inspect local `.parquet` files: schema discovery, row
sampling, column stats, key-value metadata, partition listing.
Use DuckDB for SQL queries; pqls is read-only.

## Input contract

    pqls [FLAGS] <FILE>        # single file
    pqls -r <DIR>              # partitioned dataset (recursively)

Local paths only. No remote (s3://, gs://) support.

## Output contract

- default inspect: human-readable text to stdout
- `--schema`: `NAME TYPE [LOGICAL]` one per line
- `--schema --json`: single JSON object
- `--ndjson`: NDJSON, one JSON object per row
- `--csv`: RFC-4180 CSV with header row
- `--kv-meta`: `key\tvalue` lines; with `--json`: JSON object
- `--quiet` / `-q`: tab-separated, no headers (scriptable)

Stderr: warnings and errors only.

## Exit codes

- `0` success
- `1` file not found, not readable, or invalid parquet
- `2` bad argument combination (user error)

## Agent patterns

```sh
# 1. Schema dump to JSON
pqls --schema --json file.parquet

# 2. Sample 10 rows as NDJSON
pqls --ndjson --sample 10 file.parquet

# 3. Schema diff between two files
diff <(pqls --schema a.parquet) <(pqls --schema b.parquet)

# 4. Column projection
pqls --csv --head 5 --columns col1,col2 file.parquet

# 5. First N rows as NDJSON (deterministic, no --sample):
pqls --ndjson --head 10 file.parquet
```

## --scan-stats

Reads the full file to compute per-column min, max, null count, and n_distinct.
`n_distinct` reports distinct non-null values: it subtracts 1 from Polars' n_unique when nulls are present, because Polars counts null as a distinct value.

## Gotchas
- `--json` requires `--schema` or `--kv-meta`; standalone is an error (exit 3).
- `--sample` is non-deterministic (no seed). Do not rely on row order.
- NDJSON: NaN/Inf → null. Binary columns → base64.
- Nested schemas are flattened to leaf columns.
