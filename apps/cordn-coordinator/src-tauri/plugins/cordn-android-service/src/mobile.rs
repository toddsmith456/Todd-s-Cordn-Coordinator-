use serde::{de::DeserializeOwned, Serialize};
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{
    ConfigResponse, CreateInstanceRequest, DeleteInstanceRequest, EncodePubkeyRequest,
    InstanceInfo, InstanceRequest, InstanceStartRequest, InstancesResponse, PubkeyDisplay,
    PublishRelayListResponse, RenameInstanceRequest, ServiceDiagnostics, ServiceStatus, StartRequest,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeStartRequest {
    config_json: String,
    private_key_hex: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeInstanceStartRequest {
    instance_id: String,
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
    #[cfg(not(target_os = "android"))]
    let handle = api.register_android_plugin("org.cordn.coordinator.service", "CordnServicePlugin")?;
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

    pub fn start_instance(&self, payload: InstanceStartRequest) -> crate::Result<ServiceStatus> {
        payload.config.validate()?;
        #[cfg(target_os = "android")]
        if let Some(private_key_hex) = &payload.private_key_hex {
            crate::android::validate_private_key(private_key_hex)
                .map_err(crate::Error::InvalidConfig)?;
        }
        self.0
            .run_mobile_plugin(
                "startInstance",
                NativeInstanceStartRequest {
                    instance_id: payload.instance_id,
                    config_json: serde_json::to_string(&payload.config)?,
                    private_key_hex: payload.private_key_hex,
                },
            )
            .map_err(Into::into)
    }

    pub fn restart_schedule(&self, payload: serde_json::Value) -> crate::Result<serde_json::Value> {
        self.0.run_mobile_plugin("restartSchedule", payload).map_err(Into::into)
    }

    pub fn restart_schedule_instance(&self, instance_id: &str, payload: serde_json::Value) -> crate::Result<serde_json::Value> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Req { instance_id: String, payload: serde_json::Value }
        self.0.run_mobile_plugin("restartScheduleInstance", Req { instance_id: instance_id.to_owned(), payload }).map_err(Into::into)
    }

    pub fn stop(&self) -> crate::Result<ServiceStatus> {
        self.0.run_mobile_plugin("stop", ()).map_err(Into::into)
    }
    pub fn stop_instance(&self, payload: InstanceRequest) -> crate::Result<ServiceStatus> {
        self.0.run_mobile_plugin("stopInstance", payload).map_err(Into::into)
    }
    pub fn status(&self) -> crate::Result<ServiceStatus> {
        self.0.run_mobile_plugin("status", ()).map_err(Into::into)
    }
    pub fn status_instance(&self, payload: InstanceRequest) -> crate::Result<ServiceStatus> {
        self.0.run_mobile_plugin("statusInstance", payload).map_err(Into::into)
    }
    pub fn config(&self) -> crate::Result<ConfigResponse> {
        self.0.run_mobile_plugin("config", ()).map_err(Into::into)
    }
    pub fn config_instance(&self, payload: InstanceRequest) -> crate::Result<ConfigResponse> {
        self.0.run_mobile_plugin("configInstance", payload).map_err(Into::into)
    }
    pub fn diagnostics(&self) -> crate::Result<ServiceDiagnostics> {
        self.0.run_mobile_plugin("diagnostics", ()).map_err(Into::into)
    }
    pub fn diagnostics_instance(&self, payload: InstanceRequest) -> crate::Result<ServiceDiagnostics> {
        self.0.run_mobile_plugin("diagnosticsInstance", payload).map_err(Into::into)
    }
    pub fn flush_data(&self) -> crate::Result<ServiceStatus> {
        self.0.run_mobile_plugin("flushData", ()).map_err(Into::into)
    }
    pub fn flush_data_instance(&self, payload: InstanceRequest) -> crate::Result<ServiceStatus> {
        self.0.run_mobile_plugin("flushDataInstance", payload).map_err(Into::into)
    }
    pub fn reset_identity(&self) -> crate::Result<ServiceStatus> {
        self.0.run_mobile_plugin("resetIdentity", ()).map_err(Into::into)
    }
    pub fn reset_identity_instance(&self, payload: InstanceRequest) -> crate::Result<ServiceStatus> {
        self.0.run_mobile_plugin("resetIdentityInstance", payload).map_err(Into::into)
    }
    pub fn list_instances(&self) -> crate::Result<InstancesResponse> {
        self.0.run_mobile_plugin("listInstances", ()).map_err(Into::into)
    }
    pub fn create_instance(&self, payload: CreateInstanceRequest) -> crate::Result<InstanceInfo> {
        self.0.run_mobile_plugin("createInstance", payload).map_err(Into::into)
    }
    pub fn rename_instance(&self, payload: RenameInstanceRequest) -> crate::Result<InstanceInfo> {
        self.0.run_mobile_plugin("renameInstance", payload).map_err(Into::into)
    }
    pub fn delete_instance(&self, payload: DeleteInstanceRequest) -> crate::Result<InstancesResponse> {
        self.0.run_mobile_plugin("deleteInstance", payload).map_err(Into::into)
    }

    #[cfg(target_os = "android")]
    pub fn encode_pubkey(&self, payload: EncodePubkeyRequest) -> crate::Result<PubkeyDisplay> {
        crate::android::encode_pubkey(payload)
    }
    #[cfg(not(target_os = "android"))]
    pub fn encode_pubkey(&self, payload: EncodePubkeyRequest) -> crate::Result<PubkeyDisplay> {
        self.0.run_mobile_plugin("encodePubkey", payload).map_err(Into::into)
    }

    pub fn request_battery_exemption(&self) -> crate::Result<()> {
        self.0.run_mobile_plugin::<serde_json::Value>("requestBatteryExemption", ()).map(|_| ()).map_err(Into::into)
    }
    pub fn request_notification_permission(&self) -> crate::Result<()> {
        self.0
            .run_mobile_plugin::<serde_json::Value>(
                "requestPermissions",
                PermissionRequest { permissions: ["postNotifications"] },
            )
            .map(|_| ())
            .map_err(Into::into)
    }

    pub fn publish_relay_list(&self) -> crate::Result<PublishRelayListResponse> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct KotlinResponse { private_key_hex: String, relay_urls: Vec<String> }
        let response: KotlinResponse = self.0.run_mobile_plugin("publishRelayList", serde_json::json!({}))?;
        #[cfg(target_os = "android")]
        let result = crate::android::publish_relay_list(crate::android::PublishRelayListRequest {
            private_key_hex: response.private_key_hex,
            relay_urls: response.relay_urls,
        })?;
        #[cfg(not(target_os = "android"))]
        let result = PublishRelayListResponse { sent: 0, failed: 0, targets: vec![] };
        Ok(PublishRelayListResponse { sent: result.sent, failed: result.failed, targets: result.targets })
    }

    pub fn publish_relay_list_instance(&self, payload: InstanceRequest) -> crate::Result<PublishRelayListResponse> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct KotlinResponse { private_key_hex: String, relay_urls: Vec<String> }
        let response: KotlinResponse = self.0.run_mobile_plugin("publishRelayListInstance", payload)?;
        #[cfg(target_os = "android")]
        let result = crate::android::publish_relay_list(crate::android::PublishRelayListRequest {
            private_key_hex: response.private_key_hex,
            relay_urls: response.relay_urls,
        })?;
        #[cfg(not(target_os = "android"))]
        let result = PublishRelayListResponse { sent: 0, failed: 0, targets: vec![] };
        Ok(PublishRelayListResponse { sent: result.sent, failed: result.failed, targets: result.targets })
    }
}
