//! Ventana opaca mientras dura el reproductor (apps/web/src/lib/glass.svelte.ts, `opaqueWindow`).
//!
//! La ventana nace transparente por el vidrio esmerilado (tao lo hace con
//! `DwmEnableBlurBehindWindow` y una region vacia). Asi Windows mezcla cada cuadro de la ventana con
//! lo que hay detras, al ritmo de la pantalla principal: en una de menos Hz (una tele de 60 con un
//! monitor de 180) el video se ve a saltos. Probado en la tele: con la ventana opaca anda fluido, y
//! dejar opaca solo la vista del WebView no alcanza. Aca se apaga y se vuelve a prender eso mismo.

#[tauri::command]
pub fn window_opaque(window: tauri::WebviewWindow, on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Dwm::{DwmEnableBlurBehindWindow, DWM_BB_BLURREGION, DWM_BB_ENABLE, DWM_BLURBEHIND};
        use windows::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject};

        let hwnd = window.hwnd().map_err(|e| e.to_string())?;
        unsafe {
            if on {
                let bb = DWM_BLURBEHIND { dwFlags: DWM_BB_ENABLE, fEnable: false.into(), ..Default::default() };
                DwmEnableBlurBehindWindow(hwnd, &bb).map_err(|e| e.to_string())?;
            } else {
                // Igual que tao al crear la ventana: region vacia, toda la ventana transparente.
                let region = CreateRectRgn(0, 0, -1, -1);
                let bb = DWM_BLURBEHIND {
                    dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
                    fEnable: true.into(),
                    hRgnBlur: region,
                    fTransitionOnMaximized: false.into(),
                };
                let r = DwmEnableBlurBehindWindow(hwnd, &bb);
                let _ = DeleteObject(region.into());
                r.map_err(|e| e.to_string())?;
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (window, on);
    Ok(())
}
