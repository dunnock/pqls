use std::process::Command;
use wiremock::matchers::{method, path, path_regex, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

// ─── parquet fixture generators ─────────────────────────────────────────────

fn parquet_with_schema(fields: Vec<std::sync::Arc<parquet::schema::types::Type>>) -> Vec<u8> {
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::types::Type;
    use std::sync::Arc;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let schema = Arc::new(
        Type::group_type_builder("schema")
            .with_fields(fields)
            .build()
            .unwrap(),
    );
    let props = Arc::new(WriterProperties::builder().build());
    let writer =
        SerializedFileWriter::new(tmp.reopen().unwrap(), schema, props).unwrap();
    writer.close().unwrap();
    std::fs::read(tmp.path()).unwrap()
}

fn primitive_field(
    name: &str,
    physical: parquet::basic::Type,
) -> std::sync::Arc<parquet::schema::types::Type> {
    use parquet::basic::Repetition;
    use parquet::schema::types::Type;
    use std::sync::Arc;
    Arc::new(
        Type::primitive_type_builder(name, physical)
            .with_repetition(Repetition::OPTIONAL)
            .build()
            .unwrap(),
    )
}

fn gen_single_col() -> Vec<u8> {
    parquet_with_schema(vec![primitive_field("value", parquet::basic::Type::INT64)])
}

fn gen_multi_col() -> Vec<u8> {
    parquet_with_schema(vec![
        primitive_field("id", parquet::basic::Type::INT64),
        primitive_field("price", parquet::basic::Type::DOUBLE),
        primitive_field("qty", parquet::basic::Type::FLOAT),
        primitive_field("side", parquet::basic::Type::BYTE_ARRAY),
    ])
}

/// Generates a parquet file whose Thrift-encoded FileMetaData exceeds 64 KiB.
/// Uses 200 columns with 430-char names; estimated footer ≈ 90 KiB.
fn gen_wide_schema() -> Vec<u8> {
    let fields = (0..200)
        .map(|i| {
            let name = format!("col_{:03}_{}", i, "x".repeat(426));
            primitive_field(&name, parquet::basic::Type::INT64)
        })
        .collect();
    parquet_with_schema(fields)
}

// ─── S3-compatible range-GET responder ──────────────────────────────────────

struct RangeResponder {
    content: Vec<u8>,
}

impl Respond for RangeResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let total = self.content.len() as u64;

        let range_hdr = request
            .headers
            .get("range")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if let Some((start, end)) = parse_range(range_hdr, total) {
            let slice = &self.content[start as usize..=end as usize];
            return ResponseTemplate::new(206)
                .insert_header("content-range", format!("bytes {start}-{end}/{total}"))
                .insert_header("content-length", slice.len().to_string())
                .insert_header("accept-ranges", "bytes")
                .set_body_bytes(slice.to_vec());
        }

        // No range header — return full object.
        ResponseTemplate::new(200)
            .insert_header("content-length", total.to_string())
            .set_body_bytes(self.content.clone())
    }
}

fn parse_range(hdr: &str, total: u64) -> Option<(u64, u64)> {
    let s = hdr.strip_prefix("bytes=")?;
    let (a, b) = s.split_once('-')?;
    let start: u64 = a.parse().ok()?;
    let end: u64 = b.parse().ok()?;
    Some((start, end.min(total.saturating_sub(1))))
}

// ─── shared test helpers ─────────────────────────────────────────────────────

fn pqls(endpoint: &str) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pqls"));
    cmd.env("AWS_ENDPOINT_URL", endpoint)
        .env("AWS_ACCESS_KEY_ID", "test")
        .env("AWS_SECRET_ACCESS_KEY", "test")
        .env("AWS_REGION", "us-east-1")
        // Suppress SDK credential-chain probes (IMDSv2, ECS, etc.)
        .env("AWS_EC2_METADATA_DISABLED", "true");
    cmd
}

fn xml_error(code: &str, message: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Error><Code>{code}</Code><Message>{message}</Message></Error>"#
    )
}

fn xml_list(bucket: &str, prefix: &str, objects: &[(&str, u64)]) -> String {
    let contents: String = objects
        .iter()
        .map(|(key, size)| {
            format!(
                "<Contents><Key>{key}</Key><Size>{size}</Size>\
                 <LastModified>2024-01-01T00:00:00.000Z</LastModified></Contents>"
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>{bucket}</Name><Prefix>{prefix}</Prefix><KeyCount>{}</KeyCount><MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated>{contents}</ListBucketResult>"#,
        objects.len()
    )
}

async fn mount_object(server: &MockServer, bucket: &str, key: &str, data: Vec<u8>) {
    let obj_path = format!("/{bucket}/{key}");
    let size = data.len() as u64;

    // HEAD → Content-Length
    Mock::given(method("HEAD"))
        .and(path(obj_path.clone()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", size.to_string())
                .insert_header("content-type", "application/octet-stream"),
        )
        .mount(server)
        .await;

    // GET with optional Range header → handled by RangeResponder
    Mock::given(method("GET"))
        .and(path(obj_path))
        .respond_with(RangeResponder { content: data })
        .mount(server)
        .await;
}

// ─── test cases ──────────────────────────────────────────────────────────────

/// TC-1: single file schema — output contains expected column names.
#[tokio::test]
async fn single_file_schema() {
    let server = MockServer::start().await;
    let data = gen_multi_col();
    mount_object(&server, "test-bucket", "multi_col.parquet", data).await;

    let out = pqls(&server.uri())
        .args(["s3://test-bucket/multi_col.parquet"])
        .output()
        .unwrap();

    assert!(out.status.success(), "exit should be 0: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("id"), "expected 'id' in output: {stdout}");
    assert!(stdout.contains("price"), "expected 'price' in output: {stdout}");
    assert!(stdout.contains("qty"), "expected 'qty' in output: {stdout}");
    assert!(stdout.contains("side"), "expected 'side' in output: {stdout}");
}

/// TC-2: prefix listing — shows all 3 files with non-empty schema briefs.
#[tokio::test]
async fn dir_listing_with_schemas() {
    let server = MockServer::start().await;
    let single = gen_single_col();
    let multi = gen_multi_col();
    let wide = gen_wide_schema();

    let keys = [
        ("prefix/single_col.parquet", single.len() as u64),
        ("prefix/multi_col.parquet", multi.len() as u64),
        ("prefix/wide_schema.parquet", wide.len() as u64),
    ];

    // ListObjectsV2
    Mock::given(method("GET"))
        .and(path("/test-bucket/"))
        .and(query_param("list-type", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/xml")
                .set_body_string(xml_list("test-bucket", "prefix/", &keys)),
        )
        .mount(&server)
        .await;

    mount_object(&server, "test-bucket", "prefix/single_col.parquet", single).await;
    mount_object(&server, "test-bucket", "prefix/multi_col.parquet", multi).await;
    mount_object(&server, "test-bucket", "prefix/wide_schema.parquet", wide).await;

    let out = pqls(&server.uri())
        .args(["s3://test-bucket/prefix/"])
        .output()
        .unwrap();

    assert!(out.status.success(), "exit should be 0: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // All three files should appear in the listing
    assert!(stdout.contains("single_col"), "listing missing single_col: {stdout}");
    assert!(stdout.contains("multi_col"), "listing missing multi_col: {stdout}");
    assert!(stdout.contains("wide_schema"), "listing missing wide_schema: {stdout}");
    // Schema briefs appear (parenthesised tuples)
    assert!(stdout.contains('('), "listing missing schema briefs: {stdout}");
}

/// TC-3: --schema --json — JSON output contains expected field names.
#[tokio::test]
async fn schema_json_output() {
    let server = MockServer::start().await;
    let data = gen_multi_col();
    mount_object(&server, "test-bucket", "multi_col.parquet", data).await;

    let out = pqls(&server.uri())
        .args(["--schema", "--json", "s3://test-bucket/multi_col.parquet"])
        .output()
        .unwrap();

    assert!(out.status.success(), "exit should be 0: {:?}", out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("not valid JSON: {e}\n{stdout}"));
    let fields = v["fields"].as_array().expect("fields array");
    let names: Vec<&str> = fields
        .iter()
        .filter_map(|f| f["name"].as_str())
        .collect();
    assert!(names.contains(&"id"), "missing 'id': {names:?}");
    assert!(names.contains(&"price"), "missing 'price': {names:?}");
}

/// TC-4: auth failure — access denied produces exit 1 + IAM hint.
#[tokio::test]
async fn auth_failure() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path_regex(r"^/test-bucket/.*"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("content-type", "application/xml")
                .set_body_string(xml_error("AccessDenied", "Access Denied")),
        )
        .mount(&server)
        .await;

    let out = pqls(&server.uri())
        .args(["s3://test-bucket/secret.parquet"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1), "access denied should exit 1");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.to_lowercase().contains("access") || stderr.to_lowercase().contains("iam"),
        "stderr should mention access/IAM: {stderr}"
    );
}

/// TC-5: bucket not found — NoSuchBucket → exit 1 + message.
#[tokio::test]
async fn bucket_not_found() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/no-such-bucket/"))
        .respond_with(
            ResponseTemplate::new(404)
                .insert_header("content-type", "application/xml")
                .set_body_string(xml_error("NoSuchBucket", "The specified bucket does not exist")),
        )
        .mount(&server)
        .await;

    let out = pqls(&server.uri())
        .args(["s3://no-such-bucket/"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1), "NoSuchBucket should exit 1");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("bucket") || stderr.contains("exist"),
        "stderr should mention bucket: {stderr}"
    );
}

/// TC-6: key not found — NoSuchKey → exit 1 + message.
#[tokio::test]
async fn key_not_found() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/test-bucket/missing.parquet"))
        .respond_with(
            ResponseTemplate::new(404)
                .insert_header("content-type", "application/xml")
                .set_body_string(xml_error("NoSuchKey", "The specified key does not exist")),
        )
        .mount(&server)
        .await;

    let out = pqls(&server.uri())
        .args(["s3://test-bucket/missing.parquet"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1), "NoSuchKey should exit 1");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not found") || stderr.contains("object"),
        "stderr should mention key not found: {stderr}"
    );
}

/// TC-7: empty prefix — exit 0, stderr says no parquet files found.
#[tokio::test]
async fn empty_prefix() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/test-bucket/"))
        .and(query_param("list-type", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/xml")
                .set_body_string(xml_list("test-bucket", "empty/", &[])),
        )
        .mount(&server)
        .await;

    let out = pqls(&server.uri())
        .args(["s3://test-bucket/empty/"])
        .output()
        .unwrap();

    assert!(out.status.success(), "empty prefix should exit 0: {:?}", out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no parquet"),
        "stderr should say no parquet files found: {stderr}"
    );
}

/// TC-8: wide schema second fetch — footer > 64 KiB triggers exactly 2 GETs.
#[tokio::test]
async fn wide_schema_second_fetch() {
    let server = MockServer::start().await;
    let data = gen_wide_schema();
    let footer_size = measure_footer_size(&data);

    // Verify our fixture actually triggers the second-fetch path (footer > 64 KiB).
    assert!(
        footer_size > 65_536,
        "wide_schema footer ({footer_size} bytes) must exceed 65536 to exercise second-fetch path"
    );

    mount_object(&server, "test-bucket", "wide_schema.parquet", data).await;

    let out = pqls(&server.uri())
        .args(["s3://test-bucket/wide_schema.parquet"])
        .output()
        .unwrap();

    assert!(out.status.success(), "wide schema should exit 0: {:?}", out);

    // Count how many GET requests were made for the wide schema key.
    let reqs = server.received_requests().await.unwrap_or_default();
    let get_count = reqs
        .iter()
        .filter(|r| {
            r.method == wiremock::http::Method::GET
                && r.url.path().ends_with("wide_schema.parquet")
        })
        .count();
    assert_eq!(
        get_count, 2,
        "expected exactly 2 range GETs for wide schema (first estimate + second exact fetch), got {get_count}"
    );
}

/// Returns `metadata_len + 8` from the parquet footer trailer.
fn measure_footer_size(parquet_bytes: &[u8]) -> u64 {
    let n = parquet_bytes.len();
    assert!(n >= 8, "parquet file too small");
    assert_eq!(&parquet_bytes[n - 4..], b"PAR1", "bad PAR1 magic");
    let metadata_len =
        u32::from_le_bytes(parquet_bytes[n - 8..n - 4].try_into().unwrap()) as u64;
    metadata_len + 8
}
