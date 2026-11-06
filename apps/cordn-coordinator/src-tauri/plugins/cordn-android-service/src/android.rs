use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, Once,
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

static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static NETWORK_CHANGED: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP: Mutex<Option<oneshot::Sender<()>>> = Mutex::new(None);
static LOGGING: Once = Once::new();

pub(crate) fn validate_private_key(private_key_hex: &str) -> Result<(), String> {
    runtime::validate_private_key(private_key_hex)
        .map_err(|_| "private key is not a valid Nostr secret key".into())
}

pub(crate) fn ensure_linked() {
    let _ = Java_org_cordn_coordinator_service_CordnNative_runCoordinator as *const ();
    let _ = Java_org_cordn_coordinator_service_CordnNative_stopCoordinator as *const ();
}

pub(crate) fn encode_pubkey(payload: EncodePubkeyRequest) -> crate::Result<PubkeyDisplay> {
    let (npub, nprofile) =
        cordn_server::runtime::pubkey_display(&payload.public_key, &payload.relay_urls)
            .map_err(|error| crate::Error::InvalidConfig(format!("{error:#}")))?;
    Ok(PubkeyDisplay { npub, nprofile })
}

#[no_mangle]
pub extern "system" fn Java_org_cordn_coordinator_service_CordnNative_runCoordinator(
    mut env: JNIEnv,
    _object: JObject,
    config_json: JString,
    private_key_hex: JString,
    data_dir: JString,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let config_json: String = env
            .get_string(&config_json)
            .map_err(|error| error.to_string())?
            .into();
        let private_key_hex: String = env
            .get_string(&private_key_hex)
            .map_err(|error| error.to_string())?
            .into();
        let data_dir: String = env
            .get_string(&data_dir)
            .map_err(|error| error.to_string())?
            .into();
        let data_dir = Path::new(&data_dir);
        init_logging(data_dir);
        run_coordinator(&config_json, private_key_hex, data_dir)
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
    _env: JNIEnv,
    _object: JObject,
) {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
    if let Some(stop) = STOP.lock().unwrap().take() {
        let _ = stop.send(());
    }
}

#[no_mangle]
pub extern "system" fn Java_org_cordn_coordinator_service_CordnNative_networkChanged(
    _env: JNIEnv,
    _object: JObject,
) {
    NETWORK_CHANGED.store(true, Ordering::Relaxed);
}

#[no_mangle]
pub extern "system" fn Java_org_cordn_coordinator_service_CordnNative_prepareStart(
    _env: JNIEnv,
    _object: JObject,
) {
    STOP_REQUESTED.store(false, Ordering::SeqCst);
}

fn init_logging(data_dir: &Path) {
    // ponytail: one current-process log file; add rotation if field logs become noisy.
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
    config_json: &str,
    private_key_hex: String,
    data_dir: &Path,
) -> Result<(), String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("cordn coordinator is already running".into());
    }
    struct ResetRunning;
    impl Drop for ResetRunning {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::SeqCst);
            STOP.lock().unwrap().take();
        }
    }
    let _reset = ResetRunning;

    let config: CoordinatorConfig =
        serde_json::from_str(config_json).map_err(|error| error.to_string())?;
    config.validate().map_err(|error| error.to_string())?;
    let server_config = server_config(config, private_key_hex, data_dir);
    let status_path = data_dir.join("cordn-status.json");
    let (stop_tx, stop_rx) = oneshot::channel();
    *STOP.lock().unwrap() = Some(stop_tx);
    // A Stop arriving before the native thread registered its channel must win too.
    if STOP_REQUESTED.load(Ordering::SeqCst) {
        if let Some(stop) = STOP.lock().unwrap().take() {
            let _ = stop.send(());
        }
    }

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
                    if NETWORK_CHANGED.swap(false, Ordering::Relaxed) { health.network_changed(); }
                    // Atomic, fixed-size metadata snapshot, overwritten rather than accumulated.
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
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn write_status(path: &Path, status: &ServiceStatus) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "status path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    // The coordinator identity outlives run states; keep the last known
    // public key on stop/error writes so the UI can still display it.
    let mut status = status.clone();
    if status.public_key.is_none() {
        if let Ok(previous) = std::fs::read(path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<ServiceStatus>(&bytes).map_err(|e| e.to_string())
            })
        {
            status.public_key = previous.public_key;
        }
    }
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(
        &temporary,
        serde_json::to_vec(&status).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    std::fs::rename(temporary, path).map_err(|error| error.to_string())
}
