//! S3 and the services that speak its API (Cloudflare R2, MinIO, Backblaze
//! B2, 阿里云 OSS, 腾讯云 COS…) as a place to sync through. Requests are
//! signed with AWS Signature Version 4; the bucket must exist.

use std::time::{SystemTime, UNIX_EPOCH};

use paperloom_http::{BytesResponse, Http, Request};
use paperloom_sync::SyncError;
use sha2::{Digest, Sha256};

use super::Shelf;

/// A bucket (or a folder in it) to sync through.
pub(crate) struct S3<'a> {
    pub http: &'a dyn Http,
    /// `https://s3.us-east-1.amazonaws.com`, `https://<account>.r2.cloudflarestorage.com`,
    /// `http://nas.local:9000`.
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    /// A folder in the bucket (`paperloom/`), or empty for its top.
    pub prefix: String,
    pub access_key: String,
    pub secret_key: String,
}

/// RFC 3986 unreserved characters stay; everything else is `%XX`.
fn encode(text: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// HMAC-SHA256 (RFC 2104).
fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// `20130524T000000Z` for a time, and its date.
fn amz_date(seconds: u64) -> (String, String) {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Days since 1970-01-01 as a date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let date = format!("{year:04}{month:02}{day:02}");
    let time = format!("{date}T{:02}{:02}{:02}Z", rest / 3_600, rest % 3_600 / 60, rest % 60);
    (date, time)
}

/// What a request needs signed.
pub(crate) struct Unsigned<'a> {
    pub method: &'a str,
    pub host: &'a str,
    /// Already encoded, from `/`.
    pub path: &'a str,
    /// Names and values, not encoded.
    pub query: &'a [(&'a str, &'a str)],
    /// Besides `host`, `x-amz-date` and `x-amz-content-sha256`.
    pub headers: &'a [(&'a str, &'a str)],
    pub payload_hash: &'a str,
}

/// The `Authorization` header for a request at a time (`20130524T000000Z`).
pub(crate) fn authorization(
    request: &Unsigned<'_>,
    amz_time: &str,
    region: &str,
    access_key: &str,
    secret_key: &str,
) -> String {
    let date = &amz_time[..8];
    let mut query: Vec<(String, String)> =
        request.query.iter().map(|(k, v)| (encode(k, false), encode(v, false))).collect();
    query.sort();
    let query = query.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
    let mut headers: Vec<(String, String)> = vec![
        ("host".into(), request.host.to_string()),
        ("x-amz-content-sha256".into(), request.payload_hash.to_string()),
        ("x-amz-date".into(), amz_time.to_string()),
    ];
    headers.extend(request.headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string())));
    headers.sort();
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed = headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
    let canonical = format!(
        "{}\n{}\n{query}\n{canonical_headers}\n{signed}\n{}",
        request.method, request.path, request.payload_hash
    );
    let scope = format!("{date}/{region}/s3/aws4_request");
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz_time}\n{scope}\n{}", sha256_hex(canonical.as_bytes()));
    let key = [date, region, "s3", "aws4_request"]
        .iter()
        .fold(format!("AWS4{secret_key}").into_bytes(), |key, part| hmac(&key, part.as_bytes()).to_vec());
    let signature = hex(&hmac(&key, to_sign.as_bytes()));
    format!("AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed}, Signature={signature}")
}

impl S3<'_> {
    /// `scheme://host` and the host. Services that only take a bucket in
    /// the host name (AWS, OSS, COS) get it there; others get it in the path.
    fn place(&self) -> Result<(String, String, bool), SyncError> {
        let endpoint = self.endpoint.trim().trim_end_matches('/');
        let (scheme, host) = endpoint
            .split_once("://")
            .filter(|(scheme, host)| {
                matches!(*scheme, "https" | "http") && !host.is_empty() && !host.contains('/')
            })
            .ok_or_else(|| SyncError::Transport("An S3 address looks like https://s3.example.com.".into()))?;
        let virtual_host =
            ["amazonaws.com", "aliyuncs.com", "myqcloud.com"].iter().any(|d| host.ends_with(d));
        Ok(if virtual_host {
            (format!("{scheme}://{}.{host}", self.bucket), format!("{}.{host}", self.bucket), true)
        } else {
            (format!("{scheme}://{host}"), host.to_string(), false)
        })
    }

    /// An object's key: the path under the prefix.
    fn key(&self, path: &str) -> String {
        let prefix = self.prefix.trim_matches('/');
        match (prefix.is_empty(), path.is_empty()) {
            (true, _) => path.to_string(),
            (false, true) => prefix.to_string(),
            (false, false) => format!("{prefix}/{path}"),
        }
    }

    fn send(
        &self,
        method: &'static str,
        key: Option<&str>,
        query: &[(&str, &str)],
        body: Option<Vec<u8>>,
    ) -> Result<BytesResponse, SyncError> {
        let (base, host, virtual_host) = self.place()?;
        let mut path =
            if virtual_host { String::from("/") } else { format!("/{}/", encode(&self.bucket, false)) };
        if let Some(key) = key {
            path.push_str(&encode(key, true));
        }
        let payload_hash = sha256_hex(body.as_deref().unwrap_or_default());
        let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let (_, amz_time) = amz_date(seconds);
        let unsigned =
            Unsigned { method, host: &host, path: &path, query, headers: &[], payload_hash: &payload_hash };
        let signature = authorization(&unsigned, &amz_time, &self.region, &self.access_key, &self.secret_key);
        let mut url = format!("{base}{path}");
        if !query.is_empty() {
            let pairs: Vec<String> =
                query.iter().map(|(k, v)| format!("{}={}", encode(k, false), encode(v, false))).collect();
            url = format!("{url}?{}", pairs.join("&"));
        }
        let headers = vec![
            ("Authorization".to_string(), signature),
            ("x-amz-date".to_string(), amz_time),
            ("x-amz-content-sha256".to_string(), payload_hash),
        ];
        let response =
            self.http.send(Request { method, url, headers, body }).map_err(|e| SyncError::Transport(e.0))?;
        if response.status == 401 || response.status == 403 {
            return Err(SyncError::Transport(format!(
                "S3 did not accept the keys (HTTP {}): check the access key, the secret, the region and the bucket.",
                response.status
            )));
        }
        Ok(response)
    }
}

fn failure(method: &str, path: &str, response: &BytesResponse) -> SyncError {
    SyncError::Transport(format!("S3 {method} {path} answered HTTP {}.", response.status))
}

impl Shelf for S3<'_> {
    fn put(&self, path: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
        let response = self.send("PUT", Some(&self.key(path)), &[], Some(bytes))?;
        if !response.is_success() {
            return Err(failure("PUT", path, &response));
        }
        Ok(())
    }

    fn get(&self, path: &str) -> Result<Option<Vec<u8>>, SyncError> {
        let response = self.send("GET", Some(&self.key(path)), &[], None)?;
        match response.status {
            404 => Ok(None),
            _ if response.is_success() => Ok(Some(response.body)),
            _ => Err(failure("GET", path, &response)),
        }
    }

    fn exists(&self, path: &str) -> Result<bool, SyncError> {
        let response = self.send("HEAD", Some(&self.key(path)), &[], None)?;
        match response.status {
            404 => Ok(false),
            _ if response.is_success() => Ok(true),
            _ => Err(failure("HEAD", path, &response)),
        }
    }

    /// Objects and "folders" (common prefixes) right under a folder, page
    /// by page.
    fn list(&self, folder: &str) -> Result<Vec<String>, SyncError> {
        let folder_key = self.key(folder.trim_matches('/'));
        let prefix = if folder_key.is_empty() { String::new() } else { format!("{folder_key}/") };
        let mut names = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut query = vec![("list-type", "2"), ("delimiter", "/"), ("prefix", prefix.as_str())];
            if let Some(token) = &token {
                query.push(("continuation-token", token.as_str()));
            }
            let response = self.send("GET", None, &query, None)?;
            if !response.is_success() {
                return Err(failure("LIST", folder, &response));
            }
            let page = Listing::read(&String::from_utf8_lossy(&response.body));
            names.extend(
                page.keys
                    .iter()
                    .chain(&page.folders)
                    .filter_map(|key| key.strip_prefix(&prefix))
                    .map(|name| name.trim_end_matches('/').to_string())
                    .filter(|name| !name.is_empty()),
            );
            match page.next {
                Some(next) if page.truncated => token = Some(next),
                _ => return Ok(names),
            }
        }
    }
}

/// One page of a ListObjectsV2 answer.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Listing {
    pub keys: Vec<String>,
    pub folders: Vec<String>,
    pub truncated: bool,
    pub next: Option<String>,
}

impl Listing {
    pub(crate) fn read(xml: &str) -> Self {
        let mut listing = Listing::default();
        for (tag, text) in elements(xml) {
            match tag {
                "Key" => listing.keys.push(text),
                "IsTruncated" => listing.truncated = text == "true",
                "NextContinuationToken" => listing.next = Some(text),
                _ => {}
            }
        }
        for part in xml.split("<CommonPrefixes>").skip(1) {
            let inside = part.split("</CommonPrefixes>").next().unwrap_or_default();
            listing
                .folders
                .extend(elements(inside).filter(|(tag, _)| *tag == "Prefix").map(|(_, text)| text));
        }
        listing
    }
}

/// `<Tag>text</Tag>` leaves of an XML answer, text unescaped.
fn elements(xml: &str) -> impl Iterator<Item = (&str, String)> {
    xml.split('<').skip(1).filter_map(|piece| {
        let (tag, text) = piece.split_once('>')?;
        let tag = tag.split_whitespace().next()?;
        if tag.starts_with('/') || tag.ends_with('/') || text.trim().is_empty() {
            return None;
        }
        let text = text
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&");
        Some((tag, text))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const ACCESS: &str = "AKIAIOSFODNN7EXAMPLE";
    const SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

    #[test]
    fn hmac_matches_rfc_4231() {
        assert_eq!(
            hex(&hmac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    /// The examples in AWS's “Authenticating Requests: Using the
    /// Authorization Header” guide.
    #[test]
    fn signs_like_aws_examples() {
        let get = Unsigned {
            method: "GET",
            host: "examplebucket.s3.amazonaws.com",
            path: "/test.txt",
            query: &[],
            headers: &[("Range", "bytes=0-9")],
            payload_hash: EMPTY,
        };
        assert_eq!(
            authorization(&get, "20130524T000000Z", "us-east-1", ACCESS, SECRET),
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
        let list = Unsigned {
            method: "GET",
            host: "examplebucket.s3.amazonaws.com",
            path: "/",
            query: &[("prefix", "J"), ("max-keys", "2")],
            headers: &[],
            payload_hash: EMPTY,
        };
        assert!(
            authorization(&list, "20130524T000000Z", "us-east-1", ACCESS, SECRET)
                .ends_with("Signature=34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7")
        );
    }

    #[test]
    fn dates_and_encoding() {
        assert_eq!(amz_date(1_369_353_600), ("20130524".into(), "20130524T000000Z".into()));
        assert_eq!(amz_date(951_782_400 + 3_661), ("20000229".into(), "20000229T010101Z".into()));
        assert_eq!(encode("blobs/ab/x y+z", true), "blobs/ab/x%20y%2Bz");
        assert_eq!(encode("a/b", false), "a%2Fb");
    }

    #[test]
    fn reads_listings() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Name>bucket</Name><Prefix>p/changes/</Prefix><KeyCount>2</KeyCount>
  <IsTruncated>true</IsTruncated><NextContinuationToken>1ueGcxL+tok&amp;en</NextContinuationToken>
  <Contents><Key>p/changes/readme &amp; notes.txt</Key><Size>3</Size></Contents>
  <CommonPrefixes><Prefix>p/changes/0190a0d4/</Prefix></CommonPrefixes>
  <CommonPrefixes><Prefix>p/changes/0190a0d5/</Prefix></CommonPrefixes>
</ListBucketResult>"#;
        let page = Listing::read(xml);
        assert_eq!(page.keys, ["p/changes/readme & notes.txt"]);
        assert_eq!(page.folders, ["p/changes/0190a0d4/", "p/changes/0190a0d5/"]);
        assert!(page.truncated);
        assert_eq!(page.next.as_deref(), Some("1ueGcxL+tok&en"));
    }
}
