//! Actualizaciones del APK (igual que en Haze): al rato de abrir y despues cada seis horas se
//! fija si el ultimo Release de GitHub trae una version mas nueva en `latest-android.json`, la
//! baja de fondo y verifica el sha256. Se instala sola al salir de la app sin nada
//! reproduciendose (lo decide `MythosPlugin.onStop`) o con "Update now" en Settings.
//!
//! La interfaz no necesita esto para estar al dia (sale del server); el APK se actualiza para
//! lo nativo y para la pagina local de respaldo.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

#[cfg(target_os = "android")]
use std::time::Duration;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// "downloading" o "ready"
    pub status: String,
    pub progress: f64,
}

/// `candidate` es mas nueva que `current` ("0.10.0" > "0.9.3").
pub(crate) fn newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.trim().trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parse(candidate) > parse(current)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn set<R: Runtime>(app: &AppHandle<R>, info: Option<UpdateInfo>) {
    *app.state::<crate::Shell>().update.lock().unwrap() = info.clone();
    let _ = app.emit("mythos://update", info);
}

#[cfg(target_os = "android")]
fn on_progress<R: Runtime>(app: &AppHandle<R>, progress: f64) {
    let info = {
        let shell = app.state::<crate::Shell>();
        let mut u = shell.update.lock().unwrap();
        match u.as_mut() {
            Some(i) if i.status == "downloading" => {
                i.progress = progress;
                i.clone()
            }
            _ => return,
        }
    };
    let _ = app.emit("mythos://update", Some(info));
}

/// Busca y, si hay algo nuevo, lo baja. Devuelve la version encontrada.
#[cfg(target_os = "android")]
async fn check<R: Runtime>(app: &AppHandle<R>) -> Result<Option<String>, String> {
    if app.state::<crate::Shell>().update.lock().unwrap().is_some() {
        // Ya se esta bajando o esta lista.
        return Ok(None);
    }
    let latest = crate::native(app, |n| n.update_latest()).await?;
    let current = app.package_info().version.to_string();
    let Some(release) = latest.filter(|r| newer(&r.version, &current)) else {
        return Ok(None);
    };
    let version = release.version.clone();
    set(app, Some(UpdateInfo { version: version.clone(), status: "downloading".into(), progress: 0.0 }));
    let h = app.clone();
    let done = crate::native(app, move |n| n.update_download(&release, move |p| on_progress(&h, p))).await;
    match done {
        Ok(()) => {
            set(app, Some(UpdateInfo { version: version.clone(), status: "ready".into(), progress: 1.0 }));
            Ok(Some(version))
        }
        Err(e) => {
            set(app, None);
            Err(e)
        }
    }
}

/// Corre de fondo mientras viva la app.
#[cfg(target_os = "android")]
pub(crate) async fn run<R: Runtime>(app: AppHandle<R>) {
    const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
    // Que arranque primero lo demas (la interfaz, el video).
    tokio::time::sleep(Duration::from_secs(30)).await;
    loop {
        if let Err(e) = check(&app).await {
            log::warn!("mythos: no se pudo buscar actualizaciones: {e}");
        }
        tokio::time::sleep(EVERY).await;
    }
}

/// Lo que se esta bajando o esta listo (para Settings al abrirse).
#[tauri::command]
pub(crate) fn update_state<R: Runtime>(app: AppHandle<R>) -> Option<UpdateInfo> {
    app.state::<crate::Shell>().update.lock().unwrap().clone()
}

/// "Check for updates" en Settings. Devuelve la version nueva, si hay.
#[tauri::command]
pub(crate) async fn update_check<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
    #[cfg(target_os = "android")]
    return check(&app).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(None)
    }
}

/// "Update now". `false` si antes hay que dar el permiso de instalar apps (se abrio ese ajuste).
#[tauri::command]
pub(crate) async fn update_install<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    return crate::native(&app, |n| n.update_install()).await;
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(false)
    }
}
