//! Port of `GitHub/GitHubService.swift`: repository parsing, the release model, the REST
//! client used to publish a product's GitHub release ([`GitHubReleasing`]) and the
//! app-update check ([`GitHubServicing::latest_release`] with [`ReleaseCheckCache`]).

use crate::http::{self, DECODING_FAILED, header};
use crate::url_text::{encode_allowing, encode_path};
use async_trait::async_trait;
use rd_model::{SwiftDate, iso8601};
use rd_versioning::php_version;
use reqwest::header::HeaderMap;
use reqwest::{Method, RequestBuilder};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::Path;
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GitHubRepository {
    pub owner: String,
    pub name: String,
}

impl GitHubRepository {
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
        }
    }

    /// Parses `owner/name`, `https://github.com/owner/name(.git)` or `git@github.com:owner/name.git`.
    pub fn parse(string: &str) -> Option<Self> {
        let mut s = string.trim();
        for prefix in [
            "https://github.com/",
            "http://github.com/",
            "git@github.com:",
            "github.com/",
        ] {
            if s.len() >= prefix.len()
                && s.is_char_boundary(prefix.len())
                && s[..prefix.len()].eq_ignore_ascii_case(prefix)
            {
                s = &s[prefix.len()..];
                break;
            }
        }
        if let Some(rest) = s.strip_suffix(".git") {
            s = rest;
        }
        if let Some(rest) = s.strip_suffix('/') {
            s = rest;
        }
        // Swift's `split` drops empty pieces.
        let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() != 2 {
            return None;
        }
        let allowed = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.');
        if !parts.iter().all(|p| *p != "." && *p != ".." && p.chars().all(allowed)) {
            return None;
        }
        Some(Self::new(parts[0], parts[1]))
    }

    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    pub fn html_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GitHubAsset {
    pub id: i64,
    pub name: String,
    pub size: i64,
    pub url: String,
    pub browser_download_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

/// A release as the GitHub REST API returns it (snake_case keys, ISO 8601 dates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub id: i64,
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub html_url: String,
    #[serde(
        default,
        serialize_with = "serialize_optional_date",
        deserialize_with = "deserialize_optional_date"
    )]
    pub published_at: Option<SwiftDate>,
    pub assets: Vec<GitHubAsset>,
    /// `https://uploads.github.com/repos/o/r/releases/1/assets{?name,label}`
    #[serde(default)]
    pub upload_url: Option<String>,
}

impl GitHubRelease {
    /// Version derived from the tag (`v1.2.3` → `1.2.3`).
    pub fn version(&self) -> String {
        let t = self.tag_name.trim_matches(|c: char| c == ' ' || c == '\t');
        let mut chars = t.chars();
        if matches!(chars.next(), Some('v' | 'V')) && chars.next().is_some_and(char::is_numeric) {
            return t[1..].to_owned();
        }
        t.to_owned()
    }

    /// Decodes a release payload (Swift's `JSONDecoder` with `.iso8601` dates).
    pub fn decode(data: &[u8]) -> Result<Self, GitHubError> {
        serde_json::from_slice(data).map_err(|error| GitHubError::Decoding(error.to_string()))
    }
}

fn serialize_optional_date<S: Serializer>(date: &Option<SwiftDate>, serializer: S) -> Result<S::Ok, S::Error> {
    match date {
        Some(date) => iso8601::serialize(date, serializer),
        None => serializer.serialize_none(),
    }
}

fn deserialize_optional_date<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<SwiftDate>, D::Error> {
    match Option::<String>::deserialize(deserializer)? {
        None => Ok(None),
        Some(text) => iso8601::parse(&text)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom(format!("Expected date string to be ISO8601-formatted: {text}"))),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<SwiftDate>,
}

impl RateLimitStatus {
    /// `x-ratelimit-limit`, `x-ratelimit-remaining` and `x-ratelimit-reset` (Unix seconds).
    pub fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            limit: header(headers, "x-ratelimit-limit").and_then(|v| v.parse().ok()),
            remaining: header(headers, "x-ratelimit-remaining").and_then(|v| v.parse().ok()),
            reset: header(headers, "x-ratelimit-reset")
                .and_then(|v| v.parse::<f64>().ok())
                .map(SwiftDate::from_unix_seconds),
        }
    }
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum GitHubError {
    #[error("The repository has no published releases.")]
    NoReleases,
    #[error(
        "No published release was found. The repository may have no releases yet, or it is private and needs a token with Contents: read permission."
    )]
    NotFound,
    #[error("The GitHub token was rejected.")]
    Unauthorized,
    #[error("{}", rate_limited_message(*.0))]
    RateLimited(Option<SwiftDate>),
    #[error("GitHub returned HTTP {0}.")]
    Http(u16),
    #[error("Unexpected response from GitHub.")]
    InvalidResponse,
    #[error(
        "The GitHub token may not write to this repository. Use a fine-grained token with Contents: read and write."
    )]
    Forbidden,
    #[error("Commit {} is not on GitHub. Push it, then create the release again.", prefix(.0, 10))]
    CommitNotOnGitHub(String),
    #[error("GitHub rejected the request: {0}")]
    ValidationFailed(String),
    /// The payload was not the expected JSON (Swift's `DecodingError`); the detail is for logs.
    #[error("{DECODING_FAILED}")]
    Decoding(String),
    /// The request failed before a response arrived (Swift's `URLError`).
    #[error("{0}")]
    Transport(String),
    /// The asset file could not be read (Swift's `CocoaError`).
    #[error("{0}")]
    File(String),
}

/// `reset.formatted(date: .omitted, time: .shortened)` in the local time zone (en-US style).
fn rate_limited_message(reset: Option<SwiftDate>) -> String {
    match reset {
        Some(reset) => {
            let seconds = reset.unix_seconds();
            let time = chrono::DateTime::from_timestamp(seconds.floor() as i64, 0)
                .map(|utc| utc.with_timezone(&chrono::Local).format("%-I:%M %p").to_string())
                .unwrap_or_default();
            format!("GitHub API rate limit reached. Try again after {time}.")
        }
        None => "GitHub API rate limit reached.".into(),
    }
}

/// The first `count` characters (Swift `prefix`).
fn prefix(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseChannel {
    Stable,
    Beta,
}

/// Cached result of the last release check, reused with ETag conditional requests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCheckCache {
    pub repository: String,
    pub channel: ReleaseChannel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<GitHubRelease>,
    pub checked_at: SwiftDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitStatus>,
}

/// Update notifications (the Swift app's own update check). The Tauri app uses its
/// updater instead; this is kept because it shares the client with release publishing.
#[async_trait]
pub trait GitHubServicing: Send + Sync {
    async fn latest_release(
        &self,
        repository: &GitHubRepository,
        channel: ReleaseChannel,
        token: Option<&str>,
        cache: Option<&ReleaseCheckCache>,
    ) -> Result<ReleaseCheckCache, GitHubError>;
}

/// What [`GitHubReleasing::create_release`] creates (Swift passes these as parameters).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateRelease<'a> {
    pub tag: &'a str,
    pub commit: &'a str,
    pub name: &'a str,
    pub body: &'a str,
    pub draft: bool,
    pub prerelease: bool,
}

/// Write access used to publish a product's GitHub release. Separate from update checks,
/// which only read.
#[async_trait]
pub trait GitHubReleasing: Send + Sync {
    async fn commit_exists(&self, sha: &str, repository: &GitHubRepository, token: &str) -> Result<bool, GitHubError>;
    /// The release for `tag`, or None when there is none.
    async fn release(
        &self,
        tag: &str,
        repository: &GitHubRepository,
        token: &str,
    ) -> Result<Option<GitHubRelease>, GitHubError>;
    async fn create_release(
        &self,
        repository: &GitHubRepository,
        release: &CreateRelease<'_>,
        token: &str,
    ) -> Result<GitHubRelease, GitHubError>;
    async fn upload_asset(
        &self,
        file: &Path,
        name: &str,
        release: &GitHubRelease,
        token: &str,
    ) -> Result<(), GitHubError>;
}

/// GitHub REST client. Public repositories need no token for update checks; private
/// repositories use a fine-grained token with *Contents: read*, releases need
/// *Contents: read and write*.
#[derive(Clone, Debug)]
pub struct GitHubService {
    client: reqwest::Client,
    base_url: Url,
}

struct Response {
    status: u16,
    headers: HeaderMap,
    data: Vec<u8>,
}

impl GitHubService {
    pub const API_VERSION: &'static str = "2022-11-28";
    pub const DEFAULT_BASE_URL: &'static str = "https://api.github.com/";

    /// A client for `https://api.github.com/`. Fails only when TLS cannot be initialised.
    pub fn new() -> Result<Self, GitHubError> {
        let base_url = Url::parse(Self::DEFAULT_BASE_URL).map_err(|_| GitHubError::InvalidResponse)?;
        Self::with_base_url(base_url)
    }

    /// A client for `base_url` (tests use a local server). Paths are resolved like Swift's
    /// `URL(string:relativeTo:)`, so a base with a path must end in `/`.
    pub fn with_base_url(base_url: Url) -> Result<Self, GitHubError> {
        Ok(Self::with_client(
            http::client().map_err(GitHubError::Transport)?,
            base_url,
        ))
    }

    pub fn with_client(client: reqwest::Client, base_url: Url) -> Self {
        Self { client, base_url }
    }

    fn url(&self, path: &str) -> Result<Url, GitHubError> {
        self.base_url.join(path).map_err(|_| GitHubError::InvalidResponse)
    }

    fn request(&self, method: Method, url: Url, token: Option<&str>, timeout: Duration) -> RequestBuilder {
        let mut request = self
            .client
            .request(method, url)
            .timeout(timeout)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", Self::API_VERSION)
            .header("User-Agent", http::USER_AGENT);
        if let Some(token) = token.filter(|t| !t.is_empty()) {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        request
    }

    async fn execute(request: RequestBuilder) -> Result<Response, GitHubError> {
        let response = request
            .send()
            .await
            .map_err(|error| GitHubError::Transport(http::transport_error(error)))?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let data = response
            .bytes()
            .await
            .map_err(|error| GitHubError::Transport(http::transport_error(error)))?
            .to_vec();
        Ok(Response { status, headers, data })
    }

    /// Sends a release request and maps the answers every endpoint shares.
    async fn send(request: RequestBuilder) -> Result<Response, GitHubError> {
        let response = Self::execute(request).await?;
        match response.status {
            401 => Err(GitHubError::Unauthorized),
            403 if header(&response.headers, "x-ratelimit-remaining") == Some("0") => Err(GitHubError::RateLimited(
                RateLimitStatus::from_headers(&response.headers).reset,
            )),
            403 => Err(GitHubError::Forbidden),
            _ => Ok(response),
        }
    }

    /// `{"message": "Validation Failed", "errors": [{"code": "already_exists", ...}]}`
    pub fn error_message(data: &[u8]) -> Option<String> {
        let object = serde_json::from_slice::<serde_json::Value>(data).ok()?;
        let object = object.as_object()?;
        let codes: Vec<String> = object
            .get("errors")
            .and_then(|e| e.as_array())
            .map(|errors| {
                errors
                    .iter()
                    .filter_map(|e| e.as_object())
                    .filter_map(|e| {
                        e.get("message")
                            .and_then(|m| m.as_str())
                            .or_else(|| e.get("code").and_then(|c| c.as_str()))
                            .map(str::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let message = object.get("message").and_then(|m| m.as_str()).map(str::to_owned);
        Some(message.into_iter().chain(codes).collect::<Vec<_>>().join(": "))
    }

    /// The upload URL for `name`: the release's `upload_url` template without its
    /// `{?name,label}` part, with `?name=<name>`.
    pub fn upload_url(template: &str, name: &str) -> Result<Url, GitHubError> {
        let base = template
            .split('{')
            .find(|piece| !piece.is_empty())
            .ok_or(GitHubError::InvalidResponse)?;
        let mut url = Url::parse(base).map_err(|_| GitHubError::InvalidResponse)?;
        let encoded = encode_allowing(name, |b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
        });
        url.set_query(Some(&format!("name={encoded}")));
        Ok(url)
    }
}

#[async_trait]
impl GitHubServicing for GitHubService {
    async fn latest_release(
        &self,
        repository: &GitHubRepository,
        channel: ReleaseChannel,
        token: Option<&str>,
        cache: Option<&ReleaseCheckCache>,
    ) -> Result<ReleaseCheckCache, GitHubError> {
        // /releases/latest excludes drafts and pre-releases; the beta channel lists releases instead.
        let path = match channel {
            ReleaseChannel::Stable => format!("repos/{}/{}/releases/latest", repository.owner, repository.name),
            ReleaseChannel::Beta => format!("repos/{}/{}/releases?per_page=20", repository.owner, repository.name),
        };
        let mut request = self.request(Method::GET, self.url(&path)?, token, Duration::from_secs(30));
        let same_request = cache.is_some_and(|c| c.repository == repository.full_name() && c.channel == channel);
        if same_request {
            if let Some(etag) = cache.and_then(|c| c.etag.as_deref()) {
                request = request.header("If-None-Match", etag);
            }
        }

        let response = Self::execute(request).await?;
        let rate = RateLimitStatus::from_headers(&response.headers);
        match response.status {
            200 => {
                let release = match channel {
                    ReleaseChannel::Stable => Some(GitHubRelease::decode(&response.data)?),
                    ReleaseChannel::Beta => {
                        let releases: Vec<GitHubRelease> = serde_json::from_slice(&response.data)
                            .map_err(|error| GitHubError::Decoding(error.to_string()))?;
                        // Swift's `max(by:)`: the first of equal maxima wins.
                        let mut best: Option<GitHubRelease> = None;
                        for release in releases.into_iter().filter(|r| !r.draft) {
                            let better = best
                                .as_ref()
                                .is_none_or(|b| php_version::compare(&b.version(), &release.version()) < 0);
                            if better {
                                best = Some(release);
                            }
                        }
                        best
                    }
                };
                Ok(ReleaseCheckCache {
                    repository: repository.full_name(),
                    channel,
                    etag: header(&response.headers, "ETag").map(str::to_owned),
                    release,
                    checked_at: SwiftDate::now(),
                    rate_limit: Some(rate),
                })
            }
            304 => {
                let Some(cached) = cache.filter(|_| same_request) else {
                    return Err(GitHubError::InvalidResponse);
                };
                let mut cached = cached.clone();
                cached.checked_at = SwiftDate::now();
                cached.rate_limit = Some(rate);
                Ok(cached)
            }
            401 => Err(GitHubError::Unauthorized),
            // GitHub answers 404 both for unknown/private repositories and for /releases/latest
            // when nothing is published, so the two cannot be told apart.
            404 => Err(GitHubError::NotFound),
            status @ (403 | 429) => {
                let retry_after = header(&response.headers, "Retry-After");
                if rate.remaining == Some(0) || retry_after.is_some() || status == 429 {
                    let retry = retry_after
                        .and_then(|v| v.parse::<f64>().ok())
                        .map(|seconds| SwiftDate::now().adding(seconds));
                    return Err(GitHubError::RateLimited(retry.or(rate.reset)));
                }
                Err(GitHubError::Http(status))
            }
            status => Err(GitHubError::Http(status)),
        }
    }
}

#[async_trait]
impl GitHubReleasing for GitHubService {
    async fn commit_exists(&self, sha: &str, repository: &GitHubRepository, token: &str) -> Result<bool, GitHubError> {
        let url = self.url(&format!("repos/{}/commits/{sha}", repository.full_name()))?;
        let response = Self::send(self.request(Method::GET, url, Some(token), Duration::from_secs(60))).await?;
        match response.status {
            200 => Ok(true),
            404 | 422 => Ok(false),
            status => Err(GitHubError::Http(status)),
        }
    }

    async fn release(
        &self,
        tag: &str,
        repository: &GitHubRepository,
        token: &str,
    ) -> Result<Option<GitHubRelease>, GitHubError> {
        let url = self.url(&format!(
            "repos/{}/releases/tags/{}",
            repository.full_name(),
            encode_path(tag)
        ))?;
        let response = Self::send(self.request(Method::GET, url, Some(token), Duration::from_secs(60))).await?;
        match response.status {
            200 => GitHubRelease::decode(&response.data).map(Some),
            404 => Ok(None),
            status => Err(GitHubError::Http(status)),
        }
    }

    async fn create_release(
        &self,
        repository: &GitHubRepository,
        release: &CreateRelease<'_>,
        token: &str,
    ) -> Result<GitHubRelease, GitHubError> {
        let url = self.url(&format!("repos/{}/releases", repository.full_name()))?;
        let payload = serde_json::json!({
            "tag_name": release.tag,
            "target_commitish": release.commit,
            "name": release.name,
            "body": release.body,
            "draft": release.draft,
            "prerelease": release.prerelease,
        });
        let request = self
            .request(Method::POST, url, Some(token), Duration::from_secs(60))
            .header("Content-Type", "application/json")
            .body(payload.to_string());
        let response = Self::send(request).await?;
        match response.status {
            201 => GitHubRelease::decode(&response.data),
            404 => Err(GitHubError::NotFound),
            422 => Err(GitHubError::ValidationFailed(
                Self::error_message(&response.data).unwrap_or_else(|| "invalid release".into()),
            )),
            status => Err(GitHubError::Http(status)),
        }
    }

    async fn upload_asset(
        &self,
        file: &Path,
        name: &str,
        release: &GitHubRelease,
        token: &str,
    ) -> Result<(), GitHubError> {
        let template = release.upload_url.as_deref().ok_or(GitHubError::InvalidResponse)?;
        let url = Self::upload_url(template, name)?;
        let data = tokio::fs::read(file)
            .await
            .map_err(|error| GitHubError::File(format!("{}: {error}", file.display())))?;
        let request = self
            .request(Method::POST, url, Some(token), Duration::from_secs(600))
            .header("Content-Type", "application/zip")
            .body(data);
        let response = Self::send(request).await?;
        match response.status {
            201 => Ok(()),
            422 => Err(GitHubError::ValidationFailed(
                Self::error_message(&response.data).unwrap_or_else(|| "asset already exists".into()),
            )),
            status => Err(GitHubError::Http(status)),
        }
    }
}
