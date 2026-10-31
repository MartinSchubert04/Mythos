//! Mythos desktop: solo la ventana. La interfaz es la misma de apps/web (con su barra de titulo
//! propia, por eso la ventana va sin marco). Recuerda tamaño y posicion, y hay una sola instancia:
//! abrirla otra vez trae al frente la que ya estaba.

use tauri::Manager;

pub fn run() {
    // En debug, WebView2 abre el puerto de depuracion remota (chrome://inspect →
    // localhost:9223) para inspeccionar la interfaz desde Chrome. Tiene que estar antes de
    // crear el WebView.
    #[cfg(debug_assertions)]
    if std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--remote-debugging-port=9223");
    }

    tauri::Builder::default()
        // Primero: si ya hay una abierta, esta se cierra y la otra viene al frente.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            // La ventana nace oculta: el plugin le devuelve el tamaño y lugar de la ultima vez
            // y recien ahi se muestra (sin un salto visible).
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("no se pudo abrir Mythos");
}
