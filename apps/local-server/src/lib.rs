//! Modo local (docs/local-mode.md): el server de Mythos embebido en la app, en
//! `http://127.0.0.1:<puerto libre>`, con su propia SQLite y torrents en la carpeta de la app.
//! Lo usan las dos apps (desktop y Android) igual.
//!
//! - **Modos** (los elige el usuario en Settings y quedan guardados en `local-mode` dentro de la
//!   carpeta de la app): `auto` (por defecto: server de casa mientras responda, el local si no),
//!   `server` (solo el de casa: el local ni arranca) y `local` (siempre el local; sincroniza solo
//!   cuando alcanza al de casa).
//! - **Arranque**: en `auto` y `local`, apenas abre la app y en segundo plano, para que pasar al
//!   local sea instantaneo cuando el server se cae. Tarda unos cientos de ms y no frena la
//!   ventana; `info` espera a que termine si la interfaz pregunta antes.
//! - **ffmpeg**: no viene con la app (la deja liviana). En Windows, si hay un `ffmpeg` y un
//!   `ffprobe` en el PATH se usan; si no, solo reproduccion directa. En Android, nunca.
//! - **Cierre**: al salir de la app, `Handle::shutdown()` (suelta puertos y archivos).
//! - **TV y poca RAM** (Android): en un TV box o con menos de 2 GB el server embebido no arranca
//!   nunca: el modo queda en `server` sin importar lo guardado y `set_mode` no acepta los otros
//!   (`Info.unavailable` dice por que).
//!
//! La interfaz (apps/web/src/lib/local.svelte.ts) llama a `info` y `set_mode`; elegir a que server
//! hablarle y sincronizar al volver es cosa suya y del server local.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Auto: cuanto espera el server local para arrancar solo (ver `init`).
const AUTO_START_DELAY: Duration = Duration::from_secs(8);

use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Manager, RunEvent, Runtime};

#[cfg(target_os = "android")]
mod android;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Auto,
    Server,
    Local,
}

impl Mode {
    fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "auto" => Some(Mode::Auto),
            "server" => Some(Mode::Server),
            "local" => Some(Mode::Local),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Server => "server",
            Mode::Local => "local",
        }
    }

    /// Con este modo hace falta el server embebido.
    fn embedded(self) -> bool {
        self != Mode::Server
    }
}

enum Slot {
    Stopped,
    Running(mythos::Handle),
    Failed(String),
}

struct Local {
    /// Carpeta de la app (el server usa `<dir>/local`).
    dir: PathBuf,
    /// Este aparato no corre el server embebido (`"tv"` o `"low_ram"`): solo server.
    limit: Option<&'static str>,
    mode: Mutex<Mode>,
    /// Async: quien pregunta mientras arranca espera a que termine.
    slot: tokio::sync::Mutex<Slot>,
}

/// Lo que recibe la interfaz.
#[derive(Serialize)]
struct Info {
    mode: Mode,
    /// `http://127.0.0.1:<puerto>`; `null` en modo `server` o si no pudo arrancar.
    base_url: Option<String>,
    /// Hay ffmpeg en el dispositivo (si no, solo reproduccion directa).
    transcode_available: bool,
    error: Option<String>,
    /// El modo local no existe en este aparato: `"tv"` (TV box) o `"low_ram"` (menos de 2 GB).
    /// Con esto el modo es siempre `server`.
    unavailable: Option<&'static str>,
}

fn info_of(mode: Mode, slot: &Slot, unavailable: Option<&'static str>) -> Info {
    let (base_url, transcode_available, error) = match slot {
        Slot::Running(h) => (Some(h.base_url()), h.media_tools().ffmpeg, None),
        Slot::Failed(e) => (None, false, Some(e.clone())),
        Slot::Stopped => (None, false, None),
    };
    Info { mode, base_url, transcode_available, error, unavailable }
}

/// En Android: si el aparato puede con el server embebido (ver `android::limit`).
fn device_limit() -> Option<&'static str> {
    #[cfg(target_os = "android")]
    return android::limit();
    #[cfg(not(target_os = "android"))]
    None
}

fn mode_file(dir: &Path) -> PathBuf {
    dir.join("local-mode")
}

/// `ffmpeg` y `ffprobe` del PATH (los dos o ninguno). Sin ellos, rutas vacias: el motor ni
/// intenta lanzarlos.
fn media_tools() -> (PathBuf, PathBuf) {
    #[cfg(not(target_os = "android"))]
    {
        let exe = |name: &str| if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
        let find = |name: &str| {
            std::env::var_os("PATH")
                .and_then(|p| std::env::split_paths(&p).map(|d| d.join(exe(name))).find(|f| f.is_file()))
        };
        if let (Some(ffmpeg), Some(ffprobe)) = (find("ffmpeg"), find("ffprobe")) {
            return (ffmpeg, ffprobe);
        }
    }
    (PathBuf::new(), PathBuf::new())
}

async fn start(dir: &Path) -> Slot {
    let t = Instant::now();
    let mut cfg = mythos::Config::local(dir.join("local"));
    (cfg.ffmpeg, cfg.ffprobe) = media_tools();
    match mythos::run(cfg).await {
        Ok(h) => {
            tracing::info!(
                "server local en {} ({} ms, ffmpeg: {})",
                h.base_url(),
                t.elapsed().as_millis(),
                h.media_tools().ffmpeg
            );
            Slot::Running(h)
        }
        Err(e) => {
            tracing::error!("no arranco el server local: {e:#}");
            Slot::Failed(format!("{e:#}"))
        }
    }
}

/// Arranca el server si el modo lo pide y no esta andando (o fallo antes: se reintenta).
async fn ensure(local: &Local) -> Info {
    let mode = *local.mode.lock().unwrap();
    let mut slot = local.slot.lock().await;
    if mode.embedded() && !matches!(*slot, Slot::Running(_)) {
        *slot = start(&local.dir).await;
    }
    info_of(mode, &slot, local.limit)
}

async fn stop(local: &Local) {
    let mut slot = local.slot.lock().await;
    if let Slot::Running(h) = std::mem::replace(&mut *slot, Slot::Stopped) {
        // `shutdown` ya tiene sus propios topes (~10 s con un <video> abierto); por las dudas.
        if tokio::time::timeout(Duration::from_secs(12), h.shutdown()).await.is_err() {
            tracing::warn!("el server local no termino de cerrar a tiempo");
        }
    }
}

/// Modo y donde esta el server local (espera si esta arrancando).
#[tauri::command]
async fn info<R: Runtime>(app: AppHandle<R>) -> Info {
    ensure(&app.state::<Local>()).await
}

/// Cambia el modo (queda guardado): `server` apaga el local, `auto`/`local` lo arrancan.
#[tauri::command]
async fn set_mode<R: Runtime>(app: AppHandle<R>, mode: Mode) -> Result<Info, String> {
    let local = app.state::<Local>();
    if mode.embedded() && local.limit.is_some() {
        return Err("Local mode isn't available on this device".into());
    }
    std::fs::create_dir_all(&local.dir).map_err(|e| e.to_string())?;
    std::fs::write(mode_file(&local.dir), mode.as_str()).map_err(|e| e.to_string())?;
    *local.mode.lock().unwrap() = mode;
    if !mode.embedded() {
        stop(&local).await;
    }
    Ok(ensure(&local).await)
}

/// `tracing` para el server embebido (la biblioteca no inicializa nada): en Android a logcat
/// (etiqueta `mythos`), en el resto a stderr. `RUST_LOG` lo cambia.
/// Apaga el server local ya (lo mismo que al salir). Para el actualizador de desktop: el
/// instalador cierra el proceso con `exit` y no llega `RunEvent::Exit`.
pub fn shutdown<R: Runtime>(app: &AppHandle<R>) {
    if let Some(local) = app.try_state::<Local>() {
        tauri::async_runtime::block_on(stop(&local));
    }
}

pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,librqbit=warn,librqbit_dht=warn,tracker_comms=warn"));
    let fmt = tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false);
    #[cfg(target_os = "android")]
    let _ = fmt.without_time().with_writer(android::Logcat).try_init();
    #[cfg(not(target_os = "android"))]
    let _ = fmt.with_writer(std::io::stderr).try_init();
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("mythos-local")
        .invoke_handler(tauri::generate_handler![info, set_mode])
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            android::init_tls();
            let dir = app.path().app_local_data_dir()?;
            let limit = device_limit();
            let saved = std::fs::read_to_string(mode_file(&dir)).ok().and_then(|s| Mode::parse(&s)).unwrap_or(Mode::Auto);
            // En un TV box o con poca RAM, siempre solo server (lo guardado no se toca).
            let mode = if limit.is_some() { Mode::Server } else { saved };
            app.manage(Local { dir, limit, mode: Mutex::new(mode), slot: tokio::sync::Mutex::new(Slot::Stopped) });
            // En segundo plano: la ventana no espera al server. En Auto (se usa el server de casa y
            // el local es el respaldo) arranca unos segundos despues: en una TV box o un celular
            // flojo competiria por CPU y red con el primer render. Si hace falta antes (login,
            // server caido), `info` lo arranca en el momento.
            if mode.embedded() {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if mode == Mode::Auto {
                        tokio::time::sleep(AUTO_START_DELAY).await;
                    }
                    ensure(&app.state::<Local>()).await;
                });
            }
            Ok(())
        })
        .on_event(|app, ev| {
            if let RunEvent::Exit = ev {
                if let Some(local) = app.try_state::<Local>() {
                    tauri::async_runtime::block_on(stop(&local));
                }
            }
        })
        .build()
}
