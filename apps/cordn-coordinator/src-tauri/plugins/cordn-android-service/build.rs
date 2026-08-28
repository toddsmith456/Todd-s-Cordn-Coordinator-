const COMMANDS: &[&str] = &[
    "start",
    "stop",
    "status",
    "config",
    "diagnostics",
    "encode_pubkey",
    "flush_data",
    "reset_identity",
    "request_battery_exemption",
    "request_notification_permission",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
