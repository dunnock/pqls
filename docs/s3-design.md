# S3 Support Design — pqls

## 1. Surface Unchanged

The existing `pqls <PATH>` interface gains no new flags. The `s3://` scheme is
auto-detected early in `main()` before any local-filesystem dispatch; everything
else falls through unchanged.

### Existing invocations that continue to work unmodified

```
# local file — unchanged
pqls foo.parquet
pqls --schema foo.parquet
pqls --schema --json foo.parquet
pqls --ndjson --sample 100 foo.parquet
pqls --csv --columns id,ts foo.parquet
pqls --kv-meta foo.parquet
pqls -r /data/events/

# schema diff — unchanged (two local paths)
pqls --diff a.parquet b.parquet
```

### New invocations enabled by this design

```
# single S3 object — schema dump without downloading the file
pqls s3://my-bucket/path/to/data.parquet
pqls --schema s3://my-bucket/path/to/data.parquet
pqls --schema --json s3://my-bucket/path/to/data.parquet

# S3 prefix (trailing slash) — listing with per-file brief schema
pqls s3://my-bucket/events/2024/
pqls --json s3://my-bucket/events/2024/    # JSON listing output
pqls --quiet s3://my-bucket/events/2024/   # tab-separated, no headers
```

### Detection rule

The `path` argument (currently `PathBuf`) will be accepted as a `String` first;
if `str::starts_with("s3://")` the S3 dispatch path is taken. Otherwise the
string is passed to `PathBuf::from()` and the existing local logic runs. This
requires changing `Cli::path` from `PathBuf` to `String` and inserting a
conversion before every existing local call-site — a two-line change per
call-site since all modules take `&std::path::Path`.

**Flags that work with S3 paths:**
- `--schema` / `--schema --json`: full schema dump via lazy footer fetch.
- `--json`: machine-readable listing output.
- `--quiet`: tab-separated listing, no headers.
- Default (no flags): same as `--schema` for a single object; listing for a prefix.

**Flags that do NOT apply to S3 paths** and will exit 2 with a clear message:
`--csv`, `--ndjson`, `--diff`, `--kv-meta`, `--check`, `--partition-stats`,
`--scan-stats`, `--sample`, `--head`, `--columns`.

---

## 2. Dependency Choice

### aws-sdk-s3 (chosen)

```toml
[dependencies]
aws-sdk-s3    = { version = "1.78", features = [] }
aws-config    = { version = "1.8",  features = ["behavior-version-latest"] }
aws-credential-types = "1.2"
tokio         = { version = "1",    features = ["rt-multi-thread", "macros"] }
```

**Why aws-sdk-s3 1.x:**
- Official AWS SDK for Rust, actively maintained by AWS.
- Generated from the AWS service model — API surface tracks S3 exactly.
- Built on `smithy-rs`; async-first with Tokio.
- Binary footprint: ~700 KB stripped on Linux x86_64 (measured against 1.78.0).
- `behavior-version-latest` on `aws-config` opts in to current SDK defaults
  (e.g., endpoint resolution v2); without it, the SDK emits a deprecation warning
  on every run. Always enable it on new projects.

**Why not rusoto_s3:**
- Unmaintained since 2022; last release 0.48.0 targets `tokio` 1.x but is
  otherwise frozen. No security patches. Incompatible with Rust editions > 2021
  in practice due to unmaintained transitive deps.
- The project's own README says "use aws-sdk-rust instead."

**Why not object_store:**
- The `object_store` crate (Apache Arrow project) is a thin abstraction over S3,
  GCS, and Azure Blob. It is excellent if multi-cloud support is planned. For
  pqls today, it adds an unnecessary indirection layer and its S3 feature still
  pulls in `aws-sdk-s3` transitively. Revisit in a later cycle if GCS/Azure
  support is requested.

**Tokio runtime:**
The current `pqls` binary is synchronous. Adding `aws-sdk-s3` requires an async
runtime. The approach: add `#[tokio::main]` to `main()` and wrap the S3 dispatch
branch in `async`. The existing synchronous local branches are unaffected — they
run inside the Tokio runtime but do not use `.await`. Binary size increase is
dominated by the SDK itself (~700 KB), not by the Tokio runtime (~100 KB).

---

## 3. Path Parsing

### Grammar

```
s3_url  = "s3://" bucket "/" key_or_prefix
bucket  = 1*(label ".")* label              -- DNS-compliant bucket name
label   = [a-z0-9][a-z0-9-]{1,61}[a-z0-9]
key     = *(%x00-FF)                        -- any S3 key bytes
```

### Dispatch rules

| Input                              | Parsed as                          | Mode         |
|------------------------------------|------------------------------------|--------------|
| `s3://bucket/path/file.parquet`    | bucket=`bucket`, key=`path/file.parquet` | single object |
| `s3://bucket/prefix/`             | bucket=`bucket`, prefix=`prefix/`  | directory listing |
| `s3://bucket/`                    | bucket=`bucket`, prefix=`""`       | directory listing (root) |
| `s3://bucket`                     | bucket=`bucket`, prefix=`""`       | directory listing (root) |

A trailing `/` unconditionally selects listing mode. A path ending in `.parquet`
(case-insensitive) with no trailing `/` selects single-object mode. Any other
non-slash-terminated path is also single-object mode; if the key does not exist
on S3, the error surfaces at fetch time.

### Rust type

```rust
pub enum S3Path {
    Object { bucket: String, key: String },
    Prefix { bucket: String, prefix: String },
}

impl S3Path {
    pub fn parse(raw: &str) -> Result<Self, S3PathError>;
}

pub enum S3PathError {
    MissingScheme,     // does not start with "s3://"
    EmptyBucket,       // "s3://" with nothing after
    InvalidBucket(String),  // bucket fails DNS label validation
}
```

`S3PathError` implements `std::error::Error` and `Display` with user-facing
messages (e.g. `"s3:// path has no bucket name"`).

### Validation

Bucket names are validated against S3 naming rules:
- 3–63 characters.
- Lowercase letters, digits, hyphens only.
- Cannot start or end with a hyphen.
- Cannot be an IP address (e.g. `192.168.1.1`).

An invalid bucket name is a hard error at parse time. Key/prefix content is not
validated locally — S3 accepts nearly any byte sequence.

---

## 4. Auth Provider Chain

### What `aws_config::defaults(BehaviorVersion::latest()).load().await` covers

The loader tries credentials in this order (first match wins):

1. **Environment variables** — `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY`
   (+ `AWS_SESSION_TOKEN` for temporary credentials).
2. **AWS profile** — `~/.aws/credentials` and `~/.aws/config`, profile selected
   by `AWS_PROFILE` (default: `"default"`).
3. **Web Identity Token** — `AWS_WEB_IDENTITY_TOKEN_FILE` + `AWS_ROLE_ARN`
   (Kubernetes IRSA, GitHub OIDC).
4. **ECS task role** — `AWS_CONTAINER_CREDENTIALS_RELATIVE_URI` or
   `AWS_CONTAINER_CREDENTIALS_FULL_URI` (set automatically by ECS).
5. **EC2 instance metadata (IMDSv2)** — `http://169.254.169.254/...` (EC2,
   EKS node role).
6. **SSO** — via `~/.aws/config` `sso_*` keys; requires `aws sso login` first.

Region resolution (separate from credentials):
1. `AWS_REGION` env var.
2. `AWS_DEFAULT_REGION` env var.
3. `~/.aws/config` `region =` for the active profile.
4. EC2 instance metadata region.
5. Falls back to `us-east-1` if nothing else resolves.

### No CLI flags for credentials

Credentials are never accepted on the command line. If the caller needs to
override the account, they set `AWS_PROFILE=my-profile` or
`AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=...` before invoking pqls. This
matches the AWS CLI, `awscurl`, and every other standard tool.

### Auth verification strategy: deferred to first real call

We do **not** call `sts:GetCallerIdentity` at start-up. Reasons:
- It requires `sts` permissions that S3-only IAM policies typically omit.
- It adds ~100 ms latency before every invocation even when credentials work.
- S3's first API call (HEAD for single-object, `list_objects_v2` for prefix)
  returns a clear `403 AccessDenied` or `401 InvalidClientTokenId` immediately.

The error handler for S3 API calls translates AWS error codes into user-facing
messages (see §8 Errors).

---

## 5. Lazy Footer Fetch Algorithm

### Parquet file layout (PAR1 spec §7)

```
[ROW GROUP 0 column chunks ...]
[ROW GROUP 1 column chunks ...]
...
[ROW GROUP N column chunks ...]
[File Metadata (Thrift-encoded FileMetaData)]
[4-byte little-endian metadata length]
[4-byte magic: PAR1]
```

The last 8 bytes are always `<metadata_len (4 LE bytes)><PAR1>`. The metadata
itself sits immediately before those 8 bytes. This is the parquet spec invariant
that lets us skip the body entirely.

### Two-step range-get algorithm

**Step 1 — HEAD + initial range get**

```
HEAD s3://bucket/key
  → Content-Length: N bytes

GET s3://bucket/key  Range: bytes=(N - FOOTER_ESTIMATE), (N-1)
  → up to FOOTER_ESTIMATE bytes from the end
```

`FOOTER_ESTIMATE` defaults to **65 536 bytes (64 KiB)**. Empirically, almost
all parquet files have footers ≤ 32 KiB; the 64 KiB estimate gives headroom for
large schemas (wide tables with hundreds of columns and embedded row-group stats)
while keeping the I/O cost to a single round-trip for the common case.

For files smaller than `FOOTER_ESTIMATE`, the range covers the whole file — this
is fine, S3 returns whatever bytes exist.

**Step 2 — Validate and optionally re-fetch**

From the tail of the fetched buffer:
1. Check `buf[last-3 ..] == b"PAR1"` — validates Parquet magic.
2. Read `metadata_len = u32::from_le_bytes(buf[last-7 .. last-3])`.
3. If `metadata_len + 8 <= FOOTER_ESTIMATE`, the full metadata is in the buffer.
   Slice it out and decode.
4. If `metadata_len + 8 > FOOTER_ESTIMATE` (footer larger than estimate):
   Issue a second range get:
   ```
   GET Range: bytes=(N - metadata_len - 8), (N - 9)
   ```
   This fetches exactly the Thrift-encoded `FileMetaData` blob.

The second fetch is the **worst case** and is hit only by exceptionally large
schemas or files with thousands of row groups. In practice this affects < 1% of
production parquet files.

### ChunkReader implementation

`parquet::file::serialized_reader::SerializedFileReader` accepts a generic
`ChunkReader: parquet::file::reader::ChunkReader`. We implement this trait on a
struct that holds the already-fetched footer bytes in a `Bytes` buffer:

```rust
pub struct FooterBuffer {
    bytes: bytes::Bytes,
    file_size: u64,
}

impl ChunkReader for FooterBuffer {
    type T = bytes::buf::Reader<bytes::Bytes>;
    fn get_read(&self, start: u64) -> Result<Self::T> { ... }
    fn get_bytes(&self, start: u64, length: usize) -> Result<bytes::Bytes> { ... }
}
```

The `start` offset in `ChunkReader` is an absolute file offset. Since we only
hold the footer region, any read whose `start` offset falls outside the footer
is a logic error (the parquet reader only asks for footer bytes when
`skip_row_groups = true`). We assert this in debug builds.

For the single-file `--schema` dump, `SerializedFileReader::new_with_options`
is called with `ArrowReaderOptions::new().with_skip_arrow_metadata(false)` and
`ReadOptionsBuilder::new().with_page_index(false)` so that only the footer is
parsed. No row data is read.

---

## 6. Schema-Brief Renderer

### Purpose

For directory listing, each parquet file gets a one-line schema summary in
numpy-ish `(name: type)` notation. This is the **brief**, not the full schema
(`--schema` still dumps the full column list).

### Type mapping

The renderer walks `parquet::schema::types::Type` and applies this table:

| Physical type             | Logical annotation          | Brief type name              |
|---------------------------|-----------------------------|------------------------------|
| `BOOLEAN`                 | —                           | `bool`                       |
| `INT32`                   | —                           | `int32`                      |
| `INT32`                   | `DATE`                      | `date`                       |
| `INT32`                   | `DECIMAL(p,s)`              | `decimal(p,s)`               |
| `INT32`                   | `INT(8,  signed)`           | `int8`                       |
| `INT32`                   | `INT(16, signed)`           | `int16`                      |
| `INT32`                   | `INT(32, signed)`           | `int32`                      |
| `INT32`                   | `INT(8,  unsigned)`         | `uint8`                      |
| `INT32`                   | `INT(16, unsigned)`         | `uint16`                     |
| `INT32`                   | `INT(32, unsigned)`         | `uint32`                     |
| `INT32`                   | `TIME(MILLIS, adj)`         | `time[ms]`                   |
| `INT64`                   | —                           | `int64`                      |
| `INT64`                   | `INT(64, signed)`           | `int64`                      |
| `INT64`                   | `INT(64, unsigned)`         | `uint64`                     |
| `INT64`                   | `TIMESTAMP(MILLIS, true)`   | `timestamp[ms,UTC]`          |
| `INT64`                   | `TIMESTAMP(MICROS, true)`   | `timestamp[us,UTC]`          |
| `INT64`                   | `TIMESTAMP(NANOS,  true)`   | `timestamp[ns,UTC]`          |
| `INT64`                   | `TIMESTAMP(MILLIS, false)`  | `timestamp[ms]`              |
| `INT64`                   | `TIMESTAMP(MICROS, false)`  | `timestamp[us]`              |
| `INT64`                   | `TIMESTAMP(NANOS,  false)`  | `timestamp[ns]`              |
| `INT64`                   | `TIME(MICROS, adj)`         | `time[us]`                   |
| `INT64`                   | `DECIMAL(p,s)`              | `decimal(p,s)`               |
| `INT96`                   | —                           | `timestamp`                  |
| `FLOAT`                   | —                           | `float32`                    |
| `DOUBLE`                  | —                           | `float64`                    |
| `BYTE_ARRAY`              | —                           | `bytes`                      |
| `BYTE_ARRAY`              | `STRING` / `UTF8`           | `utf8`                       |
| `BYTE_ARRAY`              | `JSON`                      | `json`                       |
| `BYTE_ARRAY`              | `BSON`                      | `bson`                       |
| `BYTE_ARRAY`              | `ENUM`                      | `enum`                       |
| `BYTE_ARRAY`              | `DECIMAL(p,s)`              | `decimal(p,s)`               |
| `FIXED_LEN_BYTE_ARRAY(N)` | —                           | `bytes(N)`                   |
| `FIXED_LEN_BYTE_ARRAY(16)`| `UUID`                      | `uuid`                       |
| `FIXED_LEN_BYTE_ARRAY(N)` | `DECIMAL(p,s)`              | `decimal(p,s)`               |
| `FIXED_LEN_BYTE_ARRAY(N)` | `FLOAT16`                   | `float16`                    |
| Group (nested)            | `LIST`                      | `list<item_type>`            |
| Group (nested)            | `MAP`                       | `map<key_type,value_type>`   |
| Group (nested)            | `STRUCT` / no annotation    | `struct`                     |

For nested types, the brief recurses one level but caps depth at 1: a `LIST` of
structs is rendered as `list<struct>`, not `list<struct{a:int32,b:utf8}>`.

### Rust API

```rust
/// Returns comma-separated "(name: type)" tuples for the top-level columns.
pub fn schema_brief(schema: &parquet::schema::types::Type) -> String;
```

The function takes the root `MessageType` (i.e.
`file_metadata.schema_descr().root_schema()`). It iterates `schema.get_fields()`
and joins with `", "` inside `(...)`.

Example output:
```
(id: int64, ts: timestamp[ms,UTC], price: float64, qty: int32, side: utf8)
```

---

## 7. Listing Mode

### Plain-text output (default)

One line per `.parquet` object under the prefix:

```
FILENAME          SIZE      MODIFIED    SCHEMA_BRIEF
20231217.parquet  214.3 MB  2023-12-18  (ts: timestamp[ms,UTC], price: float64, qty: int32, side: utf8)
20231218.parquet    1.2 GB  2023-12-19  (ts: timestamp[ms,UTC], price: float64, qty: int32, side: utf8)
```

- `FILENAME`: the key with the prefix stripped (basename only by default, with
  full key when prefix is empty).
- `SIZE`: `humansize::format_size(bytes, BINARY)` — same formatter as local
  dir mode.
- `MODIFIED`: `last_modified` from S3 `ListObjectsV2Output`, formatted as
  `YYYY-MM-DD`.
- `SCHEMA_BRIEF`: the `(name: type)` string from §6.

Column widths are right-padded to the longest value in each column (computed
after all objects are fetched; listing waits for all concurrent schema fetches to
complete before printing).

### `--quiet` mode

Tab-separated, no header line:
```
20231217.parquet\t214.3 MB\t2023-12-18\t(ts: timestamp[ms,UTC], ...)
```

### `--json` mode

JSON array of objects, one element per file:
```json
[
  {
    "key": "events/2024/20231217.parquet",
    "size_bytes": 224870400,
    "modified": "2023-12-18T00:00:00Z",
    "schema": "(ts: timestamp[ms,UTC], price: float64, qty: int32, side: utf8)"
  }
]
```

This reuses the existing `--json` flag (not a new `--output json` flag). The
`--json` flag is already supported with schema, kv-meta, check, etc.; listing
is a natural addition. The `--csv` flag is **not** supported for S3 listing —
the existing CSV mode dumps row data, which has no meaning for listing. Passing
`--csv` with an `s3://` prefix will exit 2 with a clear error.

### No output for empty prefix

If `list_objects_v2` returns zero `.parquet` objects under the prefix, print one
informational line to stderr:
```
pqls: no parquet files found under s3://bucket/prefix/
```
Exit 0.

---

## 8. Errors and Edge Cases

### Structured error handling

All S3 errors flow through `anyhow::Error` with context annotations, consistent
with how local path errors are handled today.

### Error table

| Condition | AWS error code | pqls message | Exit |
|-----------|---------------|--------------|------|
| Bucket does not exist | `NoSuchBucket` | `s3://bucket: bucket does not exist` | 1 |
| Key not found | `NoSuchKey` / 404 | `s3://bucket/key: object not found` | 1 |
| Access denied (any operation) | `AccessDenied` / 403 | `s3://bucket/...: access denied — ensure IAM policy allows s3:GetObject (and s3:ListBucket for prefix listing)` | 1 |
| Invalid credentials | `InvalidClientTokenId` / `AuthFailure` | `AWS credentials invalid — check AWS_ACCESS_KEY_ID / AWS_PROFILE` | 1 |
| No credentials found | SDK config error | `No AWS credentials found — set AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY, configure ~/.aws/credentials, or attach an IAM role` | 1 |
| Region mismatch | `PermanentRedirect` (301) or `AuthorizationHeaderMalformed` with correct region | `s3://bucket: bucket is in region <X>, not <Y> — set AWS_REGION=<X>` | 1 |
| Prefix with no parquet files | — (empty list) | `pqls: no parquet files found under s3://bucket/prefix/` (stderr) | 0 |
| Not a valid parquet file | bad magic / Thrift decode error | `s3://bucket/key: not a valid parquet file (bad footer magic)` | 1 |
| Footer larger than file | HEAD returns N < 8 | `s3://bucket/key: file too small to be valid parquet (N bytes)` | 1 |
| Object too small for footer estimate but still valid | 206 range returns whole file | Handled: second-fetch path in §5 is not needed; footer is parsed from the full response |
| Network timeout / connection error | `timeout`, `connection refused` | Pass through with `anyhow` context: `fetching s3://bucket/key: <underlying error>` | 1 |

### Region redirect

When a `PermanentRedirect` (HTTP 301) is received, the response body contains
the correct endpoint. The `aws-sdk-s3` client handles regional redirects
automatically when `force_path_style(false)` (default) and
`behavior_version = latest` is set. No manual redirect handling is needed in
pqls code.

### Very small files

If `HEAD` returns `Content-Length < 8`, the file cannot possibly be valid
Parquet (magic + footer length require 8 bytes minimum). Reject immediately.

If `Content-Length >= 8` but `Content-Length < FOOTER_ESTIMATE`, range the
entire file (`bytes=0-`). The buffer received is the whole file; proceed with
normal footer parsing from the end.

---

## 9. Performance Budget

### Target

For a directory of **30 daily 200 MB parquet files**, schema-brief listing must
complete in **≤ 5 s wall clock** from a co-located (same-region) EC2 instance or
GitHub Actions runner.

### Breakdown

| Operation | Latency estimate | Count | Total |
|-----------|-----------------|-------|-------|
| `list_objects_v2` (paginated, 1 page for 30 objects) | 30 ms | 1 | 30 ms |
| HEAD per object (concurrent) | 15 ms | 30 | ~15 ms (all parallel) |
| Range GET 64 KiB per object (concurrent) | 50 ms + 64 KiB @ 1 Gbps | 30 | ~120 ms (all parallel) |
| Footer parse (Thrift decode) per object | 1 ms | 30 | 30 ms |
| **Total (concurrent path)** | | | **~200 ms** |

The bottleneck is the range GET round-trips. With concurrency 16
(`buffer_unordered(16)`), 30 objects complete in ≈ 2 batches × 120 ms = 240 ms,
well inside the 5 s budget. The budget is conservative: even at 10× this
latency (cross-region, `us-east-1` → `eu-west-1`), 30 files complete in ~2 s.

### Implementation pattern

```rust
use futures::stream::{StreamExt, iter};

let schema_stream = iter(objects)
    .map(|obj| fetch_footer_and_brief(client.clone(), bucket, obj))
    .buffer_unordered(16);

let results: Vec<_> = schema_stream.collect().await;
```

`fetch_footer_and_brief` issues HEAD + range GET for one object and returns
`(ObjectMeta, String)`. All 30 are in flight concurrently up to the window of 16.

The `aws-sdk-s3` client is `Clone`-able (it holds an `Arc` internally); one
client instance is constructed at start-up and cloned into each task.

### Worst-case: second fetch needed

If any object's footer exceeds 64 KiB, a second range GET is issued for that
object only. This adds ~50 ms latency for that file, serialized with its first
fetch. Across 30 objects this worst case adds at most one extra batch (~120 ms
total), staying comfortably within budget.

### No caching

Footer bytes are kept in memory for the duration of the listing call only.
No disk cache is written. For users who need to re-query schemas frequently, the
recommendation is: pipe `pqls --json s3://bucket/prefix/` output to a file and
`jq` locally. A persistent cache layer (e.g. `~/.cache/pqls/`) is a future
enhancement, explicitly out of scope for this cycle.

---

## Module map

```
src/
  s3/
    mod.rs          — public API: parse_path(), run_s3_path()
    path.rs         — S3Path enum, parse(), S3PathError
    auth.rs         — aws_config::defaults() loader, SdkConfig wrapper
    footer.rs       — FooterBuffer, lazy_fetch_footer()
    list.rs         — list_objects_v2 pagination, concurrent fetch loop
    schema_brief.rs — schema_brief(Type) → String, type_name() table
    error.rs        — S3Error enum, From<SdkError<_>> impls, user messages
```

`main.rs` gains one branch before the existing local dispatch:

```rust
if let Some(s3_path) = raw_path.strip_prefix("s3://") {
    let parsed = s3::path::S3Path::parse(&format!("s3://{s3_path}"))?;
    tokio::runtime::Handle::current().block_on(s3::run_s3_path(parsed, &cli))?;
    return Ok(());
}
```

(Or, if `main` becomes `async`, use `.await` directly.)

---

## Open questions (for implementation cycle)

1. **`--schema` on a single S3 object with `--columns` filter**: the lazy footer
   fetch reads the full schema; column projection is applied at render time, not
   at fetch time. This is fine for typical schemas (< 200 columns). Document
   this in the code.

2. **Requester-pays buckets**: not in scope for this cycle. If a `403` is
   received with `RequestorPaysBucketError`, add a hint: "if this is a
   requester-pays bucket, add `x-amz-request-payer: requester` — not yet
   supported by pqls."

3. **S3 Express One Zone (Directory Buckets)**: uses a different endpoint
   pattern (`bucket-name--az-id--x-s3.s3express-region.amazonaws.com`). The
   `aws-sdk-s3` 1.x client handles this transparently when `behavior-version-latest`
   is set. No special handling needed in pqls.
