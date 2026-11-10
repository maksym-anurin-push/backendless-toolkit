use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, Method, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::error::{ApiFault, Error, Result};

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

    pub(crate) async fn request_raw(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
        require_auth: bool,
    ) -> Result<Response> {
        let url = self.url(path)?;
        let mut builder = self.http.request(method, url);

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

        Ok(builder.send().await?)
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
        Self::parse_json_response(response).await
    }

    pub(crate) async fn parse_json_response<T: DeserializeOwned>(response: Response) -> Result<T> {
        let status = response.status();
        let body = response.text().await?;

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
