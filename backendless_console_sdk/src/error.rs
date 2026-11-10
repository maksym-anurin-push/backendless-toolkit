use serde::Deserialize;

/// Error returned by the Backendless Console API or the SDK transport layer.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Backendless API error {code}: {message}")]
    Api { code: i64, message: String },

    #[error("HTTP {status}: {body}")]
    UnexpectedStatus { status: u16, body: String },

    #[error("login requires two-factor authentication (OTP), which is not supported")]
    OtpRequired,

    #[error("missing auth-key in login response")]
    MissingAuthKey,

    #[error("not authenticated: call login() first")]
    NotAuthenticated,

    #[error("table not found: {0}")]
    TableNotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize)]
pub(crate) struct ApiFault {
    pub code: Option<i64>,
    pub message: Option<String>,
}

impl ApiFault {
    pub fn into_error(self, status: u16, raw_body: String) -> Error {
        match (self.code, self.message) {
            (Some(code), Some(message)) => Error::Api { code, message },
            (Some(code), None) => Error::Api {
                code,
                message: raw_body.clone(),
            },
            (None, Some(message)) => Error::Api { code: 0, message },
            (None, None) => Error::UnexpectedStatus {
                status,
                body: raw_body,
            },
        }
    }
}
