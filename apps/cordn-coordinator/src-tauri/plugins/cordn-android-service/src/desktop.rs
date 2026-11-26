use std::{
    collections::HashMap,
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
    ConfigResponse, CoordinatorConfig, CreateInstanceRequest, DeleteInstanceRequest,
    EncodePubkeyRequest, InstanceEncodePubkeyRequest, InstanceInfo, InstanceRequest,
    InstanceStartRequest, InstanceStatus, InstancesResponse, PubkeyDisplay,
    PublishRelayListResponse, RenameInstanceRequest, ServiceDiagnostics, ServiceStatus,
    StartRequest, DEFAULT_INSTANCE_ID, DEFAULT_INSTANCE_NAME, MAX_INSTANCES,
};

const MAX_LOG_BYTES: u64 = 64 * 1024;
const INSTANCES_FILE: &str = "instances.json";

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

#[derive(Default)]
struct WorkerState {
    state: String,
    public_key: Option<String>,
    last_error: Option<String>,
    manually_stopped: bool,
    stop_tx: Option<oneshot::Sender<()>>,
    schedule_cancel: Option<oneshot::Sender<()>>,
}

impl WorkerState {
    fn stopped() -> Self {
        Self {
            state: "stopped".into(),
            manually_stopped: true,
            ..Self::default()
        }
    }
}

struct InstanceRuntime {
    info: InstanceInfo,
    worker: WorkerState,
}

struct Inner {
    data_dir: PathBuf,
    instances: Mutex<HashMap<String, InstanceRuntime>>,
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

    let mut instances = load_or_migrate_instances(&data_dir)?;
    for rt in instances.values_mut() {
        hydrate_worker_state(&data_dir, rt);
    }

    let inner = Arc::new(Inner {
        data_dir,
        instances: Mutex::new(instances),
    });

    // Reschedule timers for instances that have schedules.
    let ids: Vec<String> = {
        let guard = inner.instances.lock().unwrap();
        guard.keys().cloned().collect()
    };
    for id in ids {
        reschedule_timer(&inner, &id);
    }

    Ok(CordnAndroidService {
        inner,
        _marker: std::marker::PhantomData,
    })
}

fn instance_dir(data_dir: &Path, instance_id: &str) -> PathBuf {
    data_dir.join("instances").join(instance_id)
}

fn validate_instance_id(instance_id: &str) -> crate::Result<()> {
    let ok = !instance_id.is_empty()
        && instance_id.len() <= 64
        && instance_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(crate::Error::InvalidConfig(
            "invalid instance id".into(),
        ));
    }
    Ok(())
}

fn instances_index_path(data_dir: &Path) -> PathBuf {
    data_dir.join(INSTANCES_FILE)
}

fn load_instances_index(data_dir: &Path) -> Option<Vec<InstanceInfo>> {
    read_json::<Vec<InstanceInfo>>(&instances_index_path(data_dir))
}

fn save_instances_index(data_dir: &Path, instances: &HashMap<String, InstanceRuntime>) -> crate::Result<()> {
    let mut list: Vec<InstanceInfo> = instances.values().map(|rt| rt.info.clone()).collect();
    list.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    write_atomic(&instances_index_path(data_dir), &serde_json::to_vec(&list)?)
}

fn load_or_migrate_instances(data_dir: &Path) -> crate::Result<HashMap<String, InstanceRuntime>> {
    if let Some(list) = load_instances_index(data_dir) {
        let mut map = HashMap::new();
        for info in list {
            if validate_instance_id(&info.id).is_err() {
                continue;
            }
            let dir = instance_dir(data_dir, &info.id);
            let _ = fs::create_dir_all(&dir);
            map.insert(
                info.id.clone(),
                InstanceRuntime {
                    info,
                    worker: WorkerState::stopped(),
                },
            );
        }
        if !map.is_empty() {
            return Ok(map);
        }
    }

    // Legacy migration: existing single-instance layout at data_dir root.
    let has_legacy = ["cordn-config.json", "cordn-identity.json", "cordn-status.json", "cordn.sqlite", "cordn.log"]
        .iter()
        .any(|name| data_dir.join(name).exists());
    let mut map = HashMap::new();
    let default_info = InstanceInfo {
        id: DEFAULT_INSTANCE_ID.into(),
        name: DEFAULT_INSTANCE_NAME.into(),
        created_at: now_millis(),
    };
    let dir = instance_dir(data_dir, &default_info.id);
    fs::create_dir_all(&dir)?;
    if has_legacy {
        for name in [
            "cordn-config.json",
            "cordn-identity.json",
            "cordn-status.json",
            "cordn-health.json",
            "cordn.log",
            "cordn-restart-schedule.json",
            "cordn.sqlite",
            "cordn.sqlite-wal",
            "cordn.sqlite-shm",
        ] {
            let src = data_dir.join(name);
            if src.exists() {
                let dst = dir.join(name);
                if !dst.exists() {
                    let _ = fs::rename(&src, &dst).or_else(|_| fs::copy(&src, &dst).map(|_| ())).and_then(|_| {
                        // If copied, remove source to avoid stale duplicate.
                        if src.exists() && src != dst {
                            // rename succeeded means src gone; copy case: try remove
                            let _ = fs::remove_file(&src);
                        }
                        Ok(())
                    });
                }
            }
        }
    }
    map.insert(
        default_info.id.clone(),
        InstanceRuntime {
            info: default_info,
            worker: WorkerState::stopped(),
        },
    );
    save_instances_index(data_dir, &map)?;
    Ok(map)
}

fn hydrate_worker_state(data_dir: &Path, rt: &mut InstanceRuntime) {
    let dir = instance_dir(data_dir, &rt.info.id);
    if let Some(status) = read_json::<ServiceStatus>(&dir.join("cordn-status.json")) {
        rt.worker.public_key = status.public_key;
        rt.worker.last_error = status.last_error;
        // Do not auto-mark running; process restart means no live worker.
        rt.worker.state = "stopped".into();
        rt.worker.manually_stopped = true;
    } else if let Some(id) = read_json::<StoredIdentity>(&dir.join("cordn-identity.json")) {
        if let Ok(keys) = nostr::Keys::parse(&id.private_key_hex) {
            rt.worker.public_key = Some(keys.public_key().to_hex());
        }
        rt.worker.state = "stopped".into();
        rt.worker.manually_stopped = true;
    } else {
        rt.worker.state = "stopped".into();
        rt.worker.manually_stopped = true;
    }
}

impl<R: Runtime> CordnAndroidService<R> {
    fn ensure_instance(&self, instance_id: &str) -> crate::Result<()> {
        validate_instance_id(instance_id)?;
        let guard = self.inner.instances.lock().unwrap();
        if guard.contains_key(instance_id) {
            Ok(())
        } else {
            Err(crate::Error::InvalidConfig(format!(
                "unknown instance: {instance_id}"
            )))
        }
    }

    fn instance_info(&self, instance_id: &str) -> crate::Result<InstanceInfo> {
        let guard = self.inner.instances.lock().unwrap();
        guard
            .get(instance_id)
            .map(|rt| rt.info.clone())
            .ok_or_else(|| crate::Error::InvalidConfig(format!("unknown instance: {instance_id}")))
    }

    pub fn list_instances(&self) -> crate::Result<InstancesResponse> {
        let instances = {
            let guard = self.inner.instances.lock().unwrap();
            let mut list: Vec<InstanceInfo> = guard.values().map(|rt| rt.info.clone()).collect();
            list.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
            list
        };
        let mut out = Vec::with_capacity(instances.len());
        for info in instances {
            let dir = instance_dir(&self.inner.data_dir, &info.id);
            let status = {
                let guard = self.inner.instances.lock().unwrap();
                guard
                    .get(&info.id)
                    .map(|rt| build_status(&dir, &rt.worker))
                    .unwrap_or_default()
            };
            let config = read_json::<CoordinatorConfig>(&dir.join("cordn-config.json"));
            out.push(InstanceStatus { instance: info, status, config });
        }
        Ok(InstancesResponse { instances: out })
    }

    pub fn create_instance(&self, payload: CreateInstanceRequest) -> crate::Result<InstanceInfo> {
        let name = payload
            .name
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                let guard = self.inner.instances.lock().unwrap();
                format!("Coordinator {}", guard.len() + 1)
            });
        if name.len() > 64 {
            return Err(crate::Error::InvalidConfig(
                "instance name is too long".into(),
            ));
        }

        let mut guard = self.inner.instances.lock().unwrap();
        if guard.len() >= MAX_INSTANCES {
            return Err(crate::Error::InvalidConfig(format!(
                "maximum of {MAX_INSTANCES} coordinators allowed"
            )));
        }
        let mut id = format!("c{}", now_millis());
        let mut suffix = 0u32;
        while guard.contains_key(&id) {
            suffix += 1;
            id = format!("c{}-{suffix}", now_millis());
        }
        validate_instance_id(&id)?;
        let info = InstanceInfo {
            id: id.clone(),
            name,
            created_at: now_millis(),
        };
        let dir = instance_dir(&self.inner.data_dir, &id);
        fs::create_dir_all(&dir)?;
        guard.insert(
            id.clone(),
            InstanceRuntime {
                info: info.clone(),
                worker: WorkerState::stopped(),
            },
        );
        save_instances_index(&self.inner.data_dir, &guard)?;
        Ok(info)
    }

    pub fn rename_instance(&self, payload: RenameInstanceRequest) -> crate::Result<InstanceInfo> {
        validate_instance_id(&payload.instance_id)?;
        let name = payload.name.trim().to_owned();
        if name.is_empty() || name.len() > 64 {
            return Err(crate::Error::InvalidConfig(
                "instance name must be 1-64 characters".into(),
            ));
        }
        let mut guard = self.inner.instances.lock().unwrap();
        let rt = guard
            .get_mut(&payload.instance_id)
            .ok_or_else(|| crate::Error::InvalidConfig("unknown instance".into()))?;
        rt.info.name = name;
        let info = rt.info.clone();
        save_instances_index(&self.inner.data_dir, &guard)?;
        Ok(info)
    }

    pub fn delete_instance(&self, payload: DeleteInstanceRequest) -> crate::Result<InstancesResponse> {
        validate_instance_id(&payload.instance_id)?;
        let mut guard = self.inner.instances.lock().unwrap();
        if guard.len() <= 1 {
            return Err(crate::Error::InvalidConfig(
                "at least one coordinator is required".into(),
            ));
        }
        let rt = guard
            .get(&payload.instance_id)
            .ok_or_else(|| crate::Error::InvalidConfig("unknown instance".into()))?;
        if matches!(rt.worker.state.as_str(), "starting" | "running" | "stopping") {
            return Err(crate::Error::InvalidConfig(
                "Stop the coordinator before deleting it".into(),
            ));
        }
        guard.remove(&payload.instance_id);
        save_instances_index(&self.inner.data_dir, &guard)?;
        drop(guard);

        // Remove instance data dir only after it is removed from registry.
        let dir = instance_dir(&self.inner.data_dir, &payload.instance_id);
        if dir.starts_with(self.inner.data_dir.join("instances")) {
            let _ = fs::remove_dir_all(&dir);
        }
        self.list_instances()
    }

    fn resolve_any_instance_id(&self) -> crate::Result<String> {
        let guard = self.inner.instances.lock().unwrap();
        if let Some(rt) = guard.get(DEFAULT_INSTANCE_ID) {
            return Ok(rt.info.id.clone());
        }
        let mut list: Vec<_> = guard.values().map(|rt| rt.info.clone()).collect();
        list.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
        list.first()
            .map(|i| i.id.clone())
            .ok_or_else(|| crate::Error::InvalidConfig("no coordinator instances".into()))
    }

    // Legacy-compatible methods (default instance)
    pub fn start(&self, payload: StartRequest) -> crate::Result<ServiceStatus> {
        let id = self.resolve_any_instance_id()?;
        self.start_instance(InstanceStartRequest {
            instance_id: id,
            config: payload.config,
            private_key_hex: payload.private_key_hex,
        })
    }

    pub fn stop(&self) -> crate::Result<ServiceStatus> {
        let id = self.resolve_any_instance_id()?;
        self.stop_instance(InstanceRequest { instance_id: id })
    }

    pub fn status(&self) -> crate::Result<ServiceStatus> {
        let id = self.resolve_any_instance_id()?;
        self.status_instance(InstanceRequest { instance_id: id })
    }

    pub fn config(&self) -> crate::Result<ConfigResponse> {
        let id = self.resolve_any_instance_id()?;
        self.config_instance(InstanceRequest { instance_id: id })
    }

    pub fn diagnostics(&self) -> crate::Result<ServiceDiagnostics> {
        let id = self.resolve_any_instance_id()?;
        self.diagnostics_instance(InstanceRequest { instance_id: id })
    }

    pub fn flush_data(&self) -> crate::Result<ServiceStatus> {
        let id = self.resolve_any_instance_id()?;
        self.flush_data_instance(InstanceRequest { instance_id: id })
    }

    pub fn reset_identity(&self) -> crate::Result<ServiceStatus> {
        let id = self.resolve_any_instance_id()?;
        self.reset_identity_instance(InstanceRequest { instance_id: id })
    }

    pub fn publish_relay_list(&self) -> crate::Result<PublishRelayListResponse> {
        let id = self.resolve_any_instance_id()?;
        self.publish_relay_list_instance(InstanceRequest { instance_id: id })
    }

    pub fn restart_schedule(&self, payload: serde_json::Value) -> crate::Result<serde_json::Value> {
        let id = self.resolve_any_instance_id()?;
        self.restart_schedule_instance(&id, payload)
    }

    pub fn start_instance(&self, payload: InstanceStartRequest) -> crate::Result<ServiceStatus> {
        validate_instance_id(&payload.instance_id)?;
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

        let dir = instance_dir(&self.inner.data_dir, &payload.instance_id);
        fs::create_dir_all(&dir)?;
        let private_key =
            get_or_create_identity(&dir, payload.private_key_hex.as_deref())?;
        write_atomic(
            &dir.join("cordn-config.json"),
            &serde_json::to_vec(&payload.config)?,
        )?;

        {
            let mut guard = self.inner.instances.lock().unwrap();
            let rt = guard
                .get_mut(&payload.instance_id)
                .ok_or_else(|| crate::Error::InvalidConfig("unknown instance".into()))?;
            if matches!(rt.worker.state.as_str(), "starting" | "running") {
                return Ok(build_status(&dir, &rt.worker));
            }
            rt.worker.manually_stopped = false;
            rt.worker.state = "starting".into();
            rt.worker.last_error = None;
        }

        spawn_worker(self.inner.clone(), payload.instance_id.clone(), payload.config, private_key);
        reschedule_timer(&self.inner, &payload.instance_id);

        let guard = self.inner.instances.lock().unwrap();
        let rt = guard
            .get(&payload.instance_id)
            .ok_or_else(|| crate::Error::InvalidConfig("unknown instance".into()))?;
        Ok(build_status(&dir, &rt.worker))
    }

    pub fn stop_instance(&self, payload: InstanceRequest) -> crate::Result<ServiceStatus> {
        validate_instance_id(&payload.instance_id)?;
        let dir = instance_dir(&self.inner.data_dir, &payload.instance_id);
        let mut guard = self.inner.instances.lock().unwrap();
        let rt = guard
            .get_mut(&payload.instance_id)
            .ok_or_else(|| crate::Error::InvalidConfig("unknown instance".into()))?;
        rt.worker.manually_stopped = true;
        if let Some(tx) = rt.worker.stop_tx.take() {
            rt.worker.state = "stopping".into();
            let _ = tx.send(());
        } else {
            rt.worker.state = "stopped".into();
        }
        let status = build_status(&dir, &rt.worker);
        let _ = persist_status(&dir, &status);
        Ok(status)
    }

    pub fn status_instance(&self, payload: InstanceRequest) -> crate::Result<ServiceStatus> {
        validate_instance_id(&payload.instance_id)?;
        let dir = instance_dir(&self.inner.data_dir, &payload.instance_id);
        let guard = self.inner.instances.lock().unwrap();
        let rt = guard
            .get(&payload.instance_id)
            .ok_or_else(|| crate::Error::InvalidConfig("unknown instance".into()))?;
        Ok(build_status(&dir, &rt.worker))
    }

    pub fn config_instance(&self, payload: InstanceRequest) -> crate::Result<ConfigResponse> {
        validate_instance_id(&payload.instance_id)?;
        let dir = instance_dir(&self.inner.data_dir, &payload.instance_id);
        let config = read_json::<CoordinatorConfig>(&dir.join("cordn-config.json"));
        Ok(ConfigResponse { config })
    }

    pub fn diagnostics_instance(&self, payload: InstanceRequest) -> crate::Result<ServiceDiagnostics> {
        validate_instance_id(&payload.instance_id)?;
        let dir = instance_dir(&self.inner.data_dir, &payload.instance_id);
        let db_path = dir.join("cordn.sqlite");
        let wal_path = PathBuf::from(format!("{}-wal", db_path.display()));
        let shm_path = PathBuf::from(format!("{}-shm", db_path.display()));
        let storage_bytes = [&db_path, &wal_path, &shm_path]
            .iter()
            .filter_map(|p| fs::metadata(p).ok().map(|m| m.len()))
            .sum();

        let running = {
            let guard = self.inner.instances.lock().unwrap();
            guard
                .get(&payload.instance_id)
                .map(|rt| rt.worker.state == "running")
                .unwrap_or(false)
        };
        let started_at = if running {
            read_json::<serde_json::Value>(&dir.join("cordn-health.json"))
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
            logs: tail_file(&dir.join("cordn.log")),
            storage_bytes,
            started_at,
            key_packages,
            welcomes,
            join_requests,
            group_messages,
        })
    }

    pub fn flush_data_instance(&self, payload: InstanceRequest) -> crate::Result<ServiceStatus> {
        validate_instance_id(&payload.instance_id)?;
        let dir = instance_dir(&self.inner.data_dir, &payload.instance_id);
        let guard = self.inner.instances.lock().unwrap();
        let rt = guard
            .get(&payload.instance_id)
            .ok_or_else(|| crate::Error::InvalidConfig("unknown instance".into()))?;
        if rt.worker.state != "stopped" && rt.worker.state != "error" {
            return Err(crate::Error::InvalidConfig(
                "Stop the coordinator before erasing data".into(),
            ));
        }
        delete_database(&dir);
        Ok(build_status(&dir, &rt.worker))
    }

    pub fn reset_identity_instance(&self, payload: InstanceRequest) -> crate::Result<ServiceStatus> {
        validate_instance_id(&payload.instance_id)?;
        let dir = instance_dir(&self.inner.data_dir, &payload.instance_id);
        let mut guard = self.inner.instances.lock().unwrap();
        let rt = guard
            .get_mut(&payload.instance_id)
            .ok_or_else(|| crate::Error::InvalidConfig("unknown instance".into()))?;
        if rt.worker.state != "stopped" && rt.worker.state != "error" {
            return Err(crate::Error::InvalidConfig(
                "Stop the coordinator before resetting the identity".into(),
            ));
        }
        let _ = fs::remove_file(dir.join("cordn-identity.json"));
        let _ = fs::remove_file(dir.join("cordn-status.json"));
        delete_database(&dir);
        rt.worker.public_key = None;
        rt.worker.last_error = None;
        rt.worker.state = "stopped".into();
        Ok(build_status(&dir, &rt.worker))
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

    pub fn publish_relay_list_instance(&self, payload: InstanceRequest) -> crate::Result<PublishRelayListResponse> {
        validate_instance_id(&payload.instance_id)?;
        let dir = instance_dir(&self.inner.data_dir, &payload.instance_id);
        let private_key = get_or_create_identity(&dir, None)?;
        let config = read_json::<CoordinatorConfig>(&dir.join("cordn-config.json"))
            .unwrap_or_default();
        let relay_urls = if config.relay_urls.is_empty() {
            CoordinatorConfig::default().relay_urls
        } else {
            config.relay_urls
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| crate::Error::InvalidConfig(e.to_string()))?;
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

            let publish_future = async {
                let secret_key = SecretKey::from_hex(&private_key)
                    .map_err(|e| format!("invalid private key: {e}"))?;
                let keys = Keys::new(secret_key);
                let client = nostr_sdk::Client::new(keys);
                for url in &all_targets {
                    let _ = client.add_relay(url.as_str()).await;
                }
                client.connect().await;
                client
                    .wait_for_connection(std::time::Duration::from_secs(10))
                    .await;
                let connected = client.relays().await.values().any(|r| r.is_connected());
                if !connected {
                    client.disconnect().await;
                    return Err("could not connect to any relay (check network/relay URLs)".to_string());
                }

                let tags: Vec<Tag> = relay_urls
                    .iter()
                    .filter_map(|url| {
                        let trimmed = url.trim().trim_end_matches('/');
                        Tag::parse(["r", trimmed]).ok()
                    })
                    .collect();
                let builder = EventBuilder::new(Kind::Custom(10002), "").tags(tags);
                let result = client.send_event_builder_to(all_targets.clone(), builder).await;
                client.disconnect().await;
                let output = result.map_err(|e| format!("publish failed: {e}"))?;
                let failed_set: std::collections::HashSet<String> = output
                    .failed
                    .keys()
                    .map(|url| url.to_string().trim_end_matches('/').to_owned())
                    .collect();
                let sent = all_targets.iter().filter(|t| !failed_set.contains(t.as_str())).count();
                if sent == 0 {
                    return Err("publish failed: no relay accepted the event".to_string());
                }
                Ok::<_, String>((sent, failed_set.len(), all_targets))
            };

            match tokio::time::timeout(std::time::Duration::from_secs(30), publish_future).await {
                Ok(result) => result,
                Err(_) => Err("publish timed out after 30s (check network/relay connectivity)".to_string()),
            }
        }).map_err(|e: String| crate::Error::InvalidConfig(e))?;

        Ok(PublishRelayListResponse { sent, failed, targets })
    }

    pub fn restart_schedule_instance(&self, instance_id: &str, payload: serde_json::Value) -> crate::Result<serde_json::Value> {
        validate_instance_id(instance_id)?;
        let dir = instance_dir(&self.inner.data_dir, instance_id);
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
                write_atomic(&dir.join("cordn-restart-schedule.json"), &serde_json::to_vec(&stored)?)?;
                reschedule_timer(&self.inner, instance_id);
            }
            "get" | "permission" => {}
            other => {
                return Err(crate::Error::InvalidConfig(format!(
                    "Unknown schedule operation: {other}"
                )));
            }
        }

        let stored = read_schedule(&dir);
        let next_alarm = if stored.enabled {
            stored.times.iter().filter_map(|t| next_occurrence_ms(t)).min()
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
}

fn spawn_worker(inner: Arc<Inner>, instance_id: String, config: CoordinatorConfig, private_key_hex: String) {
    let (stop_tx, stop_rx) = oneshot::channel();
    let dir = instance_dir(&inner.data_dir, &instance_id);
    {
        let mut guard = inner.instances.lock().unwrap();
        if let Some(rt) = guard.get_mut(&instance_id) {
            rt.worker.stop_tx = Some(stop_tx);
        } else {
            return;
        }
    }

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(err) => {
                let mut guard = inner.instances.lock().unwrap();
                if let Some(rtst) = guard.get_mut(&instance_id) {
                    rtst.worker.state = "error".into();
                    rtst.worker.last_error = Some(err.to_string());
                    rtst.worker.stop_tx = None;
                    let st = build_status(&dir, &rtst.worker);
                    let _ = persist_status(&dir, &st);
                }
                return;
            }
        };

        let server_cfg = build_server_config(config, private_key_hex, &dir);
        let run_result: Result<(), String> = rt.block_on(async {
            let server = runtime::start(server_cfg)
                .await
                .map_err(|error| format!("{error:#}"))?;
            let pubkey = server.identity().public_key.clone();
            let health = server.health();
            let health_path = dir.join("cordn-health.json");

            if let Ok(bytes) = serde_json::to_vec(&health.snapshot()) {
                let _ = write_atomic(&health_path, &bytes);
            }

            {
                let mut guard = inner.instances.lock().unwrap();
                if let Some(rtst) = guard.get_mut(&instance_id) {
                    rtst.worker.state = "running".into();
                    rtst.worker.public_key = Some(pubkey);
                    rtst.worker.last_error = None;
                    let st = build_status(&dir, &rtst.worker);
                    let _ = persist_status(&dir, &st);
                }
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

        let mut guard = inner.instances.lock().unwrap();
        if let Some(rtst) = guard.get_mut(&instance_id) {
            rtst.worker.stop_tx = None;
            match run_result {
                Ok(()) => {
                    rtst.worker.state = "stopped".into();
                }
                Err(err) => {
                    rtst.worker.state = "error".into();
                    rtst.worker.last_error = Some(err);
                }
            }
            let st = build_status(&dir, &rtst.worker);
            let _ = persist_status(&dir, &st);
        }
    });
}

fn reschedule_timer(inner: &Arc<Inner>, instance_id: &str) {
    let dir = instance_dir(&inner.data_dir, instance_id);
    let cancel_rx = {
        let mut guard = inner.instances.lock().unwrap();
        let Some(rt) = guard.get_mut(instance_id) else { return; };
        if let Some(prev) = rt.worker.schedule_cancel.take() {
            let _ = prev.send(());
        }
        let stored = read_schedule(&dir);
        if !stored.enabled {
            return;
        }
        let (tx, rx) = oneshot::channel();
        rt.worker.schedule_cancel = Some(tx);
        rx
    };

    let weak_inner = Arc::downgrade(inner);
    let id = instance_id.to_owned();
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
        else {
            return;
        };
        rt.block_on(async move {
            let mut cancel_rx = cancel_rx;
            loop {
                let Some(strong) = weak_inner.upgrade() else { break; };
                let dir = instance_dir(&strong.data_dir, &id);
                let stored = read_schedule(&dir);
                if !stored.enabled {
                    break;
                }
                let Some(next_ms) = stored.times.iter().filter_map(|t| next_occurrence_ms(t)).min() else { break; };
                let now_ms = now_millis();
                let wait_ms = next_ms.saturating_sub(now_ms).max(1_000);
                drop(strong);

                tokio::select! {
                    _ = &mut cancel_rx => break,
                    _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => {
                        let Some(strong) = weak_inner.upgrade() else { break; };
                        perform_scheduled_restart(&strong, &id).await;
                    }
                }
            }
        });
    });
}

async fn perform_scheduled_restart(inner: &Arc<Inner>, instance_id: &str) {
    let dir = instance_dir(&inner.data_dir, instance_id);
    let should_restart = {
        let mut guard = inner.instances.lock().unwrap();
        let Some(rt) = guard.get_mut(instance_id) else { return; };
        if rt.worker.manually_stopped || rt.worker.state != "running" {
            false
        } else if let Some(tx) = rt.worker.stop_tx.take() {
            rt.worker.state = "stopping".into();
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
            let guard = inner.instances.lock().unwrap();
            let Some(rt) = guard.get(instance_id) else { return; };
            if rt.worker.manually_stopped {
                return;
            }
            rt.worker.state == "stopped" || rt.worker.state == "error"
        };
        if stopped {
            break;
        }
    }

    let config = read_json::<CoordinatorConfig>(&dir.join("cordn-config.json")).unwrap_or_default();
    let Ok(private_key) = get_or_create_identity(&dir, None) else { return; };

    {
        let mut guard = inner.instances.lock().unwrap();
        let Some(rt) = guard.get_mut(instance_id) else { return; };
        if rt.worker.manually_stopped || matches!(rt.worker.state.as_str(), "starting" | "running") {
            return;
        }
        rt.worker.state = "starting".into();
        rt.worker.last_error = None;
    }
    spawn_worker(inner.clone(), instance_id.to_owned(), config, private_key);
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
        health: if worker.state == "running" { health } else { None },
        state: worker.state.clone(),
        public_key: worker.public_key.clone(),
        last_error: worker.last_error.clone(),
        recovery_attempt: None,
        next_retry_at: None,
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
    let critical = h.get("criticalTaskFailed").and_then(|v| v.as_bool()).unwrap_or(false);
    let loop_age = h.get("loopAgeMs").and_then(|v| v.as_i64()).unwrap_or(0);
    if critical || loop_age > 180_000 {
        return "Degraded — transport not progressing".into();
    }
    let connected = h.get("connectedRelays").and_then(|v| v.as_i64()).unwrap_or(0);
    let total = h.get("totalRelays").and_then(|v| v.as_i64()).unwrap_or(0);
    if connected == 0 {
        return if total > 0 { format!("Waiting for relay connection (0/{total})") } else { "Waiting for relay connection".into() };
    }
    let rejected = h.get("rejectedSubscriptions").and_then(|v| v.as_i64()).unwrap_or(0);
    if rejected > 0 {
        return if total > 0 {
            format!("Degraded — subscription rejected ({rejected}, {connected}/{total} connected)")
        } else {
            format!("Degraded — subscription rejected ({rejected})")
        };
    }
    if total > 0 && connected < total {
        format!("Partially connected ({connected}/{total}) — delivery unverified")
    } else if total > 0 {
        format!("Relay connected ({connected}/{total}) — delivery unverified")
    } else {
        "Relay connected — delivery unverified".into()
    }
}

fn validate_schedule(times: &[String]) -> crate::Result<()> {
    if !(1..=2).contains(&times.len()) {
        return Err(crate::Error::InvalidConfig("Choose one or two restart times".into()));
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
            return Err(crate::Error::InvalidConfig("Restart times must be different".into()));
        }
        let gap = (parsed_mins[0] - parsed_mins[1]).abs();
        if gap.min(1440 - gap) < 15 {
            return Err(crate::Error::InvalidConfig("Keep restarts at least 15 minutes apart".into()));
        }
    }
    Ok(())
}

fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    if s.len() != 5 || &s[2..3] != ":" { return None; }
    let hh: u32 = s[0..2].parse().ok()?;
    let mm: u32 = s[3..5].parse().ok()?;
    if hh < 24 && mm < 60 { Some((hh, mm)) } else { None }
}

fn next_occurrence_ms(time_str: &str) -> Option<u64> {
    let (hh, mm) = parse_hhmm(time_str)?;
    let target_time = NaiveTime::from_hms_opt(hh, mm, 0)?;
    let now = Local::now();
    let today_naive = now.date_naive().and_time(target_time);
    let candidate = Local.from_local_datetime(&today_naive).earliest().or_else(|| Local.from_local_datetime(&today_naive).latest())?;
    let next = if candidate > now {
        candidate
    } else {
        let tomorrow_naive = now.date_naive().checked_add_days(Days::new(1))?.and_time(target_time);
        Local.from_local_datetime(&tomorrow_naive).earliest().or_else(|| Local.from_local_datetime(&tomorrow_naive).latest())?
    };
    u64::try_from(next.timestamp_millis()).ok()
}

fn read_schedule(data_dir: &Path) -> StoredSchedule {
    read_json::<StoredSchedule>(&data_dir.join("cordn-restart-schedule.json")).unwrap_or_default()
}

fn count_table(conn: &Connection, table: &str) -> Option<u64> {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get::<_, i64>(0))
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
    let Ok(mut file) = File::open(path) else { return String::new(); };
    let Ok(meta) = file.metadata() else { return String::new(); };
    let start = meta.len().saturating_sub(MAX_LOG_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() { return String::new(); }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() { return String::new(); }
    let text = String::from_utf8_lossy(&buf).into_owned();
    if start == 0 { text } else { text.split_once('\n').map(|(_, rest)| rest.to_owned()).unwrap_or(text) }
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
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
