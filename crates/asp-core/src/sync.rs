//! Sync remote building blocks.
//!
//! The local filesystem remote keeps protocol tests deterministic; object-store
//! adapters share the same conditional-write trait.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::error::{Error, ErrorCode, Result};

const LOCAL_REMOTE_LOCK: &str = ".asp-local-remote.lock";
const S3_SERVICE: &str = "s3";
const S3_TERMINATOR: &str = "aws4_request";
const SHA256_BLOCK_BYTES: usize = 64;

#[derive(Debug, Clone)]
pub struct LocalRemote {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct S3Remote<T> {
    config: S3RemoteConfig,
    endpoint: S3Endpoint,
    transport: T,
}

#[derive(Clone)]
pub struct S3RemoteConfig {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub prefix: String,
    pub path_style: bool,
}

impl fmt::Debug for S3RemoteConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3RemoteConfig")
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"<redacted>")
            .field("session_token_present", &self.session_token.is_some())
            .field("prefix", &self.prefix)
            .field("path_style", &self.path_style)
            .finish()
    }
}

impl S3RemoteConfig {
    pub fn new(
        endpoint: impl Into<String>,
        bucket: impl Into<String>,
        region: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            region: region.into(),
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            session_token: None,
            prefix: String::new(),
            path_style: true,
        }
    }

    pub fn with_session_token(mut self, session_token: impl Into<String>) -> Self {
        self.session_token = Some(session_token.into());
        self
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn with_virtual_host_style(mut self) -> Self {
        self.path_style = false;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3Method {
    Get,
    Put,
}

impl S3Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Put => "PUT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Request {
    pub method: S3Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl S3Response {
    pub fn new(
        status: u16,
        headers: Vec<(impl Into<String>, impl Into<String>)>,
        body: Vec<u8>,
    ) -> Self {
        Self {
            status,
            headers: headers
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
            body,
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

pub trait S3Transport: Send + Sync {
    fn send(&self, request: S3Request) -> Result<S3Response>;
}

#[derive(Debug, Clone)]
struct S3Endpoint {
    scheme: String,
    authority: String,
    origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteObject {
    pub bytes: Vec<u8>,
    pub version: RemoteVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub key: String,
    pub bytes: u64,
    pub version: RemoteVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteVersion(String);

impl RemoteVersion {
    pub fn token(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    Created,
    Replaced,
    AlreadyExists,
}

pub trait SyncRemote {
    fn get(&self, key: &str) -> Result<Option<RemoteObject>>;
    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>>;
    fn put_immutable(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome>;
    fn put_if_match(
        &self,
        key: &str,
        bytes: &[u8],
        expected: Option<&RemoteVersion>,
    ) -> Result<PutOutcome>;
}

impl<T: S3Transport> S3Remote<T> {
    pub fn new(mut config: S3RemoteConfig, transport: T) -> Result<Self> {
        if config.bucket.trim().is_empty()
            || config.bucket.contains('/')
            || config.bucket.contains('\\')
            || config.bucket.bytes().any(|b| b.is_ascii_control())
        {
            return Err(s3_config_error(
                "S3 bucket name must be non-empty and must not contain path separators",
            ));
        }
        if config.region.trim().is_empty() {
            return Err(s3_config_error("S3 region must be non-empty"));
        }
        if config.access_key_id.trim().is_empty() {
            return Err(s3_config_error("S3 access key id must be non-empty"));
        }
        if config.secret_access_key.is_empty() {
            return Err(s3_config_error("S3 secret access key must be non-empty"));
        }
        config.prefix = normalize_s3_prefix(&config.prefix)?;
        let endpoint = parse_s3_endpoint(&config.endpoint)?;
        Ok(Self {
            config,
            endpoint,
            transport,
        })
    }

    pub fn config(&self) -> &S3RemoteConfig {
        &self.config
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    fn object_request(
        &self,
        method: S3Method,
        key: &str,
        body: &[u8],
        extra_headers: Vec<(String, String)>,
    ) -> Result<S3Request> {
        let scoped_key = self.scoped_key(key)?;
        self.signed_request(
            method,
            Some(&scoped_key),
            Vec::new(),
            body.to_vec(),
            extra_headers,
            OffsetDateTime::now_utc(),
        )
    }

    fn list_request(&self, prefix: &str, continuation_token: Option<&str>) -> Result<S3Request> {
        let mut query = vec![
            ("list-type".to_string(), "2".to_string()),
            ("prefix".to_string(), self.scoped_list_prefix(prefix)?),
        ];
        if let Some(token) = continuation_token {
            query.push(("continuation-token".to_string(), token.to_string()));
        }
        self.signed_request(
            S3Method::Get,
            None,
            query,
            Vec::new(),
            Vec::new(),
            OffsetDateTime::now_utc(),
        )
    }

    fn signed_request(
        &self,
        method: S3Method,
        key: Option<&str>,
        query: Vec<(String, String)>,
        body: Vec<u8>,
        extra_headers: Vec<(String, String)>,
        now: OffsetDateTime,
    ) -> Result<S3Request> {
        let target = self.request_target(key);
        let payload_hash = sha256_hex(&body);
        let amz_date = amz_datetime(now);
        let short_date = amz_date[..8].to_string();
        let mut headers = BTreeMap::new();
        headers.insert("host".to_string(), target.host_header.clone());
        headers.insert("x-amz-content-sha256".to_string(), payload_hash.clone());
        headers.insert("x-amz-date".to_string(), amz_date.clone());
        if let Some(token) = &self.config.session_token {
            headers.insert(
                "x-amz-security-token".to_string(),
                normalize_header_value(token),
            );
        }
        for (name, value) in extra_headers {
            headers.insert(name.to_ascii_lowercase(), normalize_header_value(&value));
        }

        let canonical_headers = headers
            .iter()
            .map(|(name, value)| format!("{name}:{value}\n"))
            .collect::<String>();
        let signed_headers = headers.keys().cloned().collect::<Vec<_>>().join(";");
        let canonical_query = canonical_query(&query);
        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method.as_str(),
            target.canonical_uri,
            canonical_query,
            canonical_headers,
            signed_headers,
            payload_hash
        );
        let credential_scope = format!(
            "{}/{}/{}/{}",
            short_date, self.config.region, S3_SERVICE, S3_TERMINATOR
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date,
            credential_scope,
            sha256_hex(canonical_request.as_bytes())
        );
        let signature = sigv4_signature(
            &self.config.secret_access_key,
            &short_date,
            &self.config.region,
            string_to_sign.as_bytes(),
        );
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.config.access_key_id, credential_scope, signed_headers, signature
        );

        let mut request_headers = Vec::new();
        for (name, value) in headers {
            request_headers.push((canonical_header_name(&name), value));
        }
        request_headers.push(("Authorization".to_string(), authorization));
        Ok(S3Request {
            method,
            url: if canonical_query.is_empty() {
                format!("{}{}", target.origin, target.canonical_uri)
            } else {
                format!(
                    "{}{}?{}",
                    target.origin, target.canonical_uri, canonical_query
                )
            },
            headers: request_headers,
            body,
        })
    }

    fn request_target(&self, key: Option<&str>) -> S3RequestTarget {
        let key = key.unwrap_or("");
        if self.config.path_style {
            let canonical_uri = if key.is_empty() {
                format!("/{}", percent_encode_path_segment(&self.config.bucket))
            } else {
                format!(
                    "/{}/{}",
                    percent_encode_path_segment(&self.config.bucket),
                    percent_encode_key_path(key)
                )
            };
            S3RequestTarget {
                origin: self.endpoint.origin.clone(),
                canonical_uri,
                host_header: self.endpoint.authority.clone(),
            }
        } else {
            let origin = format!(
                "{}://{}.{}",
                self.endpoint.scheme, self.config.bucket, self.endpoint.authority
            );
            S3RequestTarget {
                origin,
                canonical_uri: if key.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", percent_encode_key_path(key))
                },
                host_header: format!("{}.{}", self.config.bucket, self.endpoint.authority),
            }
        }
    }

    fn scoped_key(&self, key: &str) -> Result<String> {
        validate_key(key)?;
        if self.config.prefix.is_empty() {
            Ok(key.to_string())
        } else {
            Ok(format!("{}/{}", self.config.prefix, key))
        }
    }

    fn scoped_list_prefix(&self, prefix: &str) -> Result<String> {
        let prefix = normalize_prefix(prefix)?;
        let scoped = match (self.config.prefix.is_empty(), prefix.is_empty()) {
            (true, true) => String::new(),
            (true, false) => prefix,
            (false, true) => format!("{}/", self.config.prefix),
            (false, false) => format!("{}/{prefix}/", self.config.prefix),
        };
        if scoped.is_empty() || scoped.ends_with('/') {
            Ok(scoped)
        } else {
            Ok(format!("{scoped}/"))
        }
    }

    fn unscoped_key(&self, key: &str) -> Option<String> {
        if self.config.prefix.is_empty() {
            Some(key.to_string())
        } else {
            key.strip_prefix(&format!("{}/", self.config.prefix))
                .map(|rest| rest.to_string())
        }
    }
}

impl<T: S3Transport> SyncRemote for S3Remote<T> {
    fn get(&self, key: &str) -> Result<Option<RemoteObject>> {
        let request = self.object_request(S3Method::Get, key, &[], Vec::new())?;
        let response = self.transport.send(request)?;
        match response.status {
            200 => {
                let version = s3_etag_version(&response, key)?;
                Ok(Some(RemoteObject {
                    bytes: response.body,
                    version,
                }))
            }
            404 => Ok(None),
            status => Err(s3_status_error(status, format!("read object '{key}'"))),
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>> {
        let mut entries = Vec::new();
        let mut continuation_token = None;
        loop {
            let request = self.list_request(prefix, continuation_token.as_deref())?;
            let response = self.transport.send(request)?;
            if response.status != 200 {
                return Err(s3_status_error(
                    response.status,
                    format!("list prefix '{prefix}'"),
                ));
            }
            let page = parse_s3_list_page(&response.body)?;
            for object in page.objects {
                let Some(key) = self.unscoped_key(&object.key) else {
                    continue;
                };
                validate_key(&key)?;
                entries.push(RemoteEntry {
                    key,
                    bytes: object.size,
                    version: RemoteVersion(object.etag),
                });
            }
            if !page.is_truncated {
                break;
            }
            let Some(next) = page.next_continuation_token else {
                return Err(remote_corrupt(
                    "S3 list result is truncated but has no continuation token",
                ));
            };
            continuation_token = Some(next);
        }
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(entries)
    }

    fn put_immutable(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        let request = self.object_request(
            S3Method::Put,
            key,
            bytes,
            vec![("If-None-Match".to_string(), "*".to_string())],
        )?;
        let response = self.transport.send(request)?;
        match response.status {
            200 | 201 | 204 => Ok(PutOutcome::Created),
            409 | 412 => match self.get(key)? {
                Some(existing) if existing.bytes == bytes => Ok(PutOutcome::AlreadyExists),
                Some(_) => Err(remote_corrupt(format!(
                    "remote immutable key '{key}' already exists with different bytes"
                ))),
                None => Err(sync_conflict(format!(
                    "remote immutable key '{key}' changed during conditional create"
                ))),
            },
            status => Err(s3_status_error(
                status,
                format!("write immutable object '{key}'"),
            )),
        }
    }

    fn put_if_match(
        &self,
        key: &str,
        bytes: &[u8],
        expected: Option<&RemoteVersion>,
    ) -> Result<PutOutcome> {
        let Some(expected) = expected else {
            return self.put_immutable(key, bytes);
        };
        let request = self.object_request(
            S3Method::Put,
            key,
            bytes,
            vec![("If-Match".to_string(), expected.token().to_string())],
        )?;
        let response = self.transport.send(request)?;
        match response.status {
            200 | 201 | 204 => Ok(PutOutcome::Replaced),
            409 | 412 => Err(sync_conflict(format!(
                "remote key '{key}' changed before conditional write"
            ))),
            404 => Err(sync_conflict(format!(
                "remote key '{key}' is missing; conditional write expected an existing version"
            ))),
            status => Err(s3_status_error(
                status,
                format!("conditionally write object '{key}'"),
            )),
        }
    }
}

#[derive(Debug, Clone)]
struct S3RequestTarget {
    origin: String,
    canonical_uri: String,
    host_header: String,
}

impl LocalRemote {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        std::fs::create_dir_all(root.as_ref())?;
        let root = root.as_ref().canonicalize()?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn get(&self, key: &str) -> Result<Option<RemoteObject>> {
        <Self as SyncRemote>::get(self, key)
    }

    pub fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>> {
        <Self as SyncRemote>::list(self, prefix)
    }

    pub fn put_immutable(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        <Self as SyncRemote>::put_immutable(self, key, bytes)
    }

    pub fn put_if_match(
        &self,
        key: &str,
        bytes: &[u8],
        expected: Option<&RemoteVersion>,
    ) -> Result<PutOutcome> {
        <Self as SyncRemote>::put_if_match(self, key, bytes, expected)
    }

    fn get_unlocked(&self, key: &str) -> Result<Option<RemoteObject>> {
        let path = self.key_path(key)?;
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if meta.file_type().is_symlink() {
            return Err(remote_corrupt(format!(
                "remote key is a symlink: {}",
                path.display()
            )));
        }
        if !meta.is_file() {
            return Err(remote_corrupt(format!("remote key '{key}' is not a file")));
        }
        let bytes = std::fs::read(path)?;
        Ok(Some(RemoteObject {
            version: version_for(&bytes),
            bytes,
        }))
    }

    fn list_unlocked(&self, prefix: &str) -> Result<Vec<RemoteEntry>> {
        let prefix = normalize_prefix(prefix)?;
        let path = if prefix.is_empty() {
            self.root.clone()
        } else {
            self.key_path(&prefix)?
        };
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        if meta.file_type().is_symlink() {
            return Err(remote_corrupt(format!(
                "remote key is a symlink: {}",
                path.display()
            )));
        }

        let mut entries = Vec::new();
        for entry in walkdir::WalkDir::new(&path).follow_links(false) {
            let entry = entry.map_err(|e| {
                Error::new(
                    ErrorCode::Io,
                    format!("read remote {}: {e}", path.display()),
                )
            })?;
            if entry.path() == path {
                continue;
            }
            if entry.file_type().is_symlink() {
                return Err(remote_corrupt(format!(
                    "remote key contains a symlink: {}",
                    entry.path().display()
                )));
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().strip_prefix(&self.root).map_err(|e| {
                Error::new(
                    ErrorCode::Io,
                    format!("remote path escaped {}: {e}", self.root.display()),
                )
                .with_source(e)
            })?;
            let key = rel_key(rel)?;
            if key == LOCAL_REMOTE_LOCK {
                continue;
            }
            let bytes = std::fs::read(entry.path())?;
            entries.push(RemoteEntry {
                key,
                bytes: bytes.len() as u64,
                version: version_for(&bytes),
            });
        }
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(entries)
    }

    fn put_immutable_unlocked(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        let path = self.key_path(key)?;
        ensure_parent_dirs(&self.root, key)?;

        match self.get_unlocked(key)? {
            Some(existing) if existing.bytes == bytes => return Ok(PutOutcome::AlreadyExists),
            Some(_) => {
                return Err(remote_corrupt(format!(
                    "remote immutable key '{key}' already exists with different bytes"
                )));
            }
            None => {}
        }

        let parent = path.parent().ok_or_else(|| {
            Error::new(
                ErrorCode::Io,
                format!("remote key '{key}' has no parent directory"),
            )
        })?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| {
            Error::new(
                ErrorCode::Io,
                format!("temp remote object in {}: {e}", parent.display()),
            )
            .with_source(e)
        })?;
        tmp.write_all(bytes)?;
        tmp.as_file().sync_data()?;

        match tmp.persist_noclobber(&path) {
            Ok(_) => {
                let _ = sync_dir(parent);
                Ok(PutOutcome::Created)
            }
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = std::fs::read(&path)?;
                if existing == bytes {
                    Ok(PutOutcome::AlreadyExists)
                } else {
                    Err(remote_corrupt(format!(
                        "remote immutable key '{key}' appeared with different bytes"
                    )))
                }
            }
            Err(e) => {
                let error = e.error;
                Err(Error::new(
                    ErrorCode::Io,
                    format!("publish remote object {}: {error}", path.display()),
                )
                .with_source(error))
            }
        }
    }

    fn put_if_match_unlocked(
        &self,
        key: &str,
        bytes: &[u8],
        expected: Option<&RemoteVersion>,
    ) -> Result<PutOutcome> {
        let current = self.get_unlocked(key)?;
        match (current, expected) {
            (None, None) => self.put_immutable_unlocked(key, bytes),
            (None, Some(_)) => Err(sync_conflict(format!(
                "remote key '{key}' is missing; conditional write expected an existing version"
            ))),
            (Some(_), None) => Err(sync_conflict(format!(
                "remote key '{key}' already exists; conditional create expected it to be absent"
            ))),
            (Some(current), Some(expected)) => {
                if &current.version != expected {
                    return Err(sync_conflict(format!(
                        "remote key '{key}' changed before conditional write"
                    )));
                }
                if current.bytes == bytes {
                    return Ok(PutOutcome::AlreadyExists);
                }
                self.replace_existing(key, bytes)?;
                Ok(PutOutcome::Replaced)
            }
        }
    }

    fn replace_existing(&self, key: &str, bytes: &[u8]) -> Result<()> {
        let path = self.key_path(key)?;
        ensure_parent_dirs(&self.root, key)?;
        let parent = path.parent().ok_or_else(|| {
            Error::new(
                ErrorCode::Io,
                format!("remote key '{key}' has no parent directory"),
            )
        })?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| {
            Error::new(
                ErrorCode::Io,
                format!("temp remote object in {}: {e}", parent.display()),
            )
            .with_source(e)
        })?;
        tmp.write_all(bytes)?;
        tmp.as_file().sync_data()?;
        tmp.persist(&path).map_err(|e| {
            Error::new(
                ErrorCode::Io,
                format!("replace remote object {}: {}", path.display(), e.error),
            )
            .with_source(e.error)
        })?;
        let _ = sync_dir(parent);
        Ok(())
    }

    fn with_lock<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let lock_path = self.root.join(LOCAL_REMOTE_LOCK);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)?;
        lock.lock_exclusive().map_err(|e| {
            Error::new(
                ErrorCode::Locked,
                "another asp process is modifying this sync remote",
            )
            .with_hint("wait for it to finish and retry")
            .with_source(e)
        })?;
        let result = f();
        let _ = FileExt::unlock(&lock);
        result
    }

    fn key_path(&self, key: &str) -> Result<PathBuf> {
        let parts = validate_key(key)?;
        let mut path = self.root.clone();
        for part in parts {
            path.push(part);
        }
        Ok(path)
    }
}

impl SyncRemote for LocalRemote {
    fn get(&self, key: &str) -> Result<Option<RemoteObject>> {
        self.get_unlocked(key)
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>> {
        self.list_unlocked(prefix)
    }

    fn put_immutable(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        self.with_lock(|| self.put_immutable_unlocked(key, bytes))
    }

    fn put_if_match(
        &self,
        key: &str,
        bytes: &[u8],
        expected: Option<&RemoteVersion>,
    ) -> Result<PutOutcome> {
        self.with_lock(|| self.put_if_match_unlocked(key, bytes, expected))
    }
}

fn validate_key(key: &str) -> Result<Vec<&str>> {
    if key.is_empty()
        || key.starts_with('/')
        || key.ends_with('/')
        || key.contains('\\')
        || key.as_bytes().contains(&0)
    {
        return Err(invalid_key(key));
    }
    let mut parts = Vec::new();
    for part in key.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(invalid_key(key));
        }
        parts.push(part);
    }
    Ok(parts)
}

fn normalize_prefix(prefix: &str) -> Result<String> {
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        Ok(String::new())
    } else {
        validate_key(prefix)?;
        Ok(prefix.to_string())
    }
}

fn ensure_parent_dirs(root: &Path, key: &str) -> Result<()> {
    let parts = validate_key(key)?;
    let mut dir = root.to_path_buf();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        dir.push(part);
        match std::fs::symlink_metadata(&dir) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(remote_corrupt(format!(
                    "remote parent is a symlink: {}",
                    dir.display()
                )));
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(remote_corrupt(format!(
                    "remote parent is not a directory: {}",
                    dir.display()
                )));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&dir)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn rel_key(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for part in path.components() {
        let std::path::Component::Normal(name) = part else {
            return Err(remote_corrupt(format!(
                "remote path has non-normal component: {}",
                path.display()
            )));
        };
        let name = name.to_str().ok_or_else(|| {
            remote_corrupt(format!("remote path is not UTF-8: {}", path.display()))
        })?;
        parts.push(name);
    }
    Ok(parts.join("/"))
}

fn invalid_key(key: &str) -> Error {
    Error::new(
        ErrorCode::StoreCorrupt,
        format!("invalid remote key: {key:?}"),
    )
    .with_hint("remote keys must be non-empty slash-separated relative paths")
}

fn remote_corrupt(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::StoreCorrupt, message)
        .with_hint("inspect the sync remote before retrying")
}

fn sync_conflict(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::SyncConflict, message)
        .with_hint("fetch the latest remote state, review conflicts, and retry")
}

fn s3_config_error(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::Io, message).with_hint("fix the S3 remote configuration and retry sync")
}

fn s3_status_error(status: u16, action: impl AsRef<str>) -> Error {
    Error::new(
        ErrorCode::Io,
        format!("S3 remote failed to {}: HTTP {status}", action.as_ref()),
    )
    .with_hint("verify the endpoint, credentials, bucket policy, and object prefix")
}

fn parse_s3_endpoint(endpoint: &str) -> Result<S3Endpoint> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    let (scheme, rest) = endpoint
        .strip_prefix("https://")
        .map(|rest| ("https", rest))
        .or_else(|| endpoint.strip_prefix("http://").map(|rest| ("http", rest)))
        .ok_or_else(|| s3_config_error("S3 endpoint must start with http:// or https://"))?;
    if rest.is_empty()
        || rest.contains('/')
        || rest.contains('?')
        || rest.contains('#')
        || rest.contains('@')
    {
        return Err(s3_config_error(
            "S3 endpoint must be an origin such as https://s3.example.com",
        ));
    }
    Ok(S3Endpoint {
        scheme: scheme.to_string(),
        authority: rest.to_string(),
        origin: format!("{scheme}://{rest}"),
    })
}

fn normalize_s3_prefix(prefix: &str) -> Result<String> {
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        Ok(String::new())
    } else {
        validate_key(prefix)?;
        Ok(prefix.to_string())
    }
}

fn canonical_header_name(name: &str) -> String {
    match name {
        "host" => "Host".to_string(),
        "if-match" => "If-Match".to_string(),
        "if-none-match" => "If-None-Match".to_string(),
        "x-amz-content-sha256" => "X-Amz-Content-Sha256".to_string(),
        "x-amz-date" => "X-Amz-Date".to_string(),
        "x-amz-security-token" => "X-Amz-Security-Token".to_string(),
        other => other.to_string(),
    }
}

fn normalize_header_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn canonical_query(params: &[(String, String)]) -> String {
    let mut encoded = params
        .iter()
        .map(|(key, value)| (percent_encode_query(key), percent_encode_query(value)))
        .collect::<Vec<_>>();
    encoded.sort();
    encoded
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_encode_key_path(key: &str) -> String {
    key.split('/')
        .map(percent_encode_path_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode_path_segment(segment: &str) -> String {
    percent_encode_bytes(segment.as_bytes())
}

fn percent_encode_query(value: &str) -> String {
    percent_encode_bytes(value.as_bytes())
}

fn percent_encode_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &byte in bytes {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn hmac_sha256(key: &[u8], bytes: &[u8]) -> Vec<u8> {
    let mut key_block = [0_u8; SHA256_BLOCK_BYTES];
    if key.len() > SHA256_BLOCK_BYTES {
        let hashed = Sha256::digest(key);
        key_block[..hashed.len()].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_key = [0x36_u8; SHA256_BLOCK_BYTES];
    let mut outer_key = [0x5c_u8; SHA256_BLOCK_BYTES];
    for i in 0..SHA256_BLOCK_BYTES {
        inner_key[i] ^= key_block[i];
        outer_key[i] ^= key_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(inner_key);
    inner.update(bytes);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_key);
    outer.update(inner_digest);
    outer.finalize().to_vec()
}

fn sigv4_signature(secret: &str, date: &str, region: &str, string_to_sign: &[u8]) -> String {
    let date_key = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, S3_SERVICE.as_bytes());
    let signing_key = hmac_sha256(&service_key, S3_TERMINATOR.as_bytes());
    let signature = hmac_sha256(&signing_key, string_to_sign);
    hex_lower(&signature)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn amz_datetime(now: OffsetDateTime) -> String {
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn s3_etag_version(response: &S3Response, key: &str) -> Result<RemoteVersion> {
    response
        .header("etag")
        .map(|etag| RemoteVersion(etag.to_string()))
        .ok_or_else(|| {
            remote_corrupt(format!(
                "S3 response for key '{key}' did not include an ETag version"
            ))
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct S3ListPage {
    objects: Vec<S3ListedObject>,
    is_truncated: bool,
    next_continuation_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct S3ListedObject {
    key: String,
    size: u64,
    etag: String,
}

fn parse_s3_list_page(bytes: &[u8]) -> Result<S3ListPage> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut path = Vec::<String>::new();
    let mut objects = Vec::new();
    let mut current = PartialS3ListedObject::default();
    let mut is_truncated = false;
    let mut next_continuation_token = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                path.push(String::from_utf8_lossy(event.name().as_ref()).to_string());
            }
            Ok(Event::End(event)) => {
                let name = String::from_utf8_lossy(event.name().as_ref()).to_string();
                if name == "Contents" {
                    objects.push(current.finish()?);
                    current = PartialS3ListedObject::default();
                }
                path.pop();
            }
            Ok(Event::Text(text)) => {
                let value = text.decode().map_err(|e| {
                    remote_corrupt(format!("S3 list response contains invalid text: {e}"))
                })?;
                let Some(last) = path.last().map(String::as_str) else {
                    buf.clear();
                    continue;
                };
                if in_s3_contents(&path) {
                    match last {
                        "Key" => current.key = Some(value.into_owned()),
                        "ETag" => current.etag = Some(value.into_owned()),
                        "Size" => {
                            current.size = Some(value.parse::<u64>().map_err(|e| {
                                remote_corrupt(format!("S3 list object size is not numeric: {e}"))
                            })?)
                        }
                        _ => {}
                    }
                } else {
                    match last {
                        "IsTruncated" => is_truncated = value.eq_ignore_ascii_case("true"),
                        "NextContinuationToken" => {
                            next_continuation_token = Some(value.into_owned())
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(remote_corrupt(format!(
                    "S3 list response is not valid XML at byte {}: {e}",
                    reader.error_position()
                )));
            }
        }
        buf.clear();
    }

    Ok(S3ListPage {
        objects,
        is_truncated,
        next_continuation_token,
    })
}

#[derive(Default)]
struct PartialS3ListedObject {
    key: Option<String>,
    size: Option<u64>,
    etag: Option<String>,
}

impl PartialS3ListedObject {
    fn finish(&mut self) -> Result<S3ListedObject> {
        let key = self
            .key
            .take()
            .ok_or_else(|| remote_corrupt("S3 list object is missing a Key element"))?;
        let size = self.size.take().ok_or_else(|| {
            remote_corrupt(format!("S3 list object '{key}' is missing a Size element"))
        })?;
        let etag = self.etag.take().ok_or_else(|| {
            remote_corrupt(format!("S3 list object '{key}' is missing an ETag element"))
        })?;
        Ok(S3ListedObject { key, size, etag })
    }
}

fn in_s3_contents(path: &[String]) -> bool {
    path.iter().any(|element| element == "Contents")
}

fn version_for(bytes: &[u8]) -> RemoteVersion {
    RemoteVersion(blake3::hash(bytes).to_hex().to_string())
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    OpenOptions::new().read(true).open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct RecordingS3Transport {
        responses: Mutex<VecDeque<S3Response>>,
        requests: Mutex<Vec<S3Request>>,
    }

    impl RecordingS3Transport {
        fn new(responses: Vec<S3Response>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<S3Request> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl S3Transport for RecordingS3Transport {
        fn send(&self, request: S3Request) -> Result<S3Response> {
            self.requests.lock().unwrap().push(request);
            self.responses.lock().unwrap().pop_front().ok_or_else(|| {
                Error::new(ErrorCode::Io, "mock S3 transport has no queued response")
            })
        }
    }

    fn test_s3_remote(responses: Vec<S3Response>) -> S3Remote<RecordingS3Transport> {
        S3Remote::new(
            S3RemoteConfig::new(
                "https://s3.example.com",
                "team-bucket",
                "us-east-1",
                "AKIDEXAMPLE",
                "secret",
            )
            .with_prefix("/team-a/asp/"),
            RecordingS3Transport::new(responses),
        )
        .unwrap()
    }

    fn request_header<'a>(request: &'a S3Request, name: &str) -> &'a str {
        request
            .headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
            .unwrap_or_else(|| panic!("missing request header {name}"))
    }

    #[test]
    fn hmac_sha256_matches_known_vector() {
        assert_eq!(
            hex_lower(&hmac_sha256(
                b"key",
                b"The quick brown fox jumps over the lazy dog"
            )),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }

    #[test]
    fn local_remote_put_get_and_list_are_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = LocalRemote::open(tmp.path().join("remote")).unwrap();

        assert_eq!(
            remote
                .put_immutable("objects/git/sha1/aa/1111", b"one")
                .unwrap(),
            PutOutcome::Created
        );
        assert_eq!(
            remote
                .put_immutable("objects/git/sha1/aa/1111", b"one")
                .unwrap(),
            PutOutcome::AlreadyExists
        );
        assert_eq!(
            remote
                .put_immutable("objects/blobs/blake3/bbbb", b"two")
                .unwrap(),
            PutOutcome::Created
        );

        let object = remote.get("objects/git/sha1/aa/1111").unwrap().unwrap();
        assert_eq!(object.bytes, b"one");
        assert_eq!(object.version.token(), version_for(b"one").token());

        let keys: Vec<_> = remote
            .list("objects")
            .unwrap()
            .into_iter()
            .map(|entry| (entry.key, entry.bytes))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("objects/blobs/blake3/bbbb".to_string(), 3),
                ("objects/git/sha1/aa/1111".to_string(), 3)
            ]
        );
    }

    #[test]
    fn local_remote_supports_conditional_writes_through_trait() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = LocalRemote::open(tmp.path().join("remote")).unwrap();
        let remote_trait: &dyn SyncRemote = &remote;

        assert_eq!(
            remote_trait
                .put_if_match("refs/head.json", br#"{"seq":1}"#, None)
                .unwrap(),
            PutOutcome::Created
        );
        let v1 = remote_trait.get("refs/head.json").unwrap().unwrap().version;
        assert_eq!(
            remote_trait
                .put_if_match("refs/head.json", br#"{"seq":2}"#, Some(&v1))
                .unwrap(),
            PutOutcome::Replaced
        );
        assert_eq!(
            remote_trait.get("refs/head.json").unwrap().unwrap().bytes,
            br#"{"seq":2}"#
        );

        let stale = remote_trait
            .put_if_match("refs/head.json", br#"{"seq":3}"#, Some(&v1))
            .unwrap_err();
        assert_eq!(stale.code, ErrorCode::SyncConflict);
        assert_eq!(
            remote_trait.get("refs/head.json").unwrap().unwrap().bytes,
            br#"{"seq":2}"#
        );

        let duplicate_create = remote_trait
            .put_if_match("refs/head.json", br#"{"seq":2}"#, None)
            .unwrap_err();
        assert_eq!(duplicate_create.code, ErrorCode::SyncConflict);
    }

    #[test]
    fn local_remote_rejects_conflicting_immutable_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = LocalRemote::open(tmp.path().join("remote")).unwrap();
        remote.put_immutable("refs/head.json", b"one").unwrap();

        let err = remote.put_immutable("refs/head.json", b"two").unwrap_err();
        assert_eq!(err.code, ErrorCode::StoreCorrupt);
        assert_eq!(remote.get("refs/head.json").unwrap().unwrap().bytes, b"one");
    }

    #[test]
    fn local_remote_rejects_escaping_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = LocalRemote::open(tmp.path().join("remote")).unwrap();
        for key in ["", "/abs", "a/", "a//b", "a/./b", "a/../b", "../x", "a\\b"] {
            let err = remote.put_immutable(key, b"x").unwrap_err();
            assert_eq!(err.code, ErrorCode::StoreCorrupt, "{key}");
        }
        assert!(!tmp.path().join("x").exists());
    }

    #[cfg(unix)]
    #[test]
    fn local_remote_rejects_parent_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = LocalRemote::open(tmp.path().join("remote")).unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, remote.root().join("objects")).unwrap();

        let err = remote
            .put_immutable("objects/git/sha1/aa/1111", b"one")
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::StoreCorrupt);
        assert!(!outside.join("git/sha1/aa/1111").exists());
    }

    #[test]
    fn s3_remote_signs_and_writes_immutable_objects() {
        let remote = test_s3_remote(vec![S3Response::new(
            200,
            vec![("ETag", "\"etag-one\"")],
            Vec::new(),
        )]);

        assert_eq!(
            remote
                .put_immutable("objects/git/sha1/aa/1111", b"one")
                .unwrap(),
            PutOutcome::Created
        );

        let requests = remote.transport().requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, S3Method::Put);
        assert_eq!(
            request.url,
            "https://s3.example.com/team-bucket/team-a/asp/objects/git/sha1/aa/1111"
        );
        assert_eq!(request.body, b"one");
        assert_eq!(request_header(request, "Host"), "s3.example.com");
        assert_eq!(request_header(request, "If-None-Match"), "*");
        assert_eq!(
            request_header(request, "X-Amz-Content-Sha256"),
            sha256_hex(b"one")
        );
        assert!(request_header(request, "Authorization").contains("Credential=AKIDEXAMPLE/"));
        assert!(request_header(request, "Authorization")
            .contains("SignedHeaders=host;if-none-match;x-amz-content-sha256;x-amz-date"));
    }

    #[test]
    fn s3_remote_treats_same_immutable_bytes_as_present() {
        let remote = test_s3_remote(vec![
            S3Response::new(412, Vec::<(&str, &str)>::new(), Vec::new()),
            S3Response::new(200, vec![("ETag", "\"etag-one\"")], b"one".to_vec()),
        ]);

        assert_eq!(
            remote
                .put_immutable("objects/git/sha1/aa/1111", b"one")
                .unwrap(),
            PutOutcome::AlreadyExists
        );

        let requests = remote.transport().requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, S3Method::Put);
        assert_eq!(requests[1].method, S3Method::Get);
    }

    #[test]
    fn s3_remote_uses_etags_for_conditional_ref_writes() {
        let remote = test_s3_remote(vec![
            S3Response::new(204, Vec::<(&str, &str)>::new(), Vec::new()),
            S3Response::new(412, Vec::<(&str, &str)>::new(), Vec::new()),
        ]);
        let version = RemoteVersion("\"head-v1\"".to_string());

        assert_eq!(
            remote
                .put_if_match("refs/head.json", br#"{"seq":2}"#, Some(&version))
                .unwrap(),
            PutOutcome::Replaced
        );
        let stale = remote
            .put_if_match("refs/head.json", br#"{"seq":3}"#, Some(&version))
            .unwrap_err();
        assert_eq!(stale.code, ErrorCode::SyncConflict);

        let requests = remote.transport().requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(request_header(&requests[0], "If-Match"), "\"head-v1\"");
        assert_eq!(request_header(&requests[1], "If-Match"), "\"head-v1\"");
    }

    #[test]
    fn s3_remote_lists_paginated_prefixed_objects() {
        let first_page = br#"
            <ListBucketResult>
              <IsTruncated>true</IsTruncated>
              <Contents>
                <Key>team-a/asp/objects/git/sha1/aa/1111</Key>
                <ETag>"etag-git"</ETag>
                <Size>3</Size>
              </Contents>
              <NextContinuationToken>next-page</NextContinuationToken>
            </ListBucketResult>
        "#;
        let second_page = br#"
            <ListBucketResult>
              <IsTruncated>false</IsTruncated>
              <Contents>
                <Key>team-a/asp/objects/blobs/blake3/bbbb</Key>
                <ETag>"etag-blob"</ETag>
                <Size>4</Size>
              </Contents>
            </ListBucketResult>
        "#;
        let remote = test_s3_remote(vec![
            S3Response::new(200, Vec::<(&str, &str)>::new(), first_page.to_vec()),
            S3Response::new(200, Vec::<(&str, &str)>::new(), second_page.to_vec()),
        ]);

        let entries = remote.list("objects").unwrap();
        assert_eq!(
            entries,
            vec![
                RemoteEntry {
                    key: "objects/blobs/blake3/bbbb".to_string(),
                    bytes: 4,
                    version: RemoteVersion("\"etag-blob\"".to_string())
                },
                RemoteEntry {
                    key: "objects/git/sha1/aa/1111".to_string(),
                    bytes: 3,
                    version: RemoteVersion("\"etag-git\"".to_string())
                }
            ]
        );

        let requests = remote.transport().requests();
        assert_eq!(
            requests[0].url,
            "https://s3.example.com/team-bucket?list-type=2&prefix=team-a%2Fasp%2Fobjects%2F"
        );
        assert_eq!(
            requests[1].url,
            "https://s3.example.com/team-bucket?continuation-token=next-page&list-type=2&prefix=team-a%2Fasp%2Fobjects%2F"
        );
    }

    #[test]
    fn s3_config_debug_redacts_secrets() {
        let debug = format!(
            "{:?}",
            S3RemoteConfig::new(
                "https://s3.example.com",
                "team-bucket",
                "us-east-1",
                "AKIDEXAMPLE",
                "super-secret",
            )
            .with_session_token("temporary-token")
        );

        assert!(debug.contains("<redacted>"));
        assert!(debug.contains("session_token_present"));
        assert!(!debug.contains("super-secret"));
        assert!(!debug.contains("temporary-token"));
    }
}
