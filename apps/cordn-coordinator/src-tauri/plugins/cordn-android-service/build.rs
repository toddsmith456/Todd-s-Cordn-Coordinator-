const COMMANDS: &[&str] = &[
    "start",
    "restart_schedule",
    "stop",
    "status",
    "config",
    "diagnostics",
    "encode_pubkey",
    "flush_data",
    "reset_identity",
    "request_battery_exemption",
    "request_notification_permission",
    "publish_relay_list",
    "list_instances",
    "create_instance",
    "rename_instance",
    "delete_instance",
    "start_instance",
    "stop_instance",
    "status_instance",
    "config_instance",
    "diagnostics_instance",
    "flush_data_instance",
    "reset_identity_instance",
    "publish_relay_list_instance",
    "restart_schedule_instance",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
