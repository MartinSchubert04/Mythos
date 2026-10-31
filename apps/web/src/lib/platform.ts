// Web, desktop o Android (las dos ultimas con Tauri). En desktop la diferencia visible es la barra
// de titulo; en Android, el boton atras, la pantalla completa nativa y las actualizaciones
// (lib/shell.svelte.ts). Aca esta lo demas: la URL del server y los controles de la ventana.
// @tauri-apps/api se importa solo dentro de Tauri (import dinamico): en la web nunca se descarga.
import { back, route } from "./router.svelte";
import { read, write } from "./store";

/** Corre dentro de Tauri (desktop o Android). */
export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
export const isAndroid = isTauri && /Android/i.test(navigator.userAgent);
export const isDesktop = isTauri && !isAndroid;

/** Android con la interfaz que sirve el server (la pagina local del APK navega ahi cuando el
 *  server contesta: ver apps/mobile/plugin). Es el mismo origen que la API. */
export const isRemoteShell = isAndroid && location.hostname !== "tauri.localhost" && !import.meta.env.DEV;

/** Se puede elegir el server desde la interfaz: la pagina local de desktop o de Android. */
export const pickServer = isTauri && !isRemoteShell;

/** Server por defecto para probar en la PC (VITE_DEFAULT_SERVER en apps/web/.env.local). El repo
 *  y los builds publicos no lo llevan: el login pide la direccion la primera vez y la guarda. */
export const DEFAULT_SERVER = (import.meta.env.VITE_DEFAULT_SERVER ?? "").trim();
const SERVER_KEY = "mythos.server";

/** Server activo en esta sesion, sin guardarlo (para el modo local: el server embebido en
 *  http://127.0.0.1:<puerto>). Gana sobre el guardado; null vuelve a ese. Cambia en caliente: todo
 *  pedido arma su URL con `serverBase()` en el momento (quien lo cambie recarga la vista con
 *  `net.epoch++`). */
let active: string | null = null;
export function setActiveServer(url: string | null) {
  active = url ? url.trim().replace(/\/+$/, "") : null;
}

/** Base de las URLs del server. Web y Android con la interfaz del server: el mismo origen ("").
 *  Pagina local de Tauri: la activa o la guardada; vacia = mismo origen (sirve con `tauri dev` y
 *  el proxy de Vite). */
export function serverBase(): string {
  // En modo mock el "server" vive en esta misma pagina.
  if (!pickServer || (import.meta.env.DEV && import.meta.env.VITE_MOCK === "1")) return "";
  if (active !== null) return active;
  const s = read(SERVER_KEY);
  return (s ?? DEFAULT_SERVER).trim().replace(/\/+$/, "");
}

/** Lo que se muestra en el campo "Server URL" (paginas locales de Tauri). */
export function serverSetting(): string {
  return read(SERVER_KEY) ?? DEFAULT_SERVER;
}

/** En Tauri hace falta saber a que server ir antes de poder entrar. */
export function needsServer(): boolean {
  // En `tauri dev` vacio sirve (mismo origen, con el proxy de Vite); en el build no.
  return pickServer && !(read(SERVER_KEY) ?? DEFAULT_SERVER).trim() && !import.meta.env.DEV;
}

/** "my-server:7700" → "https://my-server:7700" (sin barra final). */
export function normalizeServer(u: string): string {
  const t = u.trim().replace(/\/+$/, "");
  if (!t) return "";
  return /^https?:\/\//i.test(t) ? t : `https://${t}`;
}

/** ¿Hay un Mythos en esa direccion? Devuelve el error para mostrar, o "" si contesta. */
export async function checkServer(url: string): Promise<string> {
  if (!/^https?:\/\/[^/\s]+$/.test(url)) return "That doesn't look like an address. Example: https://my-server.ts.net:7700";
  const ok = () =>
    fetch(url + "/api/health", { signal: AbortSignal.timeout(6000) })
      .then((r) => (r.ok ? r.json() : null))
      .then((j) => !!j?.ok)
      .catch(() => false);
  if (await ok()) return "";
  // Android 17 corta la red local sin permiso (un server con IP de la casa): se pide y se
  // prueba de nuevo. Si ya estaba dado (o no existe), no se repite.
  if (isAndroid && (await shellInvoke<boolean>("request_network").catch(() => true)) === true && (await ok())) return "";
  return `Can't reach a Mythos server at ${url.replace(/^https?:\/\//, "")}. Check the address and that the server is running.`;
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

/** Un comando del plugin de Android (apps/mobile/plugin). */
export async function shellInvoke<T = void>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(`plugin:mythos|${cmd}`, args);
}

/** Android: la pantalla completa es nativa (sin barras del sistema y en horizontal). El
 *  reproductor la pide al abrirse (lib/shell.svelte.ts). Con `backPress` (el boton atras manda un
 *  Esc), salir de pantalla completa sale del reproductor: en el celular no hay un paso intermedio. */
export const androidFs = { on: false, backPress: false };

/** Pantalla completa. En desktop es la ventana (el fullscreen del WebView no la agranda). */
export async function setFullscreen(el: HTMLElement, on: boolean) {
  if (isAndroid) {
    if (!on && androidFs.backPress) {
      if (route.view !== "player") return;
      const [type, id = ""] = route.params;
      back(type === "series" ? `/title/series/${id.split(":")[0]}` : `/title/movie/${id}`);
      return;
    }
    androidFs.on = on;
    await shellInvoke("set_fullscreen", { on }).catch(() => {});
    // El reproductor relee isFullscreen() con cada resize.
    window.dispatchEvent(new Event("resize"));
    return;
  }
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
  if (isAndroid) return androidFs.on;
  if (isDesktop) return (await win()).isFullscreen();
  return !!document.fullscreenElement;
}
