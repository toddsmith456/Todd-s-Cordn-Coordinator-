use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{
    ConfigResponse, EncodePubkeyRequest, PubkeyDisplay, ServiceDiagnostics, ServiceStatus,
    StartRequest,
};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<CordnAndroidService<R>> {
    Ok(CordnAndroidService(std::marker::PhantomData))
}

pub struct CordnAndroidService<R: Runtime>(std::marker::PhantomData<R>);

impl<R: Runtime> CordnAndroidService<R> {
    pub fn start(&self, _payload: StartRequest) -> crate::Result<ServiceStatus> {
        Err(crate::Error::Unsupported)
    }

    pub fn restart_schedule(
        &self,
        _payload: serde_json::Value,
    ) -> crate::Result<serde_json::Value> {
        Err(crate::Error::Unsupported)
    }

    pub fn stop(&self) -> crate::Result<ServiceStatus> {
        Err(crate::Error::Unsupported)
    }

    pub fn status(&self) -> crate::Result<ServiceStatus> {
        Ok(ServiceStatus::default())
    }

    pub fn config(&self) -> crate::Result<ConfigResponse> {
        Ok(ConfigResponse::default())
    }

    pub fn diagnostics(&self) -> crate::Result<ServiceDiagnostics> {
        Ok(ServiceDiagnostics::default())
    }

    pub fn flush_data(&self) -> crate::Result<ServiceStatus> {
        Ok(ServiceStatus::default())
    }

    pub fn reset_identity(&self) -> crate::Result<ServiceStatus> {
        Ok(ServiceStatus::default())
    }

    pub fn encode_pubkey(&self, _payload: EncodePubkeyRequest) -> crate::Result<PubkeyDisplay> {
        Err(crate::Error::Unsupported)
    }

    pub fn request_battery_exemption(&self) -> crate::Result<()> {
        Err(crate::Error::Unsupported)
    }

    pub fn request_notification_permission(&self) -> crate::Result<()> {
        Err(crate::Error::Unsupported)
    }
}
