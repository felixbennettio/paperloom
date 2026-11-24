//! Attachments and snapshots, by library and hash, in a folder or an
//! S3-compatible bucket (through OpenDAL).

use axum::body::Bytes;
use opendal::{Operator, services};
use paperloom_sync_proto::BlobHash;
use uuid::Uuid;

use crate::config::BlobConfig;
use crate::error::ApiError;

pub struct Blobs {
    operator: Operator,
}

impl Blobs {
    pub fn open(config: &BlobConfig) -> Result<Self, opendal::Error> {
        let operator = match config {
            BlobConfig::Fs(root) => {
                Operator::new(services::Fs::default().root(&root.display().to_string()))?.finish()
            }
            BlobConfig::S3 { endpoint, bucket, region, access_key_id, secret_access_key } => Operator::new(
                services::S3::default()
                    .endpoint(endpoint)
                    .bucket(bucket)
                    .region(region)
                    .access_key_id(access_key_id)
                    .secret_access_key(secret_access_key),
            )?
            .finish(),
        };
        Ok(Self { operator })
    }

    /// Kept in memory, for tests.
    pub fn memory() -> Result<Self, opendal::Error> {
        Ok(Self { operator: Operator::new(services::Memory::default())?.finish() })
    }

    fn path(library: Uuid, hash: &BlobHash) -> String {
        let hash = hash.as_str();
        format!("libraries/{library}/blobs/{}/{hash}", &hash[..2])
    }

    pub async fn put(&self, library: Uuid, hash: &BlobHash, bytes: Bytes) -> Result<(), ApiError> {
        self.operator.write(&Self::path(library, hash), bytes).await?;
        Ok(())
    }

    pub async fn get(&self, library: Uuid, hash: &BlobHash) -> Result<Vec<u8>, ApiError> {
        Ok(self.operator.read(&Self::path(library, hash)).await?.to_vec())
    }
}
