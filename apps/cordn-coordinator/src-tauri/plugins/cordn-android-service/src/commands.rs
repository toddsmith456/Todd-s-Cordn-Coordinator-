use tauri::{command, AppHandle, Runtime};

use crate::{
    ConfigResponse, CordnAndroidServiceExt, CreateInstanceRequest, DeleteInstanceRequest,
    EncodePubkeyRequest, InstanceRequest, InstanceStartRequest, InstancesResponse, PubkeyDisplay,
    PublishRelayListResponse, RenameInstanceRequest, ServiceDiagnostics, ServiceStatus, StartRequest,
};

#[command]
pub(crate) fn start<R: Runtime>(app: AppHandle<R>, payload: StartRequest) -> crate::Result<ServiceStatus> {
    app.cordn_android_service().start(payload)
}

#[command]
pub(crate) fn stop<R: Runtime>(app: AppHandle<R>) -> crate::Result<ServiceStatus> {
    app.cordn_android_service().stop()
}

#[command]
pub(crate) fn status<R: Runtime>(app: AppHandle<R>) -> crate::Result<ServiceStatus> {
    app.cordn_android_service().status()
}

#[command]
pub(crate) fn config<R: Runtime>(app: AppHandle<R>) -> crate::Result<ConfigResponse> {
    app.cordn_android_service().config()
}

#[command]
pub(crate) fn diagnostics<R: Runtime>(app: AppHandle<R>) -> crate::Result<ServiceDiagnostics> {
    app.cordn_android_service().diagnostics()
}

#[command]
pub(crate) fn encode_pubkey<R: Runtime>(
    app: AppHandle<R>,
    payload: EncodePubkeyRequest,
) -> crate::Result<PubkeyDisplay> {
    app.cordn_android_service().encode_pubkey(payload)
}

#[command]
pub(crate) fn flush_data<R: Runtime>(app: AppHandle<R>) -> crate::Result<ServiceStatus> {
    app.cordn_android_service().flush_data()
}

#[command]
pub(crate) fn reset_identity<R: Runtime>(app: AppHandle<R>) -> crate::Result<ServiceStatus> {
    app.cordn_android_service().reset_identity()
}

#[command]
pub(crate) fn request_battery_exemption<R: Runtime>(app: AppHandle<R>) -> crate::Result<()> {
    app.cordn_android_service().request_battery_exemption()
}

#[command]
pub(crate) fn request_notification_permission<R: Runtime>(app: AppHandle<R>) -> crate::Result<()> {
    app.cordn_android_service().request_notification_permission()
}

#[command]
pub(crate) fn restart_schedule<R: Runtime>(
    app: AppHandle<R>,
    payload: serde_json::Value,
) -> crate::Result<serde_json::Value> {
    app.cordn_android_service().restart_schedule(payload)
}

#[command]
pub(crate) async fn publish_relay_list<R: Runtime>(
    app: AppHandle<R>,
) -> crate::Result<PublishRelayListResponse> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || app.cordn_android_service().publish_relay_list())
        .await
        .map_err(|e| crate::Error::InvalidConfig(format!("publish task failed: {e}")))?
}

// Instance-aware commands
#[command]
pub(crate) fn list_instances<R: Runtime>(app: AppHandle<R>) -> crate::Result<InstancesResponse> {
    app.cordn_android_service().list_instances()
}

#[command]
pub(crate) fn create_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: CreateInstanceRequest,
) -> crate::Result<crate::InstanceInfo> {
    app.cordn_android_service().create_instance(payload)
}

#[command]
pub(crate) fn rename_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: RenameInstanceRequest,
) -> crate::Result<crate::InstanceInfo> {
    app.cordn_android_service().rename_instance(payload)
}

#[command]
pub(crate) fn delete_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: DeleteInstanceRequest,
) -> crate::Result<InstancesResponse> {
    app.cordn_android_service().delete_instance(payload)
}

#[command]
pub(crate) fn start_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: InstanceStartRequest,
) -> crate::Result<ServiceStatus> {
    app.cordn_android_service().start_instance(payload)
}

#[command]
pub(crate) fn stop_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: InstanceRequest,
) -> crate::Result<ServiceStatus> {
    app.cordn_android_service().stop_instance(payload)
}

#[command]
pub(crate) fn status_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: InstanceRequest,
) -> crate::Result<ServiceStatus> {
    app.cordn_android_service().status_instance(payload)
}

#[command]
pub(crate) fn config_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: InstanceRequest,
) -> crate::Result<ConfigResponse> {
    app.cordn_android_service().config_instance(payload)
}

#[command]
pub(crate) fn diagnostics_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: InstanceRequest,
) -> crate::Result<ServiceDiagnostics> {
    app.cordn_android_service().diagnostics_instance(payload)
}

#[command]
pub(crate) fn flush_data_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: InstanceRequest,
) -> crate::Result<ServiceStatus> {
    app.cordn_android_service().flush_data_instance(payload)
}

#[command]
pub(crate) fn reset_identity_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: InstanceRequest,
) -> crate::Result<ServiceStatus> {
    app.cordn_android_service().reset_identity_instance(payload)
}

#[command]
pub(crate) async fn publish_relay_list_instance<R: Runtime>(
    app: AppHandle<R>,
    payload: InstanceRequest,
) -> crate::Result<PublishRelayListResponse> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.cordn_android_service().publish_relay_list_instance(payload)
    })
    .await
    .map_err(|e| crate::Error::InvalidConfig(format!("publish task failed: {e}")))?
}

#[command]
pub(crate) fn restart_schedule_instance<R: Runtime>(
    app: AppHandle<R>,
    instance_id: String,
    payload: serde_json::Value,
) -> crate::Result<serde_json::Value> {
    app.cordn_android_service().restart_schedule_instance(&instance_id, payload)
}
