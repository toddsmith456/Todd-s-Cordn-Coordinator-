//! cordn-server bin entrypoint — loads desktop configuration and runs the
//! shared ContextVM coordinator runtime until shutdown.
#![cfg(feature = "server")]

use anyhow::{Context, Result};

use cordn_server::{
    config,
    runtime::{self, ServerIdentity},
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                // Include relay lifecycle logs because nostr-sdk connects in the
                // background after the runtime starts.
                tracing_subscriber::EnvFilter::new(
                    "cordn_server=info,contextvm_sdk=info,nostr_relay_pool=info,nostr=info,rmcp=warn",
                )
            }),
        )
        .init();

    let cfg = config::load().context("reading cordn server config")?;
    let relay_urls = cfg.relay_urls.clone();
    let announced = cfg.is_announced;
    let server = runtime::start(cfg).await?;

    print_banner(server.identity(), &relay_urls);
    tracing::info!(
        relays = ?relay_urls,
        announced,
        server_pubkey = %server.identity().public_key,
        "ContextVM MLS coordinator server starting (relay connection completes in the background)"
    );

    server
        .run_until(async {
            shutdown_signal().await;
            tracing::info!("cordn server shutting down");
        })
        .await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        ctrl_c.await;
    }
}

fn print_banner(identity: &ServerIdentity, relay_urls: &[String]) {
    let rule: String = "═".repeat(68);
    let relay_lines = if relay_urls.is_empty() {
        "     (none configured)".to_string()
    } else {
        relay_urls
            .iter()
            .map(|relay| format!("     • {relay}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let coordinator_url = format!(
        "https://cordn.net/chat/coordinators?c={}",
        identity.nprofile
    );

    println!("\n  {rule}");
    println!("   🔑  CORDN COORDINATOR — Server Public Key");
    println!("  {rule}\n");
    println!("   pubkey    {}", identity.public_key);
    println!("   nprofile  {}", identity.nprofile);
    println!("\n   📡  relays");
    println!("{relay_lines}");
    println!("\n   🌐  add in cordn.net");
    println!("     {coordinator_url}");
    println!();
}
