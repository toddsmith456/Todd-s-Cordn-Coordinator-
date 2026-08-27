//! Shared server runtime used by the desktop binary and embedded hosts.

use std::{future::Future, sync::Arc};

use anyhow::{Context, Result};
use contextvm_sdk::transport::open_stream::OpenStreamConfig;
use contextvm_sdk::transport::server::{NostrServerTransport, NostrServerTransportConfig};
use contextvm_sdk::{signer, EncryptionMode, GiftWrapMode, ServerInfo};
use rmcp::ServiceExt;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use cordn_core::{
    default_now, Coordinator, CoordinatorOptions, InMemoryCoordinatorStorage,
    SqliteCoordinatorStorage, DEFAULT_CLEANUP_INTERVAL_MS,
};

use crate::{
    adapter::{CoordinatorAdapter, Now},
    config::{ServerConfig, StorageBackend},
    methods::CordnServer,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerIdentity {
    pub public_key: String,
    pub nprofile: String,
}

pub struct RunningServer {
    identity: ServerIdentity,
    coordinator: Arc<Coordinator>,
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<()>>>,
}

impl RunningServer {
    pub fn identity(&self) -> &ServerIdentity {
        &self.identity
    }

    /// Run until the transport exits or the host requests shutdown.
    pub async fn run_until<F>(mut self, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()>,
    {
        let mut task = self.task.take().expect("server task already taken");
        let joined = tokio::select! {
            result = &mut task => result,
            _ = shutdown => {
                self.cancellation.cancel();
                task.await
            }
        };
        self.coordinator.close();
        joined.context("cordn server task")?
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.coordinator.close();
    }
}

pub fn validate_private_key(private_key_hex: &str) -> Result<()> {
    signer::from_sk(private_key_hex)
        .context("server private key")
        .map(|_| ())
}

pub async fn start(cfg: ServerConfig) -> Result<RunningServer> {
    let signer = match &cfg.private_key_hex {
        Some(hex) => signer::from_sk(hex).context("server private key")?,
        None => signer::generate(),
    };
    let public_key = signer.public_key();
    let identity = ServerIdentity {
        public_key: public_key.to_hex(),
        nprofile: build_nprofile(&public_key, &cfg.relay_urls),
    };

    let now: Now = default_now();
    let storage: Arc<dyn cordn_core::CoordinatorStorage> = match cfg.storage.backend {
        StorageBackend::Memory => Arc::new(InMemoryCoordinatorStorage::new()),
        StorageBackend::Sqlite => {
            let path = cfg
                .storage
                .sqlite_path
                .clone()
                .unwrap_or_else(|| "./cordn.sqlite".into());
            Arc::new(
                SqliteCoordinatorStorage::open(Some(&path), cfg.storage.sqlite_synchronous)
                    .with_context(|| format!("opening sqlite store at {path}"))?,
            )
        }
    };
    let coordinator = Coordinator::new(CoordinatorOptions {
        storage: Some(storage),
        now: Some(now.clone()),
        cleanup_interval_ms: Some(DEFAULT_CLEANUP_INTERVAL_MS),
        max_age_ms: Some(cfg.max_age_ms),
    });
    let adapter = Arc::new(CoordinatorAdapter::new(
        coordinator.clone(),
        cfg.abuse_protection.clone(),
        now,
    ));

    let transport = NostrServerTransport::new(signer, build_transport_config(&cfg))
        .await
        .context("creating ContextVM server transport")?;
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        let service = match CordnServer::new(adapter)
            .serve_with_ct(transport, task_cancellation.clone())
            .await
        {
            Ok(service) => service,
            Err(_) if task_cancellation.is_cancelled() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        service
            .waiting()
            .await
            .context("waiting for cordn server")?;
        Ok(())
    });

    Ok(RunningServer {
        identity,
        coordinator,
        cancellation,
        task: Some(task),
    })
}

fn build_transport_config(cfg: &ServerConfig) -> NostrServerTransportConfig {
    let mut server_info = ServerInfo::default()
        .with_name(cfg.server_name.clone())
        .with_about(cfg.server_about.clone().unwrap_or_default());
    if let Some(website) = &cfg.server_website {
        server_info = server_info.with_website(website.clone());
    }

    NostrServerTransportConfig::default()
        .with_relay_urls(cfg.relay_urls.clone())
        .with_announced_server(cfg.is_announced)
        .with_encryption_mode(EncryptionMode::Optional)
        .with_gift_wrap_mode(GiftWrapMode::Optional)
        .with_server_info(server_info)
        // CEP-41 streams are opt-in; keep 120s of idle/probe time for quiet
        // group subscriptions rather than the SDK's shorter default.
        .with_open_stream(
            OpenStreamConfig::default()
                .with_enabled(true)
                .with_idle_timeout_ms(60_000)
                .with_probe_timeout_ms(60_000),
        )
}

fn build_nprofile(public_key: &nostr::PublicKey, relay_urls: &[String]) -> String {
    use nostr::nips::nip19::{Nip19Profile, ToBech32};

    let relays: Vec<nostr::RelayUrl> = relay_urls
        .iter()
        .filter_map(|relay| nostr::RelayUrl::parse(relay).ok())
        .collect();
    Nip19Profile::new(*public_key, relays)
        .to_bech32()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_does_not_wait_for_the_first_client() {
        let mut config = ServerConfig::default();
        config.relay_urls.clear();

        let server = tokio::time::timeout(std::time::Duration::from_secs(1), start(config))
            .await
            .expect("start blocked waiting for an rmcp client")
            .unwrap();
        assert!(!server.identity().public_key.is_empty());
    }
}
