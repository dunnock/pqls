use anyhow::Result;
use bytes::Bytes;
use parquet::errors::ParquetError;
use parquet::file::reader::{ChunkReader, Length};
use std::io::Cursor;

use super::error;

const FOOTER_ESTIMATE: u64 = 65_536; // 64 KiB

/// In-memory buffer holding the parquet footer region of an S3 object.
///
/// `bytes` contains the last `bytes.len()` bytes of the file (at minimum the
/// full metadata blob + 8-byte trailer).  `file_size` is the full S3 object
/// size and is returned by `Length::len()` so the parquet reader sees the
/// correct logical file length when computing absolute footer offsets.
pub struct FooterBuffer {
    pub bytes: Bytes,
    pub file_size: u64,
}

impl Length for FooterBuffer {
    fn len(&self) -> u64 {
        self.file_size
    }
}

impl ChunkReader for FooterBuffer {
    type T = Cursor<Bytes>;

    fn get_read(&self, start: u64) -> parquet::errors::Result<Self::T> {
        let offset = self.abs_to_rel(start)?;
        Ok(Cursor::new(self.bytes.slice(offset..)))
    }

    fn get_bytes(&self, start: u64, length: usize) -> parquet::errors::Result<Bytes> {
        let offset = self.abs_to_rel(start)?;
        let end = offset + length;
        if end > self.bytes.len() {
            return Err(ParquetError::General(format!(
                "footer buffer read [{offset}..{end}) exceeds buffer length {}",
                self.bytes.len()
            )));
        }
        Ok(self.bytes.slice(offset..end))
    }
}

impl FooterBuffer {
    fn footer_start(&self) -> u64 {
        self.file_size - self.bytes.len() as u64
    }

    fn abs_to_rel(&self, start: u64) -> parquet::errors::Result<usize> {
        let footer_start = self.footer_start();
        if start < footer_start {
            return Err(ParquetError::General(format!(
                "requested offset {start} is before footer buffer start {footer_start}"
            )));
        }
        Ok((start - footer_start) as usize)
    }
}

/// Fetches just the footer bytes of a Parquet object from S3.
///
/// Algorithm (two-step range-get per design §5):
/// 1. HEAD → Content-Length
/// 2. GET bytes=-65536 (or whole file if smaller)
/// 3. Validate PAR1 magic; read metadata_len
/// 4. If metadata_len+8 > 65536, issue a second GET for the exact metadata range
pub async fn lazy_fetch_footer(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    key: &str,
) -> Result<FooterBuffer> {
    let location = format!("{bucket}/{key}");

    // Step 1: HEAD to determine file size
    let head = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| error::from_head_err(e, &location))?;

    let file_size = head
        .content_length()
        .ok_or_else(|| anyhow::anyhow!("HEAD s3://{location}: missing Content-Length"))? as u64;

    if file_size < 8 {
        return Err(error::S3Error::FileTooSmall {
            location,
            size: file_size,
        }
        .into());
    }

    // Step 2: range GET — up to FOOTER_ESTIMATE bytes from the tail
    let range_start = file_size.saturating_sub(FOOTER_ESTIMATE);
    let range = format!("bytes={range_start}-{}", file_size - 1);

    let resp = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .range(range)
        .send()
        .await
        .map_err(|e| error::from_get_err(e, &location))?;

    let buf: Bytes = resp
        .body
        .collect()
        .await
        .map_err(|e| anyhow::anyhow!("fetching s3://{location}: {e}"))?
        .into_bytes();

    let buf_len = buf.len();
    if buf_len < 8 {
        return Err(error::S3Error::NotParquet(location).into());
    }

    // Step 3: validate PAR1 magic at tail
    if &buf[buf_len - 4..] != b"PAR1" {
        return Err(error::S3Error::NotParquet(location).into());
    }

    // metadata_len is the 4 LE bytes immediately before the magic
    let metadata_len =
        u32::from_le_bytes(buf[buf_len - 8..buf_len - 4].try_into().unwrap()) as u64;
    let footer_size = metadata_len + 8;

    let footer_bytes = if footer_size <= FOOTER_ESTIMATE {
        // Full footer already in the buffer; slice to just the footer region
        let footer_offset = (file_size - footer_size - range_start) as usize;
        buf.slice(footer_offset..)
    } else {
        // Step 4: second GET for exact metadata range
        let meta_range = format!("bytes={}-{}", file_size - footer_size, file_size - 1);
        let resp2 = client
            .get_object()
            .bucket(bucket)
            .key(key)
            .range(meta_range)
            .send()
            .await
            .map_err(|e| error::from_get_err(e, &location))?;

        resp2
            .body
            .collect()
            .await
            .map_err(|e| anyhow::anyhow!("fetching s3://{location}: {e}"))?
            .into_bytes()
    };

    Ok(FooterBuffer {
        bytes: footer_bytes,
        file_size,
    })
}
