use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Once},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::{Days, Local, NaiveTime, TimeZone};
use cordn_core::Synchronous;
use cordn_server::{
    config::{ServerConfig, StorageBackend, StorageConfig},
    runtime,
};
use rusqlite::{Connection, OpenFlags};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tauri::{plugin::PluginApi, AppHandle, Manager, Runtime};
use tokio::sync::oneshot;

use crate::{
    ConfigResponse, CoordinatorConfig, EncodePubkeyRequest, PubkeyDisplay, PublishRelayListPayload,
    PublishRelayListResponse, ServiceDiagnostics, ServiceStatus, StartRequest,
};

const MAX_LOG_BYTES: u64 = 64 * 1024;

static LOGGING: Once = Once::new();

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StoredSchedule {
    enabled: bool,
    times: Vec<String>,
}

impl Default for StoredSchedule {
    fn default() -> Self {
        Self {
            enabled: false,
            times: vec!["03:00".into()],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleCommand {
    #[serde(default = "default_op")]
    operation: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_times")]
    times: Vec<String>,
}

fn default_op() -> String {
    "get".into()
}

fn default_times() -> Vec<String> {
    vec!["03:00".into()]
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredIdentity {
    private_key_hex: String,
}

struct WorkerState {
    state: String,
    public_key: Option<String>,
    last_error: Option<String>,
    manually_stopped: bool,
    stop_tx: Option<oneshot::Sender<()>>,
    schedule_cancel: Option<oneshot::Sender<()>>,
}

impl Default for WorkerState {
    fn default() -> Self {
        Self {
            state: "stopped".into(),
            public_key: None,
            last_error: None,
            manually_stopped: true,
            stop_tx: None,
            schedule_cancel: None,
        }
    }
}

struct Inner {
    data_dir: PathBuf,
    worker: Mutex<WorkerState>,
}

pub struct CordnAndroidService<R: Runtime> {
    inner: Arc<Inner>,
    _marker: std::marker::PhantomData<fn() -> R>,
}

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<CordnAndroidService<R>> {
    let data_dir = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("com.toddsmith456.cordn.coordinator"));
    fs::create_dir_all(&data_dir)?;
    init_logging(&data_dir);

    let mut initial = WorkerState::default();
    if let Some(status) = read_json::<ServiceStatus>(&data_dir.join("cordn-status.json")) {
        initial.public_key = status.public_key;
    } else if let Some(id) = read_json::<StoredIdentity>(&data_dir.join("cordn-identity.json")) {
        if let Ok(keys) = nostr::Keys::parse(&id.private_key_hex) {
            initial.public_key = Some(keys.public_key().to_hex());
        }
    }

    let inner = Arc::new(Inner {
        data_dir,
        worker: Mutex::new(initial),
    });
    reschedule_timer(&inner);

    Ok(CordnAndroidService {
        inner,
        _marker: std::marker::PhantomData,
    })
}

impl<R: Runtime> CordnAndroidService<R> {
    pub fn start(&self, payload: StartRequest) -> crate::Result<ServiceStatus> {
        payload.config.validate()?;
        if let Some(private_key_hex) = &payload.private_key_hex {
            let trimmed = private_key_hex.trim();
            if !trimmed.is_empty() {
                runtime::validate_private_key(trimmed).map_err(|_| {
                    crate::Error::InvalidConfig(
                        "private key is not a valid Nostr secret key".into(),
                    )
                })?;
            }
        }

        let private_key =
            get_or_create_identity(&self.inner.data_dir, payload.private_key_hex.as_deref())?;
        write_atomic(
            &self.inner.data_dir.join("cordn-config.json"),
            &serde_json::to_vec(&payload.config)?,
        )?;

        {
            let mut guard = self.inner.worker.lock().unwrap();
            if matches!(guard.state.as_str(), "starting" | "running") {
                return Ok(build_status(&self.inner.data_dir, &guard));
            }
            guard.manually_stopped = false;
            guard.state = "starting".into();
            guard.last_error = None;
        }

        spawn_worker(self.inner.clone(), payload.config, private_key);
        reschedule_timer(&self.inner);

        let guard = self.inner.worker.lock().unwrap();
        Ok(build_status(&self.inner.data_dir, &guard))
    }

    pub fn restart_schedule(
        &self,
        _payload: serde_json::Value,
    ) -> crate::Result<serde_json::Value> {
        Err(crate::Error::Unsupported)
    }

    pub fn stop(&self) -> crate::Result<ServiceStatus> {
        let mut guard = self.inner.worker.lock().unwrap();
        guard.manually_stopped = true;
        if let Some(tx) = guard.stop_tx.take() {
            guard.state = "stopping".into();
            let _ = tx.send(());
        } else {
            guard.state = "stopped".into();
        }
        let status = build_status(&self.inner.data_dir, &guard);
        let _ = persist_status(&self.inner.data_dir, &status);
        Ok(status)
    }

    pub fn status(&self) -> crate::Result<ServiceStatus> {
        let guard = self.inner.worker.lock().unwrap();
        Ok(build_status(&self.inner.data_dir, &guard))
    }

    pub fn config(&self) -> crate::Result<ConfigResponse> {
        let config = read_json::<CoordinatorConfig>(&self.inner.data_dir.join("cordn-config.json"));
        Ok(ConfigResponse { config })
    }

    pub fn diagnostics(&self) -> crate::Result<ServiceDiagnostics> {
        let db_path = self.inner.data_dir.join("cordn.sqlite");
        let wal_path = PathBuf::from(format!("{}-wal", db_path.display()));
        let shm_path = PathBuf::from(format!("{}-shm", db_path.display()));
        let storage_bytes = [&db_path, &wal_path, &shm_path]
            .iter()
            .filter_map(|p| fs::metadata(p).ok().map(|m| m.len()))
            .sum();

        let running = {
            let guard = self.inner.worker.lock().unwrap();
            guard.state == "running"
        };
        let started_at = if running {
            read_json::<serde_json::Value>(&self.inner.data_dir.join("cordn-health.json"))
                .and_then(|v| v.get("startedAt").and_then(|s| s.as_u64()))
        } else {
            None
        };

        let mut key_packages = None;
        let mut welcomes = None;
        let mut join_requests = None;
        let mut group_messages = None;

        if db_path.exists() {
            if let Ok(conn) =
                Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            {
                key_packages = count_table(&conn, "key_packages");
                welcomes = count_table(&conn, "welcomes");
                join_requests = count_table(&conn, "join_requests");
                group_messages = count_table(&conn, "group_messages");
            }
        }

        Ok(ServiceDiagnostics {
            logs: tail_file(&self.inner.data_dir.join("cordn.log")),
            storage_bytes,
            started_at,
            key_packages,
            welcomes,
            join_requests,
            group_messages,
        })
    }

    pub fn flush_data(&self) -> crate::Result<ServiceStatus> {
        let guard = self.inner.worker.lock().unwrap();
        if guard.state != "stopped" && guard.state != "error" {
            return Err(crate::Error::InvalidConfig(
                "Stop the coordinator before erasing data".into(),
            ));
        }
        delete_database(&self.inner.data_dir);
        Ok(build_status(&self.inner.data_dir, &guard))
    }

    pub fn reset_identity(&self) -> crate::Result<ServiceStatus> {
        let mut guard = self.inner.worker.lock().unwrap();
        if guard.state != "stopped" && guard.state != "error" {
            return Err(crate::Error::InvalidConfig(
                "Stop the coordinator before resetting the identity".into(),
            ));
        }
        let _ = fs::remove_file(self.inner.data_dir.join("cordn-identity.json"));
        let _ = fs::remove_file(self.inner.data_dir.join("cordn-status.json"));
        delete_database(&self.inner.data_dir);
        guard.public_key = None;
        guard.last_error = None;
        guard.state = "stopped".into();
        Ok(build_status(&self.inner.data_dir, &guard))
    }

    pub fn encode_pubkey(&self, payload: EncodePubkeyRequest) -> crate::Result<PubkeyDisplay> {
        let (npub, nprofile) = runtime::pubkey_display(&payload.public_key, &payload.relay_urls)
            .map_err(|error| crate::Error::InvalidConfig(format!("{error:#}")))?;
        Ok(PubkeyDisplay { npub, nprofile })
    }

    pub fn request_battery_exemption(&self) -> crate::Result<()> {
        Ok(())
    }

    pub fn request_notification_permission(&self) -> crate::Result<()> {
        Ok(())
    }

    pub fn publish_relay_list(
        &self,
        _payload: PublishRelayListPayload,
    ) -> crate::Result<PublishRelayListResponse> {
        // Desktop: read identity from the local file and publish via nostr-sdk.
        let private_key = get_or_create_identity(&self.inner.data_dir, None)?;
        let config = read_json::<CoordinatorConfig>(&self.inner.data_dir.join("cordn-config.json"))
            .unwrap_or_default();
        let relay_urls = if config.relay_urls.is_empty() {
            _payload.relay_urls
        } else {
            config.relay_urls
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let (sent, failed, targets) = rt.block_on(async {
            use nostr_sdk::prelude::*;
            let fallbacks: Vec<String> = vec![
                "wss://purplepag.es".into(),
                "wss://relay.damus.io".into(),
                "wss://relay.primal.net".into(),
                "wss://nos.lol".into(),
                "wss://relay.nostr.net".into(),
            ];
            let mut all_targets: Vec<String> = relay_urls
                .iter()
                .chain(fallbacks.iter())
                .map(|u| u.trim().trim_end_matches('/').to_owned())
                .filter(|u| !u.is_empty())
                .collect();
            all_targets.sort();
            all_targets.dedup();

            let secret_key = SecretKey::from_hex(&private_key)
                .map_err(|e| format!("invalid private key: {e}"))?;
            let keys = Keys::new(secret_key);
            let client = nostr_sdk::Client::new(keys);
            for url in &all_targets {
                let _ = client.add_relay(url.as_str()).await;
            }
            client.connect().await;
            client.wait_for_connection(std::time::Duration::from_secs(10)).await;

            let tags: Vec<Tag> = relay_urls
                .iter()
                .filter_map(|url| {
                    let trimmed = url.trim().trim_end_matches('/');
                    Tag::parse(["r", trimmed]).ok()
                })
                .collect();
            let builder = EventBuilder::new(Kind::Custom(10002), "").tags(tags);
            let output = client.send_event_builder_to(all_targets.clone(), builder).await
                .map_err(|e| format!("publish failed: {e}"))?;
            let failed_set: std::collections::HashSet<String> = output
                .failed
                .keys()
                .map(|url| url.to_string().trim_end_matches('/').to_owned())
                .collect();
            let sent = all_targets.iter().filter(|t| !failed_set.contains(t.as_str())).count();
            Ok::<_, String>((sent, failed_set.len(), all_targets))
        }).map_err(|e: String| crate::Error::InvalidConfig(e))?;

        Ok(PublishRelayListResponse { sent, failed, targets })
    }

    pub fn restart_schedule(&self, payload: serde_json::Value) -> crate::Result<serde_json::Value> {
        let cmd: ScheduleCommand = serde_json::from_value(payload).unwrap_or(ScheduleCommand {
            operation: "get".into(),
            enabled: false,
            times: vec!["03:00".into()],
        });

        match cmd.operation.as_str() {
            "save" => {
                validate_schedule(&cmd.times)?;
                let stored = StoredSchedule {
                    enabled: cmd.enabled,
                    times: cmd.times,
                };
                write_atomic(
                    &self.inner.data_dir.join("cordn-restart-schedule.json"),
                    &serde_json::to_vec(&stored)?,
                )?;
                reschedule_timer(&self.inner);
            }
            "get" | "permission" => {}
            other => {
                return Err(crate::Error::InvalidConfig(format!(
                    "Unknown schedule operation: {other}"
                )));
            }
        }

        let stored = read_schedule(&self.inner.data_dir);
        let next_alarm = if stored.enabled {
            stored
                .times
                .iter()
                .filter_map(|t| next_occurrence_ms(t))
                .min()
        } else {
            None
        };

        Ok(serde_json::json!({
            "enabled": stored.enabled,
            "times": stored.times,
            "exactAlarmAllowed": true,
            "nextAlarm": next_alarm,
        }))
    }

    pub fn publish_relay_list(
        &self,
        _payload: PublishRelayListPayload,
    ) -> crate::Result<PublishRelayListResponse> {
        Err(crate::Error::Unsupported)
    }
}

fn spawn_worker(inner: Arc<Inner>, config: CoordinatorConfig, private_key_hex: String) {
    let (stop_tx, stop_rx) = oneshot::channel();
    {
        let mut guard = inner.worker.lock().unwrap();
        guard.stop_tx = Some(stop_tx);
    }

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(err) => {
                let mut guard = inner.worker.lock().unwrap();
                guard.state = "error".into();
                guard.last_error = Some(err.to_string());
                guard.stop_tx = None;
                return;
            }
        };

        let server_cfg = build_server_config(config, private_key_hex, &inner.data_dir);
        let run_result: Result<(), String> = rt.block_on(async {
            let server = runtime::start(server_cfg)
                .await
                .map_err(|error| format!("{error:#}"))?;
            let pubkey = server.identity().public_key.clone();
            let health = server.health();
            let health_path = inner.data_dir.join("cordn-health.json");

            if let Ok(bytes) = serde_json::to_vec(&health.snapshot()) {
                let _ = write_atomic(&health_path, &bytes);
            }

            {
                let mut guard = inner.worker.lock().unwrap();
                guard.state = "running".into();
                guard.public_key = Some(pubkey);
                guard.last_error = None;
                let st = build_status(&inner.data_dir, &guard);
                let _ = persist_status(&inner.data_dir, &st);
            }

            let running = server.run_until(async {
                let _ = stop_rx.await;
            });
            tokio::pin!(running);

            let mut timer = tokio::time::interval(Duration::from_secs(15));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    res = &mut running => {
                        break res.map_err(|e| format!("Coordinator transport exited: {e:#}"));
                    }
                    _ = timer.tick() => {
                        if let Ok(bytes) = serde_json::to_vec(&health.snapshot()) {
                            let _ = write_atomic(&health_path, &bytes);
                        }
                    }
                }
            }
        });

        let mut guard = inner.worker.lock().unwrap();
        guard.stop_tx = None;
        match run_result {
            Ok(()) => {
                guard.state = "stopped".into();
            }
            Err(err) => {
                guard.state = "error".into();
                guard.last_error = Some(err);
            }
        }
        let st = build_status(&inner.data_dir, &guard);
        let _ = persist_status(&inner.data_dir, &st);
    });
}

fn reschedule_timer(inner: &Arc<Inner>) {
    let cancel_tx = {
        let mut guard = inner.worker.lock().unwrap();
        if let Some(prev) = guard.schedule_cancel.take() {
            let _ = prev.send(());
        }
        let stored = read_schedule(&inner.data_dir);
        if !stored.enabled {
            return;
        }
        let (tx, rx) = oneshot::channel();
        guard.schedule_cancel = Some(tx);
        rx
    };

    let weak_inner = Arc::downgrade(inner);
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
        else {
            return;
        };
        rt.block_on(async move {
            let mut cancel_rx = cancel_tx;
            loop {
                let Some(strong) = weak_inner.upgrade() else {
                    break;
                };
                let stored = read_schedule(&strong.data_dir);
                if !stored.enabled {
                    break;
                }
                let Some(next_ms) = stored
                    .times
                    .iter()
                    .filter_map(|t| next_occurrence_ms(t))
                    .min()
                else {
                    break;
                };
                let now_ms = now_millis();
                let wait_ms = next_ms.saturating_sub(now_ms).max(1_000);
                drop(strong);

                tokio::select! {
                    _ = &mut cancel_rx => break,
                    _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => {
                        let Some(strong) = weak_inner.upgrade() else {
                            break;
                        };
                        perform_scheduled_restart(&strong).await;
                    }
                }
            }
        });
    });
}

async fn perform_scheduled_restart(inner: &Arc<Inner>) {
    let should_restart = {
        let mut guard = inner.worker.lock().unwrap();
        if guard.manually_stopped || guard.state != "running" {
            false
        } else if let Some(tx) = guard.stop_tx.take() {
            guard.state = "stopping".into();
            let _ = tx.send(());
            true
        } else {
            false
        }
    };

    if !should_restart {
        return;
    }

    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let stopped = {
            let guard = inner.worker.lock().unwrap();
            if guard.manually_stopped {
                return;
            }
            guard.state == "stopped" || guard.state == "error"
        };
        if stopped {
            break;
        }
    }

    let config = read_json::<CoordinatorConfig>(&inner.data_dir.join("cordn-config.json"))
        .unwrap_or_default();
    let Ok(private_key) = get_or_create_identity(&inner.data_dir, None) else {
        return;
    };

    {
        let mut guard = inner.worker.lock().unwrap();
        if guard.manually_stopped || matches!(guard.state.as_str(), "starting" | "running") {
            return;
        }
        guard.state = "starting".into();
        guard.last_error = None;
    }
    spawn_worker(inner.clone(), config, private_key);
}

fn build_server_config(
    config: CoordinatorConfig,
    private_key_hex: String,
    data_dir: &Path,
) -> ServerConfig {
    ServerConfig {
        private_key_hex: Some(private_key_hex),
        relay_urls: config.relay_urls,
        server_name: config.server_name.trim().into(),
        server_about: non_empty(config.server_about),
        server_website: non_empty(config.server_website),
        is_announced: config.is_announced,
        storage: StorageConfig {
            backend: StorageBackend::Sqlite,
            sqlite_path: Some(data_dir.join("cordn.sqlite").to_string_lossy().into_owned()),
            sqlite_synchronous: Synchronous::Normal,
        },
        max_age_ms: i64::from(config.max_age_days) * 86_400_000,
        ..ServerConfig::default()
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn get_or_create_identity(data_dir: &Path, imported: Option<&str>) -> crate::Result<String> {
    let path = data_dir.join("cordn-identity.json");
    if let Some(raw) = imported {
        let normalized = raw.trim().to_ascii_lowercase();
        if !normalized.is_empty() {
            if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(crate::Error::InvalidConfig(
                    "private key must be 64 hex characters".into(),
                ));
            }
            runtime::validate_private_key(&normalized).map_err(|_| {
                crate::Error::InvalidConfig("private key is not a valid Nostr secret key".into())
            })?;
            store_identity_file(&path, &normalized)?;
            return Ok(normalized);
        }
    }

    if let Some(stored) = read_json::<StoredIdentity>(&path) {
        if runtime::validate_private_key(&stored.private_key_hex).is_ok() {
            return Ok(stored.private_key_hex);
        }
    }

    let keys = nostr::Keys::generate();
    let hex_secret = keys.secret_key().to_secret_hex();
    store_identity_file(&path, &hex_secret)?;
    Ok(hex_secret)
}

fn store_identity_file(path: &Path, secret_hex: &str) -> crate::Result<()> {
    let payload = serde_json::to_vec(&StoredIdentity {
        private_key_hex: secret_hex.to_owned(),
    })?;
    write_atomic(path, &payload)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn build_status(data_dir: &Path, worker: &WorkerState) -> ServiceStatus {
    let health = read_json::<serde_json::Value>(&data_dir.join("cordn-health.json"));
    let label = health_label(&worker.state, health.as_ref());
    ServiceStatus {
        health_label: Some(label),
        health: if worker.state == "running" {
            health
        } else {
            None
        },
        state: worker.state.clone(),
        public_key: worker.public_key.clone(),
        last_error: worker.last_error.clone(),
        // Desktop OSes do not impose Android Doze battery restrictions; suppress the Android banner.
        battery_exempt: true,
    }
}

fn health_label(state: &str, health: Option<&serde_json::Value>) -> String {
    if state != "running" {
        return state.to_owned();
    }
    let Some(h) = health else {
        return "Health not yet available".into();
    };
    let updated_at = h.get("updatedAt").and_then(|v| v.as_i64()).unwrap_or(0);
    let now_i64 = i64::try_from(now_millis()).unwrap_or(i64::MAX);
    let age = now_i64.saturating_sub(updated_at);
    if !(-30_000..=90_000).contains(&age) {
        return "Status stale — check coordinator".into();
    }
    let critical = h
        .get("criticalTaskFailed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let loop_age = h.get("loopAgeMs").and_then(|v| v.as_i64()).unwrap_or(0);
    if critical || loop_age > 180_000 {
        return "Degraded — transport not progressing".into();
    }
    let connected = h
        .get("connectedRelays")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if connected == 0 {
        return "Waiting for relay connection".into();
    }
    let rejected = h
        .get("rejectedSubscriptions")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if rejected > 0 {
        return "Degraded — subscription rejected".into();
    }
    "Relay connected — delivery unverified".into()
}

fn validate_schedule(times: &[String]) -> crate::Result<()> {
    if !(1..=2).contains(&times.len()) {
        return Err(crate::Error::InvalidConfig(
            "Choose one or two restart times".into(),
        ));
    }
    let mut parsed_mins = Vec::with_capacity(times.len());
    for t in times {
        let Some((hh, mm)) = parse_hhmm(t) else {
            return Err(crate::Error::InvalidConfig("Invalid restart time".into()));
        };
        parsed_mins.push((hh as i32) * 60 + (mm as i32));
    }
    if times.len() == 2 {
        if parsed_mins[0] == parsed_mins[1] {
            return Err(crate::Error::InvalidConfig(
                "Restart times must be different".into(),
            ));
        }
        let gap = (parsed_mins[0] - parsed_mins[1]).abs();
        if gap.min(1440 - gap) < 15 {
            return Err(crate::Error::InvalidConfig(
                "Keep restarts at least 15 minutes apart".into(),
            ));
        }
    }
    Ok(())
}

fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    if s.len() != 5 || &s[2..3] != ":" {
        return None;
    }
    let hh: u32 = s[0..2].parse().ok()?;
    let mm: u32 = s[3..5].parse().ok()?;
    if hh < 24 && mm < 60 {
        Some((hh, mm))
    } else {
        None
    }
}

fn next_occurrence_ms(time_str: &str) -> Option<u64> {
    let (hh, mm) = parse_hhmm(time_str)?;
    let target_time = NaiveTime::from_hms_opt(hh, mm, 0)?;
    let now = Local::now();
    let today_naive = now.date_naive().and_time(target_time);
    let candidate = Local
        .from_local_datetime(&today_naive)
        .earliest()
        .or_else(|| Local.from_local_datetime(&today_naive).latest())?;
    let next = if candidate > now {
        candidate
    } else {
        let tomorrow_naive = now
            .date_naive()
            .checked_add_days(Days::new(1))?
            .and_time(target_time);
        Local
            .from_local_datetime(&tomorrow_naive)
            .earliest()
            .or_else(|| Local.from_local_datetime(&tomorrow_naive).latest())?
    };
    u64::try_from(next.timestamp_millis()).ok()
}

fn read_schedule(data_dir: &Path) -> StoredSchedule {
    read_json::<StoredSchedule>(&data_dir.join("cordn-restart-schedule.json")).unwrap_or_default()
}

fn count_table(conn: &Connection, table: &str) -> Option<u64> {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get::<_, i64>(0)
    })
    .ok()
    .and_then(|n| u64::try_from(n).ok())
}

fn delete_database(data_dir: &Path) {
    let db = data_dir.join("cordn.sqlite");
    let wal = PathBuf::from(format!("{}-wal", db.display()));
    let shm = PathBuf::from(format!("{}-shm", db.display()));
    for p in [&db, &wal, &shm] {
        let _ = fs::remove_file(p);
    }
}

fn init_logging(data_dir: &Path) {
    let log_path = data_dir.join("cordn.log");
    LOGGING.call_once(|| {
        if let Ok(file) = File::create(log_path) {
            let _ = tracing_subscriber::fmt()
                .with_ansi(false)
                .with_writer(Mutex::new(file))
                .try_init();
        }
    });
}

fn tail_file(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let Ok(meta) = file.metadata() else {
        return String::new();
    };
    let start = meta.len().saturating_sub(MAX_LOG_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    if start == 0 {
        text
    } else {
        text.split_once('\n')
            .map(|(_, rest)| rest.to_owned())
            .unwrap_or(text)
    }
}

fn persist_status(data_dir: &Path, status: &ServiceStatus) -> crate::Result<()> {
    let bytes = serde_json::to_vec(status)?;
    write_atomic(&data_dir.join("cordn-status.json"), &bytes)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
