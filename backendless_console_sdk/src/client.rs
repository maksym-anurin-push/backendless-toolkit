use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::{ApiFault, Error, Result};

/// Initial backoff for retryable failures (exponential: 100ms, 200ms, 400ms, …).
const RETRY_INITIAL_DELAY_MS: u64 = 100;
/// Stop retrying once cumulative sleep reaches this budget.
const RETRY_MAX_WAIT_MS: u64 = 10 * 60 * 1000;

/// HTTP response with body already buffered (so decode failures can be retried).
#[derive(Debug)]
pub(crate) struct HttpTextResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

/// HTTP client for the Backendless Console (admin) API.
///
/// Endpoints mirror the official JS Console SDK (`backendless-console-sdk`).
/// The Console API is undocumented and may change without notice.
#[derive(Debug, Clone)]
pub struct ConsoleClient {
    http: Client,
    base_url: Url,
    auth_key: Option<String>,
}

impl ConsoleClient {
    /// Create a client pointed at a Console base URL
    /// (e.g. `https://eu-develop.backendless.com` or a self-hosted host).
    pub fn new(base_url: impl AsRef<str>) -> Result<Self> {
        let mut base_url = Url::parse(base_url.as_ref())?;
        // Ensure trailing slash so `join("console/...")` appends under the base path.
        if !base_url.path().ends_with('/') {
            let mut path = base_url.path().to_string();
            path.push('/');
            base_url.set_path(&path);
        }

        let http = Client::builder()
            .user_agent(concat!(
                "backendless-console-sdk/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()?;

        Ok(Self {
            http,
            base_url,
            auth_key: None,
        })
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn auth_key(&self) -> Option<&str> {
        self.auth_key.as_deref()
    }

    pub(crate) fn set_auth_key(&mut self, auth_key: impl Into<String>) {
        self.auth_key = Some(auth_key.into());
    }

    pub(crate) fn require_auth_key(&self) -> Result<&str> {
        self.auth_key
            .as_deref()
            .ok_or(Error::NotAuthenticated)
    }

    pub(crate) fn url(&self, path: &str) -> Result<Url> {
        let path = path.trim_start_matches('/');
        Ok(self.base_url.join(path)?)
    }

    /// Send a request and buffer the response body.
    ///
    /// Retries (exponential backoff, ~10 min budget) on:
    /// - transport / connection failures during `send`
    /// - HTTP 502 / 504
    /// - failures while reading/decoding the response body
    pub(crate) async fn request_raw(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
        require_auth: bool,
    ) -> Result<HttpTextResponse> {
        let url = self.url(path)?;
        let mut delay_ms = RETRY_INITIAL_DELAY_MS;
        let mut waited_ms = 0u64;

        loop {
            let mut builder = self.http.request(method.clone(), url.clone());

            let mut headers = HeaderMap::new();
            headers.insert(
                reqwest::header::ACCEPT,
                HeaderValue::from_static("application/json"),
            );

            if require_auth {
                let key = self.require_auth_key()?;
                headers.insert(
                    "auth-key",
                    HeaderValue::from_str(key).map_err(|e| Error::UnexpectedStatus {
                        status: 0,
                        body: format!("invalid auth-key header value: {e}"),
                    })?,
                );
            } else if let Some(key) = self.auth_key.as_deref() {
                if let Ok(value) = HeaderValue::from_str(key) {
                    headers.insert("auth-key", value);
                }
            }

            builder = builder.headers(headers);

            if let Some(body) = body {
                builder = builder.json(body);
            }

            let response = match builder.send().await {
                Ok(response) => response,
                Err(err) => {
                    if !sleep_before_retry(
                        &format!("transport error: {err}"),
                        &mut delay_ms,
                        &mut waited_ms,
                    )
                    .await
                    {
                        return Err(err.into());
                    }
                    continue;
                }
            };

            let status = response.status();
            let status_code = status.as_u16();
            if is_retryable_status(status_code) && waited_ms < RETRY_MAX_WAIT_MS {
                let _ = response.bytes().await;
                let _ = sleep_before_retry(
                    &format!("HTTP {status_code}"),
                    &mut delay_ms,
                    &mut waited_ms,
                )
                .await;
                continue;
            }

            let headers = response.headers().clone();
            match response.text().await {
                Ok(body) => {
                    return Ok(HttpTextResponse {
                        status,
                        headers,
                        body,
                    });
                }
                Err(err) => {
                    if !sleep_before_retry(
                        &format!("error decoding response body: {err}"),
                        &mut delay_ms,
                        &mut waited_ms,
                    )
                    .await
                    {
                        return Err(err.into());
                    }
                }
            }
        }
    }

    pub(crate) async fn request_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
        require_auth: bool,
    ) -> Result<T> {
        let response = self
            .request_raw(method, path, body, require_auth)
            .await?;
        Self::parse_json_response(response)
    }

    pub(crate) fn parse_json_response<T: DeserializeOwned>(
        response: HttpTextResponse,
    ) -> Result<T> {
        let status = response.status;
        let body = response.body;

        if status.is_success() {
            if body.trim().is_empty() {
                // Allow empty success bodies for callers that expect unit-like types via Value.
                return Ok(serde_json::from_str("null")?);
            }
            return Ok(serde_json::from_str(&body)?);
        }

        if let Ok(fault) = serde_json::from_str::<ApiFault>(&body) {
            return Err(fault.into_error(status.as_u16(), body));
        }

        Err(Error::UnexpectedStatus {
            status: status.as_u16(),
            body,
        })
    }
}

fn is_retryable_status(status: u16) -> bool {
    matches!(status, 502 | 504)
}

/// Sleep with exponential backoff. Returns `false` when the retry budget is exhausted.
async fn sleep_before_retry(reason: &str, delay_ms: &mut u64, waited_ms: &mut u64) -> bool {
    if *waited_ms >= RETRY_MAX_WAIT_MS {
        return false;
    }
    let sleep_ms = (*delay_ms).min(RETRY_MAX_WAIT_MS - *waited_ms);
    eprintln!(
        "{reason}; retry in {sleep_ms}ms (waited {waited_ms}ms / {RETRY_MAX_WAIT_MS}ms)"
    );
    tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    *waited_ms = waited_ms.saturating_add(sleep_ms);
    *delay_ms = delay_ms.saturating_mul(2);
    true
}
