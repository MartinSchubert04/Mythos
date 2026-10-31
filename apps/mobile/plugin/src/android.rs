//! Las llamadas a Kotlin (`MythosPlugin.kt`). Bloquean hasta que Kotlin contesta: llamarlas
//! siempre fuera del hilo principal de Android (ver `crate::native`).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::plugin::PluginHandle;
use tauri::Runtime;

/// Una version publicada en GitHub (`latest-android.json`).
#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

pub struct Native<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Native<R> {
    pub(crate) fn new(handle: PluginHandle<R>) -> Self {
        Self(handle)
    }

    fn call<T: DeserializeOwned>(&self, command: &str, payload: impl Serialize) -> Result<T, String> {
        self.0.run_mobile_plugin(command, payload).map_err(|e| e.to_string())
    }

    pub fn set_fullscreen(&self, on: bool) -> Result<(), String> {
        self.call::<Value>("setFullscreen", json!({ "on": on })).map(drop)
    }

    pub fn set_playing(&self, playing: bool) -> Result<(), String> {
        self.call::<Value>("setPlaying", json!({ "playing": playing })).map(drop)
    }

    pub fn set_bars(&self, light: bool) -> Result<(), String> {
        self.call::<Value>("setBars", json!({ "light": light })).map(drop)
    }

    pub fn minimize(&self) -> Result<(), String> {
        self.call::<Value>("minimize", ()).map(drop)
    }

    /// Pide el permiso de red local (Android 17+). `true` si esta dado (o no existe).
    pub fn request_network(&self) -> Result<bool, String> {
        #[derive(Deserialize)]
        struct Access {
            granted: bool,
        }
        Ok(self.call::<Access>("requestNetwork", ())?.granted)
    }

    /// La ultima version publicada para Android, si hay.
    pub fn update_latest(&self) -> Result<Option<Release>, String> {
        let v = self.call::<Value>("updateLatest", ())?;
        Ok(serde_json::from_value(v).ok())
    }

    /// Baja y verifica el APK. `on_progress` recibe de 0 a 1 (en el hilo principal de
    /// Android: tiene que volver enseguida).
    pub fn update_download(&self, release: &Release, on_progress: impl Fn(f64) + Send + Sync + 'static) -> Result<(), String> {
        #[derive(Deserialize)]
        struct Progress {
            progress: f64,
        }
        let channel: Channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                if let Ok(p) = serde_json::from_str::<Progress>(&json) {
                    on_progress(p.progress);
                }
            }
            Ok(())
        });
        self.call::<Value>("updateDownload", json!({ "url": release.url, "sha256": release.sha256, "channel": channel }))
            .map(drop)
    }

    /// Instala lo bajado. `Ok(false)` si antes hay que dar el permiso de instalar apps (se abrio
    /// ese ajuste).
    pub fn update_install(&self) -> Result<bool, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Installed {
            needs_permission: bool,
        }
        Ok(!self.call::<Installed>("updateInstall", ())?.needs_permission)
    }
}
