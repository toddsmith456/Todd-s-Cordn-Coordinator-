use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, Once, OnceLock,
    },
};

use cordn_core::Synchronous;
use cordn_server::{
    config::{ServerConfig, StorageBackend, StorageConfig},
    runtime,
};
use jni::{
    objects::{JObject, JString},
    sys::jstring,
    JNIEnv,
};
use tokio::sync::oneshot;

use crate::{CoordinatorConfig, EncodePubkeyRequest, PubkeyDisplay, ServiceStatus};

struct InstanceRuntime {
    stop_requested: AtomicBool,
    network_changed: AtomicBool,
    running: AtomicBool,
    stop_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl Default for InstanceRuntime {
    fn default() -> Self {
        Self {
            stop_requested: AtomicBool::new(false),
            network_changed: AtomicBool::new(false),
            running: AtomicBool::new(false),
            stop_tx: Mutex::new(None),
        }
    }
}

static INSTANCES: OnceLock<Mutex<HashMap<String, InstanceRuntime>>> = OnceLock::new();
static LOGGING: Once = Once::new();

fn instances() -> &'static Mutex<HashMap<String, InstanceRuntime>> {
    INSTANCES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_instance<T>(instance_id: &str, f: impl FnOnce(&InstanceRuntime) -> T) -> T {
    let mut guard = instances().lock().unwrap();
    let rt = guard.entry(instance_id.to_owned()).or_insert_with(InstanceRuntime::default);
    f(rt)
}

pub(crate) fn validate_private_key(private_key_hex: &str) -> Result<(), String> {
    runtime::validate_private_key(private_key_hex)
        .map_err(|_| "private key is not a valid Nostr secret key".into())
}

pub(crate) fn ensure_linked() {
    let _ = Java_org_cordn_coordinator_service_CordnNative_runCoordinator as *const ();
    let _ = Java_org_cordn_coordinator_service_CordnNative_stopCoordinator as *const ();
    let _ = Java_org_cordn_coordinator_service_CordnNative_networkChanged as *const ();
    let _ = Java_org_cordn_coordinator_service_CordnNative_prepareStart as *const ();
}

pub(crate) fn encode_pubkey(payload: EncodePubkeyRequest) -> crate::Result<PubkeyDisplay> {
    let (npub, nprofile) =
        cordn_server::runtime::pubkey_display(&payload.public_key, &payload.relay_urls)
            .map_err(|error| crate::Error::InvalidConfig(format!("{error:#}")))?;
    Ok(PubkeyDisplay { npub, nprofile })
}

fn jstring_to_string(env: &mut JNIEnv, s: &JString) -> Result<String, String> {
    env.get_string(s).map_err(|e| e.to_string()).map(Into::into)
}

#[no_mangle]
pub extern "system" fn Java_org_cordn_coordinator_service_CordnNative_runCoordinator(
    mut env: JNIEnv,
    _object: JObject,
    instance_id: JString,
    config_json: JString,
    private_key_hex: JString,
    data_dir: JString,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let instance_id = jstring_to_string(&mut env, &instance_id)?;
        let config_json = jstring_to_string(&mut env, &config_json)?;
        let private_key_hex = jstring_to_string(&mut env, &private_key_hex)?;
        let data_dir = jstring_to_string(&mut env, &data_dir)?;
        let data_dir = Path::new(&data_dir);
        init_logging(data_dir);
        run_coordinator(&instance_id, &config_json, private_key_hex, data_dir)
    }))
    .unwrap_or_else(|_| Err("cordn coordinator panicked".into()));

    match result {
        Ok(()) => std::ptr::null_mut(),
        Err(error) => env
            .new_string(error)
            .map(JString::into_raw)
            .unwrap_or(std::ptr::null_mut()),
    }
}

#[no_mangle]
pub extern "system" fn Java_org_cordn_coordinator_service_CordnNative_stopCoordinator(
    mut env: JNIEnv,
    _object: JObject,
    instance_id: JString,
) {
    let Ok(instance_id) = jstring_to_string(&mut env, &instance_id) else { return; };
    with_instance(&instance_id, |rt| {
        rt.stop_requested.store(true, Ordering::SeqCst);
        if let Some(stop) = rt.stop_tx.lock().unwrap().take() {
            let _ = stop.send(());
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_org_cordn_coordinator_service_CordnNative_networkChanged(
    mut env: JNIEnv,
    _object: JObject,
    instance_id: JString,
) {
    let Ok(instance_id) = jstring_to_string(&mut env, &instance_id) else { return; };
    with_instance(&instance_id, |rt| {
        rt.network_changed.store(true, Ordering::Relaxed);
    });
}

#[no_mangle]
pub extern "system" fn Java_org_cordn_coordinator_service_CordnNative_prepareStart(
    mut env: JNIEnv,
    _object: JObject,
    instance_id: JString,
) {
    let Ok(instance_id) = jstring_to_string(&mut env, &instance_id) else { return; };
    with_instance(&instance_id, |rt| {
        rt.stop_requested.store(false, Ordering::SeqCst);
    });
}

fn init_logging(data_dir: &Path) {
    LOGGING.call_once(|| {
        if let Ok(file) = std::fs::File::create(data_dir.join("cordn.log")) {
            let _ = tracing_subscriber::fmt()
                .with_ansi(false)
                .with_writer(Mutex::new(file))
                .try_init();
        }
    });
}

fn run_coordinator(
    instance_id: &str,
    config_json: &str,
    private_key_hex: String,
    data_dir: &Path,
) -> Result<(), String> {
    let already_running = with_instance(instance_id, |rt| rt.running.swap(true, Ordering::SeqCst));
    if already_running {
        return Err("cordn coordinator is already running".into());
    }
    struct ResetRunning<'a>(&'a str);
    impl Drop for ResetRunning<'_> {
        fn drop(&mut self) {
            with_instance(self.0, |rt| {
                rt.running.store(false, Ordering::SeqCst);
                rt.stop_tx.lock().unwrap().take();
            });
        }
    }
    let _reset = ResetRunning(instance_id);

    let config: CoordinatorConfig =
        serde_json::from_str(config_json).map_err(|error| error.to_string())?;
    config.validate().map_err(|error| error.to_string())?;
    let server_config = server_config(config, private_key_hex, data_dir);
    let status_path = data_dir.join("cordn-status.json");
    let (stop_tx, stop_rx) = oneshot::channel();
    with_instance(instance_id, |rt| {
        *rt.stop_tx.lock().unwrap() = Some(stop_tx);
        if rt.stop_requested.load(Ordering::SeqCst) {
            if let Some(stop) = rt.stop_tx.lock().unwrap().take() {
                let _ = stop.send(());
            }
        }
    });

    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let result = tokio_runtime.block_on(async {
        let server = runtime::start(server_config)
            .await
            .map_err(|error| format!("{error:#}"))?;
        let identity = server.identity().clone();
        let health = server.health();
        let health_path = data_dir.join("cordn-health.json");
        write_status(
            &status_path,
            &ServiceStatus {
                state: "running".into(),
                public_key: Some(identity.public_key),
                last_error: None,
                battery_exempt: false,
                ..ServiceStatus::default()
            },
        )?;
        let running = server.run_until(async { let _ = stop_rx.await; });
        tokio::pin!(running);
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(15));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                result = &mut running => break result.map_err(|_| "Coordinator transport exited; local recovery may retry".to_string()),
                _ = timer.tick() => {
                    let network_changed = with_instance(instance_id, |rt| rt.network_changed.swap(false, Ordering::Relaxed));
                    if network_changed { health.network_changed(); }
                    if let Ok(bytes) = serde_json::to_vec(&health.snapshot()) {
                        let temporary = health_path.with_extension("tmp");
                        if std::fs::write(&temporary, bytes).is_ok() {
                            let _ = std::fs::rename(temporary, &health_path);
                        }
                    }
                }
            }
        }
    });

    match result {
        Ok(()) => write_status(&status_path, &ServiceStatus::default()),
        Err(error) => {
            let _ = write_status(
                &status_path,
                &ServiceStatus {
                    state: "error".into(),
                    public_key: None,
                    last_error: Some(error.clone()),
                    battery_exempt: false,
                    ..ServiceStatus::default()
                },
            );
            Err(error)
        }
    }
}

fn server_config(
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
    value.map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())
}

pub(crate) struct PublishRelayListRequest {
    pub private_key_hex: String,
    pub relay_urls: Vec<String>,
}

pub(crate) struct PublishRelayListResult {
    pub sent: usize,
    pub failed: usize,
    pub targets: Vec<String>,
}

pub(crate) fn publish_relay_list(req: PublishRelayListRequest) -> crate::Result<PublishRelayListResult> {
    use nostr_sdk::prelude::*;
    if req.relay_urls.is_empty() {
        return Err(crate::Error::InvalidConfig("at least one relay URL is required".into()));
    }
    let fallbacks: Vec<String> = vec![
        "wss://purplepag.es".into(), "wss://relay.damus.io".into(),
        "wss://relay.primal.net".into(), "wss://nos.lol".into(), "wss://relay.nostr.net".into(),
    ];
    let mut all_targets: Vec<String> = req.relay_urls.iter().chain(fallbacks.iter())
        .map(|u| u.trim().trim_end_matches('/').to_owned()).filter(|u| !u.is_empty()).collect();
    all_targets.sort(); all_targets.dedup();

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()
        .map_err(|e| crate::Error::InvalidConfig(format!("tokio runtime: {e}")))?;
    rt.block_on(async {
        let publish_future = async {
            let secret_key = SecretKey::from_hex(&req.private_key_hex)
                .map_err(|e| crate::Error::InvalidConfig(format!("invalid private key: {e}")))?;
            let keys = Keys::new(secret_key);
            let client = nostr_sdk::Client::new(keys);
            for url in &all_targets { let _ = client.add_relay(url.as_str()).await; }
            client.connect().await;
            client.wait_for_connection(std::time::Duration::from_secs(10)).await;
            let connected = client.relays().await.values().any(|r| r.is_connected());
            if !connected {
                client.disconnect().await;
                return Err(crate::Error::InvalidConfig("could not connect to any relay (check network/relay URLs)".into()));
            }
            let tags: Vec<Tag> = req.relay_urls.iter().filter_map(|url| {
                let trimmed = url.trim().trim_end_matches('/'); Tag::parse(["r", trimmed]).ok()
            }).collect();
            let builder = EventBuilder::new(Kind::Custom(10002), "").tags(tags);
            let result = client.send_event_builder_to(all_targets.clone(), builder).await;
            client.disconnect().await;
            match result {
                Ok(output) => {
                    let failed_urls: std::collections::HashSet<String> = output.failed.keys()
                        .map(|url| url.to_string().trim_end_matches('/').to_owned()).collect();
                    let sent = all_targets.iter().filter(|t| !failed_urls.contains(t.as_str())).count();
                    let failed = failed_urls.len();
                    if sent == 0 { return Err(crate::Error::InvalidConfig("publish failed: no relay accepted the event".into())); }
                    Ok(PublishRelayListResult { sent, failed, targets: all_targets })
                }
                Err(error) => Err(crate::Error::InvalidConfig(format!("publish failed: {error}"))),
            }
        };
        match tokio::time::timeout(std::time::Duration::from_secs(30), publish_future).await {
            Ok(result) => result,
            Err(_) => Err(crate::Error::InvalidConfig("publish timed out after 30s (check network/relay connectivity)".into())),
        }
    })
}

fn write_status(path: &Path, status: &ServiceStatus) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "status path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut status = status.clone();
    if status.public_key.is_none() {
        if let Ok(previous) = std::fs::read(path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| serde_json::from_slice::<ServiceStatus>(&bytes).map_err(|e| e.to_string()))
        {
            status.public_key = previous.public_key;
        }
    }
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&temporary, serde_json::to_vec(&status).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    std::fs::rename(temporary, path).map_err(|error| error.to_string())
}
