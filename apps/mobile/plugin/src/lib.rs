//! Mythos en Android: lo que la interfaz (apps/web) no puede hacer sola dentro del WebView.
//!
//! - **La interfaz es la del APK** (el build de apps/web, la pagina "local"): pide la direccion
//!   del server la primera vez y habla con el por HTTP. Se actualiza con el APK.
//! - **Opcional (Settings → App, apagado por defecto): la interfaz del server.** Si el usuario lo
//!   prende y el server contesta, la pagina local navega a la interfaz que sirve el server
//!   (`open_server`): cada deploy la actualiza al instante, sin reinstalar. Si el server no
//!   contesta, queda la local.
//! - **Permisos de la pagina del server.** Solo el origen exacto que eligio el usuario recibe,
//!   en tiempo de ejecucion, los comandos de este plugin que necesita (nunca `open_server`) y
//!   escuchar eventos. Cualquier otra pagina no tiene IPC.
//! - **Lo nativo del reproductor:** pantalla completa inmersiva con horizontal, pantalla
//!   encendida mientras se reproduce y el color de los iconos de las barras del sistema.
//! - **Actualizaciones del APK** desde GitHub Releases (ver `updater.rs` y `Updater.kt`).
//!
//! En otras plataformas los comandos nativos no hacen nada.
//!
//! Contrato con la interfaz: los comandos solo se agregan, nunca se cambian ni se sacan. La
//! interfaz del server puede ser mas nueva o mas vieja que el APK instalado, y tiene que
//! tolerar que un comando falte (lo trata como "no hace nada").

use std::collections::HashSet;
use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::CapabilityBuilder;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime, Url, Webview};

#[cfg(target_os = "android")]
mod android;
mod updater;

pub use updater::UpdateInfo;

/// Lo que la pagina del server puede pedir. `open_server` no: elegir a donde navegar es cosa
/// de la pagina local.
const REMOTE_PERMISSIONS: &[&str] = &[
    "mythos:allow-leave-server",
    "mythos:allow-handoff",
    "mythos:allow-set-fullscreen",
    "mythos:allow-set-playing",
    "mythos:allow-set-bars",
    "mythos:allow-minimize",
    "mythos:allow-request-network",
    "mythos:allow-update-state",
    "mythos:allow-update-check",
    "mythos:allow-update-install",
    // El boton "atras" (onBackButtonPress) y el aviso de actualizaciones (listen).
    "core:app:allow-register-listener",
    "core:app:allow-remove-listener",
    "core:event:allow-listen",
    "core:event:allow-unlisten",
];

#[derive(Default)]
pub(crate) struct Shell {
    /// Origenes de server que ya tienen su capability.
    allowed: Mutex<HashSet<String>>,
    /// Token del login hecho en la pagina local, para la del server (se entrega una vez).
    token: Mutex<Option<String>>,
    /// Es el primer arranque despues de instalar una version nueva (se entrega una vez).
    updated: Mutex<bool>,
    pub(crate) update: Mutex<Option<UpdateInfo>>,
}

/// `https://host:7700/lo-que-sea` → `https://host:7700`. Solo http(s) con host.
fn origin_of(url: &str) -> Result<Url, String> {
    let u: Url = url.trim().parse().map_err(|_| "That doesn't look like an address".to_string())?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
        return Err("The server address must start with https://".into());
    }
    let origin = u.origin().ascii_serialization();
    format!("{origin}/").parse().map_err(|_| "That doesn't look like an address".to_string())
}

/// La pagina local (el build del APK).
fn local_url<R: Runtime>(app: &AppHandle<R>) -> Url {
    if tauri::is_dev() {
        if let Some(u) = app.config().build.dev_url.clone() {
            return u;
        }
    }
    let https = app.config().app.windows.first().is_some_and(|w| w.use_https_scheme);
    let scheme = if https { "https" } else { "http" };
    format!("{scheme}://tauri.localhost/").parse().expect("url local")
}

/// Le da a la pagina del server (solo ese origen) los permisos de `REMOTE_PERMISSIONS`.
fn allow_remote<R: Runtime>(app: &AppHandle<R>, origin: &Url) -> Result<(), String> {
    let shell = app.state::<Shell>();
    let mut allowed = shell.allowed.lock().unwrap();
    let key = origin.as_str().to_string();
    if allowed.contains(&key) {
        return Ok(());
    }
    let mut cap = CapabilityBuilder::new(format!("server-{}", allowed.len()))
        .remote(key.clone())
        .local(false)
        .window("main");
    for p in REMOTE_PERMISSIONS {
        cap = cap.permission(*p);
    }
    app.add_capability(cap).map_err(|e| e.to_string())?;
    allowed.insert(key);
    Ok(())
}

/// Pagina local → la interfaz del server. `token`: el del login recien hecho en la local.
/// `route`: el hash a abrir ("#/title/movie/tt…").
#[tauri::command]
async fn open_server<R: Runtime>(
    app: AppHandle<R>,
    webview: Webview<R>,
    url: String,
    token: Option<String>,
    route: Option<String>,
) -> Result<(), String> {
    let origin = origin_of(&url)?;
    allow_remote(&app, &origin)?;
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        *app.state::<Shell>().token.lock().unwrap() = Some(t);
    }
    let mut target = origin.clone();
    if let Some(r) = route.filter(|r| r.starts_with("#/")) {
        target.set_fragment(Some(&r[1..]));
    }
    webview.navigate(target).map_err(|e| e.to_string())
}

/// Pagina del server → la local. `route`: el hash a abrir; por defecto `#/server` (el login con
/// el campo del server abierto, "Change server").
#[tauri::command]
async fn leave_server<R: Runtime>(app: AppHandle<R>, webview: Webview<R>, route: Option<String>) -> Result<(), String> {
    let mut url = local_url(&app);
    let route = route.filter(|r| r.starts_with("#/")).unwrap_or_else(|| "#/server".into());
    url.set_fragment(Some(&route[1..]));
    webview.navigate(url).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct Handoff {
    token: Option<String>,
    /// Primer arranque de una version nueva: la interfaz vuelve a donde estaba.
    updated: bool,
    /// Version del APK.
    version: String,
}

/// Lo que la pagina del server recibe al arrancar (el token y el aviso de "recien
/// actualizada" se entregan una sola vez).
#[tauri::command]
fn handoff<R: Runtime>(app: AppHandle<R>) -> Handoff {
    let shell = app.state::<Shell>();
    let token = shell.token.lock().unwrap().take();
    let updated = std::mem::take(&mut *shell.updated.lock().unwrap());
    Handoff { token, updated, version: app.package_info().version.to_string() }
}

/// Corre una llamada a Kotlin fuera del hilo principal (Kotlin la atiende en el principal: si
/// se bloqueara ahi, nunca contestaria).
#[cfg(target_os = "android")]
async fn native<R: Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    f: impl FnOnce(&android::Native<R>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let h = app.clone();
    tauri::async_runtime::spawn_blocking(move || f(h.state::<android::Native<R>>().inner()))
        .await
        .map_err(|e| e.to_string())?
}

/// Pantalla completa del reproductor: sin barras del sistema y en horizontal (girando entre
/// las dos horizontales). `false` vuelve a lo normal.
#[tauri::command]
async fn set_fullscreen<R: Runtime>(app: AppHandle<R>, on: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return native(&app, move |n| n.set_fullscreen(on)).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, on);
        Ok(())
    }
}

/// Se esta reproduciendo: la pantalla no se apaga y la actualizacion no se instala.
#[tauri::command]
async fn set_playing<R: Runtime>(app: AppHandle<R>, playing: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return native(&app, move |n| n.set_playing(playing)).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, playing);
        Ok(())
    }
}

/// Iconos oscuros en las barras del sistema (temas claros) o claros (temas oscuros).
#[tauri::command]
async fn set_bars<R: Runtime>(app: AppHandle<R>, light: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return native(&app, move |n| n.set_bars(light)).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, light);
        Ok(())
    }
}

/// "Atras" sin nada abierto en el inicio: la app se va al fondo en vez de cerrarse.
#[tauri::command]
async fn minimize<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return native(&app, |n| n.minimize()).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(())
    }
}

/// Android 17+: pide el permiso de red local (la interfaz lo llama si el server no contesta:
/// sin el, Android corta las conexiones a IPs de la casa). `true` si esta dado o no hace falta.
#[tauri::command]
async fn request_network<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    return native(&app, |n| n.request_network()).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(true)
    }
}

/// Recuerda la version que arranco: si cambio desde la ultima vez, es una actualizacion.
fn mark_launch<R: Runtime>(app: &AppHandle<R>) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("last-version");
    let current = app.package_info().version.to_string();
    let last = std::fs::read_to_string(&file).ok().map(|s| s.trim().to_string());
    if last.as_deref().is_some_and(|l| l != current) {
        *app.state::<Shell>().updated.lock().unwrap() = true;
    }
    if last.as_deref() != Some(current.as_str()) {
        let _ = std::fs::write(&file, &current);
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("mythos")
        .invoke_handler(tauri::generate_handler![
            open_server,
            leave_server,
            handoff,
            set_fullscreen,
            set_playing,
            set_bars,
            minimize,
            request_network,
            updater::update_state,
            updater::update_check,
            updater::update_install,
        ])
        .setup(|app, _api| {
            app.manage(Shell::default());
            mark_launch(app);
            #[cfg(target_os = "android")]
            {
                let handle = _api.register_android_plugin("dev.mrtn.mythos.plugin", "MythosPlugin")?;
                app.manage(android::Native::new(handle));
                tauri::async_runtime::spawn(updater::run(app.clone()));
            }
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        assert_eq!(origin_of("https://srv.example.ts.net:7700").unwrap().as_str(), "https://srv.example.ts.net:7700/");
        assert_eq!(origin_of(" https://srv:7700/#/title/x ").unwrap().as_str(), "https://srv:7700/");
        assert_eq!(origin_of("http://10.0.2.2:7700/").unwrap().as_str(), "http://10.0.2.2:7700/");
        assert!(origin_of("javascript:alert(1)").is_err());
        assert!(origin_of("file:///etc/passwd").is_err());
        assert!(origin_of("srv:7700").is_err());
    }

    #[test]
    fn versions() {
        use updater::newer;
        assert!(newer("0.3.0", "0.2.9"));
        assert!(newer("1.2.10", "1.2.9"));
        assert!(newer("v1.0.0", "0.9.0"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("0.1.9", "0.2.0"));
    }
}
