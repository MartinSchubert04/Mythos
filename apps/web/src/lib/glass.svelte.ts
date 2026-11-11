// Desktop: ventana de vidrio esmerilado, como Haze. Windows desenfoca lo que hay detras de la
// ventana (efecto Acrylic) y encima va el fondo del tema con la opacidad elegida, asi funciona con
// todos los temas y paletas.
//
// Va con el tema: es de cada usuario (`Settings.ui.glass = {on, opacity}`, lib/prefs.svelte.ts).
// En localStorage queda la copia del ultimo usuario de esta PC, para arrancar con la ventana ya
// como estaba. Solo hace algo en desktop; en el resto se guarda pero no se aplica (y nunca se
// sube desde ahi: ver `saveLook` en prefs.svelte.ts).
//
// El fondo de los temas (--bg, --bg-image, --sidebar) no se toca en themes.css: con el vidrio
// prendido se leen los colores del tema actual y se pisan en <html> con la mezcla
// (`color-mix` con transparente). Al cambiar de tema o paleta se vuelven a calcular.
import { isDesktop } from "./platform";
import { readJson, write } from "./store";

const KEY = "mythos.glass";
/** Por debajo de esto el texto deja de leerse bien sobre un escritorio claro. */
export const MIN_OPACITY = 0.25;

export const glass = $state(readJson(KEY, { on: false, opacity: 0.7 }));

let applied = false;

/** Guarda y aplica (el slider llama con `persist = false` mientras se arrastra). */
export function setGlass(on: boolean, opacity = glass.opacity, persist = true) {
  glass.on = on;
  glass.opacity = Math.min(1, Math.max(MIN_OPACITY, opacity));
  if (persist) write(KEY, JSON.stringify({ on: glass.on, opacity: glass.opacity }));
  apply();
}

/** El vidrio de un usuario (Settings.ui.glass). Devuelve si habia uno valido guardado. */
export function applyRemoteGlass(g: unknown): boolean {
  if (!g || typeof g !== "object") return false;
  const { on, opacity } = g as Record<string, unknown>;
  if (typeof on !== "boolean" || typeof opacity !== "number" || !Number.isFinite(opacity)) return false;
  setGlass(on, opacity);
  return true;
}

const VARS = ["--bg", "--sidebar"] as const;

function paint() {
  const el = document.documentElement;
  for (const v of VARS) el.style.removeProperty(v);
  if (!glass.on) {
    delete el.dataset.glass;
    el.style.removeProperty("--glass");
    return;
  }
  // Los valores del tema sin el vidrio (recien sacadas las mezclas de arriba).
  const cs = getComputedStyle(el);
  const pct = Math.round(glass.opacity * 100);
  for (const v of VARS) {
    const base = cs.getPropertyValue(v).trim();
    if (base) el.style.setProperty(v, `color-mix(in srgb, ${base} ${pct}%, transparent)`);
  }
  el.style.setProperty("--glass", String(glass.opacity));
  el.dataset.glass = "";
}

async function apply() {
  if (!isDesktop) return;
  paint();
  if (glass.on === applied) return;
  applied = glass.on;
  try {
    const { getCurrentWindow, Effect } = await import("@tauri-apps/api/window");
    const w = getCurrentWindow();
    if (glass.on) await w.setEffects({ effects: [Effect.Acrylic] });
    else await w.clearEffects();
  } catch {
    applied = !glass.on;
  }
}

/** Reproductor: la ventana pasa a opaca mientras dura (y vuelve a transparente al salir). Con la
 *  ventana transparente (nace asi por el vidrio, este prendido o no) Windows mezcla cada cuadro
 *  del video con lo que hay detras al ritmo de la pantalla principal: en una de menos Hz (una
 *  tele de 60 con un monitor de 180) el video se ve a saltos. En el reproductor el vidrio no se
 *  ve igual (lo tapa el video). */
export async function opaqueWindow(on: boolean) {
  if (!isDesktop) return;
  try {
    // La ventana (apps/desktop/src-tauri/src/opaque.rs) y la vista: con una sola no alcanza.
    const { invoke } = await import("@tauri-apps/api/core");
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    if (on) await invoke("window_opaque", { on });
    await getCurrentWebview().setBackgroundColor(on ? [0, 0, 0, 255] : [0, 0, 0, 0]);
    if (!on) await invoke("window_opaque", { on });
  } catch {}
}

export function initGlass() {
  if (!isDesktop) return;
  apply();
  // Otro tema o paleta: otros colores de base.
  new MutationObserver(() => glass.on && paint()).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme", "data-palette"],
  });
}
