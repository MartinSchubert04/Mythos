//! Actualizaciones automaticas, como en Ethereon y Haze:
//!
//! 1. Al rato de arrancar y cada 6 horas mira `latest.json` del ultimo Release de GitHub.
//! 2. Si hay una version nueva, la baja de fondo y verifica la firma (clave publica en
//!    tauri.conf.json, la privada es un secret del repo).
//! 3. Lista para instalar: la barra de titulo muestra "Update" (reinicia ya y vuelve a la misma
//!    pantalla) y, si no, se instala sola al cerrar Mythos. Nunca corta una pelicula.
//! 4. "Update": el instalador (NSIS, silencioso) vuelve a abrir la app (`/R`) y `relaunch.json` le
//!    dice a la version nueva que vuelva a la misma pantalla. Al cerrar: sin `/R`, no se abre.
//! 5. El instalador termina el proceso con `exit` (no llega `RunEvent::Exit`): antes se guardan el
//!    tamaño y lugar de la ventana y se apaga el server local, como Haze y Ethereon.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_plugin_window_state::AppHandleExt;

const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
const EVENT: &str = "mythos://update";

#[derive(Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    /// "downloading" o "ready".
    pub status: String,
    /// 0..1 mientras baja.
    pub progress: f64,
}

#[derive(Default)]
pub struct Pending {
    info: Option<UpdateInfo>,
    ready: Option<(Update, Vec<u8>)>,
    /// Esta ejecucion es la version nueva recien instalada con "Update" (la interfaz vuelve a
    /// donde estaba). Se lee una vez.
    just_updated: bool,
}

pub type State = Mutex<Pending>;

/// Un `relaunch.json` mas viejo que esto no es de esta actualizacion (el instalador no relanzo, o
/// fallo): se ignora, asi nunca cierra ni mueve una apertura normal.
const RELAUNCH_FRESH: Duration = Duration::from_secs(180);

/// Lo que deja "Update" para la version nueva.
#[derive(Serialize, Deserialize)]
struct Relaunch {
    show: bool,
}

fn relaunch_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_local_data_dir().ok().map(|d| d.join("relaunch.json"))
}

fn write_relaunch(app: &AppHandle) {
    let Some(path) = relaunch_path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serde_json::to_vec(&Relaunch { show: true }).unwrap_or_default());
}

/// Al arrancar: si esta ejecucion viene de "Update", la interfaz vuelve a donde estaba
/// (`update_handoff`).
pub fn take_relaunch(app: &AppHandle) {
    let Some(path) = relaunch_path(app) else { return };
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|d| d < RELAUNCH_FRESH);
    let r: Option<Relaunch> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let _ = std::fs::remove_file(&path);
    if r.is_some_and(|r| fresh && r.show) {
        app.state::<State>().lock().unwrap().just_updated = true;
    }
}

fn set_info(app: &AppHandle, info: Option<UpdateInfo>) {
    app.state::<State>().lock().unwrap().info = info.clone();
    let _ = app.emit(EVENT, info);
}

pub fn start(app: AppHandle) {
    // En desarrollo no: la version de `npm run tauri dev` no esta publicada.
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            if let Err(e) = check_and_download(&app).await {
                tracing::warn!("no se pudo buscar actualizaciones: {e}");
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

async fn check_and_download(app: &AppHandle) -> Result<(), String> {
    if app.state::<State>().lock().unwrap().info.is_some() {
        return Ok(());
    }
    let Some(update) = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())? else {
        return Ok(());
    };
    let version = update.version.clone();
    set_info(app, Some(UpdateInfo { version: version.clone(), status: "downloading".into(), progress: 0.0 }));
    let (mut got, mut last_pct) = (0u64, 0u64);
    let a = app.clone();
    let v = version.clone();
    let res = update
        .download(
            move |chunk, total| {
                got += chunk as u64;
                if let Some(t) = total.filter(|t| *t > 0) {
                    let pct = got * 100 / t;
                    if pct >= last_pct + 5 {
                        last_pct = pct;
                        set_info(&a, Some(UpdateInfo { version: v.clone(), status: "downloading".into(), progress: got as f64 / t as f64 }));
                    }
                }
            },
            || {},
        )
        .await;
    match res {
        Ok(bytes) => {
            app.state::<State>().lock().unwrap().ready = Some((update, bytes));
            set_info(app, Some(UpdateInfo { version, status: "ready".into(), progress: 1.0 }));
            Ok(())
        }
        Err(e) => {
            set_info(app, None); // se reintenta en el proximo chequeo
            Err(e.to_string())
        }
    }
}

/// Instala lo que se bajo. `show`: la version nueva se abre donde estaba (Update) o no se abre
/// (al cerrar). Devuelve `false` si no habia nada listo; si instala, no vuelve (el instalador
/// cierra la app).
pub fn apply(app: &AppHandle, show: bool) -> bool {
    let Some((update, bytes)) = app.state::<State>().lock().unwrap().ready.take() else { return false };
    let _ = app.save_window_state(crate::window_state_flags());
    tauri_plugin_mythos_local::shutdown(app);
    let update = update.restart_after_install(show);
    if show {
        write_relaunch(app);
        spawn_relaunch_watchdog();
    }
    if let Err(e) = update.install(&bytes) {
        if let Some(path) = relaunch_path(app) {
            let _ = std::fs::remove_file(path);
        }
        set_info(app, None);
        tracing::error!("no se pudo actualizar: {e}");
        return false;
    }
    true
}

/// Se cerro la ventana: si hay una version lista, se instala en silencio (sin volver a abrirse).
/// Apagar el server local puede tardar unos segundos: la ventana ya esta oculta.
pub fn on_close(app: &AppHandle) -> bool {
    if app.state::<State>().lock().unwrap().ready.is_none() {
        return false;
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    apply(app, false)
}

/// Respaldo (igual que Ethereon) por si el instalador no vuelve a abrir la app despues de
/// "Update": un PowerShell oculto espera a que se reemplace el ejecutable y, si Mythos no
/// arranco sola, la abre.
fn spawn_relaunch_watchdog() {
    let Ok(exe) = std::env::current_exe() else { return };
    let exe_ps = exe.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$exe = '{exe_ps}'; $antes = (Get-Item $exe).LastWriteTime; \
         for ($i = 0; $i -lt 120; $i++) {{ \
           Start-Sleep 1; \
           if ((Get-Item $exe -ErrorAction SilentlyContinue).LastWriteTime -ne $antes) {{ break }} \
         }}; \
         Start-Sleep 6; \
         if (-not (Get-Process | Where-Object {{ $_.Path -eq $exe }})) {{ Start-Process $exe }}"
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd.spawn();
}

// ---- comandos de la interfaz (los mismos nombres que el plugin de Android)

#[tauri::command]
pub fn update_state(state: tauri::State<'_, State>) -> Option<UpdateInfo> {
    state.lock().unwrap().info.clone()
}

/// "Check for updates": lo mismo que el chequeo periodico, pero ya. Devuelve la version nueva.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<String>, String> {
    check_and_download(&app).await?;
    Ok(app.state::<State>().lock().unwrap().info.as_ref().map(|i| i.version.clone()))
}

/// "Update": instala ya y vuelve a abrir Mythos donde estaba.
#[tauri::command]
pub fn update_install(app: AppHandle) -> bool {
    apply(&app, true)
}

/// Al arrancar la interfaz: si esta ejecucion viene de "Update" (para volver a la misma pantalla).
#[tauri::command]
pub fn update_handoff(state: tauri::State<'_, State>) -> bool {
    std::mem::take(&mut state.lock().unwrap().just_updated)
}
