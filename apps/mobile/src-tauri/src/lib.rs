//! Mythos en Android: el shell. La interfaz es la de apps/web (viene en el APK); lo nativo
//! (pantalla completa, atras, pantalla encendida, actualizaciones, la interfaz del server
//! opcional) esta en el plugin `apps/mobile/plugin`. Ver docs/mobile.md.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_mythos::init())
        .setup(|_app| {
            // Aca va a arrancar el server embebido del modo local (backend/server como
            // biblioteca, en 127.0.0.1 con un puerto libre). La interfaz lo usa con
            // `setActiveServer(url)` de apps/web/src/lib/platform.ts, sin recargar.
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("no se pudo abrir Mythos");
}
