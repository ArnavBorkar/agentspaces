use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use asp_core::sync::{
    AzureBlobMethod, AzureBlobRemote, AzureBlobRemoteConfig, AzureBlobRequest, AzureBlobResponse,
    AzureBlobTransport, GcsMethod, GcsRemote, GcsRemoteConfig, GcsRequest, GcsResponse,
    GcsTransport, PutOutcome, S3Method, S3Remote, S3RemoteConfig, S3Request, S3Response,
    S3Transport, SyncRemote,
};
use asp_core::{Error, ErrorCode, Result};

#[test]
#[ignore = "requires ASP_SYNC_S3_* environment variables and a running S3-compatible emulator"]
fn s3_emulator_contract() -> Result<()> {
    let Some(endpoint) = optional_env("ASP_SYNC_S3_ENDPOINT") else {
        skip("ASP_SYNC_S3_ENDPOINT is not set");
        return Ok(());
    };
    let bucket = required_env("ASP_SYNC_S3_BUCKET")?;
    let region = optional_env("ASP_SYNC_S3_REGION").unwrap_or_else(|| "us-east-1".to_string());
    let access_key = required_env("ASP_SYNC_S3_ACCESS_KEY_ID")?;
    let secret = required_env("ASP_SYNC_S3_SECRET_ACCESS_KEY")?;
    let mut config = S3RemoteConfig::new(endpoint, bucket, region, access_key, secret)
        .with_prefix(unique_prefix("s3"));
    if let Some(token) = optional_env("ASP_SYNC_S3_SESSION_TOKEN") {
        config = config.with_session_token(token);
    }
    let remote = S3Remote::new(config, CurlTransport)?;
    exercise_remote_contract(&remote)
}

#[test]
#[ignore = "requires ASP_SYNC_GCS_* environment variables and a running GCS emulator"]
fn gcs_emulator_contract() -> Result<()> {
    let Some(endpoint) = optional_env("ASP_SYNC_GCS_ENDPOINT") else {
        skip("ASP_SYNC_GCS_ENDPOINT is not set");
        return Ok(());
    };
    let bucket = required_env("ASP_SYNC_GCS_BUCKET")?;
    let token = optional_env("ASP_SYNC_GCS_BEARER_TOKEN").unwrap_or_else(|| "test-token".into());
    let remote = GcsRemote::new(
        GcsRemoteConfig::new(bucket, token)
            .with_endpoint(endpoint)
            .with_prefix(unique_prefix("gcs")),
        CurlTransport,
    )?;
    exercise_remote_contract(&remote)
}

#[test]
#[ignore = "requires ASP_SYNC_AZURE_* environment variables and a running Azurite/Azure Blob endpoint"]
fn azure_blob_emulator_contract() -> Result<()> {
    let Some(endpoint) = optional_env("ASP_SYNC_AZURE_ENDPOINT") else {
        skip("ASP_SYNC_AZURE_ENDPOINT is not set");
        return Ok(());
    };
    let container = required_env("ASP_SYNC_AZURE_CONTAINER")?;
    let sas = required_env("ASP_SYNC_AZURE_SAS")?;
    let remote = AzureBlobRemote::new(
        AzureBlobRemoteConfig::new(endpoint, container, sas).with_prefix(unique_prefix("azure")),
        CurlTransport,
    )?;
    exercise_remote_contract(&remote)
}

fn exercise_remote_contract(remote: &dyn SyncRemote) -> Result<()> {
    assert_eq!(
        remote.put_immutable("objects/blobs/blob-one", b"one")?,
        PutOutcome::Created
    );
    assert_eq!(
        remote.put_immutable("objects/blobs/blob-one", b"one")?,
        PutOutcome::AlreadyExists
    );
    let object = remote
        .get("objects/blobs/blob-one")?
        .ok_or_else(|| Error::new(ErrorCode::Io, "remote object disappeared after put"))?;
    assert_eq!(object.bytes, b"one");

    let entries = remote.list("objects")?;
    assert!(
        entries
            .iter()
            .any(|entry| entry.key == "objects/blobs/blob-one" && entry.bytes == 3),
        "remote list did not include uploaded object: {entries:?}"
    );

    assert_eq!(
        remote.put_if_match("refs/head.json", br#"{"seq":1}"#, None)?,
        PutOutcome::Created
    );
    let head = remote
        .get("refs/head.json")?
        .ok_or_else(|| Error::new(ErrorCode::Io, "remote ref disappeared after create"))?;
    assert_eq!(
        remote.put_if_match("refs/head.json", br#"{"seq":2}"#, Some(&head.version))?,
        PutOutcome::Replaced
    );
    assert_eq!(
        remote.get("refs/head.json")?.unwrap().bytes,
        br#"{"seq":2}"#
    );
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct CurlTransport;

impl S3Transport for CurlTransport {
    fn send(&self, request: S3Request) -> Result<S3Response> {
        let response = curl(
            s3_method(request.method),
            &request.url,
            &request.headers,
            &request.body,
        )?;
        Ok(S3Response {
            status: response.status,
            headers: response.headers,
            body: response.body,
        })
    }
}

impl GcsTransport for CurlTransport {
    fn send(&self, request: GcsRequest) -> Result<GcsResponse> {
        let response = curl(
            gcs_method(request.method),
            &request.url,
            &request.headers,
            &request.body,
        )?;
        Ok(GcsResponse {
            status: response.status,
            headers: response.headers,
            body: response.body,
        })
    }
}

impl AzureBlobTransport for CurlTransport {
    fn send(&self, request: AzureBlobRequest) -> Result<AzureBlobResponse> {
        let response = curl(
            azure_blob_method(request.method),
            &request.url,
            &request.headers,
            &request.body,
        )?;
        Ok(AzureBlobResponse {
            status: response.status,
            headers: response.headers,
            body: response.body,
        })
    }
}

fn s3_method(value: S3Method) -> &'static str {
    match value {
        S3Method::Get => "GET",
        S3Method::Put => "PUT",
    }
}

fn gcs_method(value: GcsMethod) -> &'static str {
    match value {
        GcsMethod::Get => "GET",
        GcsMethod::Post => "POST",
    }
}

fn azure_blob_method(value: AzureBlobMethod) -> &'static str {
    match value {
        AzureBlobMethod::Get => "GET",
        AzureBlobMethod::Put => "PUT",
    }
}

#[derive(Debug)]
struct CurlResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn curl(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<CurlResponse> {
    let mut command = Command::new("curl");
    command
        .arg("--silent")
        .arg("--show-error")
        .arg("--include")
        .arg("--request")
        .arg(method)
        .arg("--header")
        .arg("Expect:")
        .arg(url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in headers {
        command.arg("--header").arg(format!("{name}: {value}"));
    }
    if !body.is_empty() {
        command.arg("--data-binary").arg("@-");
    }

    let mut child = command.spawn().map_err(|e| {
        Error::new(
            ErrorCode::Io,
            format!("spawn curl for emulator fixture: {e}"),
        )
        .with_hint("install curl or run the emulator fixture on a host that provides it")
        .with_source(e)
    })?;
    if !body.is_empty() {
        child
            .stdin
            .as_mut()
            .expect("curl stdin should be piped")
            .write_all(body)?;
    }
    drop(child.stdin.take());
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(Error::new(
            ErrorCode::Io,
            format!(
                "curl failed while calling emulator: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        )
        .with_hint("verify the emulator endpoint, bucket/container, and credentials"));
    }
    parse_curl_response(&output.stdout)
}

fn parse_curl_response(bytes: &[u8]) -> Result<CurlResponse> {
    let splits = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(index, window)| (window == b"\r\n\r\n").then_some(index))
        .collect::<Vec<_>>();
    let split = *splits.last().ok_or_else(|| {
        Error::new(ErrorCode::Io, "curl response did not include HTTP headers")
            .with_hint("rerun the emulator fixture with a reachable HTTP endpoint")
    })?;
    let header_start = splits
        .iter()
        .rev()
        .nth(1)
        .map(|previous| previous + 4)
        .unwrap_or(0);
    let header_bytes = bytes.get(header_start..split).ok_or_else(|| {
        Error::new(ErrorCode::Io, "curl response did not include HTTP headers")
            .with_hint("rerun the emulator fixture with a reachable HTTP endpoint")
    })?;
    let body = bytes[split + 4..].to_vec();
    let header_text = String::from_utf8_lossy(header_bytes);
    let mut lines = header_text.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| Error::new(ErrorCode::Io, "curl response had no status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::Io,
                format!("curl response had invalid status line: {status_line}"),
            )
        })?;
    let headers = lines
        .filter_map(|line| {
            line.split_once(':')
                .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    Ok(CurlResponse {
        status,
        headers,
        body,
    })
}

fn required_env(name: &str) -> Result<String> {
    std::env::var(name).map_err(|_| {
        Error::new(ErrorCode::Io, format!("{name} is not set"))
            .with_hint("set the emulator fixture environment variables and retry")
    })
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn unique_prefix(provider: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!(
        "asp-sync-fixtures/{provider}/{}/{}",
        std::process::id(),
        millis
    )
}

fn skip(reason: &str) {
    eprintln!("skipping emulator fixture: {reason}");
}
