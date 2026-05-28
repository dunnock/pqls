use anyhow::Result;
use aws_config::{BehaviorVersion, SdkConfig};

pub async fn load_aws_config() -> Result<SdkConfig> {
    let config = aws_config::defaults(BehaviorVersion::latest()).load().await;
    Ok(config)
}
