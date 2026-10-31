// Equipos flojos (TV boxes de 1–2 GB, celulares viejos): se bajan los efectos caros sin cambiar el
// diseño. Con `data-lite` en <html> el CSS saca los desenfoques (backdrop-filter), el zoom lento del
// hero, la aparición escalonada de las tarjetas y las sombras animadas (ver base.css y themes.css),
// y las imágenes de fondo se piden más chicas (lib/images.ts).
//
// Se puede forzar desde la consola: localStorage["mythos.lite"] = "1" (siempre) o "0" (nunca).
import { isAndroid } from "./platform";
import { read } from "./store";

function detect(): boolean {
  const forced = read("mythos.lite");
  if (forced === "1") return true;
  if (forced === "0") return false;
  if (typeof navigator === "undefined") return false;
  // deviceMemory: Chrome, WebView y WebView2 (GB redondeados, máximo 8); Firefox/Safari no lo dan.
  const mem = (navigator as Navigator & { deviceMemory?: number }).deviceMemory;
  const cores = navigator.hardwareConcurrency || 8;
  return (mem !== undefined && mem <= 2) || cores <= 2 || (isAndroid && cores <= 4);
}

export const lite = detect();

/** Marca <html> antes del primer render (main.ts). */
export function applyLite() {
  if (lite) document.documentElement.setAttribute("data-lite", "");
}
