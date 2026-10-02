use super::*;
use crate::storage_core::backend::ImmutableBackend;
use crate::storage_core::s3::S3Backend;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

fn credentials() -> Credentials {
    // Published AWS test credentials; never load operator keys in these fixtures.
    Credentials::new(
        "[DRACON_SECRET:YWdlLWVuY3J5cHRpb24ub3JnL3YxCi0+IFgyNTUxOSB3WjlBVTl6UGI1Zkp0QkFxZEcyRmVwL216T2VoYWZ1ajA5ZVdHODBlSUdVCm1POHA0Vzk0SjBqcFdlU0FJWCtYeURoc292RzJMUXBac0l0MWN0OFdCSkUKLT4gWDI1NTE5IG4xRFVZa25xZkxsaDRDMkZmeUZwamh2djZ4TFJCdTNvL3Z3UmFyeTQ0encKc25XdlluV2FTdVk0b0hVeUx5RnBmVkxMaWkzRjZOUFFrRVR2WkxWcWx6QQotPiBYMjU1MTkgWFpYcWpKUnVIUnNhQ1FlS1dSYU9FR0thMlVlVTRuaHh0TVdMOHA2WjJ6bwpENElPeGVxWC94M2xUQnpCOGpUU2l3M0Z1TG5nYVdnTytLck1iMSt0K09RCi0+IFgyNTUxOSBEVkFtNS9OUDFZTnVtNkhKWWY3UjVZQWFSd00wNTZtZVlFSG1DWDB2MEgwCkFsSUtXZURIMFVDcTk5Z2c4OGhFczlTclBqckNETmZoY1d4T0I3cXZnMlEKLT4gWDI1NTE5IEdndUNNOFE3OW5PdVMreUo2Ym9hRHhjRUJUQ0NDUGViTW1XdUV0OTg5M1UKKy9qcFNVUXFYZ1VSRlpRNGdHZStnaGFFVmhtODRGZ2g4UTdDVFRnYTB3dwotPiBWWXEwLWdyZWFzZQp2UTZiZGRrMDVSdlByTFIvTWh1WFRTdDIreFhnRjd1aXpML1dveDhvbHpvejJZV21KcENsRU9kYWRYQmpXNXFhCkIxVDFoU2hieUs2VWlIeFdmb2NQaVpYUmZiQmZjVmxwZjVQK00zSXoKLS0tIDFmZlJmRCtLdW1kNkxKdnc3TU9DbmN6TkJ2dXg3T1N5L1NqZk5Qdktldm8KyeQNl6V4lXg4jhOFlZ3PbdkzXsXT6ddckivX/KZljLKbMC+ujty6tGLPDKC6Q4yQapDnnA==]".into(),
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
        None,
        None,
    )
    .unwrap()
}
fn config(endpoint: &str) -> HttpConfig {
    HttpConfig {
        endpoint: endpoint.into(),
        bucket: "fixture-bucket".into(),
        region: "us-east-1".into(),
        prefix: "encrypted/v1".into(),
        timeout: Duration::from_secs(2),
    }
}
fn identity(bytes: &[u8]) -> Fingerprint {
    Fingerprint::new(format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64).unwrap()
}

#[test]
fn aws_published_get_and_put_signatures_match() {
    // https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sig-v4-header-based-auth.html
    let timestamp = "20130524T000000Z";
    let get = BTreeMap::from([
        ("host".into(), "examplebucket.s3.amazonaws.com".into()),
        ("range".into(), "bytes=0-9".into()),
        ("x-amz-content-sha256".into(), EMPTY_SHA256.into()),
        ("x-amz-date".into(), timestamp.into()),
    ]);
    let auth = sign(
        "GET",
        "/test.txt",
        EMPTY_SHA256,
        &get,
        timestamp,
        "us-east-1",
        &credentials(),
    )
    .unwrap();
    assert!(auth
        .ends_with("Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"));
    let payload = "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072";
    let put = BTreeMap::from([
        ("date".into(), "Fri, 24 May 2013 00:00:00 GMT".into()),
        ("host".into(), "examplebucket.s3.amazonaws.com".into()),
        ("x-amz-content-sha256".into(), payload.into()),
        ("x-amz-date".into(), timestamp.into()),
        ("x-amz-storage-class".into(), "REDUCED_REDUNDANCY".into()),
    ]);
    let auth = sign(
        "PUT",
        "/test%24file.text",
        payload,
        &put,
        timestamp,
        "us-east-1",
        &credentials(),
    )
    .unwrap();
    assert!(auth
        .ends_with("Signature=98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"));
}

#[test]
fn conditional_header_and_session_token_are_bound_to_signature() {
    let timestamp = "20130524T000000Z";
    let creds = Credentials::new(
        "TESTACCESS123".into(),
        "test-secret-never-live-123".into(),
        Some("session-token".into()),
        None,
    )
    .unwrap();
    let mut headers = BTreeMap::from([
        ("host".into(), "objects.invalid".into()),
        ("if-none-match".into(), "*".into()),
        ("x-amz-security-token".into(), "session-token".into()),
        ("x-amz-content-sha256".into(), EMPTY_SHA256.into()),
        ("x-amz-date".into(), timestamp.into()),
    ]);
    let original = sign(
        "PUT",
        "/bucket/object",
        EMPTY_SHA256,
        &headers,
        timestamp,
        "auto",
        &creds,
    )
    .unwrap();
    assert!(original.contains(
        "SignedHeaders=host;if-none-match;x-amz-content-sha256;x-amz-date;x-amz-security-token,"
    ));
    headers.remove("if-none-match");
    assert_ne!(
        *original,
        *sign(
            "PUT",
            "/bucket/object",
            EMPTY_SHA256,
            &headers,
            timestamp,
            "auto",
            &creds
        )
        .unwrap()
    );
    headers.insert("if-none-match".into(), "*".into());
    headers.insert("x-amz-security-token".into(), "other-token".into());
    assert_ne!(
        *original,
        *sign(
            "PUT",
            "/bucket/object",
            EMPTY_SHA256,
            &headers,
            timestamp,
            "auto",
            &creds
        )
        .unwrap()
    );
    assert_ne!(
        *original,
        *sign(
            "PUT",
            "/bucket/other",
            EMPTY_SHA256,
            &headers,
            timestamp,
            "us-east-1",
            &creds
        )
        .unwrap()
    );
}

#[test]
fn endpoints_credentials_and_namespace_fail_closed_without_echoing_input() {
    for endpoint in [
        "http://127.0.0.1:1",
        "https://user:password@objects.invalid/",
        "https://objects.invalid/base/",
        "https://objects.invalid/?secret=value",
        "https://objects.invalid/#fragment",
        "broken",
    ] {
        let error = SignedHttpTransport::new(config(endpoint), credentials())
            .err()
            .unwrap();
        assert_eq!(error.to_string(), BackendFailure::Security.to_string());
    }
    for prefix in [
        "../assets",
        "assets/../other",
        "/assets",
        "assets/",
        "assets//v1",
        "assets/%2e%2e",
        "assets/a b",
    ] {
        let mut config = config("https://objects.invalid");
        config.prefix = prefix.into();
        assert!(SignedHttpTransport::new(config, credentials()).is_err());
    }
    for region in ["", "us-east-1/other", "auto\nheader"] {
        let mut config = config("https://objects.invalid");
        config.region = region.into();
        assert!(SignedHttpTransport::new(config, credentials()).is_err());
    }
    assert!(Credentials::new(
        "BAD\nACCESS".into(),
        "secret-sentinel-123".into(),
        None,
        None
    )
    .is_err());
    assert!(Credentials::new(
        "TESTACCESS123".into(),
        "secret-sentinel-123".into(),
        Some("bad\rtoken".into()),
        None
    )
    .is_err());
    let expired = Credentials::new(
        "TESTACCESS123".into(),
        "secret-sentinel-123".into(),
        Some("test-token".into()),
        Some(SystemTime::UNIX_EPOCH),
    )
    .unwrap();
    assert!(SignedHttpTransport::new(config("https://objects.invalid"), expired).is_err());
}

struct WireRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}
fn read_request(stream: &mut TcpStream) -> WireRequest {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut raw = Vec::new();
    while !raw.ends_with(b"\r\n\r\n") {
        assert!(raw.len() < 32768);
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        raw.push(byte[0]);
    }
    let text = String::from_utf8(raw).unwrap();
    let mut lines = text.split("\r\n");
    let line = lines.next().unwrap().split(' ').collect::<Vec<_>>();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().into()))
        .collect::<BTreeMap<_, String>>();
    let len = headers
        .get("content-length")
        .map_or(0, |v| v.parse::<usize>().unwrap());
    assert!(len < 65536);
    let mut body = vec![0; len];
    stream.read_exact(&mut body).unwrap();
    WireRequest {
        method: line[0].into(),
        path: line[1].into(),
        headers,
        body,
    }
}
struct Reply {
    status: u16,
    extra: Vec<(&'static str, String)>,
    body: Vec<u8>,
    trickle: Option<Duration>,
}
impl Reply {
    fn new(status: u16, body: &[u8]) -> Self {
        Self {
            status,
            extra: Vec::new(),
            body: body.to_vec(),
            trickle: None,
        }
    }
}
fn server(replies: Vec<Reply>) -> (String, mpsc::Receiver<WireRequest>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let (send, receive) = mpsc::channel();
    let thread = thread::spawn(move || {
        for reply in replies {
            let deadline = Instant::now() + Duration::from_secs(4);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("fixture listener failed: {e}"),
                }
            };
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            send.send(read_request(&mut stream)).unwrap();
            let mut headers = format!(
                "HTTP/1.1 {} fixture\r\nContent-Length: {}\r\nConnection: close\r\n",
                reply.status,
                reply.body.len()
            );
            for (name, value) in reply.extra {
                headers += &format!("{name}: {value}\r\n");
            }
            headers += "\r\n";
            if stream.write_all(headers.as_bytes()).is_err() {
                continue;
            }
            if let Some(delay) = reply.trickle {
                for byte in reply.body {
                    thread::sleep(delay);
                    if stream.write_all(&[byte]).is_err() {
                        break;
                    }
                }
            } else {
                let _ = stream.write_all(&reply.body);
            }
        }
    });
    (endpoint, receive, thread)
}
fn transport(endpoint: &str) -> SignedHttpTransport {
    SignedHttpTransport::build(config(endpoint), credentials(), true).unwrap()
}
fn verify_wire_signature(request: &WireRequest) {
    let auth = request.headers.get("authorization").unwrap();
    let names = auth
        .split("SignedHeaders=")
        .nth(1)
        .unwrap()
        .split(',')
        .next()
        .unwrap();
    let headers = names
        .split(';')
        .map(|name| (name.to_owned(), request.headers[name].clone()))
        .collect();
    let expected = sign(
        &request.method,
        &request.path,
        &request.headers["x-amz-content-sha256"],
        &headers,
        &request.headers["x-amz-date"],
        "us-east-1",
        &credentials(),
    )
    .unwrap();
    assert_eq!(auth, &*expected);
}

#[test]
fn actual_http_creation_and_existing_object_require_signed_readback() {
    let bytes = b"exact approved cipher";
    let (endpoint, requests, handle) = server(vec![
        Reply::new(200, b""),
        Reply::new(200, bytes),
        Reply::new(412, b"provider secret error body"),
        Reply::new(200, bytes),
    ]);
    let backend = S3Backend::new(transport(&endpoint), 1024).unwrap();
    let id = backend.put(&mut &bytes[..]).unwrap();
    assert_eq!(backend.put(&mut &bytes[..]).unwrap(), id);
    for method in ["PUT", "GET", "PUT", "GET"] {
        let request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(request.method, method);
        assert_eq!(
            request.path,
            format!("/fixture-bucket/encrypted/v1/{}", id.sha256())
        );
        verify_wire_signature(&request);
        if method == "PUT" {
            assert_eq!(request.headers["if-none-match"], "*");
            assert_eq!(request.body, bytes);
            assert_eq!(request.headers["x-amz-content-sha256"], id.sha256());
        } else {
            assert!(request.body.is_empty());
            assert_eq!(request.headers["x-amz-content-sha256"], EMPTY_SHA256);
        }
    }
    handle.join().unwrap();
}

#[test]
fn actual_http_corruption_and_provider_errors_produce_no_receipt_or_secret() {
    let bytes = b"original";
    for (status, expected) in [
        (403, "storage security requirement failed"),
        (409, "S3 transfer temporarily unavailable"),
        (429, "S3 transfer temporarily unavailable"),
        (507, "backend capacity requirement failed"),
    ] {
        let (endpoint, requests, handle) =
            server(vec![Reply::new(status, b"SECRET-PROVIDER-SENTINEL")]);
        let backend = S3Backend::new(transport(&endpoint), 1024).unwrap();
        let error = backend.put(&mut &bytes[..]).unwrap_err();
        assert_eq!(error.to_string(), expected);
        assert!(!format!("{error:#}").contains("SECRET-PROVIDER-SENTINEL"));
        assert_eq!(requests.recv().unwrap().method, "PUT");
        handle.join().unwrap();
    }
    for status in [200, 412] {
        let (endpoint, requests, handle) =
            server(vec![Reply::new(status, b""), Reply::new(200, b"corrupt!")]);
        let backend = S3Backend::new(transport(&endpoint), 1024).unwrap();
        assert!(backend.put(&mut &bytes[..]).is_err());
        assert_eq!(requests.recv().unwrap().method, "PUT");
        assert_eq!(requests.recv().unwrap().method, "GET");
        handle.join().unwrap();
    }
}

#[test]
fn redirects_are_not_followed_and_partial_or_encoded_bodies_are_rejected() {
    let id = identity(b"original");
    for (status, extra) in [
        (
            302,
            vec![("Location", "http://127.0.0.1:1/credentials".into())],
        ),
        (206, Vec::new()),
        (200, vec![("Content-Encoding", "gzip".into())]),
        (200, vec![("Content-Range", "bytes 0-7/8".into())]),
    ] {
        let mut reply = Reply::new(status, b"original");
        reply.extra = extra;
        let (endpoint, requests, handle) = server(vec![reply]);
        assert!(transport(&endpoint).get(&id).is_err());
        assert_eq!(requests.recv().unwrap().method, "GET");
        handle.join().unwrap();
    }
}

#[test]
fn whole_response_deadline_cannot_be_extended_by_trickled_bytes() {
    let bytes = b"original";
    let mut reply = Reply::new(200, bytes);
    reply.trickle = Some(Duration::from_millis(50));
    let (endpoint, requests, handle) = server(vec![reply]);
    let mut cfg = config(&endpoint);
    cfg.timeout = Duration::from_millis(120);
    let backend = S3Backend::new(
        SignedHttpTransport::build(cfg, credentials(), true).unwrap(),
        1024,
    )
    .unwrap();
    let start = Instant::now();
    let error = backend
        .get_verified(&identity(bytes), &mut std::io::sink())
        .unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(!format!("{error:#}").contains(&endpoint));
    assert_eq!(requests.recv().unwrap().method, "GET");
    handle.join().unwrap();
}
