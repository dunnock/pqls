use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::operation::head_object::HeadObjectError;
use aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Error;
use std::fmt;

#[derive(Debug)]
pub enum S3Error {
    BucketNotFound(String),
    ObjectNotFound(String),
    AccessDenied(String),
    InvalidCredentials,
    NoCredentials,
    RegionMismatch { location: String, region: String },
    NotParquet(String),
    FileTooSmall { location: String, size: u64 },
    Network { location: String, source: String },
}

impl fmt::Display for S3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            S3Error::BucketNotFound(b) => write!(f, "s3://{b}: bucket does not exist"),
            S3Error::ObjectNotFound(p) => write!(f, "s3://{p}: object not found"),
            S3Error::AccessDenied(p) => write!(
                f,
                "s3://{p}: access denied — ensure IAM policy allows s3:GetObject (and s3:ListBucket for prefix listing)"
            ),
            S3Error::InvalidCredentials => write!(
                f,
                "AWS credentials invalid — check AWS_ACCESS_KEY_ID / AWS_PROFILE"
            ),
            S3Error::NoCredentials => write!(
                f,
                "No AWS credentials found — set AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY, configure ~/.aws/credentials, or attach an IAM role"
            ),
            S3Error::RegionMismatch { location, region } => write!(
                f,
                "s3://{location}: bucket is in region {region} — set AWS_REGION={region}"
            ),
            S3Error::NotParquet(p) => {
                write!(f, "s3://{p}: not a valid parquet file (bad footer magic)")
            }
            S3Error::FileTooSmall { location, size } => write!(
                f,
                "s3://{location}: file too small to be valid parquet ({size} bytes)"
            ),
            S3Error::Network { location, source } => {
                write!(f, "fetching s3://{location}: {source}")
            }
        }
    }
}

impl std::error::Error for S3Error {}

fn classify_code(code: &str) -> &'static str {
    match code {
        "NoSuchBucket" => "no_such_bucket",
        "NoSuchKey" | "NotFound" => "not_found",
        "AccessDenied" => "access_denied",
        "InvalidClientTokenId" | "AuthFailure" | "InvalidAccessKeyId" => "invalid_credentials",
        "NoCredentialProviders" | "CredentialsError" => "no_credentials",
        "PermanentRedirect" | "AuthorizationHeaderMalformed" => "region_mismatch",
        _ => "",
    }
}

pub fn from_head_err(err: SdkError<HeadObjectError>, location: &str) -> S3Error {
    if let SdkError::ServiceError(se) = &err {
        let code = se.err().code().unwrap_or("");
        match classify_code(code) {
            "no_such_bucket" => return S3Error::BucketNotFound(location.to_string()),
            "not_found" => return S3Error::ObjectNotFound(location.to_string()),
            "access_denied" => return S3Error::AccessDenied(location.to_string()),
            "invalid_credentials" => return S3Error::InvalidCredentials,
            "no_credentials" => return S3Error::NoCredentials,
            "region_mismatch" => {
                return S3Error::RegionMismatch {
                    location: location.to_string(),
                    region: "the correct region".to_string(),
                }
            }
            _ => {}
        }
        // HEAD responses carry no body, so the SDK can't extract the error code.
        // Fall back to the HTTP status code for the most important cases.
        return match se.raw().status().as_u16() {
            403 => S3Error::AccessDenied(location.to_string()),
            404 => S3Error::ObjectNotFound(location.to_string()),
            _ => S3Error::Network {
                location: location.to_string(),
                source: se.err().to_string(),
            },
        };
    }
    S3Error::Network {
        location: location.to_string(),
        source: err.to_string(),
    }
}

pub fn from_get_err(err: SdkError<GetObjectError>, location: &str) -> S3Error {
    if let SdkError::ServiceError(se) = &err {
        let code = se.err().code().unwrap_or("");
        match classify_code(code) {
            "no_such_bucket" => return S3Error::BucketNotFound(location.to_string()),
            "not_found" => return S3Error::ObjectNotFound(location.to_string()),
            "access_denied" => return S3Error::AccessDenied(location.to_string()),
            "invalid_credentials" => return S3Error::InvalidCredentials,
            "no_credentials" => return S3Error::NoCredentials,
            "region_mismatch" => {
                return S3Error::RegionMismatch {
                    location: location.to_string(),
                    region: "the correct region".to_string(),
                }
            }
            _ => {}
        }
        return S3Error::Network {
            location: location.to_string(),
            source: se.err().to_string(),
        };
    }
    S3Error::Network {
        location: location.to_string(),
        source: err.to_string(),
    }
}

pub fn from_list_err(err: SdkError<ListObjectsV2Error>, location: &str) -> S3Error {
    if let SdkError::ServiceError(se) = &err {
        let code = se.err().code().unwrap_or("");
        match classify_code(code) {
            "no_such_bucket" => return S3Error::BucketNotFound(location.to_string()),
            "access_denied" => return S3Error::AccessDenied(location.to_string()),
            "invalid_credentials" => return S3Error::InvalidCredentials,
            "no_credentials" => return S3Error::NoCredentials,
            "region_mismatch" => {
                return S3Error::RegionMismatch {
                    location: location.to_string(),
                    region: "the correct region".to_string(),
                }
            }
            _ => {}
        }
        return S3Error::Network {
            location: location.to_string(),
            source: se.err().to_string(),
        };
    }
    S3Error::Network {
        location: location.to_string(),
        source: err.to_string(),
    }
}

// Required From impls per spec — path context is "unknown"; prefer the named helpers above.
impl From<SdkError<HeadObjectError>> for S3Error {
    fn from(err: SdkError<HeadObjectError>) -> Self {
        from_head_err(err, "unknown")
    }
}

impl From<SdkError<GetObjectError>> for S3Error {
    fn from(err: SdkError<GetObjectError>) -> Self {
        from_get_err(err, "unknown")
    }
}

impl From<SdkError<ListObjectsV2Error>> for S3Error {
    fn from(err: SdkError<ListObjectsV2Error>) -> Self {
        from_list_err(err, "unknown")
    }
}
