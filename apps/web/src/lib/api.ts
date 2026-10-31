// Cliente tipado del contrato (docs/API.md). El token vive en localStorage; un 401 lleva al login.
import { detectCaps } from "./caps";
import { serverBase } from "./platform";
import { read, write } from "./store";
import type {
  AdminStats,
  ClientCaps,
  ExternalSub,
  MediaStatus,
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
} from "./types";

const TOKEN_KEY = "mythos.token";
let token = read(TOKEN_KEY);

export function getToken() {
  return token;
}
export function setToken(t: string | null) {
  token = t;
  write(TOKEN_KEY, t);
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

type Opts = { signal?: AbortSignal; keepalive?: boolean; noAuthRedirect?: boolean };

async function req<T>(method: string, path: string, body?: unknown, o: Opts = {}): Promise<T> {
  const headers: Record<string, string> = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const r = await fetch(serverBase() + path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: o.signal,
    keepalive: o.keepalive,
  });
  if (r.status === 401 && !o.noAuthRedirect) {
    setToken(null);
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

/** URL de media lista para <video>, hls.js o fetch: con la base del server y el token en ?t=.
 *  Las absolutas de otro lado (y data:/blob:) quedan como estan. */
export function mediaUrl(path: string, extra?: Record<string, string | number | undefined>): string {
  if (/^(data|blob):/.test(path)) return path;
  const base = serverBase();
  const abs = /^https?:\/\//.test(path);
  const ours = !abs || (base && path.startsWith(base)) || path.startsWith(location.origin);
  const u = new URL(abs ? path : (base || location.origin) + path);
  if (ours && token && !u.searchParams.has("t")) u.searchParams.set("t", token);
  for (const [k, v] of Object.entries(extra ?? {})) if (v !== undefined) u.searchParams.set(k, String(v));
  return u.toString();
}

/** Agrega el token a una URL que arma hls.js (segmentos relativos a la playlist). */
export function withToken(url: string): string {
  if (!token || /[?&]t=/.test(url)) return url;
  const base = serverBase() || location.origin;
  if (/^https?:\/\//.test(url) && !url.startsWith(base)) return url;
  return url + (url.includes("?") ? "&" : "?") + "t=" + encodeURIComponent(token);
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
  login: (username: string, password: string) =>
    req<{ token: string; user: User }>("POST", "/api/auth/login", { username, password }, { noAuthRedirect: true }),
  logout: () => req<void>("POST", "/api/auth/logout", undefined, { noAuthRedirect: true }),
  me: () => req<User>("GET", "/api/me"),
  changePassword: (current: string, next: string) => req<void>("PUT", "/api/me/password", { current, new: next }),
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
  play: (body: PlayRequest) => req<PlayInfo>("POST", "/api/play", body),

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
  patchUser: (id: number, p: { password?: string; is_admin?: boolean; disabled?: boolean }) =>
    req<User>("PATCH", `/api/admin/users/${id}`, p),
  deleteUser: (id: number) => req<void>("DELETE", `/api/admin/users/${id}`),
  torrents: () => req<TorrentStat[]>("GET", "/api/admin/torrents"),
  deleteTorrent: (hash: string, files: boolean) => req<void>("DELETE", `/api/admin/torrents/${hash}${q({ files: files ? "true" : undefined })}`),
  stats: () => req<AdminStats>("GET", "/api/admin/stats"),

  status: (session: string, signal?: AbortSignal) =>
    req<MediaStatus>("GET", `/api/media/${session}/status`, undefined, { signal }),
  closeSession: (session: string) => req<void>("DELETE", `/api/media/${session}`, undefined, { keepalive: true }).catch(() => {}),
};
