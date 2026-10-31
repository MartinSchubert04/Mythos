# Contrato de Mythos

Esta es la fuente de verdad entre los tres módulos (server, media, web). Si algo cambia, se cambia
acá primero.

## Prioridades (pedido del usuario, en este orden)

1. **Que no se trabe la película.** Lo máximo posible desde el código: priorizar piezas, buffer
   inicial, precarga, recuperación de cortes, elegir bien el stream, cambiar de stream sin salir.
2. **Liviana**: poca RAM, bundle chico, nada de dependencias pesadas sin motivo.
3. **Linda y con animaciones**, user friendly, buen "feeling".

## Generalidades

- Todo JSON, `snake_case`. Errores: status HTTP + `{"error": "texto legible en inglés"}`.
- Auth: `Authorization: Bearer <token>`. Las rutas bajo `/api/media/**` aceptan además `?t=<token>`
  (para `<video>`, hls.js y `<track>`).
- 401 → la UI vuelve al login. 403 → no es admin.
- CORS: permitir `http://localhost:5173`, `tauri://localhost`, `http://tauri.localhost`,
  `https://tauri.localhost` (desktop) con `Authorization` y `Range`; exponer `Content-Range`,
  `Accept-Ranges`, `Content-Length`.
- El server sirve la UI compilada (`MYTHOS_WEB_DIR`, por defecto `./web`) en `/` con fallback a
  `index.html` (SPA).

## Tipos

```ts
type User = { id: number; username: string; is_admin: boolean; disabled: boolean; created_at: string };

type MetaPreview = {           // una tarjeta de catálogo
  id: string;                  // imdb id, "tt0111161"
  type: "movie" | "series";
  name: string;
  poster?: string; background?: string; logo?: string;
  year?: string;               // "2010" o "2008-2013"
  imdb_rating?: string;
  genres?: string[];
  description?: string;
};

type Meta = MetaPreview & {
  runtime?: string; cast?: string[]; director?: string[];
  trailer_youtube_id?: string;
  videos?: Episode[];          // solo series, ordenados por season, episode
};

type Episode = {
  id: string;                  // "tt0944947:1:1"
  season: number; episode: number;
  name?: string; overview?: string; thumbnail?: string; released?: string;
};

type Stream = {
  key: string;                 // "<info_hash>:<file_idx>" o "url:<hash corto>"
  info_hash?: string;          // en minúsculas, 40 hex
  file_idx?: number;
  url?: string;                // solo con Real-Debrid (link HTTP directo)
  title: string;               // nombre del release ("Movie.2010.1080p.BluRay.x264-GRP")
  file_name?: string;
  quality: "2160p" | "1080p" | "720p" | "480p" | "SD" | "CAM" | "unknown";
  size_bytes?: number;
  seeders?: number;
  source?: string;             // "ThePirateBay", "YTS", ...
  tags: string[];              // "HDR", "DV", "HEVC", "x264", "10bit", "Atmos", "5.1", "REMUX", "WEB-DL"...
  languages: string[];         // banderas/idiomas que trae Torrentio ("🇪🇸", "multi"...)
  cached?: boolean;            // en caché de Real-Debrid o ya descargado en el server
  score: number;               // mayor = mejor. La lista viene ordenada por score desc.
};

type ClientCaps = {            // lo que el reproductor del cliente puede reproducir directo
  h264: boolean; hevc: boolean; av1: boolean; vp9: boolean;
  mkv: boolean;                // el navegador abre .mkv con <video>
  aac: boolean; ac3: boolean; eac3: boolean; dts: boolean; opus: boolean; flac: boolean;
};

type Track = { index: number; codec: string; lang?: string; title?: string; default?: boolean };

type PlayInfo = {
  session_id: string;          // 128 bits aleatorios, url-safe
  mode: "direct" | "remux" | "transcode";
  // URLs relativas al server, sin el token (la UI agrega ?t=)
  direct_url: string;          // /api/media/{session}/file  (Range, siempre disponible)
  hls_url?: string;            // /api/media/{session}/hls/index.m3u8  (remux o transcode)
  file_name: string; size_bytes: number;
  duration_s?: number;
  container?: string; video_codec?: string; width?: number; height?: number; hdr?: boolean;
  audio_tracks: Track[];
  subtitle_tracks: Track[];    // internos; texto (srt/ass/mov_text) convertibles a VTT
};

type MediaStatus = {
  state: "metadata" | "probing" | "buffering" | "ready" | "error";
  error?: string;
  down_bps: number; up_bps: number;
  peers: number;               // conectados
  downloaded_bytes: number; total_bytes: number;   // del archivo elegido
  progress: number;            // 0..1 del archivo
  buffered_ahead_s?: number;   // segundos listos después de la posición actual, si se sabe
};

type Progress = {
  meta_id: string;             // "tt0111161" o "tt0944947:1:2"
  type: "movie" | "series";
  series_id?: string;          // "tt0944947" para episodios
  name: string; poster?: string; background?: string;
  episode_label?: string;      // "S01E02 · The Kingsroad"
  position_s: number; duration_s: number;
  stream_key?: string;         // para retomar con el mismo stream
  updated_at: string;
};

type Settings = {              // por usuario, la UI guarda lo que quiera adentro de `ui`
  ui: Record<string, unknown>; // theme, palette, idioma de subtítulos preferido, autoplay...
  has_rd_key: boolean;         // solo lectura
  preferred_quality?: "2160p" | "1080p" | "720p";
  max_size_gb?: number;
};
```

## Rutas del server (`backend/server`)

| Método | Ruta | Cuerpo / query | Respuesta |
|---|---|---|---|
| GET | `/api/health` | | `{ok:true, version}` (sin auth) |
| POST | `/api/auth/login` | `{username, password}` | `{token, user}`; 401 si falla. Rate limit por IP |
| POST | `/api/auth/logout` | | 204 |
| GET | `/api/me` | | `User` |
| PUT | `/api/me/password` | `{current, new}` | 204 |
| GET | `/api/me/settings` | | `Settings` |
| PUT | `/api/me/settings` | `Settings` parcial + `rd_key?: string` (vacío = borrar) | `Settings` |
| GET | `/api/catalog/{type}/{catalog}` | `?genre=&skip=` (`catalog`: `top`, `imdbRating`, `year`) | `MetaPreview[]` |
| GET | `/api/genres/{type}` | | `string[]` |
| GET | `/api/search` | `?q=` | `{movies: MetaPreview[], series: MetaPreview[]}` |
| GET | `/api/meta/{type}/{id}` | | `Meta` |
| GET | `/api/streams/{type}/{id}` | id con episodio para series (`tt…:1:2`) | `Stream[]` |
| GET | `/api/subtitles/{type}/{id}` | | `{id, lang, url}[]` (OpenSubtitles v3 addon; `url` = `/api/media/subs/external?u=…`, devuelve VTT) |
| POST | `/api/prefetch` | `{info_hash, file_idx?, url?}` | 202. Arranca el torrent y baja principio y final del archivo sin abrir sesión (la UI lo llama al 85 % de un episodio para el siguiente, y al abrir la lista de streams para el primero) |
| POST | `/api/play` | `{stream_key, info_hash?, file_idx?, url?, caps: ClientCaps, meta_id?}` | `PlayInfo` |
| GET | `/api/progress/continue` | | `Progress[]` (últimos 20, sin terminados > 92 %) |
| GET | `/api/progress/{meta_id}` | | `Progress` o 404 |
| PUT | `/api/progress` | `Progress` sin `updated_at` | 204 |
| DELETE | `/api/progress/{meta_id}` | | 204 |
| GET | `/api/library` | | `MetaPreview[]` |
| PUT / DELETE | `/api/library/{type}/{id}` | PUT con `MetaPreview` | 204 |
| GET | `/api/admin/users` | | `User[]` |
| POST | `/api/admin/users` | `{username, password, is_admin}` | `User` |
| PATCH | `/api/admin/users/{id}` | `{password?, is_admin?, disabled?}` | `User` (deshabilitar cierra sus sesiones) |
| DELETE | `/api/admin/users/{id}` | | 204 (no se puede borrar a uno mismo ni al último admin) |
| GET | `/api/admin/torrents` | | `TorrentStat[]` |
| DELETE | `/api/admin/torrents/{info_hash}` | `?files=true` | 204 |
| GET | `/api/admin/stats` | | `{cache_bytes, cache_limit_bytes, disk_free_bytes, active_sessions, users}` |

`type TorrentStat = { info_hash, name, size_bytes, downloaded_bytes, down_bps, up_bps, peers, state, last_used_at }`

Rutas montadas desde el crate de media, con auth (Bearer o `?t=`):

| Método | Ruta | Respuesta |
|---|---|---|
| GET | `/api/media/{session}/file` | el archivo, con `Range` (206). Prioriza las piezas pedidas |
| GET | `/api/media/{session}/hls/index.m3u8` | `?start=<s>&audio=<idx>`: playlist HLS que arranca en `start` (para buscar en modo transcode la UI recarga con otro `start` y suma el offset) |
| GET | `/api/media/{session}/hls/{segment}` | segmentos `.ts` o `.m4s` |
| GET | `/api/media/{session}/subs/{index}.vtt` | subtítulo interno en WebVTT |
| GET | `/api/media/{session}/status` | `MediaStatus` |
| DELETE | `/api/media/{session}` | 204 (para ffmpeg; el torrent queda en caché) |
| GET | `/api/media/subs/external` | `?u=<url de OpenSubtitles>` → VTT (gz/srt → vtt) — lo hace el server |

## API del crate `mythos-media` (lo que usa el server)

```rust
pub struct MediaConfig {
    pub data_dir: PathBuf,        // torrents en data_dir/torrents, HLS temporal en data_dir/hls
    pub cache_limit_bytes: u64,   // LRU: se borran los torrents menos usados que no estén en uso
    pub peer_port: u16,           // 42069
    pub ffmpeg: PathBuf, pub ffprobe: PathBuf,
    pub hwaccel: HwAccel,         // Nvenc | None  (MYTHOS_HWACCEL=nvenc|none)
    pub extra_trackers: bool,     // bajar la lista de trackers públicos al arrancar
}
#[derive(Clone)] pub struct Media { /* Arc */ }
impl Media {
    pub async fn new(cfg: MediaConfig) -> anyhow::Result<Self>;
    pub async fn open(&self, src: Source, caps: ClientCaps) -> anyhow::Result<PlayInfo>;
    pub fn router(&self) -> axum::Router;          // rutas relativas a /api/media, SIN auth (la pone el server)
    pub async fn torrents(&self) -> Vec<TorrentStat>;
    pub async fn remove_torrent(&self, info_hash: &str, delete_files: bool) -> anyhow::Result<()>;
    pub async fn stats(&self) -> MediaStats;       // cache_bytes, cache_limit_bytes, disk_free_bytes, active_sessions
    pub async fn prefetch(&self, src: Source) -> anyhow::Result<()>; // no bloquea: dispara y vuelve
    pub fn is_downloaded(&self, info_hash: &str, file_idx: Option<usize>) -> bool; // para Stream.cached
}
pub enum Source { Torrent { info_hash: String, file_idx: Option<usize>, trackers: Vec<String> }, Url(String) }
```

Los tipos `ClientCaps`, `PlayInfo`, `Track`, `MediaStatus`, `TorrentStat`, `MediaStats` viven en
`mythos-media` (serde) y el server los reexporta tal cual.
