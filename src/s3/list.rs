use std::sync::Arc;

use anyhow::Result;
use aws_sdk_s3::types::Object;
use futures::stream::{self, StreamExt};
use humansize::{format_size, BINARY};
use parquet::file::reader::{FileReader, SerializedFileReader};
use serde::Serialize;

use super::{error, footer::lazy_fetch_footer, schema_brief::schema_brief};

pub struct ListingRow {
    pub key: String,
    pub size_bytes: u64,
    pub modified: String,
    pub schema_brief: String,
}

/// Returns all objects under `prefix` whose keys end with `.parquet` (case-insensitive).
pub async fn list_parquet_objects(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    prefix: &str,
) -> Result<Vec<Object>> {
    let mut objects = Vec::new();
    let mut token: Option<String> = None;

    loop {
        let mut req = client.list_objects_v2().bucket(bucket).prefix(prefix);
        if let Some(t) = token.take() {
            req = req.continuation_token(t);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| error::from_list_err(e, bucket))?;

        for obj in resp.contents() {
            if obj
                .key()
                .map(|k| k.to_lowercase().ends_with(".parquet"))
                .unwrap_or(false)
            {
                objects.push(obj.clone());
            }
        }

        if resp.is_truncated().unwrap_or(false) {
            token = resp.next_continuation_token().map(|s| s.to_string());
        } else {
            break;
        }
    }

    Ok(objects)
}

/// Fetches the parquet footer for one object and returns a `ListingRow`.
pub async fn fetch_schema_brief(
    client: Arc<aws_sdk_s3::Client>,
    bucket: String,
    obj: Object,
) -> Result<ListingRow> {
    let key = obj.key().unwrap_or("").to_string();
    let size_bytes = obj.size().unwrap_or(0) as u64;

    let modified = obj
        .last_modified()
        .map(|dt| epoch_to_date(dt.secs()))
        .unwrap_or_else(|| "unknown".to_string());

    let footer = lazy_fetch_footer(&client, &bucket, &key).await?;
    let reader = SerializedFileReader::new(footer)?;
    let schema = reader.metadata().file_metadata().schema().clone();
    let brief = schema_brief(&schema);

    Ok(ListingRow {
        key,
        size_bytes,
        modified,
        schema_brief: brief,
    })
}

/// Paginates a prefix, fetches schema briefs concurrently, then prints results.
pub async fn run_listing(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    prefix: &str,
    json: bool,
    quiet: bool,
) -> Result<()> {
    let objects = list_parquet_objects(client, bucket, prefix).await?;

    if objects.is_empty() {
        eprintln!("pqls: no parquet files found under s3://{bucket}/{prefix}");
        return Ok(());
    }

    let client_arc = Arc::new(client.clone());

    // buffer_unordered(16) — per design §9 concurrency budget
    let mut rows: Vec<ListingRow> = stream::iter(objects)
        .map(|obj| {
            let c = Arc::clone(&client_arc);
            let b = bucket.to_string();
            async move { fetch_schema_brief(c, b, obj).await }
        })
        .buffer_unordered(16)
        .collect::<Vec<Result<ListingRow>>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;

    rows.sort_by(|a, b| a.key.cmp(&b.key));

    if json {
        emit_json(&rows, prefix)?;
    } else if quiet {
        emit_quiet(&rows);
    } else {
        emit_plain(&rows, prefix);
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonRow<'a> {
    key: &'a str,
    size_bytes: u64,
    modified: &'a str,
    schema: &'a str,
}

fn emit_json(rows: &[ListingRow], _prefix: &str) -> Result<()> {
    let json_rows: Vec<JsonRow> = rows
        .iter()
        .map(|r| JsonRow {
            key: &r.key,
            size_bytes: r.size_bytes,
            modified: &r.modified,
            schema: &r.schema_brief,
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&json_rows)?);
    Ok(())
}

fn emit_quiet(rows: &[ListingRow]) {
    for r in rows {
        println!("{}\t{}\t{}\t{}", r.key, r.size_bytes, r.modified, r.schema_brief);
    }
}

fn emit_plain(rows: &[ListingRow], prefix: &str) {
    let header = ("FILENAME", "SIZE", "MODIFIED", "SCHEMA_BRIEF");

    let display: Vec<[String; 4]> = rows
        .iter()
        .map(|r| {
            let filename = r
                .key
                .strip_prefix(prefix)
                .unwrap_or(&r.key)
                .trim_start_matches('/')
                .to_string();
            let size = format_size(r.size_bytes, BINARY);
            [filename, size, r.modified.clone(), r.schema_brief.clone()]
        })
        .collect();

    let w0 = display
        .iter()
        .map(|r| r[0].len())
        .max()
        .unwrap_or(0)
        .max(header.0.len());
    let w1 = display
        .iter()
        .map(|r| r[1].len())
        .max()
        .unwrap_or(0)
        .max(header.1.len());
    let w2 = display
        .iter()
        .map(|r| r[2].len())
        .max()
        .unwrap_or(0)
        .max(header.2.len());

    println!(
        "{:<w0$}  {:<w1$}  {:<w2$}  {}",
        header.0, header.1, header.2, header.3
    );
    for r in &display {
        println!("{:<w0$}  {:<w1$}  {:<w2$}  {}", r[0], r[1], r[2], r[3]);
    }
}

/// Converts a Unix epoch (seconds) to a `YYYY-MM-DD` string.
/// Valid for dates 1970–2099.
fn epoch_to_date(secs: i64) -> String {
    let days = (secs / 86_400) as i32;
    let mut y = 1970i32;
    let mut d = days;
    loop {
        let diy = if is_leap(y) { 366 } else { 365 };
        if d < diy {
            break;
        }
        d -= diy;
        y += 1;
    }
    let dim: [i32; 12] = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 1i32;
    for days_in_month in dim {
        if d < days_in_month {
            break;
        }
        d -= days_in_month;
        m += 1;
    }
    format!("{y:04}-{m:02}-{:02}", d + 1)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
