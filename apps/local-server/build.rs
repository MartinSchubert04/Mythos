// Cada comando genera su permiso (`allow-…`); `default` (permissions/default.toml) los da todos a
// la pagina de la app.
const COMMANDS: &[&str] = &["info", "set_mode"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
