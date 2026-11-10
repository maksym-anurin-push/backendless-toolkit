pub mod application;
pub mod domain;
pub mod mcp;
mod infrastructure;

mod compositor {
    use freemework_abstractions::sync::{FDisposable, FDisposableDisposeRet, FDisposableInitRet};
    use crate::domain::AppInfo;
    use std::net::SocketAddr;
    use std::sync::Arc;

    use backendless_toolkit_contract::Api;

    #[derive(Debug)]
    pub struct CompositorInvalidConfigurationError;
    impl std::fmt::Display for CompositorInvalidConfigurationError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "CompositorInvalidConfigurationError")
        }
    }
    impl std::error::Error for CompositorInvalidConfigurationError {}

    pub struct Compositor {
        app_info: Arc<AppInfo>,
        listen_addr: SocketAddr,
        axum_server_shutdown_event_tx:
            std::sync::Arc<std::sync::RwLock<Option<tokio::sync::oneshot::Sender<()>>>>,
        axum_server_disposed_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    }
    impl Compositor {
        pub fn new(
            app_info: AppInfo,
            listen_addr: SocketAddr,
        ) -> Self {
            //
            Compositor {
                app_info: Arc::new(app_info),
                listen_addr,
                axum_server_shutdown_event_tx: Arc::new(std::sync::RwLock::new(None)),
                axum_server_disposed_rx: None,
            }
        }
    }

    impl std::fmt::Debug for Compositor {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Compositor")
                // .field("redis_transport", &self.redis_transport)
                // .field(
                //     "axum_server_shutdown_event_tx",
                //     &self.axum_server_shutdown_event_tx,
                // )
                // .field("axum_server_disposed_rx", &self.axum_server_disposed_rx)
                .finish()
        }
    }

    impl FDisposable for Compositor {
        fn init(&mut self) -> FDisposableInitRet<'_> {
            let (axum_server_disposed_tx, axum_server_disposed_rx) =
                tokio::sync::oneshot::channel::<()>();
            self.axum_server_disposed_rx
                .replace(axum_server_disposed_rx);

            Box::pin(async move {
                let usecases: crate::application::UseCasesBundle = super::application::UseCasesBundle {
                    app_info: self.app_info.clone(),
                };
            
                let api: Arc<dyn Api> = Arc::new(super::infrastructure::api_impl::ApiImpl::new(
                    usecases.clone(),
                ));

                let cors = tower_http::cors::CorsLayer::new()
                    .allow_origin(tower_http::cors::Any)
                    .allow_methods([axum::http::Method::POST])
                    .allow_headers([axum::http::header::CONTENT_TYPE]);

                let app = axum::Router::new()
                    // .with_post()
                    // .with_api_websocket()
                    .route("/healthcheck", axum::routing::get(handle_http_healthcheck))
                    .route("/welcome", axum::routing::get(handle_http_welcome))
                    .route(
                        "/",
                        axum::routing::get(|| async { axum::response::Redirect::to("/welcome") }),
                    )
                    .layer(cors)
                    // .with_state(api.clone())
                    ;

                let (axum_server_shutdown_event_tx, axum_server_shutdown_event_rx) =
                    tokio::sync::oneshot::channel::<()>();
                self.axum_server_shutdown_event_tx
                    .write()
                    .unwrap()
                    .replace(axum_server_shutdown_event_tx);

                let listener = tokio::net::TcpListener::bind(&self.listen_addr)
                    .await
                    .unwrap();
                log::debug!("Server started at http://{}", &self.listen_addr);
                tokio::spawn(async move {
                    axum::serve(listener, app)
                        .with_graceful_shutdown(async move {
                            let _ = axum_server_shutdown_event_rx.await; // Сервер працює, поки сюди не прийде сигнал
                            log::debug!("Server got shutdown signal");
                        })
                        .await
                        .unwrap();

                    // Повідомляємо що сервер заверщив роботу
                    if let Err(_) = axum_server_disposed_tx.send(()) {
                        log::warn!("Server has been dropped");
                    }
                });

                Ok(())
            })
        }

        fn dispose(&mut self) -> FDisposableDisposeRet<'_> {
            Box::pin(async move {
                log::debug!("Disposing...");

                if let Some(tx) = self.axum_server_shutdown_event_tx.write().unwrap().take() {
                    let _ = tx.send(()); // Сервер Axum миттєво і красиво завершить роботу
                }

                if let Some(rx) = self.axum_server_disposed_rx.take() {
                    let _ = rx.await;
                }

                log::debug!("Disposed");
            })
        }
    }

    async fn handle_http_healthcheck() -> impl axum::response::IntoResponse {
        //
        axum::response::Response::builder()
            .status(axum::http::StatusCode::NO_CONTENT)
            .body(axum::body::Body::empty())
            .unwrap()
    }

    async fn handle_http_welcome() -> impl axum::response::IntoResponse {
        //
        let page_content = "<!DOCTYPE html>
<html>
<head>
    <meta http-equiv=\"Content-Type\" content=\"text/html; charset=UTF-8\" />
    <title>Apache HTTP Server Test Page</title>
</head>
<body>
    <h1>It works!</h1>
    <p>The software is running, but no content has been added to the welcome page yet.</p>
</body>
</html>
";
        axum::response::Response::builder()
            .header("content-type", "text/html")
            .body(axum::body::Body::from(page_content))
            .unwrap()
    }
}

#[allow(non_snake_case,non_upper_case_globals)]
pub const BUILD__CONFIGURATION: &str = env!("BUILD__CONFIGURATION");

pub use compositor::{Compositor, CompositorInvalidConfigurationError};
