use freemework_abstractions::sync::FException;
use freemework_common::sync::{FUsingWorkerFuture, f_using};
use std::{env, net::SocketAddr};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    //dotenvy::dotenv().expect("Failed to read .env file");

    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Trace) // Встановлює дефолтний рівень для всього
        .init();

    let app_cancellation_token: tokio_util::sync::CancellationToken =
        tokio_util::sync::CancellationToken::new();

    let listen_addr: SocketAddr = env::var("BACKENDLESS_TOOLKIT__LISTEN__ADDR")
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or("127.0.0.1:8080".parse().unwrap());

    let local = tokio::task::LocalSet::new();
    let entrypoint_app_cancellation_token = app_cancellation_token.clone();
    let task = async move {
        if let Err(_) = entrypoint(
            entrypoint_app_cancellation_token,
            listen_addr,
        )
        .await
        {
            println!("[!!] Entrypoint завершився з помилкою.");
            std::process::exit(1);
        } else {
            println!("[OK] Entrypoint завершився штатно.");
        }
    };
    println!("Програма готова. Натисніть Ctrl + C для скасування...");
    local
        .run_until(async {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    println!("\n[!] Ctrl + C отримано! Ініціюю мануальне скасування...");
                    app_cancellation_token.cancel();
                    println!("Головний потік: всі задачі завершено. Вихід.");
                }
                _ = local.spawn_local(task) => {
                    app_cancellation_token.cancel();
                    println!("Головний потік: всі задачі завершено. Вихід.");
                }
            }
        })
        .await;
}

async fn entrypoint(
    app_cancellation_token: tokio_util::sync::CancellationToken,
    listen_addr: SocketAddr,
) -> Result<(), FException> {
    #[allow(non_snake_case, non_upper_case_globals)]
    const BUILD__APP__NAME: &str = env!("CARGO_PKG_NAME");
    #[allow(non_snake_case, non_upper_case_globals)]
    const BUILD__APP__VERSION: &str = env!("CARGO_PKG_VERSION");
    #[allow(non_snake_case, non_upper_case_globals)]
    const BUILD__CONFIGURATION: &str = env!("BUILD__CONFIGURATION");
    println!(
        "Starting {} v{} ({}) ...",
        BUILD__APP__NAME, BUILD__APP__VERSION, BUILD__CONFIGURATION
    );
    #[allow(non_snake_case, non_upper_case_globals)]
    const BUILD__DEPENDENCIES: &str = env!("BUILD__DEPENDENCIES");
    for (name, version) in BUILD__DEPENDENCIES.split(';').filter_map(|s| {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 2 {
            Some((parts[0], parts[1]))
        } else {
            None
        }
    }) {
        println!("  {}: {}", name, version);
    }

    let app_info = backendless_toolkit_runtime::domain::AppInfo {
        name: BUILD__APP__NAME.to_string(),
        version: BUILD__APP__VERSION.to_string(),
        configuration: BUILD__CONFIGURATION.to_string(),
        dependencies: BUILD__DEPENDENCIES
            .split(';')
            .filter_map(|s| {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    None
                }
            })
            .collect(),
    };

    let cancellation_token = app_cancellation_token.clone();
    f_using(
        backendless_toolkit_runtime::Compositor::new(app_info, listen_addr),
        |_compositor| -> FUsingWorkerFuture<()> {
            Box::pin(async move {
                log::debug!("entrypoint_with_compositor started");

                cancellation_token.cancelled().await;

                log::debug!("entrypoint_with_compositor finished");

                Ok(())
            })
        },
    )
    .await
}
