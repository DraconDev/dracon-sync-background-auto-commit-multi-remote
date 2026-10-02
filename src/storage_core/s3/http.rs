//! Signed, bounded S3 requests. Constructors take operator-resolved configuration;
//! there is no environment credential, proxy or endpoint discovery.

use anyhow::{bail, Result};
use hmac::{Hmac, Mac};
use reqwest::blocking::{Body, Client, Response};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, StatusCode, Url};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, SystemTime};
use zeroize::Zeroizing;

use super::{ConditionalPut, S3Transport};
use crate::storage_core::backend::BackendFailure;
use crate::storage_core::reference::{validate_sha256, Fingerprint};

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Credentials supplied by a trusted operator resolver, never from a repository.
/// Secret material has no Debug/Serialize implementation and is cleared on drop.
pub struct Credentials {
    access_key: Zeroizing<String>,
    secret: Zeroizing<String>,
    token: Option<Zeroizing<String>>,
    expires_at: Option<SystemTime>,
}

impl Credentials {
    /// Validate bounded header-safe credentials, including an optional session token.
    pub fn new(
        access_key: String,
        secret: String,
        token: Option<String>,
        expires_at: Option<SystemTime>,
    ) -> Result<Self> {
        let access_key = Zeroizing::new(access_key);
        let secret = Zeroizing::new(secret);
        let token = token.map(Zeroizing::new);
        if !(8..=128).contains(&access_key.len())
            || !access_key.bytes().all(|b| b.is_ascii_alphanumeric())
            || !(16..=256).contains(&secret.len())
            || !visible(&secret)
            || token
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 16384 || !visible(s))
        {
            bail!(BackendFailure::Security);
        }
        Ok(Self {
            access_key,
            secret,
            token,
            expires_at,
        })
    }

    fn valid_now(&self) -> Result<()> {
        if self
            .expires_at
            .is_some_and(|expiry| expiry <= SystemTime::now())
        {
            bail!(BackendFailure::Security);
        }
        Ok(())
    }
}

fn visible(s: &str) -> bool {
    s.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// Explicit operator-selected path-style endpoint and ciphertext-key namespace.
/// Provider capability approval remains separate from constructing this value.
pub struct HttpConfig {
    /// HTTPS origin with no credentials, query, fragment or base path.
    pub endpoint: String,
    /// Bucket identifier under the operator's control.
    pub bucket: String,
    /// Signing region (including provider-specific region names).
    pub region: String,
    /// Optional confined ASCII path components, without leading/trailing slash.
    pub prefix: String,
    /// Whole-request deadline, including a streaming response body.
    pub timeout: Duration,
}

/// Signed create-only/full-read HTTP adapter. No bucket administration is exposed.
pub struct SignedHttpTransport {
    client: Client,
    endpoint: Url,
    host: String,
    bucket: String,
    region: String,
    prefix: String,
    timeout: Duration,
    credentials: Credentials,
}

/// A redacted transport failure suitable for bounded durable-worker retry.
#[derive(Debug)]
pub struct TransientFailure;
impl std::fmt::Display for TransientFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("S3 transfer temporarily unavailable")
    }
}
impl std::error::Error for TransientFailure {}

impl SignedHttpTransport {
    /// Construct a HTTPS-only adapter; this performs no network requests.
    pub fn new(config: HttpConfig, credentials: Credentials) -> Result<Self> {
        Self::build(config, credentials, false)
    }

    fn build(config: HttpConfig, credentials: Credentials, test_loopback: bool) -> Result<Self> {
        let endpoint = Url::parse(&config.endpoint).map_err(|_| BackendFailure::Security)?;
        let loopback = test_loopback
            && endpoint.scheme() == "http"
            && endpoint.host_str() == Some("127.0.0.1");
        if (endpoint.scheme() != "https" && !loopback)
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/"
            || config.timeout.is_zero()
            || config.timeout > Duration::from_secs(3600)
            || !(3..=63).contains(&config.bucket.len())
            || !config
                .bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
            || !config.bucket.as_bytes()[0].is_ascii_alphanumeric()
            || !config.bucket.as_bytes()[config.bucket.len() - 1].is_ascii_alphanumeric()
            || config.bucket.contains("..")
            || config.region.is_empty()
            || config.region.len() > 64
            || !config
                .region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || config.prefix.len() > 1024
            || (!config.prefix.is_empty()
                && config.prefix.split('/').any(|part| {
                    part.is_empty()
                        || part == "."
                        || part == ".."
                        || !part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                }))
        {
            bail!(BackendFailure::Security);
        }
        credentials.valid_now()?;
        let host = endpoint
            .as_str()
            .split_once("://")
            .unwrap()
            .1
            .trim_end_matches('/')
            .to_owned();
        let client = Client::builder()
            .https_only(!loopback)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .timeout(config.timeout)
            .connect_timeout(config.timeout)
            .build()
            .map_err(|_| BackendFailure::Security)?;
        Ok(Self {
            client,
            endpoint,
            host,
            bucket: config.bucket,
            region: config.region,
            prefix: config.prefix,
            timeout: config.timeout,
            credentials,
        })
    }

    fn url(&self, identity: &Fingerprint) -> Result<Url> {
        identity.validate()?;
        let mut url = self.endpoint.clone();
        let path = if self.prefix.is_empty() {
            format!("/{}/{}", self.bucket, identity.sha256())
        } else {
            format!("/{}/{}/{}", self.bucket, self.prefix, identity.sha256())
        };
        // Confined ASCII components contain no percent escapes or dot segments;
        // the exact URL path sent is also the path signed.
        url.set_path(&path);
        Ok(url)
    }

    fn request(&self, method: Method, id: &Fingerprint, file: Option<File>) -> Result<Response> {
        self.credentials.valid_now()?;
        let url = self.url(id)?;
        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let hash = if file.is_some() {
            id.sha256()
        } else {
            EMPTY_SHA256
        };
        let mut headers = BTreeMap::from([
            ("host".to_owned(), self.host.clone()),
            ("x-amz-content-sha256".to_owned(), hash.to_owned()),
            ("x-amz-date".to_owned(), timestamp.clone()),
            ("accept-encoding".to_owned(), "identity".to_owned()),
        ]);
        if let Some(token) = &self.credentials.token {
            headers.insert("x-amz-security-token".into(), token.to_string());
        }
        if file.is_some() {
            headers.insert("if-none-match".into(), "*".into());
            headers.insert("content-length".into(), id.bytes().to_string());
        }
        let authorization = sign(
            method.as_str(),
            url.path(),
            hash,
            &headers,
            &timestamp,
            &self.region,
            &self.credentials,
        )?;
        let mut outgoing = HeaderMap::new();
        for (name, value) in headers {
            let name =
                HeaderName::from_bytes(name.as_bytes()).map_err(|_| BackendFailure::Security)?;
            let mut value = HeaderValue::from_str(&value).map_err(|_| BackendFailure::Security)?;
            if name == "x-amz-security-token" {
                value.set_sensitive(true);
            }
            outgoing.insert(name, value);
        }
        let mut authorization =
            HeaderValue::from_str(&authorization).map_err(|_| BackendFailure::Security)?;
        authorization.set_sensitive(true);
        outgoing.insert(reqwest::header::AUTHORIZATION, authorization);
        // Explicit request timeout also sets reqwest's asynchronous total body
        // deadline, preventing trickled bytes from extending the transfer forever.
        let mut request = self
            .client
            .request(method, url)
            .headers(outgoing)
            .timeout(self.timeout);
        if let Some(mut file) = file {
            if !file.metadata()?.is_file() || file.metadata()?.len() != id.bytes() {
                bail!(BackendFailure::Integrity);
            }
            file.seek(SeekFrom::Start(0))?;
            request = request.body(Body::sized(file, id.bytes()));
        }
        request.send().map_err(|_| TransientFailure.into())
    }
}

fn status_failure(status: StatusCode) -> anyhow::Error {
    match status.as_u16() {
        401 | 403 => BackendFailure::Security.into(),
        404 | 412 | 416 => BackendFailure::Integrity.into(),
        413 | 507 => BackendFailure::Capacity.into(),
        408 | 409 | 429 | 500..=599 => TransientFailure.into(),
        _ => BackendFailure::Security.into(),
    }
}

struct RedactedBody(Response);
impl Read for RedactedBody {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        self.0
            .read(output)
            .map_err(|_| std::io::Error::other(TransientFailure))
    }
}

impl S3Transport for SignedHttpTransport {
    fn put_if_absent(&self, identity: &Fingerprint, source: File) -> Result<ConditionalPut> {
        let response = self.request(Method::PUT, identity, Some(source))?;
        match response.status() {
            StatusCode::OK => Ok(ConditionalPut::Created),
            StatusCode::PRECONDITION_FAILED => Ok(ConditionalPut::AlreadyExists),
            status => Err(status_failure(status)),
        }
        // Never read or expose provider error bodies.
    }

    fn get(&self, identity: &Fingerprint) -> Result<Box<dyn Read>> {
        let response = self.request(Method::GET, identity, None)?;
        if response.status() != StatusCode::OK {
            return Err(status_failure(response.status()));
        }
        if response
            .headers()
            .contains_key(reqwest::header::CONTENT_RANGE)
            || response
                .headers()
                .get(reqwest::header::CONTENT_ENCODING)
                .is_some_and(|value| value != "identity")
            || response
                .content_length()
                .is_some_and(|length| length != identity.bytes())
        {
            bail!(BackendFailure::Integrity);
        }
        Ok(Box::new(RedactedBody(response)))
    }
}

fn hmac(key: &[u8], data: &[u8]) -> Zeroizing<Vec<u8>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC supports any key length");
    mac.update(data);
    Zeroizing::new(mac.finalize().into_bytes().to_vec())
}

fn sign(
    method: &str,
    path: &str,
    payload: &str,
    headers: &BTreeMap<String, String>,
    timestamp: &str,
    region: &str,
    credentials: &Credentials,
) -> Result<Zeroizing<String>> {
    validate_sha256(payload)?;
    if timestamp.len() != 16
        || !timestamp.is_ascii()
        || &timestamp[8..9] != "T"
        || &timestamp[15..] != "Z"
        || !timestamp[..8]
            .bytes()
            .chain(timestamp[9..15].bytes())
            .all(|b| b.is_ascii_digit())
    {
        bail!(BackendFailure::Security);
    }
    let canonical_headers = headers
        .iter()
        .map(|(name, value)| format!("{name}:{}\n", value.trim()))
        .collect::<String>();
    let signed = headers
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(";");
    let canonical = Zeroizing::new(format!(
        "{method}\n{path}\n\n{canonical_headers}\n{signed}\n{payload}"
    ));
    let scope = format!("{}/{region}/s3/aws4_request", &timestamp[..8]);
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{:x}",
        Sha256::digest(canonical.as_bytes())
    );
    let base = Zeroizing::new(format!("AWS4{}", *credentials.secret));
    let date = hmac(base.as_bytes(), &timestamp.as_bytes()[..8]);
    let region_key = hmac(&date, region.as_bytes());
    let service = hmac(&region_key, b"s3");
    let key = hmac(&service, b"aws4_request");
    let signature = hmac(&key, to_sign.as_bytes());
    let signature = signature
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(Zeroizing::new(format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope},SignedHeaders={signed},Signature={signature}",
        *credentials.access_key
    )))
}

#[cfg(test)]
mod tests;
