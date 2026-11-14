// Web, desktop o Android (las dos ultimas con Tauri). En desktop la diferencia visible es la barra
// de titulo; en Android, el boton atras, la pantalla completa nativa y las actualizaciones
// (lib/shell.svelte.ts). Aca esta lo demas: las direcciones del server y los controles de la ventana.
// @tauri-apps/api se importa solo dentro de Tauri (import dinamico): en la web nunca se descarga.
import { t } from "./i18n.svelte";
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
/** Una sola direccion (lo que guardaban las versiones anteriores). Se sigue escribiendo con la que
 *  se usa, por si se vuelve a una version vieja. */
const SERVER_KEY = "mythos.server";
/** Las direcciones del server de casa en este dispositivo (lib/servers.svelte.ts). */
const SERVERS_KEY = "mythos.servers";

/** Una direccion del server de casa. Todas las de la lista llevan al MISMO server (Tailscale, la
 *  red de casa...): misma base, mismas sesiones. */
export type ServerAddr = {
  url: string;
  /** Nombre que le puso el usuario (vacio = se deduce de la direccion, `addrLabel`). */
  label?: string;
  /** `server_id` que contesto en esa direccion (GET /api/health), si alguna vez contesto. */
  id?: string;
};
export type SavedServers = {
  list: ServerAddr[];
  /** La preferida: se usa si contesta, aunque otra sea mas rapida. */
  preferred?: string | null;
  /** La ultima que se uso (con esa se arranca mientras se prueban las demas). */
  active?: string | null;
};

const trimUrl = (u: string) => u.trim().replace(/\/+$/, "");
const activeOf = (s: SavedServers) => (s.active && s.list.some((a) => a.url === s.active) ? s.active : (s.list[0]?.url ?? ""));

/** Lo guardado (la direccion unica de antes queda como lista de una). */
export function loadServers(): SavedServers {
  try {
    const s = JSON.parse(read(SERVERS_KEY) ?? "null") as SavedServers | null;
    if (s && Array.isArray(s.list)) return { ...s, list: s.list.filter((a) => a && typeof a.url === "string" && a.url) };
  } catch {}
  const one = trimUrl(read(SERVER_KEY) ?? "");
  return { list: one ? [{ url: one }] : [] };
}

/** La direccion del server de casa que se esta usando (la elige lib/servers.svelte.ts). */
let remote = activeOf(loadServers());

export function saveServers(s: SavedServers) {
  remote = activeOf(s);
  write(SERVERS_KEY, JSON.stringify(s));
  write(SERVER_KEY, remote || null);
}

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
  if (active !== null && pickServer) return active;
  return remoteBase();
}

/** El server de casa (el guardado), aunque la API le este hablando al local. */
export function remoteBase(): string {
  // En modo mock el "server" vive en esta misma pagina.
  if (!pickServer || (import.meta.env.DEV && import.meta.env.VITE_MOCK === "1")) return "";
  return trimUrl(remote || DEFAULT_SERVER);
}

/** En Tauri hace falta saber a que server ir antes de poder entrar. */
export function needsServer(): boolean {
  // En `tauri dev` vacio sirve (mismo origen, con el proxy de Vite); en el build no.
  return pickServer && !(remote || DEFAULT_SERVER).trim() && !import.meta.env.DEV;
}

/** Que tipo de direccion es: "tailscale" (*.ts.net o 100.64.0.0/10), "home" (IP privada, *.local,
 *  *.lan, *.home), "self" (loopback); si no, el host (o la URL, si no se entiende). */
export function addrKind(url: string): "tailscale" | "home" | "self" | { host: string } {
  let host = "";
  try {
    host = new URL(url).hostname.toLowerCase().replace(/^\[|\]$/g, "");
  } catch {
    return { host: url };
  }
  if (host.endsWith(".ts.net")) return "tailscale";
  const ip = /^(\d+)\.(\d+)\.\d+\.\d+$/.exec(host);
  const [a, b] = ip ? [Number(ip[1]), Number(ip[2])] : [-1, -1];
  if (a === 100 && b >= 64 && b < 128) return "tailscale";
  if (a === 127 || host === "localhost" || host === "::1") return "self";
  if (a === 10 || (a === 172 && b >= 16 && b < 32) || (a === 192 && b === 168) || /\.(local|lan|home|internal)$/.test(host) || /^f[cd][0-9a-f]{2}:/.test(host))
    return "home";
  return { host };
}

/** Nombre deducido de una direccion, para mostrar: "Tailscale", "Red de casa", "Esta compu"; si
 *  no, el host. */
export function addrLabel(url: string): string {
  const k = addrKind(url);
  if (k === "tailscale") return "Tailscale";
  if (k === "home") return t("account.addr.home");
  if (k === "self") return t("account.addr.self");
  return k.host;
}

/** Etiqueta de este dispositivo para la lista de sesiones (Settings → Account): "Mythos Desktop ·
 *  Windows", "Mythos Android · Pixel 8", "Chrome on Windows". Va en el login (≤ 64 caracteres). */
export function deviceLabel(): string {
  const ua = navigator.userAgent;
  const os = /Windows/i.test(ua) ? "Windows" : /Android/i.test(ua) ? "Android" : /iPhone|iPad/i.test(ua) ? "iOS" : /Mac OS/i.test(ua) ? "macOS" : /Linux/i.test(ua) ? "Linux" : "";
  if (isAndroid) {
    // "…(Linux; Android 14; Pixel 8 Build/…; wv)" → "Pixel 8"
    const model = /Android [\d.]+; ([^;)]+?)(?: Build\/|;|\))/.exec(ua)?.[1]?.trim();
    return (model && model !== "K" ? `Mythos Android · ${model}` : "Mythos Android").slice(0, 64);
  }
  if (isDesktop) return `Mythos Desktop · ${os || "PC"}`;
  const browser = /Edg\//.test(ua) ? "Edge" : /OPR\//.test(ua) ? "Opera" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : "Browser";
  return os ? `${browser} on ${os}` : browser;
}

/** Puerto del server cuando no se escribe ninguno (el de deploy/, CLAUDE.md "Ports"). */
export const SERVER_PORT = 7700;

/** Lo que se escribe en el login → URL del server, sin barra final. Sin puerto va `:7700`. Sin
 *  `http(s)://` va `http://` para una IP o un nombre de la red de casa (sin punto, .local, .lan) y
 *  `https://` para el resto (`*.ts.net`: Tailscale lo sirve con certificado).
 *  "192.168.1.20" → "http://192.168.1.20:7700"; "mi-server.ts.net" → "https://mi-server.ts.net:7700". */
export function normalizeServer(u: string): string {
  const s = u.trim().replace(/\/+$/, "");
  if (!s) return "";
  const [, scheme = "", auth = "", rest = ""] = /^(https?:\/\/)?([^/]*)(.*)$/i.exec(s) ?? [];
  if (!auth) return s;
  const host = auth.replace(/:\d+$/, "");
  // "https:" (escribio solo el comienzo) o un IPv6 sin corchetes: queda como esta y no pasa isAddress.
  if (!host || /:/.test(host.replace(/^\[.*\]$/, ""))) return s;
  const port = host === auth ? `:${SERVER_PORT}` : auth.slice(host.length);
  return (scheme.toLowerCase() || (plainHttp(host) ? "http://" : "https://")) + host + port + rest;
}

/** IP, IPv6 o nombre de la red de casa: ahi no hay certificado, va por http. */
function plainHttp(host: string): boolean {
  const h = host.replace(/^\[|\]$/g, "").toLowerCase();
  return !h.includes(".") || h.includes(":") || /^\d+\.\d+\.\d+\.\d+$/.test(h) || /\.(local|lan|home|internal)$/.test(h);
}

/** Lo que contesto GET /api/health en una direccion, y cuanto tardo. */
export type Probe = { ms: number; id: string | null; mode: string };

/** GET /api/health con tiempo maximo. `null` si ahi no hay un Mythos (o no contesto a tiempo). */
export async function probeServer(url: string, timeoutMs = 2500): Promise<Probe | null> {
  const t0 = performance.now();
  try {
    const r = await fetch(url + "/api/health", { signal: AbortSignal.timeout(timeoutMs), cache: "no-store" });
    if (!r.ok) return null;
    const j = await r.json();
    if (!j?.ok) return null;
    const id = typeof j.server_id === "string" && j.server_id ? j.server_id : null;
    return { ms: Math.max(1, Math.round(performance.now() - t0)), id, mode: typeof j.mode === "string" ? j.mode : "server" };
  } catch {
    return null;
  }
}

const hostOf = (url: string) => url.replace(/^https?:\/\//, "");
/** Parece una direccion de server (http(s)://host[:puerto], sin camino). */
export const isAddress = (url: string) => /^https?:\/\/[^/\s]+$/.test(url);

/** ¿Hay un Mythos en esa direccion? `error` vacio si contesta (con lo que contesto en `probe`);
 *  `down`: no contesto (los otros errores son una direccion mal escrita o el server de una app). */
export async function checkServer(url: string, timeoutMs = 6000): Promise<{ probe: Probe | null; error: string; down: boolean }> {
  if (!isAddress(url)) return { probe: null, error: t("account.addr.notAddress"), down: false };
  let p = await probeServer(url, timeoutMs);
  // Android 17 corta la red local sin permiso (un server con IP de la casa): se pide y se
  // prueba de nuevo. Si ya estaba dado (o no existe), no se repite.
  if (!p && isAndroid && (await shellInvoke<boolean>("request_network").catch(() => true)) === true) p = await probeServer(url, timeoutMs);
  if (p?.mode === "local") return { probe: null, error: t("account.addr.appServer", { host: hostOf(url) }), down: false };
  if (p) return { probe: p, error: "", down: false };
  return { probe: null, error: t("account.addr.unreachable", { host: hostOf(url) }), down: true };
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
