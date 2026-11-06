//! Local-only health metadata. No keys, URLs, payloads or active probes are exposed.
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Local wall-clock timestamp in milliseconds.
pub fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Fixed-size local diagnostic metadata; never proof of client delivery.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSnapshot {
    /// Local updated at metadata.
    pub updated_at: u64,
    /// Local started at metadata.
    pub started_at: u64,
    /// Local loop age ms metadata.
    pub loop_age_ms: u64,
    /// Local connected relays metadata.
    pub connected_relays: usize,
    /// Local total relays metadata.
    pub total_relays: usize,
    /// Local rejected subscriptions metadata.
    pub rejected_subscriptions: usize,
    /// Local subscription repairs metadata.
    pub subscription_repairs: u64,
    /// Local last request at metadata.
    pub last_request_at: Option<u64>,
    /// Relay publish acknowledgement, NOT proof of client receipt.
    pub last_relay_ack_at: Option<u64>,
    /// Local critical task failed metadata.
    pub critical_task_failed: bool,
}

struct Repair {
    next: Instant,
    attempts: u32,
    transient: bool,
}
struct State {
    snapshot: HealthSnapshot,
    progress: Instant,
    network_changed: bool,
    // Internal routing only. Never serialized, logged or exported.
    closed: HashMap<(String, String), Repair>,
}
#[derive(Clone)]
/// Shared local health handle for an embedded host.
pub struct ServerHealth(Arc<Mutex<State>>);
impl Default for ServerHealth {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(State {
            snapshot: HealthSnapshot {
                started_at: timestamp_ms(),
                ..Default::default()
            },
            progress: Instant::now(),
            closed: HashMap::new(),
            network_changed: false,
        })))
    }
}
impl ServerHealth {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
    /// Request reconnect assistance for existing destinations only.
    pub fn network_changed(&self) {
        self.lock().network_changed = true;
    }
    pub(crate) fn take_network_changed(&self) -> bool {
        std::mem::take(&mut self.lock().network_changed)
    }
    /// Read a metadata snapshot without exporting routing data.
    pub fn snapshot(&self) -> HealthSnapshot {
        let s = self.lock();
        let mut snapshot = s.snapshot.clone();
        snapshot.updated_at = timestamp_ms();
        snapshot.loop_age_ms = s.progress.elapsed().as_millis() as u64;
        snapshot.rejected_subscriptions = s.closed.len();
        snapshot
    }
    pub(crate) fn progress(&self) {
        self.lock().progress = Instant::now();
    }
    pub(crate) fn request(&self) {
        self.lock().snapshot.last_request_at = Some(timestamp_ms());
    }
    pub(crate) fn acknowledged(&self) {
        self.lock().snapshot.last_relay_ack_at = Some(timestamp_ms());
    }
    pub(crate) fn failed(&self) {
        self.lock().snapshot.critical_task_failed = true;
    }
    pub(crate) fn relays(&self, connected: usize, total: usize) {
        let mut s = self.lock();
        s.snapshot.connected_relays = connected;
        s.snapshot.total_relays = total;
    }
    pub(crate) fn closed(&self, url: String, id: String, reason: &str) {
        let mut s = self.lock();
        // A hostile relay cannot create an unbounded metadata map.
        if s.closed.len() >= 128 && !s.closed.contains_key(&(url.clone(), id.clone())) {
            return;
        }
        let transient = reason.starts_with("rate-limited:") || reason.starts_with("error:");
        let entry = s.closed.entry((url, id)).or_insert(Repair {
            next: Instant::now() + Duration::from_secs(60),
            attempts: 0,
            transient,
        });
        entry.transient = transient;
    }
    pub(crate) fn accepted(&self, url: &str, id: &str) {
        self.lock().closed.remove(&(url.to_owned(), id.to_owned()));
    }
    pub(crate) fn due_repairs(&self) -> Vec<(String, String)> {
        let mut s = self.lock();
        let mut due = Vec::new();
        for ((url, id), repair) in &mut s.closed {
            if repair.transient && repair.next <= Instant::now() {
                repair.attempts = repair.attempts.saturating_add(1);
                let delay = 60u64.saturating_mul(1u64 << repair.attempts.min(4));
                repair.next = Instant::now() + Duration::from_secs(delay.min(900));
                due.push((url.clone(), id.clone()));
                if due.len() == 3 {
                    break;
                }
            }
        }
        due
    }
    pub(crate) fn repaired(&self) {
        self.lock().snapshot.subscription_repairs += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quiet_is_not_failure_and_snapshot_contains_no_routing_data() {
        let h = ServerHealth::default();
        h.closed(
            "wss://private.example".into(),
            "secret-subscription".into(),
            "blocked: no",
        );
        let s = h.snapshot();
        assert!(!s.critical_task_failed);
        assert_eq!(s.last_request_at, None);
        assert!(h.due_repairs().is_empty());
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("private.example"));
        assert!(!json.contains("secret-subscription"));
    }
    #[test]
    fn repairs_are_transient_bounded_and_back_off() {
        let h = ServerHealth::default();
        h.closed("relay".into(), "sub".into(), "rate-limited: busy");
        assert!(h.due_repairs().is_empty());
        {
            h.lock()
                .closed
                .get_mut(&("relay".into(), "sub".into()))
                .unwrap()
                .next = Instant::now();
        }
        assert_eq!(h.due_repairs().len(), 1);
        assert!(h.due_repairs().is_empty());
        h.accepted("relay", "sub");
        assert_eq!(h.snapshot().rejected_subscriptions, 0);
    }
}
