//! Port of `WordPressOrg/WordPressOrgService.swift`. `DirectoryListing` lives in `rd-records`.

use crate::http::{self, DECODING_FAILED};
use crate::url_text::encode_allowing;
use async_trait::async_trait;
use rd_model::ProductType;
use rd_records::DirectoryListing;
use rd_versioning::php_version;
use regex::Regex;
use serde::Deserialize;
use std::sync::LazyLock;
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WordPressOrgError {
    #[error("WordPress.org returned an unexpected response.")]
    InvalidResponse,
    #[error("WordPress.org returned HTTP {0}.")]
    Http(u16),
    /// The payload was not the expected JSON (Swift's `DecodingError`); the detail is for logs.
    #[error("{DECODING_FAILED}")]
    Decoding(String),
    /// The request failed before a response arrived (Swift's `URLError`).
    #[error("{0}")]
    Transport(String),
}

/// Read-only WordPress.org API lookups. Used for facts that only the directory knows,
/// such as the current WordPress release that "Tested up to" is measured against.
#[async_trait]
pub trait WordPressOrgServicing: Send + Sync {
    /// The latest stable WordPress release, e.g. `7.1.5`.
    async fn latest_word_press_version(&self) -> Result<String, WordPressOrgError>;
    /// The directory's listing for `slug`, or None when the directory has no such product
    /// (not approved yet, or closed).
    async fn directory_listing(
        &self,
        slug: &str,
        product_type: ProductType,
    ) -> Result<Option<DirectoryListing>, WordPressOrgError>;
}

#[derive(Clone, Debug)]
pub struct WordPressOrgService {
    client: reqwest::Client,
    base_url: Url,
}

impl WordPressOrgService {
    pub const DEFAULT_BASE_URL: &'static str = "https://api.wordpress.org/";

    /// A client for `https://api.wordpress.org/`. Fails only when TLS cannot be initialised.
    pub fn new() -> Result<Self, WordPressOrgError> {
        let base_url = Url::parse(Self::DEFAULT_BASE_URL).map_err(|_| WordPressOrgError::InvalidResponse)?;
        Ok(Self::with_client(
            http::client().map_err(WordPressOrgError::Transport)?,
            base_url,
        ))
    }

    /// Requests are resolved against `base_url` like Swift's `URL(string:relativeTo:)`, so a
    /// base with a path must end in `/`.
    pub fn with_client(client: reqwest::Client, base_url: Url) -> Self {
        Self { client, base_url }
    }

    /// A client for `base_url` (tests use a local server).
    pub fn with_base_url(base_url: Url) -> Result<Self, WordPressOrgError> {
        Ok(Self::with_client(
            http::client().map_err(WordPressOrgError::Transport)?,
            base_url,
        ))
    }

    async fn get(&self, path: &str) -> Result<(u16, Vec<u8>), WordPressOrgError> {
        let url = self
            .base_url
            .join(path)
            .map_err(|_| WordPressOrgError::InvalidResponse)?;
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|error| WordPressOrgError::Transport(http::transport_error(error)))?;
        let status = response.status().as_u16();
        let body = response
            .bytes()
            .await
            .map_err(|error| WordPressOrgError::Transport(http::transport_error(error)))?;
        Ok((status, body.to_vec()))
    }

    pub fn parse_listing(data: &[u8]) -> Result<DirectoryListing, WordPressOrgError> {
        #[derive(Deserialize)]
        struct Payload {
            version: Option<String>,
            tested: Option<String>,
            last_updated: Option<String>,
            error: Option<String>,
        }
        let payload: Payload =
            serde_json::from_slice(data).map_err(|error| WordPressOrgError::Decoding(error.to_string()))?;
        if payload.error.is_some() {
            return Err(WordPressOrgError::InvalidResponse);
        }
        Ok(DirectoryListing {
            version: payload.version,
            tested: payload.tested,
            last_updated: payload.last_updated,
        })
    }

    /// The first offer is the newest release; older branches follow as `autoupdate` offers.
    pub fn parse_latest_version(data: &[u8]) -> Result<String, WordPressOrgError> {
        #[derive(Deserialize)]
        struct Offer {
            current: Option<String>,
        }
        #[derive(Deserialize)]
        struct Payload {
            offers: Vec<Offer>,
        }
        static VERSION: LazyLock<Regex> = LazyLock::new(|| {
            #[allow(clippy::expect_used)]
            Regex::new(r"^\d+(\.\d+)+$").expect("valid version pattern")
        });
        let payload: Payload =
            serde_json::from_slice(data).map_err(|error| WordPressOrgError::Decoding(error.to_string()))?;
        payload
            .offers
            .into_iter()
            .find_map(|offer| offer.current)
            .filter(|version| VERSION.is_match(version))
            .ok_or(WordPressOrgError::InvalidResponse)
    }
}

#[async_trait]
impl WordPressOrgServicing for WordPressOrgService {
    async fn latest_word_press_version(&self) -> Result<String, WordPressOrgError> {
        let (status, data) = self.get("core/version-check/1.7/").await?;
        if status != 200 {
            return Err(WordPressOrgError::Http(status));
        }
        Self::parse_latest_version(&data)
    }

    async fn directory_listing(
        &self,
        slug: &str,
        product_type: ProductType,
    ) -> Result<Option<DirectoryListing>, WordPressOrgError> {
        let action = match product_type {
            ProductType::Plugin => "plugins/info/1.2/?action=plugin_information",
            ProductType::Theme => "themes/info/1.2/?action=theme_information",
        };
        let encoded = encode_allowing(&slug.to_lowercase(), |b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
        });
        let path = format!(
            "{action}&request%5Bslug%5D={encoded}&request%5Bfields%5D%5Bsections%5D=0&request%5Bfields%5D%5Bversions%5D=0"
        );
        let (status, data) = self.get(&path).await?;
        match status {
            200 => Self::parse_listing(&data).map(Some),
            404 => Ok(None),
            status => Err(WordPressOrgError::Http(status)),
        }
    }
}

/// WordPress release branches. "Tested up to" is compared by branch (`7.1`), not by
/// point release, the way Plugin Check compares it.
pub struct WordPressBranch;

impl WordPressBranch {
    /// `7.1.5` → `7.1`, `7` → `7.0`. None for anything that is not a numeric version.
    pub fn of(version: &str) -> Option<String> {
        static NUMERIC: LazyLock<Regex> = LazyLock::new(|| {
            #[allow(clippy::expect_used)]
            Regex::new(r"^\d+(\.\d+)*$").expect("valid branch pattern")
        });
        let trimmed = version.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\u{A0}');
        if !NUMERIC.is_match(trimmed) {
            return None;
        }
        let parts: Vec<&str> = trimmed.split('.').collect();
        Some(format!("{}.{}", parts[0], parts.get(1).copied().unwrap_or("0")))
    }

    /// Negative when `a`'s branch is older than `b`'s, zero when equal, positive when newer.
    pub fn compare(a: &str, b: &str) -> Option<i32> {
        let (a, b) = (Self::of(a)?, Self::of(b)?);
        Some(php_version::compare(&a, &b))
    }
}
