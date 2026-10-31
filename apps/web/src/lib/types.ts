// Tipos del contrato HTTP (docs/API.md). Si algo cambia, se cambia primero alla.
// El server manda los opcionales como null (no los omite): por eso `?: T | null`.

export type MediaType = "movie" | "series";

export type User = { id: number; username: string; is_admin: boolean; disabled: boolean; created_at: string };

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
