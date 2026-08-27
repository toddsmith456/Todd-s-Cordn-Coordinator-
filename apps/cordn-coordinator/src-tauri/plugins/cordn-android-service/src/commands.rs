use tauri::{command, AppHandle, Runtime};

use crate::{
    ConfigResponse, CordnAndroidServiceExt, Result, ServiceDiagnostics, ServiceStatus, StartRequest,
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
pub(crate) fn request_battery_exemption<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    app.cordn_android_service().request_battery_exemption()
}

#[command]
pub(crate) fn request_notification_permission<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    app.cordn_android_service()
        .request_notification_permission()
}
