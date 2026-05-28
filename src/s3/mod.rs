pub mod auth;
pub mod error;
pub mod footer;
pub mod list;
pub mod path;
pub mod schema_brief;

pub use path::S3Path;

use anyhow::Result;
use parquet::file::reader::{FileReader, SerializedFileReader};

use crate::{schema, Cli};

pub async fn run_s3_path(path: S3Path, cli: &Cli) -> Result<()> {
    let config = auth::load_aws_config().await?;
    // Use path-style addressing when a custom endpoint is set (e.g. for local testing).
    let client = if config.endpoint_url().is_some() {
        aws_sdk_s3::Client::from_conf(
            aws_sdk_s3::config::Builder::from(&config)
                .force_path_style(true)
                .build(),
        )
    } else {
        aws_sdk_s3::Client::new(&config)
    };

    match path {
        S3Path::Object { bucket, key } => {
            let s3_url = format!("s3://{bucket}/{key}");
            let footer_buf = footer::lazy_fetch_footer(&client, &bucket, &key).await?;
            let reader = SerializedFileReader::new(footer_buf)?;
            let meta = reader.metadata();

            let columns: Option<Vec<String>> = cli
                .columns
                .as_ref()
                .map(|s| s.split(',').map(|c| c.trim().to_string()).collect());

            if cli.json {
                schema::emit_json_from_meta(meta, &s3_url, columns.as_deref())?;
            } else {
                schema::emit_text_from_meta(meta, columns.as_deref())?;
            }
        }
        S3Path::Prefix { bucket, prefix } => {
            list::run_listing(&client, &bucket, &prefix, cli.json, cli.quiet).await?;
        }
    }

    Ok(())
}
