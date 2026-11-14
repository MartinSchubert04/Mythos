// Equipos flojos (TV boxes de 1–2 GB, celulares viejos): se bajan los efectos caros sin cambiar el
// diseño. Con `data-lite` en <html> el CSS saca los desenfoques (backdrop-filter), el zoom lento del
// hero, la aparición escalonada de las tarjetas y las sombras animadas (ver base.css y themes.css),
// y las imágenes de fondo se piden más chicas (lib/images.ts).
//
// En TV (lib/tv.svelte.ts) siempre: una tele Samsung (Tizen) o un TV box dibujan la pagina al doble
// (viewport de 960 px en 1920) con CPU y GPU de tele; medido en una UN50DU7000, con los efectos el
// raster se comia mas de un nucleo entero al moverse por las filas (docs/tizen.md, "Performance").
//
// Se puede forzar desde la consola: localStorage["mythos.lite"] = "1" (siempre) o "0" (nunca).
import { isAndroid } from "./platform";
import { read } from "./store";

function detect(): boolean {
  const forced = read("mythos.lite");
  if (forced === "1") return true;
  if (forced === "0") return false;
  if (typeof navigator === "undefined") return false;
  // TV ya aplicado (lo guarda lib/tv.svelte.ts; index.html pone data-tv antes de pintar).
  if (read("mythos.tv") === "1") return true;
  // deviceMemory: Chrome, WebView y WebView2 (GB redondeados, máximo 8); Firefox/Safari no lo dan.
  const mem = (navigator as Navigator & { deviceMemory?: number }).deviceMemory;
  const cores = navigator.hardwareConcurrency || 8;
  return (mem !== undefined && mem <= 2) || cores <= 2 || (isAndroid && cores <= 4);
}

export let lite = detect();

/** Marca <html> antes del primer render (main.ts). */
export function applyLite() {
  if (lite) document.documentElement.setAttribute("data-lite", "");
}

/** El modo TV se prendio despues de arrancar (la primera vez, o desde Settings): liviano tambien,
 *  salvo que se haya forzado "0". Apagarlo no lo saca hasta recargar (no hace falta). */
export function liteForTv() {
  if (lite || read("mythos.lite") === "0") return;
  lite = true;
  applyLite();
}
