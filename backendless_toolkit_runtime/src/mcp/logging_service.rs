//! Service wrapper that logs every ServerHandler request/notification to a file.

use std::{borrow::Cow, sync::Arc};

use rmcp::{
    ErrorData, RoleServer, Service,
    model::ProtocolVersion,
    service::{NotificationContext, RequestContext, ServiceRole},
};

use super::request_log::RequestLog;

/// Wraps any MCP server [`Service`] and appends each request/notification to [`RequestLog`].
pub struct LoggingService<S> {
    inner: S,
    log: Arc<RequestLog>,
}

impl<S> LoggingService<S> {
    pub fn new(inner: S, log: Arc<RequestLog>) -> Self {
        Self { inner, log }
    }
}

fn json_or_error<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value)
        .unwrap_or_else(|err| serde_json::json!({ "serialize_error": err.to_string() }))
}

impl<S> Service<RoleServer> for LoggingService<S>
where
    S: Service<RoleServer>,
{
    async fn handle_request(
        &self,
        request: <RoleServer as ServiceRole>::PeerReq,
        context: RequestContext<RoleServer>,
    ) -> Result<<RoleServer as ServiceRole>::Resp, ErrorData> {
        let method = request.method().to_string();
        let request_json = json_or_error(&request);
        let started = chrono::Local::now();
        let result = self.inner.handle_request(request, context).await;

        let mut entry = serde_json::json!({
            "ts": started.to_rfc3339(),
            "kind": "request",
            "method": method,
            "request": request_json,
        });

        match &result {
            Ok(response) => {
                entry["ok"] = serde_json::Value::Bool(true);
                entry["response"] = json_or_error(response);
            }
            Err(error) => {
                entry["ok"] = serde_json::Value::Bool(false);
                entry["error"] = json_or_error(error);
            }
        }

        self.log.write_entry(&entry);
        result
    }

    async fn handle_notification(
        &self,
        notification: <RoleServer as ServiceRole>::PeerNot,
        context: NotificationContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let notification_json = json_or_error(&notification);
        let method = notification_json
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("notification")
            .to_string();
        let started = chrono::Local::now();
        let result = self.inner.handle_notification(notification, context).await;

        let mut entry = serde_json::json!({
            "ts": started.to_rfc3339(),
            "kind": "notification",
            "method": method,
            "notification": notification_json,
        });

        match &result {
            Ok(()) => {
                entry["ok"] = serde_json::Value::Bool(true);
            }
            Err(error) => {
                entry["ok"] = serde_json::Value::Bool(false);
                entry["error"] = json_or_error(error);
            }
        }

        self.log.write_entry(&entry);
        result
    }

    fn get_info(&self) -> <RoleServer as ServiceRole>::Info {
        self.inner.get_info()
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        self.inner.supported_protocol_versions()
    }
}
