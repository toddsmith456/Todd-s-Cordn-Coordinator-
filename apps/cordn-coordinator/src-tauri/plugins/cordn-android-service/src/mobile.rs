use serde::{de::DeserializeOwned, Serialize};
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{
    ConfigResponse, EncodePubkeyRequest, PubkeyDisplay, ServiceDiagnostics, ServiceStatus,
    StartRequest,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeStartRequest {
    config_json: String,
    private_key_hex: Option<String>,
}

#[derive(Serialize)]
struct PermissionRequest {
    permissions: [&'static str; 1],
}

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<CordnAndroidService<R>> {
    #[cfg(target_os = "android")]
    let handle =
        api.register_android_plugin("org.cordn.coordinator.service", "CordnServicePlugin")?;
    Ok(CordnAndroidService(handle))
}

pub struct CordnAndroidService<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> CordnAndroidService<R> {
    pub fn start(&self, payload: StartRequest) -> crate::Result<ServiceStatus> {
        payload.config.validate()?;
        #[cfg(target_os = "android")]
        if let Some(private_key_hex) = &payload.private_key_hex {
            crate::android::validate_private_key(private_key_hex)
                .map_err(crate::Error::InvalidConfig)?;
        }
        self.0
            .run_mobile_plugin(
                "start",
                NativeStartRequest {
                    config_json: serde_json::to_string(&payload.config)?,
                    private_key_hex: payload.private_key_hex,
                },
            )
            .map_err(Into::into)
    }

    pub fn stop(&self) -> crate::Result<ServiceStatus> {
        self.0.run_mobile_plugin("stop", ()).map_err(Into::into)
    }

    pub fn status(&self) -> crate::Result<ServiceStatus> {
        self.0.run_mobile_plugin("status", ()).map_err(Into::into)
    }

    pub fn config(&self) -> crate::Result<ConfigResponse> {
        self.0.run_mobile_plugin("config", ()).map_err(Into::into)
    }

    pub fn diagnostics(&self) -> crate::Result<ServiceDiagnostics> {
        self.0
            .run_mobile_plugin("diagnostics", ())
            .map_err(Into::into)
    }

    pub fn flush_data(&self) -> crate::Result<ServiceStatus> {
        self.0
            .run_mobile_plugin("flushData", ())
            .map_err(Into::into)
    }

    pub fn reset_identity(&self) -> crate::Result<ServiceStatus> {
        self.0
            .run_mobile_plugin("resetIdentity", ())
            .map_err(Into::into)
    }

    // Pure bech32/TLV encoding on the Rust side; no Android API involved.
    #[cfg(target_os = "android")]
    pub fn encode_pubkey(&self, payload: EncodePubkeyRequest) -> crate::Result<PubkeyDisplay> {
        crate::android::encode_pubkey(payload)
    }

    pub fn request_battery_exemption(&self) -> crate::Result<()> {
        // Kotlin resolves with `{}`; deserializing into `()` would fail with
        // "invalid type: map, expected unit".
        self.0
            .run_mobile_plugin::<serde_json::Value>("requestBatteryExemption", ())
            .map(|_| ())
            .map_err(Into::into)
    }

    pub fn request_notification_permission(&self) -> crate::Result<()> {
        self.0
            .run_mobile_plugin::<serde_json::Value>(
                "requestPermissions",
                PermissionRequest {
                    permissions: ["postNotifications"],
                },
            )
            .map(|_| ())
            .map_err(Into::into)
    }
}
