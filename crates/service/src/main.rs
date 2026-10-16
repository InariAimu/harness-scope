use clap::Parser;
use harness_scope_core::Engine;
use std::{path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(about = "Harness Scope local API and self-hosted usage sync")]
struct Args {
    #[arg(long, default_value = "harness-scope.db")]
    db: PathBuf,
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: String,
    #[arg(long, env = "HARNESS_SCOPE_TOKEN")]
    token: Option<String>,
    #[arg(long)]
    local: bool,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let engine = Arc::new(Engine::open(&args.db)?);
    let router = if args.local {
        let address: std::net::SocketAddr = args.bind.parse()?;
        anyhow::ensure!(
            address.ip().is_loopback(),
            "Local command API must bind a loopback address"
        );
        engine.start_background();
        harness_scope_service::local_router(engine)
    } else {
        harness_scope_service::server_router(
            engine,
            args.token
                .ok_or_else(|| anyhow::anyhow!("HARNESS_SCOPE_TOKEN or --token is required"))?,
        )?
    };
    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    println!(
        "Harness Scope listening on {} ({})",
        args.bind,
        if args.local {
            "local development"
        } else {
            "usage sync"
        }
    );
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
