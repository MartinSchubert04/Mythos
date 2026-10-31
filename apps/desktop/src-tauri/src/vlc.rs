//! "Open in VLC" de la lista de streams: abre el link directo (con el token en `?t=`) en el VLC
//! instalado. Sin plugins: busca vlc.exe donde lo deja el instalador y si no, `vlc` del PATH.

use std::path::PathBuf;
use std::process::Command;

#[tauri::command]
pub fn open_in_vlc(url: String) -> Result<(), String> {
    // Solo links http(s): nada que VLC pueda tomar como una opcion o un archivo local.
    if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() > 8192 {
        return Err("Only http(s) links can be opened in VLC".into());
    }
    for exe in candidates() {
        if exe.is_absolute() && !exe.exists() {
            continue;
        }
        if Command::new(&exe).arg(&url).spawn().is_ok() {
            return Ok(());
        }
    }
    Err("VLC isn't installed (get it from videolan.org), or copy the stream link instead".into())
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            out.push(PathBuf::from(dir).join("VideoLAN").join("VLC").join("vlc.exe"));
        }
    }
    #[cfg(target_os = "macos")]
    out.push(PathBuf::from("/Applications/VLC.app/Contents/MacOS/VLC"));
    out.push(PathBuf::from("vlc"));
    out
}
