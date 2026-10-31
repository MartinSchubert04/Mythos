// Cliente tipado del contrato (docs/API.md). El token vive en localStorage; un 401 lleva al login.
import { detectCaps } from "./caps";
import { markDown, markUp } from "./net.svelte";
import { deviceLabel, serverBase, setActiveServer } from "./platform";
import { read, write } from "./store";
import type {
  Avatar,
  AdminStats,
  AuditRow,
  ClientCaps,
  ExternalSub,
  LogFilters,
  LogRow,
  MediaStatus,
  Metrics,
  MetricsRange,
  Page,
  SessionInfo,
  MediaType,
  Meta,
  MetaPreview,
  PlayInfo,
  Progress,
  Settings,
  SettingsUpdate,
  Stream,
  TorrentStat,
  User,
  LocalLogin,
  LocalStatus,
} from "./types";

// Dos sesiones (modo local, docs/local-mode.md): la del server de casa (`token`) y, en desktop y
// Android con Auto / Solo local, la del server embebido (`localToken`). Se usa la del server al que
// se le esta hablando (`useLocal`, lib/local.svelte.ts). En la web solo existe la primera.
const TOKEN_KEY = "mythos.token";
const LOCAL_TOKEN_KEY = "mythos.localToken";
let token = read(TOKEN_KEY);
let localToken = read(LOCAL_TOKEN_KEY);
let onLocal = false;
let localBase = "";

/** El token del server activo. */
export function getToken() {
  return onLocal ? localToken : token;
}
/** El del server de casa. */
export function remoteToken() {
  return token;
}
export function setToken(t: string | null) {
  token = t;
  write(TOKEN_KEY, t);
}
export function getLocalToken() {
  return localToken;
}
export function setLocalToken(t: string | null) {
  localToken = t;
  write(LOCAL_TOKEN_KEY, t);
}
/** Donde escucha el server embebido ("" = no hay). */
export function setLocalBase(url: string) {
  localBase = url.replace(/\/+$/, "");
}
/** Pasa toda la API al server embebido (`true`) o de vuelta al de casa. Las sesiones de video ya
 *  abiertas siguen en el server donde se abrieron (ver `pins`). */
export function useLocal(on: boolean) {
  onLocal = on && !!localBase;
  setActiveServer(onLocal ? localBase : null);
}

/** Lo registra lib/local.svelte.ts: un pedido al server de casa no llego. Devuelve `true` si ya se
 *  paso al server local (y el pedido se repite ahi). */
let unreachable: () => Promise<boolean> = async () => false;
export function onUnreachable(f: () => Promise<boolean>) {
  unreachable = f;
}

/** Lo llama session.svelte.ts: un 401 cierra la sesion y muestra el login. */
let unauthorized: () => void = () => {};
export function onUnauthorized(f: () => void) {
  unauthorized = f;
}

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

/** Texto para mostrar de cualquier error. */
export function errText(e: unknown): string {
  if (e instanceof ApiError) return e.message;
  if (e instanceof DOMException && e.name === "AbortError") return "Cancelled";
  if (e instanceof TypeError) return "Can't reach the server";
  return e instanceof Error ? e.message : String(e);
}

/** `to`: un server puntual ({base, token}) en vez del activo, sin aviso de caida ni paso al local
 *  (las rutas /api/local/*, el logout de cada sesion, el estado de una sesion de video). `used`
 *  se llena con el server que contesto (el pedido puede repetirse en el local). */
type Target = { base: string; token: string | null };
type Opts = { signal?: AbortSignal; keepalive?: boolean; noAuthRedirect?: boolean; to?: Target; used?: Partial<Target>; retried?: boolean };

async function req<T>(method: string, path: string, body?: unknown, o: Opts = {}): Promise<T> {
  const base = o.to ? o.to.base : serverBase();
  const tok = o.to ? o.to.token : getToken();
  const wasLocal = onLocal;
  const headers: Record<string, string> = {};
  if (tok) headers.Authorization = `Bearer ${tok}`;
  // Un formulario (una imagen) va tal cual, con el Content-Type (y el boundary) que pone el
  // navegador; lo demas, JSON.
  const raw = body instanceof FormData;
  if (body !== undefined && !raw) headers["Content-Type"] = "application/json";
  // Server de casa caido con Auto: se pasa al local y el mismo pedido se repite ahi.
  const retry = async () => !o.to && !o.retried && !wasLocal && !o.signal?.aborted && (await unreachable());
  let r: Response;
  try {
    r = await fetch(base + path, {
      method,
      headers,
      body: body === undefined ? undefined : raw ? (body as FormData) : JSON.stringify(body),
      signal: o.signal,
      keepalive: o.keepalive,
    });
  } catch (e) {
    if (e instanceof TypeError && (await retry())) return req<T>(method, path, body, { ...o, retried: true });
    // Sin respuesta (server caido, sin red): aviso arriba y reconexion automatica.
    if (e instanceof TypeError && !o.to) markDown();
    throw e;
  }
  // 502/503/504 de un proxy (Vite, tailscale serve) = el server no esta; cualquier otra
  // respuesta, aunque sea un error, quiere decir que hay conexion.
  if ([502, 503, 504].includes(r.status) && !path.startsWith("/api/play")) {
    if (await retry()) return req<T>(method, path, body, { ...o, retried: true });
    if (!o.to) markDown();
  } else if (!o.to) markUp();
  if (o.used) Object.assign(o.used, { base, token: tok });
  if (r.status === 401 && !o.noAuthRedirect && !o.to) {
    // La sesion del server que contesto ya no sirve: al login (que renueva las dos).
    if (wasLocal) setLocalToken(null);
    else setToken(null);
    unauthorized();
  }
  if (!r.ok) {
    let msg = `Error ${r.status}`;
    try {
      const j = await r.json();
      if (j?.error) msg = j.error;
    } catch {}
    throw new ApiError(r.status, msg);
  }
  if (r.status === 204 || r.status === 202) return undefined as T;
  const text = await r.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

/** Cada sesion de video queda en el server (y con el token) donde se abrio: si a mitad de una
 *  pelicula la API pasa al local o vuelve al de casa, el video, sus subtitulos, su HLS y su
 *  /status siguen yendo al mismo lugar. */
const pins = new Map<string, Target>();
function pinOf(path: string): Target | undefined {
  const m = /\/api\/media\/([^/?#]+)\//.exec(path);
  return m ? pins.get(m[1]) : undefined;
}
/** La sesion de video se abrio en otro server que el que usa la API ahora. */
export function movedAway(session: string): boolean {
  const p = pins.get(session);
  return !!p && p.base !== serverBase();
}
/** Opciones de `req` para una sesion de video: su server, o el activo si no se conoce. */
const pinned = (session: string): Opts => {
  const to = pins.get(session);
  return to ? { to } : {};
};

/** URL de media lista para <video>, hls.js o fetch: con la base del server y el token en ?t=.
 *  Las absolutas de otro lado (y data:/blob:) quedan como estan. */
export function mediaUrl(path: string, extra?: Record<string, string | number | undefined>): string {
  if (/^(data|blob):/.test(path)) return path;
  const pin = pinOf(path);
  const base = pin ? pin.base : serverBase();
  const tok = pin ? pin.token : getToken();
  const abs = /^https?:\/\//.test(path);
  const ours = !abs || (base && path.startsWith(base)) || path.startsWith(location.origin);
  const u = new URL(abs ? path : (base || location.origin) + path);
  if (ours && tok && !u.searchParams.has("t")) u.searchParams.set("t", tok);
  for (const [k, v] of Object.entries(extra ?? {})) if (v !== undefined) u.searchParams.set(k, String(v));
  return u.toString();
}

/** Modo mock (solo desarrollo): las imagenes de avatares no pasan por fetch. */
let mockAvatar: ((id: number) => string) | null = null;
export function setMockAvatar(f: (id: number) => string) {
  mockAvatar = f;
}

/** La imagen de un avatar para <img> (con el token en ?t=). `to`: el server y la sesion de otro
 *  perfil (el selector de perfiles de la TV). */
export function avatarUrl(id: number, to?: { base: string; token: string | null }): string {
  if (mockAvatar) return mockAvatar(id);
  const base = (to ? to.base : serverBase()) || location.origin;
  const tok = to ? to.token : getToken();
  const u = new URL(`${base}/api/media/avatars/${id}`);
  if (tok) u.searchParams.set("t", tok);
  return u.toString();
}

/** Agrega el token a una URL que arma hls.js (segmentos relativos a la playlist). */
export function withToken(url: string): string {
  const pin = pinOf(url);
  const tok = pin ? pin.token : getToken();
  if (!tok || /[?&]t=/.test(url)) return url;
  const base = (pin ? pin.base : serverBase()) || location.origin;
  if (/^https?:\/\//.test(url) && !url.startsWith(base) && !url.startsWith(location.origin)) return url;
  return url + (url.includes("?") ? "&" : "?") + "t=" + encodeURIComponent(tok);
}

// ---- cache corta en memoria: volver atras o pasar el mouse no repite pedidos

const cache = new Map<string, { at: number; p: Promise<unknown> }>();
function cached<T>(key: string, ttlMs: number, f: () => Promise<T>): Promise<T> {
  const hit = cache.get(key);
  if (hit && Date.now() - hit.at < ttlMs) return hit.p as Promise<T>;
  const p = f();
  cache.set(key, { at: Date.now(), p });
  p.catch(() => cache.delete(key));
  return p;
}
export function dropCache(prefix: string) {
  for (const k of cache.keys()) if (k.startsWith(prefix)) cache.delete(k);
}

const MIN = 60_000;
const q = (o: Record<string, string | number | undefined>) => {
  const s = new URLSearchParams();
  for (const [k, v] of Object.entries(o)) if (v !== undefined && v !== "") s.set(k, String(v));
  const t = s.toString();
  return t ? `?${t}` : "";
};
const enc = encodeURIComponent;
/** ClientCaps como lista ("h264,aac,mkv"), el formato corto que acepta /api/streams. */
const capsList = () =>
  Object.entries(detectCaps())
    .filter(([, v]) => v)
    .map(([k]) => k)
    .join(",");

export type PlayRequest = {
  stream_key: string;
  info_hash?: string | null;
  file_idx?: number | null;
  url?: string | null;
  caps: ClientCaps;
  meta_id?: string;
};

export const api = {
  health: () => req<{ ok: boolean; version: string }>("GET", "/api/health", undefined, { noAuthRedirect: true }),
  /** `persistent`: sesion que no vence (la app de TV, docs/API.md). */
  login: (username: string, password: string, persistent = false) =>
    req<{ token: string; user: User }>(
      "POST",
      "/api/auth/login",
      { username, password, device: deviceLabel(), ...(persistent ? { persistent: true } : {}) },
      { noAuthRedirect: true },
    ),
  logout: () => req<void>("POST", "/api/auth/logout", undefined, { noAuthRedirect: true }),
  me: () => req<User>("GET", "/api/me"),
  /** Elegir el avatar de la galeria (null = la inicial). Devuelve el usuario actualizado. `to`:
   *  el server y la sesion de otro perfil guardado (el selector de perfiles de la TV). */
  setAvatar: (avatar_id: number | null, to?: Target) => req<User>("PATCH", "/api/me", { avatar_id }, { to }),
  avatars: (to?: Target) => req<Avatar[]>("GET", "/api/avatars", undefined, { to }),
  /** Admin: sube una imagen (PNG, JPEG, WebP o GIF, hasta 2 MB) en un formulario multipart. */
  uploadAvatar: (file: Blob, name: string) => {
    const f = new FormData();
    f.append("name", name);
    f.append("file", file);
    return req<Avatar>("POST", "/api/admin/avatars", f);
  },
  deleteAvatar: (id: number) => req<void>("DELETE", `/api/admin/avatars/${id}`),
  changePassword: (current: string, next: string) => req<void>("PUT", "/api/me/password", { current, new: next }),
  /** Cambiar el nombre propio (en modo local, 403: se cambia en el server). */
  changeUsername: (username: string) => req<User>("PUT", "/api/me/username", { username }),
  settings: () => req<Settings>("GET", "/api/me/settings"),
  saveSettings: (s: SettingsUpdate) => req<Settings>("PUT", "/api/me/settings", s),

  catalog: (type: MediaType, catalog: "top" | "imdbRating" | "year", o: { genre?: string; skip?: number } = {}) =>
    cached(`cat:${type}/${catalog}${q(o)}`, 10 * MIN, () =>
      req<MetaPreview[]>("GET", `/api/catalog/${type}/${catalog}${q(o)}`),
    ),
  genres: (type: MediaType) => cached(`genres:${type}`, 60 * MIN, () => req<string[]>("GET", `/api/genres/${type}`)),
  search: (query: string, signal?: AbortSignal) =>
    req<{ movies: MetaPreview[]; series: MetaPreview[] }>("GET", `/api/search${q({ q: query })}`, undefined, { signal }),
  meta: (type: MediaType, id: string) => cached(`meta:${type}/${id}`, 15 * MIN, () => req<Meta>("GET", `/api/meta/${type}/${enc(id)}`)),
  /** Con ?caps= el server pone primero lo que este navegador reproduce directo. */
  streams: (type: MediaType, id: string) =>
    cached(`streams:${type}/${id}`, 3 * MIN, () => req<Stream[]>("GET", `/api/streams/${type}/${enc(id)}${q({ caps: capsList() })}`)),
  subtitles: (type: MediaType, id: string) =>
    cached(`subs:${type}/${id}`, 10 * MIN, () => req<ExternalSub[]>("GET", `/api/subtitles/${type}/${enc(id)}`)),
  prefetch: (s: Pick<Stream, "info_hash" | "file_idx" | "url">) =>
    cached(`prefetch:${s.info_hash ?? s.url}:${s.file_idx ?? ""}`, 5 * MIN, () =>
      req<void>("POST", "/api/prefetch", { info_hash: s.info_hash, file_idx: s.file_idx, url: s.url }),
    ).catch(() => {}),
  /** La sesion queda atada al server que la abrio (ver `pins`). */
  play: async (body: PlayRequest) => {
    const used: Partial<Target> = {};
    const info = await req<PlayInfo>("POST", "/api/play", body, { used });
    pins.set(info.session_id, { base: used.base ?? serverBase(), token: used.token ?? getToken() });
    if (pins.size > 40) pins.delete(pins.keys().next().value!);
    return info;
  },

  continueWatching: () => req<Progress[]>("GET", "/api/progress/continue"),
  progress: (metaId: string) => req<Progress>("GET", `/api/progress/${enc(metaId)}`).catch((e) => {
    if (e instanceof ApiError && e.status === 404) return null;
    throw e;
  }),
  /** Agregado al contrato: todos los episodios vistos de una serie (incluso terminados). */
  seriesProgress: (seriesId: string) => req<Progress[]>("GET", `/api/progress${q({ series_id: seriesId })}`),
  saveProgress: (p: Omit<Progress, "updated_at">, keepalive = false) => req<void>("PUT", "/api/progress", p, { keepalive }),
  deleteProgress: (metaId: string) => req<void>("DELETE", `/api/progress/${enc(metaId)}`),

  library: () => req<MetaPreview[]>("GET", "/api/library"),
  libraryAdd: (m: MetaPreview) => req<void>("PUT", `/api/library/${m.type}/${enc(m.id)}`, m),
  libraryRemove: (type: MediaType, id: string) => req<void>("DELETE", `/api/library/${type}/${enc(id)}`),

  users: () => req<User[]>("GET", "/api/admin/users"),
  createUser: (u: { username: string; password: string; is_admin: boolean }) => req<User>("POST", "/api/admin/users", u),
  patchUser: (id: number, p: { username?: string; password?: string; is_admin?: boolean; disabled?: boolean }) =>
    req<User>("PATCH", `/api/admin/users/${id}`, p),
  deleteUser: (id: number) => req<void>("DELETE", `/api/admin/users/${id}`),
  /** Admin: las sesiones abiertas de un usuario (las de la TV no vencen) y cerrar una. */
  userSessions: (id: number) => req<SessionInfo[]>("GET", `/api/admin/users/${id}/sessions`),
  revokeUserSession: (id: number, sid: number) => req<void>("DELETE", `/api/admin/users/${id}/sessions/${sid}`),
  torrents: () => req<TorrentStat[]>("GET", "/api/admin/torrents"),
  deleteTorrent: (hash: string, files: boolean) => req<void>("DELETE", `/api/admin/torrents/${hash}${q({ files: files ? "true" : undefined })}`),
  stats: () => req<AdminStats>("GET", "/api/admin/stats"),
  metrics: (range: MetricsRange, signal?: AbortSignal) => req<Metrics>("GET", `/api/admin/metrics${q({ range })}`, undefined, { signal }),
  logs: (f: LogFilters & { before?: number; limit?: number }, signal?: AbortSignal) =>
    req<Page<LogRow>>("GET", `/api/admin/logs${q(f)}`, undefined, { signal }),
  audit: (f: { event?: string; user?: string; before?: number; limit?: number }, signal?: AbortSignal) =>
    req<Page<AuditRow>>("GET", `/api/admin/audit${q(f)}`, undefined, { signal }),

  /** Sesiones abiertas del usuario (dispositivos), nunca el token. */
  sessions: () => req<SessionInfo[]>("GET", "/api/me/sessions"),
  revokeSession: (id: number) => req<void>("DELETE", `/api/me/sessions/${id}`),
  revokeOtherSessions: () => req<{ revoked: number }>("DELETE", "/api/me/sessions"),

  status: (session: string, signal?: AbortSignal) =>
    req<MediaStatus>("GET", `/api/media/${session}/status`, undefined, { signal, ...pinned(session) }),
  closeSession: (session: string) =>
    req<void>("DELETE", `/api/media/${session}`, undefined, { keepalive: true, ...pinned(session) }).catch(() => {}),

  // ---- modo local (server embebido, docs/local-mode.md): siempre contra el local
  /** Login por el local: valida contra el server de casa si lo alcanza y devuelve las dos
   *  sesiones; si no, contra la copia del dispositivo (`offline: true`). */
  localLogin: (b: { username: string; password: string; device?: string; remote_url?: string }) =>
    req<LocalLogin>("POST", "/api/auth/login", b, { to: { base: localBase, token: null }, noAuthRedirect: true }),
  localStatus: () => req<LocalStatus>("GET", "/api/local/status", undefined, { to: { base: localBase, token: localToken } }),
  /** Sincroniza ya. Con `token` (la sesion del server de casa) el local la verifica y la guarda. */
  localSync: (remote?: string | null) =>
    req<{ pushed: number; pulled: number; last_sync_at: number | null }>(
      "POST",
      "/api/local/sync",
      remote ? { token: remote } : undefined,
      { to: { base: localBase, token: localToken } },
    ),
  /** Cierra una sesion puntual (la del server de casa o la del local), sin tocar la activa. */
  logoutAt: (base: string, tok: string) => req<void>("POST", "/api/auth/logout", undefined, { to: { base, token: tok } }),
};
