// Cada comando genera su permiso (`allow-…`). La pagina local (el build que viene en el APK)
// tiene `default` (todos); la del server tiene los que agrega `allow_remote` en src/lib.rs.
const COMMANDS: &[&str] = &[
    "open_server",
    "leave_server",
    "handoff",
    "set_fullscreen",
    "set_playing",
    "set_bars",
    "minimize",
    "request_network",
    "update_state",
    "update_check",
    "update_install",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).android_path("android").build();
}
