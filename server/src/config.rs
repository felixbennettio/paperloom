//! Settings, from environment variables (see docs/sync-protocol.md).

use std::net::SocketAddr;
use std::path::PathBuf;

/// Where blobs are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobConfig {
    /// A folder (one machine).
    Fs(PathBuf),
    /// S3-compatible object storage: R2, OSS, COS, MinIO…
    S3 { endpoint: String, bucket: String, region: String, access_key_id: String, secret_access_key: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    /// The OIDC issuer whose access tokens are accepted.
    pub issuer: String,
    /// The API resource the tokens must be for (their `aud`).
    pub audience: String,
    pub blobs: BlobConfig,
    pub max_blob_bytes: usize,
}

pub const DEFAULT_ISSUER: &str = "https://api.sso.routeforge.zone/oidc";
pub const DEFAULT_AUDIENCE: &str = "https://sync.paperloom.routeforge.zone";

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()))
    }

    /// Reads the settings through `get`, for tests.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let need = |name: &str| get(name).ok_or_else(|| format!("{name} is not set"));
        let bind = get("PAPERLOOM_BIND").unwrap_or_else(|| "0.0.0.0:8080".into());
        let bind = bind.parse().map_err(|_| format!("PAPERLOOM_BIND is not an address: {bind}"))?;
        let blobs = match get("PAPERLOOM_BLOBS").as_deref().unwrap_or("fs:/var/lib/paperloom/blobs") {
            "s3" => BlobConfig::S3 {
                endpoint: need("S3_ENDPOINT")?,
                bucket: need("S3_BUCKET")?,
                region: get("S3_REGION").unwrap_or_else(|| "auto".into()),
                access_key_id: need("S3_ACCESS_KEY_ID")?,
                secret_access_key: need("S3_SECRET_ACCESS_KEY")?,
            },
            other => match other.strip_prefix("fs:") {
                Some(path) if !path.is_empty() => BlobConfig::Fs(PathBuf::from(path)),
                _ => return Err(format!("PAPERLOOM_BLOBS must be s3 or fs:<folder>, not {other}")),
            },
        };
        let megabytes = get("PAPERLOOM_MAX_BLOB_MB").unwrap_or_else(|| "512".into());
        let megabytes: usize =
            megabytes.parse().map_err(|_| format!("PAPERLOOM_MAX_BLOB_MB is not a number: {megabytes}"))?;
        Ok(Self {
            bind,
            database_url: need("DATABASE_URL")?,
            issuer: get("PAPERLOOM_ISSUER").unwrap_or_else(|| DEFAULT_ISSUER.into()),
            audience: get("PAPERLOOM_AUDIENCE").unwrap_or_else(|| DEFAULT_AUDIENCE.into()),
            blobs,
            max_blob_bytes: megabytes * 1024 * 1024,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(pairs: &[(&str, &str)]) -> Result<Config, String> {
        let env: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(|name| env.get(name).cloned())
    }

    #[test]
    fn reads_settings_with_defaults() {
        let c = config(&[("DATABASE_URL", "postgres://db/paperloom")]).unwrap();
        assert_eq!(c.bind.port(), 8080);
        assert_eq!(c.issuer, DEFAULT_ISSUER);
        assert_eq!(c.blobs, BlobConfig::Fs("/var/lib/paperloom/blobs".into()));
        assert_eq!(c.max_blob_bytes, 512 * 1024 * 1024);
        assert!(config(&[]).unwrap_err().contains("DATABASE_URL"));
        assert!(
            config(&[("DATABASE_URL", "x"), ("PAPERLOOM_BLOBS", "s3")]).unwrap_err().contains("S3_ENDPOINT")
        );
        assert!(config(&[("DATABASE_URL", "x"), ("PAPERLOOM_BLOBS", "ftp")]).is_err());
    }
}
