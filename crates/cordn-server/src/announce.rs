//! What the coordinator publishes about itself: a NIP-65 relay list (kind
//! 10002) and a NIP-01 profile (kind 0).
//!
//! A client that is handed a coordinator's public key — in a group reference, an
//! invite link, a settings field — has one way to find out where to reach it:
//! the coordinator's own kind-10002 relay list. Without one it can only be used
//! by someone who also got its relays out of band, and a client whose stored
//! relays go stale has nothing to recover from.
//!
//! The ContextVM SDK will publish both events once, as the transport starts.
//! That is not enough for a long-lived coordinator: a relay that is unreachable
//! in that moment never learns the list, a relay that drops old replaceable
//! events quietly loses it, and nothing tries again. So this module owns the
//! publication instead ([`crate::runtime`] turns the SDK's off), and adds the
//! two things an operator actually needs:
//!
//! * **fallback relays** — the events also go to relays the coordinator does not
//!   use operationally but clients do read ([`crate::config::default_fallback_relay_urls`]),
//!   so being findable does not depend on a client already knowing where to look;
//! * **retries and a refresh** — a relay that refuses or does not answer is
//!   retried on [`RETRY_LADDER_MS`], and the whole set is published again every
//!   `CORDN_DISCOVERY_REFRESH_HOURS`.
//!
//! Event building and the retry schedule are plain functions and a trait, so
//! both are tested without a relay; [`crate::runtime`] supplies the real
//! publisher over `nostr-sdk`.

use std::collections::BTreeSet;

use crate::config::ServerConfig;

/// NIP-01 profile metadata.
pub const PROFILE_KIND: u16 = 0;
/// NIP-65 relay list metadata.
pub const RELAY_LIST_KIND: u16 = 10002;

/// How long to wait before retrying the relays that did not take an event.
/// Exhausted ladder with the refresh off means publication gives up; with the
/// refresh on, the next refresh starts the ladder again.
pub const RETRY_LADDER_MS: [i64; 4] = [30_000, 120_000, 600_000, 3_600_000];

/// One event the coordinator publishes about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfEvent {
    pub kind: u16,
    pub content: String,
    pub tags: Vec<Vec<String>>,
}

/// Publishes a [`SelfEvent`] to relays. Returns the relays that did **not**
/// take it (refused, errored, or never answered), so the caller can retry just
/// those. An implementation must not fail the whole round because one relay is
/// down.
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
pub trait SelfPublisher: Send + Sync {
    async fn publish(&self, targets: &[String], event: &SelfEvent) -> Vec<String>;
}

/// Trim a relay URL the way the config parser does, so the same relay named two
/// ways is published to once.
fn normalize(url: &str) -> String {
    url.trim().trim_end_matches('/').to_owned()
}

/// The relays the kind-10002 names: the operator's override, else the relays the
/// server actually listens on.
pub fn advertised_relay_urls(cfg: &ServerConfig) -> Vec<String> {
    let source = cfg
        .discoverability
        .relay_list_urls
        .as_ref()
        .unwrap_or(&cfg.relay_urls);
    dedupe(source.iter().map(|u| normalize(u)))
}

/// Every relay the self-description is sent to: the advertised ones first (a
/// client that already knows one of them finds the list there), then the
/// fallbacks.
pub fn publish_targets(cfg: &ServerConfig) -> Vec<String> {
    dedupe(
        advertised_relay_urls(cfg).into_iter().chain(
            cfg.discoverability
                .fallback_relay_urls
                .iter()
                .map(|u| normalize(u)),
        ),
    )
}

fn dedupe(urls: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    urls.filter(|u| !u.is_empty())
        .filter(|u| seen.insert(u.clone()))
        .collect()
}

/// The NIP-65 relay list. `None` when switched off, or when there is no relay to
/// name — an empty relay list would tell clients the coordinator is nowhere.
pub fn relay_list_event(cfg: &ServerConfig) -> Option<SelfEvent> {
    if !cfg.discoverability.publish_relay_list {
        return None;
    }
    let urls = advertised_relay_urls(cfg);
    if urls.is_empty() {
        return None;
    }
    Some(SelfEvent {
        kind: RELAY_LIST_KIND,
        content: String::new(),
        // unmarked `r` tags: read and write, which is what a coordinator's
        // relays are (clients publish requests there and read replies there)
        tags: urls
            .into_iter()
            .map(|url| vec!["r".to_owned(), url])
            .collect(),
    })
}

/// The NIP-01 profile, carrying only the fields the operator set.
pub fn profile_event(cfg: &ServerConfig) -> Option<SelfEvent> {
    if !cfg.discoverability.publish_profile {
        return None;
    }
    let mut fields = serde_json::Map::new();
    fields.insert("name".into(), cfg.server_name.clone().into());
    for (key, value) in [
        ("about", &cfg.server_about),
        ("website", &cfg.server_website),
        ("picture", &cfg.discoverability.picture),
        ("nip05", &cfg.discoverability.nip05),
        ("lud16", &cfg.discoverability.lud16),
    ] {
        if let Some(value) = value {
            fields.insert(key.into(), value.clone().into());
        }
    }
    Some(SelfEvent {
        kind: PROFILE_KIND,
        content: serde_json::Value::Object(fields).to_string(),
        tags: Vec::new(),
    })
}

/// Everything to publish, profile first (it is the cheap one and the one an
/// operator notices is missing).
pub fn self_events(cfg: &ServerConfig) -> Vec<SelfEvent> {
    [profile_event(cfg), relay_list_event(cfg)]
        .into_iter()
        .flatten()
        .collect()
}

/// What the loop decided to do after a publication round, so the schedule is
/// testable without a clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextRound {
    /// Nothing left to do: everything landed and the refresh is off.
    Done,
    /// Sleep this long, then publish to the relays that failed.
    Retry(i64),
    /// Sleep this long, then publish to every target again.
    Refresh(i64),
}

/// When to come back, given how many rounds have already failed in a row and
/// whether anything is still outstanding.
pub fn next_round(
    refresh_interval_ms: i64,
    failed: bool,
    consecutive_failures: usize,
) -> NextRound {
    if !failed {
        return if refresh_interval_ms > 0 {
            NextRound::Refresh(refresh_interval_ms)
        } else {
            NextRound::Done
        };
    }
    match RETRY_LADDER_MS.get(consecutive_failures) {
        // a retry must not sit past the next refresh
        Some(&delay) if refresh_interval_ms <= 0 || delay < refresh_interval_ms => {
            NextRound::Retry(delay)
        }
        _ if refresh_interval_ms > 0 => NextRound::Refresh(refresh_interval_ms),
        _ => NextRound::Done,
    }
}

/// Publish the self-description, then keep it published: retry the relays that
/// did not take it, and republish everything on the refresh interval. Returns
/// when there is nothing left to do or `cancel` fires.
pub async fn run(
    cfg: &ServerConfig,
    publisher: &dyn SelfPublisher,
    cancel: tokio_util::sync::CancellationToken,
) {
    let events = self_events(cfg);
    if events.is_empty() {
        tracing::info!(target: LOG_TARGET, "not publishing a relay list or a profile (both switched off)");
        return;
    }
    let every = publish_targets(cfg);
    if every.is_empty() {
        tracing::warn!(target: LOG_TARGET, "no relay to publish the coordinator's own events to");
        return;
    }

    let mut targets = every.clone();
    let mut consecutive_failures = 0usize;
    loop {
        let mut failed: BTreeSet<String> = BTreeSet::new();
        for event in &events {
            let refused = publisher.publish(&targets, event).await;
            if refused.is_empty() {
                tracing::info!(target: LOG_TARGET, kind = event.kind, relays = targets.len(), "published the coordinator's own event");
            } else {
                tracing::warn!(target: LOG_TARGET, kind = event.kind, refused = ?refused, "some relays did not take the coordinator's own event");
            }
            failed.extend(refused);
        }

        consecutive_failures = if failed.is_empty() {
            0
        } else {
            consecutive_failures + 1
        };
        let delay = match next_round(
            cfg.discoverability.refresh_interval_ms,
            !failed.is_empty(),
            consecutive_failures.saturating_sub(1),
        ) {
            NextRound::Done => {
                if !failed.is_empty() {
                    tracing::warn!(target: LOG_TARGET, relays = ?failed, "giving up on publishing the coordinator's own events (set CORDN_DISCOVERY_REFRESH_HOURS to keep trying)");
                }
                return;
            }
            NextRound::Retry(delay) => {
                targets = failed.into_iter().collect();
                delay
            }
            NextRound::Refresh(delay) => {
                targets = every.clone();
                consecutive_failures = 0;
                delay
            }
        };

        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(std::time::Duration::from_millis(delay as u64)) => {}
        }
    }
}

const LOG_TARGET: &str = "cordn_server::announce";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DiscoverabilityConfig;
    use std::sync::Mutex;

    fn cfg() -> ServerConfig {
        ServerConfig {
            relay_urls: vec!["wss://ours.example".into()],
            discoverability: DiscoverabilityConfig {
                fallback_relay_urls: vec!["wss://indexer.example".into()],
                refresh_interval_ms: 0,
                ..DiscoverabilityConfig::default()
            },
            ..ServerConfig::default()
        }
    }

    /// Records every round so a test can see who was asked, and answers with a
    /// scripted set of relays that refuse.
    struct FakePublisher {
        refuse: Mutex<Vec<Vec<String>>>,
        rounds: Mutex<Vec<(u16, Vec<String>)>>,
    }

    impl FakePublisher {
        fn new(refuse: Vec<Vec<String>>) -> Self {
            FakePublisher {
                refuse: Mutex::new(refuse),
                rounds: Mutex::new(Vec::new()),
            }
        }
        fn rounds(&self) -> Vec<(u16, Vec<String>)> {
            self.rounds.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl SelfPublisher for FakePublisher {
        async fn publish(&self, targets: &[String], event: &SelfEvent) -> Vec<String> {
            self.rounds
                .lock()
                .unwrap()
                .push((event.kind, targets.to_vec()));
            let mut refuse = self.refuse.lock().unwrap();
            if refuse.is_empty() {
                Vec::new()
            } else {
                refuse
                    .remove(0)
                    .into_iter()
                    .filter(|r| targets.contains(r))
                    .collect()
            }
        }
    }

    #[test]
    fn the_relay_list_names_every_advertised_relay_as_read_and_write() {
        let mut c = cfg();
        c.relay_urls = vec!["wss://a.example/".into(), "wss://b.example".into()];
        let event = relay_list_event(&c).expect("a relay list");
        assert_eq!(RELAY_LIST_KIND, event.kind);
        assert_eq!("", event.content);
        assert_eq!(
            vec![
                vec!["r".to_owned(), "wss://a.example".to_owned()],
                vec!["r".to_owned(), "wss://b.example".to_owned()],
            ],
            event.tags,
            "a trailing slash must not make two relays out of one"
        );
    }

    #[test]
    fn the_advertised_list_can_differ_from_the_relays_we_listen_on() {
        let mut c = cfg();
        c.discoverability.relay_list_urls = Some(vec!["wss://public.example".into()]);
        assert_eq!(
            vec!["wss://public.example".to_owned()],
            advertised_relay_urls(&c)
        );
    }

    #[test]
    fn an_empty_relay_list_is_never_published() {
        let mut c = cfg();
        c.relay_urls.clear();
        assert_eq!(
            None,
            relay_list_event(&c),
            "saying 'nowhere' is worse than saying nothing"
        );
    }

    #[test]
    fn the_profile_carries_only_the_fields_that_are_set() {
        let mut c = cfg();
        c.server_name = "cordn @ example".into();
        c.server_about = Some("MLS delivery".into());
        c.discoverability.nip05 = Some("cordn@example.com".into());
        let event = profile_event(&c).expect("a profile");
        assert_eq!(PROFILE_KIND, event.kind);
        assert_eq!(
            // serde_json orders the keys; what matters is which keys are there
            r#"{"about":"MLS delivery","name":"cordn @ example","nip05":"cordn@example.com"}"#,
            event.content
        );
        assert!(event.tags.is_empty());
    }

    #[test]
    fn each_switch_turns_off_its_own_event() {
        let mut c = cfg();
        assert_eq!(
            vec![PROFILE_KIND, RELAY_LIST_KIND],
            self_events(&c).iter().map(|e| e.kind).collect::<Vec<_>>()
        );
        c.discoverability.publish_profile = false;
        assert_eq!(
            vec![RELAY_LIST_KIND],
            self_events(&c).iter().map(|e| e.kind).collect::<Vec<_>>()
        );
        c.discoverability.publish_relay_list = false;
        assert!(self_events(&c).is_empty());
    }

    #[test]
    fn the_fallback_relays_are_published_to_as_well_and_only_once() {
        let mut c = cfg();
        c.relay_urls = vec!["wss://ours.example".into()];
        c.discoverability.fallback_relay_urls =
            vec!["wss://ours.example/".into(), "wss://indexer.example".into()];
        assert_eq!(
            vec![
                "wss://ours.example".to_owned(),
                "wss://indexer.example".to_owned()
            ],
            publish_targets(&c),
            "our own relays first, each relay once"
        );
    }

    #[test]
    fn a_retry_never_sits_past_the_next_refresh() {
        // refresh off: walk the ladder, then give up
        assert_eq!(NextRound::Retry(30_000), next_round(0, true, 0));
        assert_eq!(NextRound::Retry(3_600_000), next_round(0, true, 3));
        assert_eq!(NextRound::Done, next_round(0, true, 4));
        assert_eq!(NextRound::Done, next_round(0, false, 0));
        // refresh on: the ladder is capped by it, and success waits for it
        assert_eq!(
            NextRound::Refresh(86_400_000),
            next_round(86_400_000, false, 0)
        );
        assert_eq!(NextRound::Retry(600_000), next_round(86_400_000, true, 2));
        assert_eq!(
            NextRound::Retry(30_000),
            next_round(60_000, true, 0),
            "a retry sooner than the refresh still happens"
        );
        assert_eq!(
            NextRound::Refresh(60_000),
            next_round(60_000, true, 2),
            "a retry that would sit past the refresh gives way to it"
        );
    }

    #[tokio::test]
    async fn publication_stops_once_every_relay_has_taken_both_events() {
        let publisher = FakePublisher::new(vec![]);
        run(
            &cfg(),
            &publisher,
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        assert_eq!(
            vec![
                (
                    PROFILE_KIND,
                    vec![
                        "wss://ours.example".to_owned(),
                        "wss://indexer.example".to_owned()
                    ]
                ),
                (
                    RELAY_LIST_KIND,
                    vec![
                        "wss://ours.example".to_owned(),
                        "wss://indexer.example".to_owned()
                    ]
                ),
            ],
            publisher.rounds()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn only_the_relay_that_refused_is_asked_again() {
        // the profile is refused by the indexer in round one; everything else lands
        let publisher = FakePublisher::new(vec![vec!["wss://indexer.example".into()]]);
        run(
            &cfg(),
            &publisher,
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        let rounds = publisher.rounds();
        assert_eq!(4, rounds.len(), "two events, twice: {rounds:?}");
        assert_eq!(
            vec!["wss://indexer.example".to_owned()],
            rounds[2].1,
            "the retry goes to the one relay that refused, not to the one that already has it"
        );
        assert_eq!(rounds[2].1, rounds[3].1);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_stops_the_retries() {
        // always refuses, so without the cancel this would walk the whole ladder
        let publisher = FakePublisher::new(vec![vec!["wss://indexer.example".into()]; 8]);
        let cancel = tokio_util::sync::CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            cancel_clone.cancel();
        });
        let mut c = cfg();
        c.discoverability.refresh_interval_ms = 86_400_000;
        run(&c, &publisher, cancel).await;
        assert_eq!(2, publisher.rounds().len(), "one round, then the cancel");
    }
}
