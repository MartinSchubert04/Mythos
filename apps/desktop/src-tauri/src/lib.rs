//! Mythos desktop: solo la ventana. La interfaz es la misma de apps/web (con su barra de titulo
//! propia, por eso la ventana va sin marco). Recuerda tamaño y posicion, y hay una sola instancia:
//! abrirla otra vez trae al frente la que ya estaba. El server embebido del modo local (sin
//! depender del server de casa) es el plugin `mythos-local` (apps/local-server). Se actualiza sola
//! desde los Releases de GitHub (src/updater.rs).

use tauri::{Manager, WindowEvent};
use tauri_plugin_window_state::StateFlags;

mod updater;
mod vlc;

/// Lo que recuerda el plugin de la ventana. Sin VISIBLE: si no, la muestra apenas nace (antes de
/// que pinte la interfaz). Sin FULLSCREEN: cerrar en pantalla completa en una pelicula no tiene que
/// abrir Mythos en pantalla completa.
pub(crate) fn window_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

pub fn run() {
    // En debug, WebView2 abre el puerto de depuracion remota (chrome://inspect →
    // localhost:9223) para inspeccionar la interfaz desde Chrome. Tiene que estar antes de
    // crear el WebView.
    #[cfg(debug_assertions)]
    if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        // Reemplaza los argumentos que Tauri pone por defecto: se repiten (como en Haze).
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port=9223",
        );
    }

    tauri_plugin_mythos_local::init_tracing();

    tauri::Builder::default()
        // Primero: si ya hay una abierta, esta se cierra y la otra viene al frente.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().with_state_flags(window_state_flags()).build())
        // Arranca el server local en segundo plano segun el modo (Auto / Solo local).
        .plugin(tauri_plugin_mythos_local::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::State::default())
        // "Open in VLC" de la lista de streams (src/vlc.rs) y las actualizaciones (src/updater.rs).
        .invoke_handler(tauri::generate_handler![
            vlc::open_in_vlc,
            updater::update_state,
            updater::update_check,
            updater::update_install,
            updater::update_handoff,
        ])
        // Con una version nueva lista, cerrar la ventana la instala en silencio.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && updater::on_close(window.app_handle()) {
                    api.prevent_close();
                }
            }
        })
        .setup(|app| {
            updater::take_relaunch(app.handle());
            updater::start(app.handle().clone());
            // La ventana nace oculta y transparente (por el vidrio esmerilado): el plugin le
            // devuelve el tamaño y lugar de la ultima vez y la interfaz la muestra cuando ya pinto
            // (main.ts), asi no se ve un cuadro vacio. Por las dudas, a los 3 s se muestra igual.
            if let Some(w) = app.get_webview_window("main") {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("no se pudo abrir Mythos");
}
