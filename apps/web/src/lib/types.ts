// Tipos del contrato HTTP (docs/API.md). Si algo cambia, se cambia primero alla.
// El server manda los opcionales como null (no los omite): por eso `?: T | null`.

export type MediaType = "movie" | "series";

export type User = {
  id: number;
  username: string;
  is_admin: boolean;
  disabled: boolean;
  created_at: string;
  /** El avatar elegido de la galeria (null o ausente: la inicial; un server viejo no lo manda). */
  avatar_id?: number | null;
};

/** Una imagen de la galeria de avatares (la sube el admin). */
export type Avatar = { id: number; name: string; created_at: number };

export type MetaPreview = {
  id: string;
  type: MediaType;
  name: string;
  poster?: string | null;
  background?: string | null;
  logo?: string | null;
  year?: string | null;
  imdb_rating?: string | null;
  genres?: string[] | null;
  description?: string | null;
};

export type Episode = {
  id: string;
  season: number;
  episode: number;
  name?: string | null;
  overview?: string | null;
  thumbnail?: string | null;
  released?: string | null;
};

export type Meta = MetaPreview & {
  runtime?: string | null;
  cast?: string[] | null;
  director?: string[] | null;
  trailer_youtube_id?: string | null;
  videos?: Episode[] | null;
};

export type Quality = "2160p" | "1080p" | "720p" | "480p" | "SD" | "CAM" | "unknown";

export type Stream = {
  key: string;
  info_hash?: string | null;
  file_idx?: number | null;
  url?: string | null;
  title: string;
  file_name?: string | null;
  quality: Quality;
  size_bytes?: number | null;
  seeders?: number | null;
  source?: string | null;
  tags: string[];
  languages: string[];
  cached?: boolean | null;
  score: number;
};

export type ClientCaps = {
  h264: boolean;
  hevc: boolean;
  av1: boolean;
  vp9: boolean;
  mkv: boolean;
  aac: boolean;
  ac3: boolean;
  eac3: boolean;
  dts: boolean;
  opus: boolean;
  flac: boolean;
};

export type Track = { index: number; codec: string; lang?: string | null; title?: string | null; default?: boolean | null };

export type PlayMode = "direct" | "remux" | "transcode";

export type PlayInfo = {
  session_id: string;
  mode: PlayMode;
  direct_url: string;
  hls_url?: string | null;
  file_name: string;
  size_bytes: number;
  duration_s?: number | null;
  container?: string | null;
  video_codec?: string | null;
  width?: number | null;
  height?: number | null;
  hdr?: boolean | null;
  audio_tracks: Track[];
  subtitle_tracks: Track[];
  /** El server tiene ffmpeg. Sin el (modo local sin ffmpeg): siempre "direct" y sin hls_url. */
  transcode_available?: boolean | null;
  /** Segun lo sondeado, este dispositivo lo reproduce tal cual. */
  direct_ok?: boolean | null;
  probe_source?: "ffprobe" | "sniff" | "guess" | null;
};

export type MediaStatus = {
  state: "metadata" | "probing" | "buffering" | "ready" | "error";
  error?: string | null;
  down_bps: number;
  up_bps: number;
  peers: number;
  downloaded_bytes: number;
  total_bytes: number;
  progress: number;
  buffered_ahead_s?: number | null;
  /** Cuanto quiere tener listo el server antes de arrancar (segundos). */
  buffer_target_s?: number | null;
  /** Tramos del archivo ya bajados en el server, fracciones [desde, hasta] de 0 a 1 (torrents). */
  have_ranges?: [number, number][] | null;
};

export type Progress = {
  meta_id: string;
  type: MediaType;
  series_id?: string | null;
  name: string;
  poster?: string | null;
  background?: string | null;
  episode_label?: string | null;
  position_s: number;
  duration_s: number;
  stream_key?: string | null;
  updated_at: string;
};

export type PreferredQuality = "2160p" | "1080p" | "720p";

export type Settings = {
  ui: Record<string, unknown>;
  has_rd_key: boolean;
  preferred_quality?: PreferredQuality | null;
  max_size_gb?: number | null;
};

export type SettingsUpdate = Partial<Omit<Settings, "has_rd_key">> & { rd_key?: string };

export type ExternalSub = { id: string; lang: string; url: string };

/** "Top rated" (GET /api/top/{type}): lo mejor calificado de IMDb con todos los generos pedidos. */
export type TopRated = {
  /** "preparing": el server todavia baja los datos de IMDb; "unavailable": este server no lo
   *  tiene (el embebido de las apps). */
  state: "ready" | "preparing" | "unavailable";
  items: MetaPreview[];
  total: number;
  more: boolean;
  /** Los generos del tipo; `count` = cuantos quedarian sumandolo al filtro (0: no combina). */
  genres: { name: string; count: number }[];
  min_votes: number;
  /** unix s de los datos de IMDb. */
  updated_at: number | null;
};

export type TorrentStat = {
  info_hash: string;
  name: string;
  size_bytes: number;
  downloaded_bytes: number;
  down_bps: number;
  up_bps: number;
  peers: number;
  state: string;
  last_used_at: string;
};

export type AdminStats = {
  cache_bytes: number;
  cache_limit_bytes: number;
  disk_free_bytes: number;
  active_sessions: number;
  users: number;
  /** Que esta viendo cada uno ahora. */
  watching?: { username: string; meta_id?: string | null; stream_key?: string | null; session_id?: string | null; started_at?: number | null }[] | null;
};

// ---- sesiones propias (GET /api/me/sessions)

export type SessionInfo = {
  id: number;
  /** La etiqueta que mando la UI en el login. */
  device?: string | null;
  user_agent?: string | null;
  last_ip?: string | null;
  /** unix s */
  created_at: number;
  last_used_at: number;
  expires_at: number;
  /** No vence (la app de TV). Un server viejo no lo manda. */
  persistent?: boolean;
  /** Es la de este request. */
  current: boolean;
};

// ---- metricas, logs y auditoria (admin)

export type MetricsRange = "24h" | "7d" | "30d" | "90d";

export type Metrics = {
  range: string;
  from_ms: number;
  to_ms: number;
  totals: { requests: number; errors: number; client_errors: number; bytes_out: number; p50_ms?: number | null; p95_ms?: number | null };
  /** Una por hora con trafico (las horas sin nada no vienen). */
  series: { hour_ms: number; requests: number; errors: number; bytes_out: number; p95_ms?: number | null }[];
  top_ips: { ip: string; requests: number; bytes_out: number; users: string[]; tailscale_logins: string[] }[];
  top_users: { user_id: number; username?: string | null; requests: number; bytes_out: number }[];
  top_routes: { method: string; route: string; requests: number; errors: number; bytes_out: number; p50_ms?: number | null; p95_ms?: number | null }[];
  status: { status: number; requests: number }[];
  failed_logins: { ip: string; count: number; last_at_ms: number; usernames: string[] }[];
  dropped_events: number;
};

export type LogRow = {
  id: number;
  ts_ms: number;
  ip?: string | null;
  tailscale_login?: string | null;
  user_id?: number | null;
  username?: string | null;
  method: string;
  /** Plantilla de la ruta (/api/meta/{type}/{id}), nunca la URL real. */
  route: string;
  status: number;
  dur_ms: number;
  bytes: number;
};

export type AuditEvent =
  | "login_ok"
  | "login_fail"
  | "logout"
  | "session_revoked"
  | "password_changed"
  | "user_created"
  | "user_updated"
  | "user_disabled"
  | "user_deleted";

export type AuditRow = {
  id: number;
  ts_ms: number;
  event: AuditEvent | string;
  user_id?: number | null;
  username?: string | null;
  ip?: string | null;
  detail?: string | null;
};

/** Pagina de logs o auditoria: `next_before` va como `before` de la siguiente (null = no hay mas). */
export type Page<T> = { items: T[]; next_before: number | null };

export type LogFilters = { user?: string; ip?: string; status?: string; route?: string };

// ---- modo local (server embebido en desktop y Android, docs/local-mode.md)

export type LocalLogin = {
  token: string;
  user: User;
  /** Validado contra la copia del dispositivo: el server de casa no se alcanzo. */
  offline: boolean;
  remote: { url: string; token: string; user: { id: number; username: string } } | null;
};

export type LocalStatus = {
  mode: "local";
  username: string;
  remote_url: string | null;
  /** Hay una sesion del server de casa guardada (si no, hace falta entrar de nuevo online). */
  remote_signed_in: boolean;
  /** null = sin server de casa configurado. */
  remote_reachable: boolean | null;
  last_sync_at: number | null;
  last_sync_error: string | null;
  pending_changes: number;
  transcode_available: boolean;
  sync_interval_s: number | null;
  version: string;
};
