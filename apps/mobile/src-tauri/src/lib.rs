//! Mythos en Android: el shell. La interfaz es la de apps/web (viene en el APK); lo nativo
//! (pantalla completa, atras, pantalla encendida, actualizaciones, la interfaz del server
//! opcional) esta en el plugin `apps/mobile/plugin`; el server embebido del modo local, en
//! `apps/local-server` (el mismo del desktop). Ver docs/mobile.md y docs/local-mode.md.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri_plugin_mythos_local::init_tracing();
    tauri::Builder::default()
        .plugin(tauri_plugin_mythos::init())
        // Arranca el server local en segundo plano segun el modo (Auto / Solo local).
        .plugin(tauri_plugin_mythos_local::init())
        .run(tauri::generate_context!())
        .expect("no se pudo abrir Mythos");
}
