pub mod auth;
pub mod path;

pub use path::S3Path;

use anyhow::Result;
use crate::Cli;

pub async fn run_s3_path(_path: S3Path, _cli: &Cli) -> Result<()> {
    todo!("S3 operations not yet implemented — see s3-impl-list-and-schema task")
}
