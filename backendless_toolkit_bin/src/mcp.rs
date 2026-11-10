//! MCP entrypoint: runs as a child process; MCP client talks over stdin/stdout pipes.

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Protocol uses stdout — keep all human logs on stderr.
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stderr)
        .filter_level(log::LevelFilter::Info)
        .init();

    log::info!("backendless-toolkit MCP (hello-world) starting on stdio");
    backendless_toolkit_runtime::mcp::hello_world::serve_stdio().await?;
    log::info!("backendless-toolkit MCP stopped");
    Ok(())
}
