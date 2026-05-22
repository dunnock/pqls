# pqls — Agent Skill File

## When to use pqls

Use pqls when you need to inspect an Apache Parquet file from the command line
or within a shell pipeline. Reach for it when you want to:
- Discover the schema of an unknown .parquet file
- Sample rows to understand data shape without loading the full file
- Pipe rows into jq, xsv, or other UNIX tools
- Check file-level metadata (key-value, row counts, row groups)
- List all parquet files in a directory with aggregate stats

Do NOT use pqls for:
- SQL queries over parquet (use DuckDB)
- Writing or modifying parquet files
- Processing non-parquet formats

## Input contract

pqls accepts a single positional argument: a path to a .parquet file or a
directory containing .parquet files.

Input must be a local filesystem path. Remote paths (s3://, gs://) are not
supported in v1.

## Output contract

Default (no flags): human-readable text to stdout. Decorative headers present.
--quiet / -q: tab-separated values, no headers; stable for scripting.
--schema --json: single JSON object to stdout (see JSON schema section below).
--ndjson: one JSON object per row, newline-separated, to stdout.
--csv: RFC-4180 CSV with header row to stdout.
--kv-meta: tab-separated key\tvalue lines; with --json: JSON object.

stderr is reserved for warnings and errors. Stdout is clean for piping.

Exit codes:
  0  success
  1  file not found or not readable
  2  invalid parquet file (corrupt footer, wrong magic bytes)
  3  bad argument combination
  4  internal error (bug)

## JSON schema (--schema --json output)

{
  "file": "/absolute/path",
  "num_rows": 1048576,
  "num_row_groups": 4,
  "created_by": "parquet-mr version 1.13.0",  // null if absent
  "fields": [
    {
      "index": 0,
      "name": "col_name",
      "physical_type": "INT64",
      "logical_type": "TIMESTAMP_MICROS",       // null if absent
      "repetition": "REQUIRED"
    }
  ]
}

## Common patterns

# Inspect schema as JSON, extract field names
pqls --schema --json foo.parquet | jq '[.fields[].name]'

# Sample 100 rows as NDJSON, filter a column
pqls --ndjson --sample 100 foo.parquet | jq '.amount'

# Project two columns to CSV
pqls --csv --columns user_id,event_date foo.parquet | xsv stats

# Check key-value metadata (Spark/Hive/Pandas embedded schema)
pqls --kv-meta foo.parquet

# List all parquet files in a partitioned dataset
pqls --recursive /data/hive/events/

# Quick row count
pqls -q foo.parquet | cut -f2

## Gotchas

- --json requires --schema or --kv-meta; standalone --json is an error.
- --sample is non-deterministic (no seed in v1). Do not rely on row order.
- NDJSON NaN/Inf float values are emitted as null (JSON limitation).
- Binary columns are base64-encoded in NDJSON output.
- Nested parquet schemas are flattened to leaf columns in schema output.
