// Modo TV (docs/mobile.md, "TV"): un TV box o un Android TV. Se maneja con el control remoto
// (flechas, centro y atras) y se mira a 3 m. En el celular, el desktop y la web comun no cambia nada.
//
// Quien lo decide:
// - App de Android: el plugin (`device`: modo TV del sistema, leanback o sin pantalla tactil).
//   Automatico, sin opcion.
// - Samsung (Tizen, lib/tizen.ts): siempre, sin opcion.
// - Web (p. ej. Firefox en un TV box cuyo WebView es demasiado viejo para la app): lo elegido en
//   Settings → Appearance → "TV mode", o `?tv=1` / `?tv=0` / `?tv=auto` en la URL (queda guardado
//   en este navegador). Sin eleccion, automatico: Android sin pantalla tactil, o un navegador de TV.
//
// - `data-tv` en <html>: los estilos de styles/tv.css (foco grande, barra lateral que se abre
//   con el foco, nada que dependa del mouse).
// - Escala de 10 pies: la pagina se arma en 960 px de ancho (el lienzo de 960x540 dp de Android
//   TV) y el WebView o el navegador la agrandan hasta llenar la pantalla (<meta name="viewport"
//   width=960>; en la app, con wide viewport prendido por el plugin). El navegador de la PC ignora
//   ese ancho.
// - La navegacion con las flechas esta en lib/spatial.svelte.ts; el reproductor tiene la suya.
//
// El resultado queda en localStorage: el script de index.html lo aplica al abrir, antes de pintar.
import { liteForTv } from "./perf";
import { isAndroid, shellInvoke } from "./platform";
import { read, write } from "./store";
import { isTizen } from "./tizen";

/** Lo aplicado (lo lee index.html antes de pintar). */
const KEY = "mythos.tv";
/** Web: lo elegido a mano ("on" / "off"); sin nada, automatico. */
const PREF = "mythos.tvPref";

/** Ancho de la pagina en TV (px CSS). */
export const TV_WIDTH = 960;

type Pref = "on" | "off" | null;
const readPref = (): Pref => {
  const v = read(PREF);
  return v === "on" || v === "off" ? v : null;
};

export const tv = $state({
  /** Se maneja con el control remoto. */
  on: read(KEY) === "1",
  /** Web: lo elegido a mano (null = automatico). En la app de Android no se usa. */
  pref: readPref(),
  /** Web: lo que diria la deteccion automatica. */
  auto: false,
});

type Device = { tv: boolean; touch: boolean; lowRam: boolean; ramMb: number };

/** El ancho del meta viewport: en TV, 960 si la pantalla es mas ancha (si es mas angosta, como
 *  viene: achicar no ayuda a leer). */
function viewport(on: boolean) {
  const meta = document.querySelector<HTMLMetaElement>('meta[name="viewport"]');
  if (!meta) return;
  const base = "width=device-width, initial-scale=1.0, viewport-fit=cover";
  const wide = on && Math.max(screen.width, window.innerWidth) > TV_WIDTH;
  meta.content = wide ? `width=${TV_WIDTH}, viewport-fit=cover` : base;
}

function apply(on: boolean) {
  tv.on = on;
  document.documentElement.toggleAttribute("data-tv", on);
  viewport(on);
  write(KEY, on ? "1" : "0");
  if (on) liteForTv();
}

/** Web: un navegador en un TV (Android sin pantalla tactil, o el navegador de un Smart TV). */
function looksLikeTv(): boolean {
  const ua = navigator.userAgent;
  // (AFTxx: los Fire TV de Amazon.)
  if (/Android TV|GoogleTV|BRAVIA|SMART-TV|SmartTV|Tizen|Web0S|HbbTV/i.test(ua) || /; AFT[A-Z]/.test(ua)) return true;
  const touch = navigator.maxTouchPoints > 0 || matchMedia("(any-pointer: coarse)").matches;
  return /Android/i.test(ua) && !touch;
}

/** Web: elegir a mano (null = volver a automatico). */
export function setTvPref(p: Pref) {
  tv.pref = p;
  write(PREF, p);
  apply(p ? p === "on" : tv.auto);
}

/** Al arrancar. App de Android: pregunta al APK que aparato es. Web: lo elegido o automatico. */
export async function initTv() {
  // Samsung (app de la tele o su navegador): siempre TV, como la app de Android. Con el modo TV
  // apagado el control no mueve nada y no se podria volver a prenderlo (queda guardado).
  if (isTizen) {
    tv.auto = true;
    tv.pref = null;
    apply(true);
    return;
  }
  if (!isAndroid) {
    const q = new URLSearchParams(location.search).get("tv");
    if (q !== null) {
      tv.pref = q === "1" || q === "on" ? "on" : q === "0" || q === "off" ? "off" : null;
      write(PREF, tv.pref);
    }
    tv.auto = looksLikeTv();
    apply(tv.pref ? tv.pref === "on" : tv.auto);
    return;
  }
  // Un APK viejo no tiene `device`: queda como celular.
  const d = await shellInvoke<Device>("device").catch(() => null);
  apply(!!d?.tv);
}
