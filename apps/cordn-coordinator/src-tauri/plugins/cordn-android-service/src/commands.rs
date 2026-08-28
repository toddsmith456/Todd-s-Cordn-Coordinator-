use tauri::{command, AppHandle, Runtime};

use crate::{
    ConfigResponse, CordnAndroidServiceExt, EncodePubkeyRequest, PubkeyDisplay, Result,
    ServiceDiagnostics, ServiceStatus, StartRequest,
};

#[command]
pub(crate) fn start<R: Runtime>(app: AppHandle<R>, payload: StartRequest) -> Result<ServiceStatus> {
    app.cordn_android_service().start(payload)
}

#[command]
pub(crate) fn stop<R: Runtime>(app: AppHandle<R>) -> Result<ServiceStatus> {
    app.cordn_android_service().stop()
}

#[command]
pub(crate) fn status<R: Runtime>(app: AppHandle<R>) -> Result<ServiceStatus> {
    app.cordn_android_service().status()
}

#[command]
pub(crate) fn config<R: Runtime>(app: AppHandle<R>) -> Result<ConfigResponse> {
    app.cordn_android_service().config()
}

#[command]
pub(crate) fn diagnostics<R: Runtime>(app: AppHandle<R>) -> Result<ServiceDiagnostics> {
    app.cordn_android_service().diagnostics()
}

#[command]
pub(crate) fn encode_pubkey<R: Runtime>(
    app: AppHandle<R>,
    payload: EncodePubkeyRequest,
) -> Result<PubkeyDisplay> {
    app.cordn_android_service().encode_pubkey(payload)
}

#[command]
pub(crate) fn flush_data<R: Runtime>(app: AppHandle<R>) -> Result<ServiceStatus> {
    app.cordn_android_service().flush_data()
}

#[command]
pub(crate) fn reset_identity<R: Runtime>(app: AppHandle<R>) -> Result<ServiceStatus> {
    app.cordn_android_service().reset_identity()
}

#[command]
pub(crate) fn request_battery_exemption<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    app.cordn_android_service().request_battery_exemption()
}

#[command]
pub(crate) fn request_notification_permission<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    app.cordn_android_service()
        .request_notification_permission()
}
