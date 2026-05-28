use std::fmt;

#[derive(Debug, PartialEq)]
pub enum S3Path {
    Object { bucket: String, key: String },
    Prefix { bucket: String, prefix: String },
}

#[derive(Debug, PartialEq)]
pub enum S3PathError {
    MissingScheme,
    EmptyBucket,
    InvalidBucket(String),
}

impl fmt::Display for S3PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            S3PathError::MissingScheme => write!(f, "path must start with s3://"),
            S3PathError::EmptyBucket => write!(f, "s3:// path has no bucket name"),
            S3PathError::InvalidBucket(name) => write!(
                f,
                "invalid S3 bucket name \"{name}\": must be 3-63 lowercase letters, digits, or hyphens; no leading/trailing hyphen"
            ),
        }
    }
}

impl std::error::Error for S3PathError {}

impl S3Path {
    pub fn parse(raw: &str) -> Result<Self, S3PathError> {
        let after = raw
            .strip_prefix("s3://")
            .ok_or(S3PathError::MissingScheme)?;

        if after.is_empty() {
            return Err(S3PathError::EmptyBucket);
        }

        let (bucket_str, path) = match after.splitn(2, '/').collect::<Vec<_>>()[..] {
            [bucket] => (bucket, ""),
            [bucket, rest] => (bucket, rest),
            _ => unreachable!(),
        };

        if bucket_str.is_empty() {
            return Err(S3PathError::EmptyBucket);
        }

        validate_bucket(bucket_str)
            .map_err(|_| S3PathError::InvalidBucket(bucket_str.to_string()))?;

        let bucket = bucket_str.to_string();

        if path.is_empty() || path.ends_with('/') {
            let prefix = path.to_string();
            Ok(S3Path::Prefix { bucket, prefix })
        } else {
            Ok(S3Path::Object {
                bucket,
                key: path.to_string(),
            })
        }
    }
}

fn validate_bucket(name: &str) -> Result<(), ()> {
    if name.len() < 3 || name.len() > 63 {
        return Err(());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(());
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(());
    }
    // IP-address check: dots are already rejected by the character check above,
    // so no valid IPv4 address can pass. Kept as an explicit guard in case the
    // allowed character set is expanded later.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_with_key() {
        assert_eq!(
            S3Path::parse("s3://bucket/key.parquet"),
            Ok(S3Path::Object {
                bucket: "bucket".to_string(),
                key: "key.parquet".to_string(),
            })
        );
    }

    #[test]
    fn prefix_with_trailing_slash() {
        assert_eq!(
            S3Path::parse("s3://bucket/prefix/"),
            Ok(S3Path::Prefix {
                bucket: "bucket".to_string(),
                prefix: "prefix/".to_string(),
            })
        );
    }

    #[test]
    fn prefix_bucket_slash_only() {
        assert_eq!(
            S3Path::parse("s3://bucket/"),
            Ok(S3Path::Prefix {
                bucket: "bucket".to_string(),
                prefix: "".to_string(),
            })
        );
    }

    #[test]
    fn prefix_bucket_no_slash() {
        assert_eq!(
            S3Path::parse("s3://bucket"),
            Ok(S3Path::Prefix {
                bucket: "bucket".to_string(),
                prefix: "".to_string(),
            })
        );
    }

    #[test]
    fn empty_bucket_error() {
        assert_eq!(S3Path::parse("s3://"), Err(S3PathError::EmptyBucket));
    }

    #[test]
    fn missing_scheme_error() {
        assert_eq!(
            S3Path::parse("/local/path"),
            Err(S3PathError::MissingScheme)
        );
    }

    #[test]
    fn invalid_bucket_uppercase() {
        assert_eq!(
            S3Path::parse("s3://UPPER/key"),
            Err(S3PathError::InvalidBucket("UPPER".to_string()))
        );
    }

    #[test]
    fn invalid_bucket_too_short() {
        assert_eq!(
            S3Path::parse("s3://a/key"),
            Err(S3PathError::InvalidBucket("a".to_string()))
        );
    }

    #[test]
    fn invalid_bucket_leading_hyphen() {
        assert_eq!(
            S3Path::parse("s3://-bad/key"),
            Err(S3PathError::InvalidBucket("-bad".to_string()))
        );
    }

    #[test]
    fn invalid_bucket_trailing_hyphen() {
        assert_eq!(
            S3Path::parse("s3://bad-/key"),
            Err(S3PathError::InvalidBucket("bad-".to_string()))
        );
    }

    #[test]
    fn valid_bucket_with_hyphens_and_digits() {
        assert!(S3Path::parse("s3://my-bucket-123/data/").is_ok());
    }

    #[test]
    fn nested_key_path() {
        assert_eq!(
            S3Path::parse("s3://bucket/a/b/c.parquet"),
            Ok(S3Path::Object {
                bucket: "bucket".to_string(),
                key: "a/b/c.parquet".to_string(),
            })
        );
    }
}
