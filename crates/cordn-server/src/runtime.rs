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
    announce::{self, SelfEvent, SelfPublisher},
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
    health: contextvm_sdk::transport::server::ServerHealth,
    coordinator: Arc<Coordinator>,
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<()>>>,
    /// Publishes and refreshes the coordinator's own kind-0 / kind-10002 events.
    discoverability: Option<JoinHandle<()>>,
}

impl RunningServer {
    /// Local metadata only; not a client-delivery guarantee.
    pub fn health(&self) -> contextvm_sdk::transport::server::ServerHealth {
        self.health.clone()
    }

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
        if let Some(task) = &self.discoverability {
            task.abort();
        }
        self.coordinator.close();
    }
}

pub fn validate_private_key(private_key_hex: &str) -> Result<()> {
    signer::from_sk(private_key_hex)
        .context("server private key")
        .map(|_| ())
}

pub async fn start(cfg: ServerConfig) -> Result<RunningServer> {
    let discoverability_cfg = cfg.clone();
    let signer = match &cfg.private_key_hex {
        Some(hex) => signer::from_sk(hex).context("server private key")?,
        None => signer::generate(),
    };
    let signer_keys = signer.clone();
    let public_key = signer.public_key();
    let identity = ServerIdentity {
        public_key: public_key.to_hex(),
        nprofile: build_nprofile(&public_key, &cfg.relay_urls)?,
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
    let health = transport.health();
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let task_health = health.clone();
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
        if !task_cancellation.is_cancelled() && task_health.snapshot().critical_task_failed {
            anyhow::bail!("critical transport task exited");
        }
        Ok(())
    });

    // Say where this coordinator can be reached, and keep saying it. A client
    // holding only its public key has no other way to find it (see `announce`).
    let discoverability =
        spawn_discoverability(discoverability_cfg, signer_keys, cancellation.child_token());

    Ok(RunningServer {
        identity,
        health,
        coordinator,
        cancellation,
        task: Some(task),
        discoverability,
    })
}

/// Start the self-description publication loop, unless there is nothing to
/// publish or nowhere to publish it.
fn spawn_discoverability(
    cfg: ServerConfig,
    keys: signer::Keys,
    cancel: CancellationToken,
) -> Option<JoinHandle<()>> {
    if announce::self_events(&cfg).is_empty() {
        tracing::info!(
            "not publishing this coordinator's relay list or profile (both switched off)"
        );
        return None;
    }
    let targets = announce::publish_targets(&cfg);
    if targets.is_empty() {
        tracing::warn!("no relay to publish this coordinator's relay list to");
        return None;
    }
    Some(tokio::spawn(async move {
        let publisher = RelayPublisher::connect(keys, &targets).await;
        announce::run(&cfg, &publisher, cancel).await;
        publisher.close().await;
    }))
}

/// How long the publisher waits for its relay sockets before the first publish.
const CONNECT_WAIT_SECONDS: u64 = 10;

/// [`SelfPublisher`] over a short-lived `nostr-sdk` client. Separate from the
/// ContextVM transport's own relay pool on purpose: the fallback relays are
/// publication targets, not operational relays, and nothing is read from them.
struct RelayPublisher {
    client: nostr_sdk::Client,
}

impl RelayPublisher {
    async fn connect(keys: signer::Keys, targets: &[String]) -> Self {
        let client = nostr_sdk::Client::new(keys);
        for url in targets {
            if let Err(error) = client.add_relay(url.as_str()).await {
                tracing::warn!(relay = %url, %error, "cannot publish the coordinator's own events there");
            }
        }
        client.connect().await;
        // `connect` only spawns the connection tasks. Publishing before a socket
        // is up puts that relay in neither the success nor the failed set, so the
        // first round would report a relay as not having taken the event and wait
        // out a retry for no reason (seen against wss://relay.contextvm.org).
        client
            .wait_for_connection(std::time::Duration::from_secs(CONNECT_WAIT_SECONDS))
            .await;
        RelayPublisher { client }
    }

    async fn close(&self) {
        self.client.disconnect().await;
    }
}

#[async_trait::async_trait]
impl SelfPublisher for RelayPublisher {
    async fn publish(&self, targets: &[String], event: &SelfEvent) -> Vec<String> {
        use nostr_sdk::prelude::*;

        let tags: Vec<Tag> = event
            .tags
            .iter()
            .filter_map(|tag| Tag::parse(tag.clone()).ok())
            .collect();
        let builder = EventBuilder::new(Kind::Custom(event.kind), event.content.clone()).tags(tags);
        match self
            .client
            .send_event_builder_to(targets.to_vec(), builder)
            .await
        {
            Ok(output) => {
                let took: std::collections::HashSet<String> = output
                    .success
                    .iter()
                    .map(|url| url.to_string().trim_end_matches('/').to_owned())
                    .collect();
                // The reason is the only way an operator learns *why* a relay keeps
                // being retried — wss://relay.contextvm.org, for one, answers
                // "blocked: event kind 0 is not allowed".
                for (relay, reason) in &output.failed {
                    tracing::warn!(%relay, %reason, kind = event.kind, "relay would not take the coordinator's own event");
                }
                targets
                    .iter()
                    .filter(|target| !took.contains(target.trim_end_matches('/')))
                    .cloned()
                    .collect()
            }
            Err(error) => {
                tracing::warn!(%error, kind = event.kind, "could not publish the coordinator's own event");
                targets.to_vec()
            }
        }
    }
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
        // The transport publishes a kind 10002 (and a kind 0, when given profile
        // metadata) exactly once, as it starts, and cannot retry a relay that was
        // down or refresh a list a relay has dropped. `announce` owns both events
        // instead, so there is one place that decides what this coordinator says
        // about itself — and one place an operator configures.
        .with_publish_relay_list(false)
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

/// NIP-19 display forms (npub, nprofile) of a hex public key, embedding the
/// given relays in the nprofile.
pub fn pubkey_display(public_key_hex: &str, relay_urls: &[String]) -> Result<(String, String)> {
    use nostr::nips::nip19::ToBech32;

    let public_key = nostr::PublicKey::from_hex(public_key_hex)
        .context("server public key must be 64 hex characters")?;
    let npub = public_key
        .to_bech32()
        .context("encoding public key as npub")?;
    let nprofile = build_nprofile(&public_key, relay_urls)?;
    Ok((npub, nprofile))
}

fn build_nprofile(public_key: &nostr::PublicKey, relay_urls: &[String]) -> Result<String> {
    use nostr::nips::nip19::{Nip19Profile, ToBech32};

    let relays: Vec<nostr::RelayUrl> = relay_urls
        .iter()
        .filter_map(|relay| nostr::RelayUrl::parse(relay).ok())
        .collect();
    Nip19Profile::new(*public_key, relays)
        .to_bech32()
        .context("encoding server identity as nprofile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pubkey_display_encodes_nip19_forms() {
        // Deterministic hex key (all zeroes is not a valid x-only point; use a fixed one).
        let public_key = "2b2f5c2f4e5d6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b";
        let (npub, nprofile) =
            pubkey_display(public_key, &["wss://relay.contextvm.org".into()]).unwrap();
        assert!(npub.starts_with("npub1"));
        assert!(nprofile.starts_with("nprofile1"));
        assert!(pubkey_display("nothex", &[]).is_err());
    }

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
