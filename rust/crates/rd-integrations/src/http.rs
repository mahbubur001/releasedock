//! Shared HTTP plumbing for the WordPress.org and GitHub clients.

use reqwest::header::HeaderMap;

/// `User-Agent` of every request (GitHub requires one).
pub(crate) const USER_AGENT: &str = "ReleaseDock";

/// An HTTP client with rustls and the operating system's certificate verifier. Fails only
/// when the TLS backend cannot be initialised.
pub(crate) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| error.to_string())
}

/// A response header as text (`HTTPURLResponse.value(forHTTPHeaderField:)`).
pub(crate) fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// The error text of a failed request, without the URL (which may carry a query).
pub(crate) fn transport_error(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut text = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        text = format!("{text}: {cause}");
        source = cause.source();
    }
    text
}

/// Foundation's `DecodingError` description, shown to users when a payload does not decode.
pub(crate) const DECODING_FAILED: &str = "The data couldn’t be read because it isn’t in the correct format.";
