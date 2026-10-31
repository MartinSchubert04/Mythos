// Modo local (docs/local-mode.md): en desktop y en la pagina del APK la app trae adentro el server
// de Mythos (plugin `mythos-local`, apps/local-server). Aca se elige a cual hablarle:
//
// - Auto (por defecto): al server de casa mientras responda. Si un pedido no llega (o da
//   502/503/504) y /api/health tampoco, "Server unavailable, continuing locally" y todo pasa al
//   local (el pedido que fallo se repite ahi). Mientras tanto se mira /api/local/status cada 20 s;
//   cuando el de casa vuelve, POST /api/local/sync y de vuelta a el ("Back on the server · synced").
// - Solo server: como en la web. El local ni arranca.
// - Solo local: siempre el local; el local sincroniza solo cuando alcanza al de casa.
//
// El login en Auto / Solo local pasa por el local (que lo valida contra el de casa y devuelve las
// dos sesiones). La sincronizacion la hace el server local; la interfaz solo elige a quien hablarle.
// En la web (navegador) nada de esto existe: `conn.available` queda en false.
import { api, ApiError, getLocalToken, onUnreachable, remoteToken, setLocalBase, setLocalToken, setToken, useLocal } from "./api";
import { markUp, net } from "./net.svelte";
import { deviceLabel, isRemoteShell, isTauri, remoteBase } from "./platform";
import { prefs } from "./prefs.svelte";
import { read, write } from "./store";
import { toast } from "./toast.svelte";
import type { LocalLogin, LocalStatus, User } from "./types";

export type ConnMode = "auto" | "server" | "local";
type Info = { mode: ConnMode; base_url: string | null; transcode_available: boolean; error: string | null };

/** El modo tambien queda aca (el shell es quien manda): asi al abrir se sabe enseguida a quien
 *  hablarle, sin esperar a que arranque el server local. */
const MODE_KEY = "mythos.connMode";

export const conn = $state({
  /** La app trae el server embebido (desktop, pagina del APK). */
  available: false,
  mode: (read(MODE_KEY) as ConnMode | null) ?? "auto",
  /** A quien le habla la API ahora. */
  on: "server" as "server" | "local",
  /** El server local tiene ffmpeg. */
  transcode: false,
  /** Ultimo /api/local/status (solo mientras se usa el local). */
  status: null as LocalStatus | null,
  /** Hace falta entrar de nuevo (online) para volver a sincronizar o usar el server de casa. */
  needsSignIn: false,
  /** El server local no pudo arrancar. */
  error: "",
  /** Hay sesion en el server local (la cuenta esta en el dispositivo). */
  enrolled: !!getLocalToken(),
});

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(`plugin:mythos-local|${cmd}`, args);
}

/** Donde escucha el local (http://127.0.0.1:<puerto>). */
let localUrl = "";

function apply(i: Info) {
  localUrl = i.base_url ?? "";
  conn.available = true;
  conn.mode = i.mode;
  conn.transcode = i.transcode_available;
  conn.error = i.error ?? "";
  setLocalBase(localUrl);
  write(MODE_KEY, i.mode);
}

let infoP: Promise<boolean> | null = null;
/** Pregunta al shell el modo y donde esta el local (espera si esta arrancando). `false` si esta
 *  app no lo trae (web, interfaz del server en Android, un APK viejo). */
export function loadLocal(): Promise<boolean> {
  infoP ??= (async () => {
    if (!isTauri || isRemoteShell) return false;
    try {
      apply(await invoke<Info>("info"));
      return true;
    } catch {
      return false;
    }
  })();
  return infoP;
}

/** Con este modo el login pasa por el local. */
export async function viaLocal(): Promise<boolean> {
  return (await loadLocal()) && conn.mode !== "server";
}

// ---- cambiar de server

function goLocal(notice: boolean) {
  useLocal(true);
  poll(0);
  if (conn.on === "local") return;
  conn.on = "local";
  // El aviso de "no se llega al server" ya no corresponde: se sigue en el local.
  markUp();
  if (notice && prefs.localNotice) toast("Server unavailable, continuing locally");
}

function goRemote(notice: boolean) {
  useLocal(false);
  if (conn.on === "server") return;
  conn.on = "server";
  if (notice && prefs.localNotice) toast("Back on the server · synced");
  // Las vistas se recargan con los datos del server (el reproductor no: sigue donde arranco).
  net.epoch++;
}

/** El server de casa contesta /api/health. */
async function remoteAlive(): Promise<boolean> {
  try {
    const r = await fetch(remoteBase() + "/api/health", { signal: AbortSignal.timeout(4000), cache: "no-store" });
    return r.ok;
  } catch {
    return false;
  }
}

let switching: Promise<boolean> | null = null;
// Un pedido al server de casa no llego: ¿pasar al local? (varios a la vez comparten la decision).
onUnreachable(() => {
  if (conn.mode !== "auto" || !isTauri || isRemoteShell) return Promise.resolve(false);
  if (conn.on === "local") return Promise.resolve(true);
  switching ??= (async () => {
    if (!(await loadLocal()) || !getLocalToken() || conn.error) return false;
    if (await remoteAlive()) return false;
    goLocal(true);
    return true;
  })().finally(() => (switching = null));
  return switching;
});

// ---- mientras se usa el local: estado y vuelta al server

let timer = 0;
let syncing = false;
function poll(ms = 20_000) {
  clearTimeout(timer);
  timer = window.setTimeout(tick, ms);
}

async function tick() {
  if (conn.on !== "local" || !getLocalToken()) return;
  try {
    conn.status = await api.localStatus();
    const s = conn.status;
    conn.needsSignIn = s.remote_reachable === true && (!s.remote_signed_in || (conn.mode === "auto" && !remoteToken()));
    if (conn.mode === "auto" && s.remote_reachable && !conn.needsSignIn && !syncing) await back();
  } catch (e) {
    // El local dice que su sesion ya no sirve: al login.
    if (e instanceof ApiError && e.status === 401) return expire();
  }
  if (conn.on === "local") poll(conn.mode === "auto" ? 20_000 : 30_000);
}

/** El server de casa volvio: subir lo hecho mientras tanto, bajar lo nuevo y volver a el. */
async function back() {
  syncing = true;
  try {
    await api.localSync(remoteToken());
    // Por las dudas: la sesion del server de casa tiene que seguir sirviendo.
    if (!(await remoteAlive())) return;
    goRemote(true);
    conn.status = null;
  } catch (e) {
    if (e instanceof ApiError && e.status === 401) conn.needsSignIn = true;
  } finally {
    syncing = false;
  }
}

let onExpire: () => void = () => {};
/** Lo registra session.svelte.ts: la sesion local vencio (se vuelve al login). */
export function onLocalExpired(f: () => void) {
  onExpire = f;
}
function expire() {
  setLocalToken(null);
  conn.enrolled = false;
  onExpire();
}

/** Al abrir (session.start): con que server arrancar. */
export async function startConn() {
  const hint = conn.mode;
  // Solo local, o sin sesion del server de casa (se entro sin conexion): hace falta el local ya.
  if (hint === "local" || (hint === "auto" && !remoteToken() && getLocalToken())) {
    if (!(await loadLocal())) return;
  } else {
    // Auto con sesion del server: se arranca contra el; el local se averigua de fondo (para
    // pasar a el sin esperar si el server no contesta).
    loadLocal();
    return;
  }
  if (conn.mode === "local" || (conn.mode === "auto" && !remoteToken() && getLocalToken())) goLocal(false);
}

// ---- login y logout

/** Login por el local (Auto / Solo local). Guarda las dos sesiones y elige a quien hablarle. */
export async function loginLocal(username: string, password: string): Promise<{ user: User; r: LocalLogin }> {
  if (conn.error) throw new Error(`The local server didn't start: ${conn.error}`);
  const r = await api.localLogin({ username, password, device: deviceLabel(), remote_url: remoteBase() || location.origin });
  // Una sesion del server de casa vieja (de antes) queda de mas: se cierra.
  const old = remoteToken();
  if (r.remote && old && old !== r.remote.token) api.logoutAt(remoteBase(), old).catch(() => {});
  setLocalToken(r.token);
  conn.enrolled = true;
  if (r.remote) setToken(r.remote.token);
  conn.needsSignIn = false;
  if (conn.mode === "local" || r.offline || !r.remote) goLocal(r.offline && conn.mode === "auto");
  else goRemote(false);
  return { user: r.user, r };
}

/** Cierra las dos sesiones (la del server de casa y la del dispositivo). */
export async function logoutAll() {
  const remote = remoteToken();
  const local = getLocalToken();
  await Promise.all([
    remote ? api.logoutAt(remoteBase(), remote).catch(() => {}) : null,
    local && (await loadLocal()) && localUrl ? api.logoutAt(localUrl, local).catch(() => {}) : null,
  ]);
  setToken(null);
  setLocalToken(null);
  conn.enrolled = false;
  clearTimeout(timer);
  conn.status = null;
  conn.needsSignIn = false;
  // Para el proximo login: Solo local sigue en el local; Auto empieza por el server.
  if (conn.mode !== "local") {
    useLocal(false);
    conn.on = "server";
  }
}

/** Cambia el modo (lo guarda el shell). Devuelve si hace falta entrar de nuevo (para enrolar la
 *  cuenta en el dispositivo, o para tener sesion en el server de casa). */
export async function setMode(mode: ConnMode): Promise<"ok" | "signin"> {
  apply(await invoke<Info>("set_mode", { mode }));
  if (mode === "server") {
    clearTimeout(timer);
    conn.status = null;
    if (conn.on === "local") goRemote(false);
    return remoteToken() ? "ok" : "signin";
  }
  if (!getLocalToken()) return "signin";
  if (mode === "local") goLocal(false);
  return "ok";
}

/** Para el indicador y Settings: el texto de ayuda del estado actual. */
export function connDetail(): string {
  if (conn.on === "server")
    return conn.mode === "auto" ? "Connected to your server. Switches to this device if the server goes down." : "Connected to your server.";
  const s = conn.status;
  const parts = ["Running on this device"];
  if (conn.needsSignIn) parts.push("sign in again to sync with your server");
  else if (s) {
    if (s.pending_changes > 0) parts.push(`${s.pending_changes} ${s.pending_changes === 1 ? "change" : "changes"} waiting to sync`);
    else if (s.last_sync_at) parts.push("everything synced");
    if (s.remote_reachable === false) parts.push("server unreachable");
  }
  return parts.join(" · ");
}

/** Estado del local ahora (Settings lo muestra aunque se este usando el server). */
export async function refreshStatus() {
  if (!(await loadLocal()) || !getLocalToken()) return;
  try {
    conn.status = await api.localStatus();
    conn.needsSignIn = conn.status.remote_reachable === true && !conn.status.remote_signed_in;
  } catch {}
}

/** "Sync now": sube y baja ya (y, si el server de casa volvio, vuelve a el en Auto). */
export async function syncNow() {
  const r = await api.localSync(remoteToken());
  await refreshStatus();
  if (conn.mode === "auto" && conn.on === "local" && remoteToken() && (await remoteAlive())) goRemote(true);
  return r;
}
