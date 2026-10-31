// Web o desktop (Tauri). La unica diferencia visible es la barra de titulo; aca esta lo demas:
// la URL del server y los controles de la ventana. @tauri-apps/api se importa solo en desktop
// (import dinamico): en la web nunca se descarga.
import { read, write } from "./store";

export const isDesktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** El server por defecto del desktop sale del build (VITE_DEFAULT_SERVER en apps/web/.env.local):
 *  el repo es publico y la direccion del tailnet no va en el codigo. Sin eso, el login la pide. */
export const DEFAULT_SERVER = (import.meta.env.VITE_DEFAULT_SERVER ?? "").trim();
const SERVER_KEY = "mythos.server";

/** Base de las URLs del server. Web: el mismo origen (""). Desktop: la guardada; vacia = mismo
 *  origen (sirve con `tauri dev` y el proxy de Vite). */
export function serverBase(): string {
  // En modo mock el "server" vive en esta misma pagina.
  if (!isDesktop || (import.meta.env.DEV && import.meta.env.VITE_MOCK === "1")) return "";
  const s = read(SERVER_KEY);
  return (s ?? DEFAULT_SERVER).trim().replace(/\/+$/, "");
}

/** Lo que se muestra en el campo "Server" (desktop). */
export function serverSetting(): string {
  return read(SERVER_KEY) ?? DEFAULT_SERVER;
}

/** En desktop hace falta saber a que server ir antes de poder entrar. */
export function needsServer(): boolean {
  // En `tauri dev` vacio sirve (mismo origen, con el proxy de Vite); en el build no.
  return isDesktop && !(read(SERVER_KEY) ?? DEFAULT_SERVER).trim() && !import.meta.env.DEV;
}

export function setServer(url: string) {
  write(SERVER_KEY, url.trim());
}

type TauriWindow = import("@tauri-apps/api/window").Window;
let winP: Promise<TauriWindow> | null = null;
function win(): Promise<TauriWindow> {
  winP ??= import("@tauri-apps/api/window").then((m) => m.getCurrentWindow());
  return winP;
}

export const windowCtl = {
  minimize: () => win().then((w) => w.minimize()),
  toggleMaximize: () => win().then((w) => w.toggleMaximize()),
  close: () => win().then((w) => w.close()),
  isMaximized: () => win().then((w) => w.isMaximized()),
  /** Avisa cuando cambia el tamaño; devuelve con que dejar de escuchar. */
  onResized: async (f: () => void) => (await win()).onResized(f),
};

/** Pantalla completa. En desktop es la ventana (el fullscreen del WebView no la agranda). */
export async function setFullscreen(el: HTMLElement, on: boolean) {
  if (isDesktop) {
    await (await win()).setFullscreen(on);
    return;
  }
  try {
    if (on && !document.fullscreenElement) await el.requestFullscreen({ navigationUI: "hide" });
    else if (!on && document.fullscreenElement) await document.exitFullscreen();
  } catch {}
}

export async function isFullscreen(): Promise<boolean> {
  if (isDesktop) return (await win()).isFullscreen();
  return !!document.fullscreenElement;
}
