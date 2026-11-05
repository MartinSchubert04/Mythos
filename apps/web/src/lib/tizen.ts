// Samsung (Tizen): la interfaz abierta por la app de la tele (apps/tizen, docs/tizen.md) o por el
// navegador de la tele. El modo TV lo prende tv.svelte.ts por el user agent ("Tizen").
//
// - El control manda algunas teclas con codigos propios (atras = 10009, "XF86Back"): `keyName`
//   les da el nombre estandar (BrowserBack, MediaPlayPause...) que ya entienden
//   lib/spatial.svelte.ts y el reproductor.
// - Las teclas multimedia solo llegan si se registran (`tizen.tvinputdevice`, solo dentro de la
//   app; en el navegador de la tele no existe y no pasa nada).
// - Atras en el inicio cierra la app: con `tizen.application` si esta pagina lo tiene; si no,
//   vuelve a la pagina local de la app (history.back), que se cierra sola al volver.

type TizenApi = {
  application?: { getCurrentApplication(): { exit(): void } };
  tvinputdevice?: { getSupportedKeys(): { name: string }[]; registerKey(name: string): void };
};
const api = (): TizenApi | undefined => (window as unknown as { tizen?: TizenApi }).tizen;

export const isTizen = typeof navigator !== "undefined" && /Tizen/i.test(navigator.userAgent);

const CODES: Record<number, string> = {
  10009: "BrowserBack",
  10252: "MediaPlayPause",
  415: "MediaPlay",
  19: "MediaPause",
  413: "MediaStop",
  417: "MediaFastForward",
  412: "MediaRewind",
};

/** El nombre estandar de la tecla (en Tizen, por el codigo; en el resto, `e.key`). */
export function keyName(e: KeyboardEvent): string {
  return (isTizen && CODES[e.keyCode]) || e.key;
}

const MEDIA = ["MediaPlayPause", "MediaPlay", "MediaPause", "MediaStop", "MediaFastForward", "MediaRewind"];

export function initTizen() {
  const input = isTizen ? api()?.tvinputdevice : undefined;
  if (!input) return;
  let supported: Set<string>;
  try {
    supported = new Set(input.getSupportedKeys().map((k) => k.name));
  } catch {
    return;
  }
  for (const k of MEDIA) {
    if (!supported.has(k)) continue;
    try {
      input.registerKey(k);
    } catch {
      // Ya registrada por la pagina local, o no se puede: sigue andando lo demas.
    }
  }
}

/** Atras en el inicio: cerrar la app. */
export function exitTizenApp() {
  try {
    const app = api()?.application;
    if (app) return app.getCurrentApplication().exit();
  } catch {
    // Sin permiso en esta pagina: abajo.
  }
  history.back();
}
