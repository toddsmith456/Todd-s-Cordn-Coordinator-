use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub use models::*;

#[cfg(target_os = "android")]
mod android;
#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

mod commands;
mod error;
mod models;

pub use error::{Error, Result};

#[cfg(desktop)]
use desktop::CordnAndroidService;
#[cfg(mobile)]
use mobile::CordnAndroidService;

pub trait CordnAndroidServiceExt<R: Runtime> {
    fn cordn_android_service(&self) -> &CordnAndroidService<R>;
}

impl<R: Runtime, T: Manager<R>> CordnAndroidServiceExt<R> for T {
    fn cordn_android_service(&self) -> &CordnAndroidService<R> {
        self.state::<CordnAndroidService<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    #[cfg(target_os = "android")]
    android::ensure_linked();

    Builder::new("cordn-android-service")
        .invoke_handler(tauri::generate_handler![
            commands::start,
            commands::stop,
            commands::status,
            commands::config,
            commands::diagnostics,
            commands::request_battery_exemption,
            commands::request_notification_permission,
        ])
        .setup(|app, api| {
            #[cfg(mobile)]
            let service = mobile::init(app, api)?;
            #[cfg(desktop)]
            let service = desktop::init(app, api)?;
            app.manage(service);
            Ok(())
        })
        .build()
}
