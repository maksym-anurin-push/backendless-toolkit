use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::ConsoleClient;
use crate::error::{ApiFault, Error, Result};

#[derive(Debug, Serialize)]
struct LoginRequest<'a> {
    login: &'a str,
    password: &'a str,
}

#[derive(Debug, Deserialize)]
struct LoginBody {
    #[serde(rename = "otpCreated", default)]
    otp_created: bool,
    #[serde(flatten)]
    rest: Value,
}

#[derive(Debug, Clone)]
pub struct LoginResult {
    pub auth_key: String,
    pub body: Value,
}

impl ConsoleClient {
    /// Developer login. Stores the returned `auth-key` on this client.
    ///
    /// Corresponds to `POST /console/home/login` in the JS Console SDK.
    pub async fn login(&mut self, login: &str, password: &str) -> Result<LoginResult> {
        let response = self
            .request_raw(
                Method::POST,
                "console/home/login",
                Some(&LoginRequest { login, password }),
                false,
            )
            .await?;

        let auth_key_header = response
            .headers
            .get("auth-key")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        let status = response.status;
        let body_text = response.body;

        if !status.is_success() {
            if let Ok(fault) = serde_json::from_str::<ApiFault>(&body_text) {
                return Err(fault.into_error(status.as_u16(), body_text));
            }
            return Err(Error::UnexpectedStatus {
                status: status.as_u16(),
                body: body_text,
            });
        }

        let parsed: LoginBody = serde_json::from_str(&body_text)?;
        if parsed.otp_created {
            return Err(Error::OtpRequired);
        }

        let auth_key = auth_key_header
            .or_else(|| {
                parsed
                    .rest
                    .get("authKey")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            })
            .ok_or(Error::MissingAuthKey)?;

        self.set_auth_key(auth_key.clone());

        Ok(LoginResult {
            auth_key,
            body: parsed.rest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_request_serializes() {
        let body = LoginRequest {
            login: "dev@example.com",
            password: "secret",
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["login"], "dev@example.com");
        assert_eq!(json["password"], "secret");
    }
}
