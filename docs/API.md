# Contrato de Mythos

Esta es la fuente de verdad entre los tres módulos (server, media, web). Si algo cambia, se cambia
acá primero.

## Prioridades (pedido del usuario, en este orden)

1. **Que no se trabe la película.** Lo máximo posible desde el código: priorizar piezas, buffer
   inicial, precarga, recuperación de cortes, elegir bien el stream, cambiar de stream sin salir.
2. **Liviana**: poca RAM, bundle chico, nada de dependencias pesadas sin motivo.
3. **Linda y con animaciones**, user friendly, buen "feeling".

## Generalidades

- Todo JSON, `snake_case`. Errores: status HTTP + `{"error": "texto legible en inglés"}` (también
  para JSON mal formado y rutas `/api` inexistentes) y, en los que la UI muestra, `code` y
  `params` (ver "Errores", abajo). 502 = falló un servicio externo (Cinemeta,
  Torrentio, OpenSubtitles) o media; si hay una respuesta vieja en caché, el server la usa en vez de
  fallar.
- Auth: `Authorization: Bearer <token>`. Las rutas bajo `/api/media/**` aceptan además `?t=<token>`
  (para `<video>`, hls.js y `<track>`). Las sesiones duran **180 días desde el último uso** (se
  renuevan solas; el login es una puerta de una sola vez por dispositivo). El logout revoca la
  sesión; cada usuario ve y cierra las suyas en `/api/me/sessions`.
- Sesiones que **no vencen** (`persistent: true` en el login; las pide la app de TV, donde cada
  cuenta es un perfil guardado y no se vuelve a pedir la contraseña): `expires_at` queda en
  `253402300799` (9999-12-31) y no se renueva; solo se anota el último uso, una vez por día. Se
  revocan igual que las demás: logout, `/api/me/sessions` (desde cualquier dispositivo), cambio de
  contraseña, deshabilitar la cuenta o el admin (Admin → Users → "Signed-in devices",
  `/api/admin/users/{id}/sessions`). Las de siempre (web, celular, PC) no cambian.
- 401 → la UI vuelve al login. 403 → no es admin.
- CORS: permitir `http://localhost:5173`, `tauri://localhost`, `http://tauri.localhost`,
  `https://tauri.localhost` (desktop y Android) con `Authorization` y `Range`; exponer
  `Content-Range`, `Accept-Ranges`, `Content-Length`. Responde a los preflight de Private Network
  Access (`Access-Control-Allow-Private-Network: true`) para que la ventana de Tauri le hable al
  server local en `127.0.0.1`.
- El server sirve la UI compilada (`MYTHOS_WEB_DIR`, por defecto `./web`) en `/` con fallback a
  `index.html` (SPA). Embebido en las apps (modo local) no sirve UI: la trae la app.
- Hay dos modos (`GET /api/health` → `mode`): **`server`** (el de casa, Docker) y **`local`** (el
  server embebido en la app desktop/Android, ver `docs/local-mode.md`). Las diferencias están
  marcadas en cada ruta.
- **Identidad del server** (`GET /api/health` → `server_id`): 32 caracteres hex aleatorios que el
  server crea la primera vez y guarda en su base (`server_meta`); sobrevive a reinicios y
  actualizaciones, una base nueva es otro server. Las apps la comparan para saber que varias
  direcciones (Tailscale, la red de casa con `MYTHOS_EXTRA_BIND`) llevan al **mismo** server: las
  mismas sesiones sirven en todas (`docs/local-mode.md`, "Varias direcciones del server"). Un server
  viejo no la manda: la app lo acepta sin poder comparar. El server embebido (modo local) tiene la
  suya, distinta.

### Errores

```jsonc
{ "error": "Torrentio answered 503 Service Unavailable, try again in a moment",  // siempre, en inglés
  "code": "torrentio_status",          // estable, snake_case; falta en los de validación
  "params": { "status": 503 } }        // solo si el texto lleva datos
```

- `error` no cambia de forma (los clientes viejos lo siguen mostrando). La UI traduce por `code`
  (`apps/web/src/lib/i18n/server.ts`: la clave es `srv.` + el código en camelCase) con `params`
  para los datos; si no viene código (un server viejo) o no lo conoce, reconoce el texto, y si
  tampoco, lo muestra tal cual. **Cambiar una frase no rompe nada; cambiar un código, sí**: un
  código nuevo necesita su clave `srv.*` en `lib/i18n/es` y `lib/i18n/en`.
- Los de validación de la API (JSON mal formado, un campo inválido, `range` de métricas) van sin
  código: no los ve el usuario.
- Códigos y sus `params`:
  - Login y cuenta: `bad_login`, `bad_login_offline` (modo local sin server), `disabled`,
    `too_many_logins` (`n`: minutos), `no_token`, `expired`, `bad_token`, `admin_only`,
    `admin_local`, `user_exists`, `user_not_found`, `bad_username`, `pw_short`, `pw_long`,
    `pw_wrong`, `pw_on_server`, `rename_on_server`, `other_account`, `session_not_found`.
  - Admin: `self_admin`, `self_disable`, `self_delete`, `last_admin`, `need_admin`; avatares:
    `avatar_not_found`, `avatar_too_big`, `avatar_type`, `gallery_full` (`n`).
  - Servicios de afuera: `torrentio_down`, `torrentio_bad`, `torrentio_status` (`status`),
    `cinemeta_down`, `cinemeta_bad`, `cinemeta_status` (`status`), `title_not_found`,
    `subs_down`, `subs_bad`, `subs_status` (`status`), `sub_download`, `sub_too_large`,
    `sub_corrupt`, `img_download`, `img_too_large`, `query_long`, `max_genres` (`n`).
  - Streams y reproducción: `unknown_stream`, `rd_missing`, `rd_invalid`, `only_torrentio`,
    `media_session`, `play_failed` (`e`: el detalle, en inglés), `no_peers` (`s`: segundos que
    esperó), `no_space`, `torrent_error` (`e`), `no_ffmpeg`, `buffering`, `sub_not_found`.
  - Modo local: `bad_server_url`, `server_url`, `sync_not_linked`, `sync_signed_out`,
    `sync_unreachable`, `sync_unsupported`, `sync_failed` (`e`). El modo local pasa tal cual el
    código que mande el server de casa (p. ej. `bad_login` en el login online).
  - `internal` (500): el detalle queda en el log del server.
- `GET /api/media/{session}/status` lleva lo mismo para el error de la sesión: `error`,
  `error_code` (`no_space`, `torrent_error`) y `error_params`. `GET /api/local/status`:
  `last_sync_error` y `last_sync_error_code`.
- En el crate de media: `mythos_media::Coded { code, message, params }`, que viaja dentro del
  `anyhow::Error` de `Media::open` (el server lo saca con `downcast_ref`).

## Tipos

```ts
type User = { id: number; username: string; is_admin: boolean; disabled: boolean; created_at: string;
              avatar_id: number | null };   // el avatar elegido de la galería (null = la inicial)

type Avatar = { id: number; name: string; created_at: number };   // unix s

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
  key: string;                 // "<info_hash>:<file_idx>", "<info_hash>" si Torrentio no dice el
                               // archivo, o "url:<hash corto>" (Real-Debrid). Tratarla como opaca.
  info_hash?: string;          // en minúsculas, 40 hex
  file_idx?: number;
  url?: string;                // solo con Real-Debrid: link de Torrentio con la clave reemplazada
                               // por "_RD_" (la clave nunca sale del server; /api/play la repone)
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
  reasons?: string[];          // por qué lo sube lo aprendido del usuario ("Preferencias aprendidas"):
                               // "same_group" | "liked_group" | "subs:<idioma>" | "dual_audio"
};

type TopRated = {              // GET /api/top/{type} (ver "Top rated" más abajo)
  state: "ready"               // hay datos
       | "preparing"           // la primera bajada de IMDb está en camino (la UI reintenta sola)
       | "unavailable";        // este server no lo tiene (modo local, o MYTHOS_IMDB=0)
  items: MetaPreview[];        // por rating y después por votos, de mayor a menor
  total: number;               // títulos que cumplen el filtro
  more: boolean;               // hay más después de skip + items.length
  genres: { name: string; count: number }[];  // los géneros del tipo con ese mínimo de votos;
                               // count = cuántos quedarían sumándolo al filtro (los elegidos: total)
  min_votes: number;           // el que se usó (con el default y el piso aplicados)
  updated_at: number | null;   // unix s de la bajada de IMDb
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
                               // Recomendado también en direct (remux a pedido): la UI lo usa
                               // para cambiar de pista de audio sin salir del reproductor.
  file_name: string; size_bytes: number;
  duration_s?: number;
  container?: string; video_codec?: string; width?: number; height?: number; hdr?: boolean;
  audio_tracks: Track[];
  subtitle_tracks: Track[];    // internos; texto (srt/ass/mov_text) convertibles a VTT. Vacío
                               // si no hay ffmpeg
  transcode_available: boolean;// el server tiene ffmpeg. Si es false: `mode` siempre "direct",
                               // `hls_url` null y sin subtítulos internos (modo local sin ffmpeg)
  direct_ok: boolean;          // según lo sondeado, `caps` reproduce el archivo tal cual. Con
                               // transcode_available=false y direct_ok=false, probablemente no
                               // se vea o no se escuche: la UI debería ofrecer otro stream
  probe_source: "ffprobe" | "sniff" | "guess";  // confianza de los datos técnicos: ffprobe,
                               // cabeceras MP4/MKV leídas en Rust, o adivinado por el nombre
};

type MediaStatus = {
  state: "metadata" | "probing" | "buffering" | "ready" | "error";
  error?: string;
  error_code?: string;         // ver "Errores": no_space, torrent_error
  error_params?: object;       // {"e": "..."} en torrent_error
  down_bps: number; up_bps: number;
  peers: number;               // conectados
  downloaded_bytes: number; total_bytes: number;   // del archivo elegido
  progress: number;            // 0..1 del archivo
  buffered_ahead_s?: number;   // segundos listos después de la posición actual, si se sabe
  buffer_target_s?: number;    // segundos de buffer que espera antes de pasar a "ready" (para
                               // dibujar la barra de "cargando": buffered_ahead_s / buffer_target_s)
  have_ranges?: [number, number][]; // tramos del archivo ya bajados en el server, fracciones 0..1
                               // (≤ 64, se juntan los huecos más chicos; solo torrents). La barra
                               // del reproductor los dibuja más tenues debajo de lo que tiene el video
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

type SessionInfo = {           // una sesión abierta del usuario (nunca el token)
  id: number;
  device?: string;             // la etiqueta que mandó la UI en el login
  user_agent?: string; last_ip?: string;
  created_at: number; last_used_at: number; expires_at: number;   // unix s
  persistent: boolean;         // no vence (expires_at = 253402300799)
  current: boolean;            // es la de este request
};

// ---------- sincronización (ver "Sincronización" más abajo) ----------
type ProgressRec = Omit<Progress, "updated_at"> & { updated_ms: number; deleted: boolean };
type LibraryRec = { type: "movie" | "series"; id: string; item: MetaPreview | null;  // null en lápidas
                    updated_ms: number; deleted: boolean };
type SettingsRec = { ui: Record<string, unknown>; preferred_quality: string | null;
                     max_size_gb: number | null; updated_ms: number };      // nunca la clave de RD
type Changes = { settings?: SettingsRec | null; progress: ProgressRec[]; library: LibraryRec[] };
type SyncPull = Changes & { cursor: number; more: boolean };
type SyncPush = { applied: number; ignored: number; rejected: number };
```

## Rutas del server (`backend/server`)

| Método | Ruta | Cuerpo / query | Respuesta |
|---|---|---|---|
| GET | `/api/health` | | `{ok:true, version, mode: "server"\|"local", transcode: boolean, images: boolean, server_id: string}` (sin auth; `transcode` = hay ffmpeg; `images` = existe `/api/media/img`; `server_id` = identidad estable de este server, ver abajo) |
| POST | `/api/auth/login` | `{username, password, device?, persistent?}` (`device`: etiqueta para la lista de sesiones, ≤ 64; `persistent: true`: sesión que no vence, ver Generalidades) | `{token, user}`; 401 si falla o está deshabilitado. Rate limit por IP: 10 fallos / 5 min → 429. En modo local cambia: ver "Modo local" |
| POST | `/api/auth/logout` | | 204 (revoca la sesión) |
| GET | `/api/me` | | `User` |
| PATCH | `/api/me` | `{avatar_id?: number \| null}` (`null` saca el avatar; sin el campo no cambia) | `User`; 404 si el avatar no existe. También en modo local (el embebido no tiene galería: solo `null`) |
| GET | `/api/avatars` | | `Avatar[]` (la galería; vacía en modo local). El server trae 12 de fábrica (`backend/server/assets/avatars`, dibujos propios): se suman a la galería una sola vez por base (marca `default_avatars` en `server_meta`), así que si el admin borra alguno no vuelve |
| GET | `/api/media/avatars/{id}` | Bearer o `?t=` (para `<img>`) | la imagen, con su `Content-Type`, `nosniff` y `Cache-Control: private, max-age=31536000, immutable` (los ids no se reusan) |
| PUT | `/api/me/password` | `{current, new}` | 204 (cierra las demás sesiones). En modo local: 403 (se cambia en el server) |
| PUT | `/api/me/username` | `{username}` (mismas reglas que al crear: 1-32, letras, números, `_ - .`) | `User`; 409 si otro ya lo tiene (sin distinguir mayúsculas; cambiar solo mayúsculas del propio vale). Las sesiones siguen (van por id). Auditoría `user_renamed` ("viejo → nuevo"). En modo local: 403 (se cambia en el server y el dispositivo lo toma al sincronizar) |
| GET | `/api/me/sessions` | | `SessionInfo[]` (las propias, la más usada primero) |
| DELETE | `/api/me/sessions/{id}` | | 204; 404 si no es propia |
| DELETE | `/api/me/sessions` | | `{revoked: n}`: "cerrar sesión en los demás dispositivos" (todas menos la actual) |
| GET | `/api/me/settings` | | `Settings` |
| PUT | `/api/me/settings` | `Settings` parcial + `rd_key?: string` (vacío = borrar). `ui` se mezcla por clave (una clave en `null` se borra); `preferred_quality`/`max_size_gb` en `null` se borran | `Settings` |
| GET | `/api/catalog/{type}/{catalog}` | `?genre=&skip=` (`catalog`: `top`, `imdbRating`, `year`) | `MetaPreview[]` |
| GET | `/api/genres/{type}` | | `string[]` |
| GET | `/api/top/{type}` | `?genres=Crime,Drama` (hasta 3, todos tienen que coincidir; nombres de IMDb, sin distinguir mayúsculas) `&min_votes=` (por defecto 25000 en películas y 10000 en series; piso 1000) `&skip=&limit=` (≤ 100, def. 50) | `TopRated`; 400 con un género desconocido o más de 3. Siempre 200 aunque no haya datos (`state`) |
| GET | `/api/search` | `?q=` | `{movies: MetaPreview[], series: MetaPreview[]}` |
| GET | `/api/meta/{type}/{id}` | | `Meta` |
| GET | `/api/streams/{type}/{id}` | id con episodio para series (`tt…:1:2`). `?caps=` opcional: `ClientCaps` en JSON o lista `h264,hevc,aac,mkv,…`; da bonus a lo que el cliente reproduce directo | `Stream[]` |
| GET | `/api/subtitles/{type}/{id}` | | `{id, lang, url}[]` (OpenSubtitles v3 addon; `url` = `/api/media/subs/external?u=…`, devuelve VTT) |
| POST | `/api/prefetch` | `{info_hash?, file_idx?, url?, stream_key?, session_id?}` (alcanza con `stream_key` de un listado reciente) | 202. Sin `session_id`: arranca el torrent y baja principio y final del archivo sin abrir sesión (la UI lo llama al abrir la lista de streams, para el primero). Con `session_id` (la sesión de media que se está viendo; mandarlo al server de esa sesión): **precarga del próximo episodio** atada a esa sesión, con las reglas de abajo; 404 si la sesión no existe |
| POST | `/api/play` | `{stream_key, info_hash?, file_idx?, url?, caps: ClientCaps, meta_id?, manual?, top_key?, after_stall?}` (`manual`: elegido a mano; `top_key`: el que la app ponía primero; `after_stall`: elegido desde el cartel de "trabado"; ver "Preferencias aprendidas") (si `stream_key` salió de `/api/streams` en las últimas 6 h, el resto es opcional; así el server usa los trackers que trajo Torrentio). Tarda unos segundos (metadata + ffprobe) | `PlayInfo`; 502 con mensaje si no se pudo abrir |
| GET | `/api/progress/continue` | | `Progress[]` (últimos 20, sin terminados > 92 %) |
| GET | `/api/progress/recent` | | `Progress[]` (últimos 30, terminados incluidos, uno por serie: "Recently watched" en la Library; si falta, la UI usa `/continue`) |
| GET | `/api/progress` | `?series_id=tt…` | `Progress[]` de todos los episodios de esa serie, terminados incluidos (la UI marca vistos y el progreso de cada episodio; si falta, usa `/continue`) |
| GET | `/api/progress/{meta_id}` | | `Progress` o 404 |
| PUT | `/api/progress` | `Progress` sin `updated_at` | 204 |
| DELETE | `/api/progress/{meta_id}` | | 204 |
| GET | `/api/library` | | `MetaPreview[]` |
| PUT / DELETE | `/api/library/{type}/{id}` | PUT con `MetaPreview` | 204 |
| GET | `/api/sync` | `?since=<cursor>&limit=` (sin `since` = todo; `limit` ≤ 1000 por tipo) | `SyncPull`: cambios del usuario después del cursor, con lápidas. Si `more`, pedir de nuevo con `since=cursor` |
| POST | `/api/sync` | `Changes` (≤ 5000 registros, ≤ 8 MB) | `SyncPush`. Last-write-wins por registro, idempotente; los inválidos se cuentan en `rejected` y no frenan el lote |
| GET | `/api/admin/users` | | `User[]` |
| POST | `/api/admin/users` | `{username, password, is_admin}` | `User` |
| PATCH | `/api/admin/users/{id}` | `{username?, password?, is_admin?, disabled?}` | `User` (deshabilitar cierra sus sesiones; `username` como en `/api/me/username`: 400/409, auditoría `user_renamed`) |
| DELETE | `/api/admin/users/{id}` | | 204 (no se puede borrar a uno mismo ni al último admin) |
| GET | `/api/admin/users/{id}/sessions` | | `SessionInfo[]` de ese usuario (las de TV que no vencen incluidas; `current`: la del admin); 404 si no existe |
| DELETE | `/api/admin/users/{id}/sessions/{sid}` | | 204 (cierra esa sesión, p. ej. una TV que ya no se usa; auditoría `session_revoked` "usuario: session N"); 404 si no es de ese usuario |
| POST | `/api/admin/avatars` | `multipart/form-data` con `file` (la imagen) y opcional `name`; o la imagen tal cual en el cuerpo con `?name=` (una por pedido). La interfaz recorta antes de subir (centrar y zoom, sale un WebP cuadrado de 512 px; "Upload original" sube el archivo tal cual) | 201 `Avatar`. Solo PNG, JPEG, WebP o GIF reconocidos por el contenido (nunca SVG) → si no, 415; más de 2 MB → 413; más de 200 en la galería → 409. Se guarda en `<data_dir>/avatars/<id>.<ext>` (el nombre del archivo es el id) |
| DELETE | `/api/admin/avatars/{id}` | | 204 (borra el archivo; quien lo tenía queda sin avatar) |
| GET | `/api/admin/torrents` | | `TorrentStat[]` |
| DELETE | `/api/admin/torrents/{info_hash}` | `?files=true` | 204 |
| GET | `/api/admin/stats` | | `{cache_bytes, cache_limit_bytes, disk_free_bytes, active_sessions, users, watching: {username, meta_id, stream_key, session_id, started_at}[]}` (`started_at` en unix s; último play de cada usuario en las últimas 6 h) |
| GET | `/api/admin/metrics` | `?range=24h\|7d\|30d\|90d` (por defecto 24h) | `Metrics` (abajo) |
| GET | `/api/admin/logs` | `?limit=` (≤ 1000, def. 100) `&before=<id>&user=<nombre o id>&ip=&status=404\|5xx&route=<parte de la plantilla>` | `{items: LogRow[], next_before: number \| null}` (del más nuevo al más viejo; `next_before` → `before` de la página siguiente) |
| GET | `/api/admin/audit` | `?limit=&before=&event=&user=` | `{items: AuditRow[], next_before}` |

Todas las rutas `/api/admin/**` dan 403 en modo local.

`type TorrentStat = { info_hash, name, size_bytes, downloaded_bytes, down_bps, up_bps, peers, state, last_used_at }`

### Métricas y auditoría (admin)

Cada request queda registrado sin frenarlo (middleware → canal acotado → escritura por lotes en
SQLite cada ≤ 2 s; si el canal se llena se descarta y se cuenta en `dropped_events`). Se guarda
la **plantilla** de la ruta (`/api/media/{session}/file`), nunca la URL real: ni `?t=` ni ninguna
query llegan a la base. Nunca contraseñas, tokens ni claves de RD. La IP es la del socket; solo si
el socket es loopback (`tailscale serve`) se usan `X-Forwarded-For` (último valor) y
`Tailscale-User-Login`/`Tailscale-User-Name`. La latencia es hasta los headers de la respuesta
(un stream de video no cuenta como lento); los bytes se cuentan al terminar o cortarse el cuerpo.
Retención: crudos 14 días (tope 1 M de filas), agregados por hora 90 días, auditoría 1 año.
`MYTHOS_METRICS=0` lo apaga; en modo local está apagado.

```ts
type Metrics = {
  range: string; from_ms: number; to_ms: number;
  totals: { requests; errors /* ≥500 */; client_errors /* 4xx */; bytes_out; p50_ms?; p95_ms? };
  series: { hour_ms; requests; errors; bytes_out; p95_ms? }[];      // una por hora con tráfico
  top_ips: { ip; requests; bytes_out; users: string[]; tailscale_logins: string[] }[];
  top_users: { user_id; username?; requests; bytes_out }[];
  top_routes: { method; route; requests; errors; bytes_out; p50_ms?; p95_ms? }[];
  status: { status; requests }[];
  failed_logins: { ip; count; last_at_ms; usernames: string[] }[];  // de la auditoría
  dropped_events: number;
};  // percentiles aproximados por histograma (5, 10, 25, 50, 100, 250, 500 ms, 1, 2.5, 5, 10 s)
type LogRow = { id; ts_ms; ip?; tailscale_login?; user_id?; username?; method; route; status; dur_ms; bytes };
type AuditRow = { id; ts_ms; event; user_id?; username?; ip?; detail? };
// event: login_ok, login_fail (username = el nombre probado), logout, session_revoked,
// password_changed, user_created, user_updated, user_disabled, user_deleted, user_renamed ("viejo → nuevo"),
// avatar_added, avatar_removed. En los de admin,
// user_id/username son los del admin que lo hizo y `detail` dice a quién y qué cambió.
```

### Sincronización

Datos: settings (`ui`, `preferred_quality`, `max_size_gb`; **nunca** la clave de RD), progreso y
biblioteca. Cada escritura (por la API normal o por sync) toma un `seq` creciente del server que
la recibe (el cursor de `since`, independiente de los relojes) y un `updated_ms` (reloj del
dispositivo que hizo el cambio). Reglas:

- **Last-write-wins por registro** por `updated_ms`. Empate exacto: gana el borrado y, si no, el
  contenido mayor (JSON canónico): orden total, todas las réplicas convergen.
- **Lápidas**: `DELETE /api/progress/{id}` y `DELETE /api/library/...` dejan `deleted: true` (las
  lecturas normales no las muestran). Una versión vieja que llega tarde no resucita nada.
- **Idempotente**: reaplicar lo mismo no cambia nada ni toma seq nuevo (no rebota).
- Una escritura por la API normal usa `max(ahora, updated_ms anterior + 1)`: lo que el usuario
  hace en un dispositivo siempre le gana a lo que ese dispositivo tenía.
- Settings es un solo registro (no se mezcla por clave de `ui`).

La UI no tiene que implementar nada de esto: en modo local, el server embebido sincroniza solo
(ver abajo). Las rutas existen en los dos modos.

### Modo local (server embebido en la app)

Detalle de flujos y seguridad en `docs/local-mode.md`. Cambios respecto del modo server:

| Método | Ruta | Cuerpo | Respuesta |
|---|---|---|---|
| POST | `/api/auth/login` | `{username, password, device?, remote_url?, offline?}` | `{token, user, offline: boolean, remote: {url, token, user} \| null}`. Con server remoto (de `remote_url`, del vínculo de esa cuenta o de la config) y sin `offline: true`, valida **contra el remoto**: si acepta, enrola/actualiza la cuenta local y devuelve también la sesión del remoto (`remote.token`, para que la UI le hable directo al server). Si el remoto rechaza → 401/429 con su mensaje (no se prueba la local; si dice "This account is disabled", se deshabilita también acá). Si no se alcanza → login offline contra la verificación local (`offline: true`, `remote: null`). Cuenta nunca logueada online en este dispositivo → 401 "… sign in once while the server is reachable" |
| POST | `/api/auth/logout` | | 204. Si era la última sesión local de esa cuenta, cierra también la sesión del remoto guardada (deja de sincronizar hasta otro login online) |
| GET | `/api/local/status` | | `LocalStatus` |
| POST | `/api/local/sync` | opcional `{remote_url?, token?}` | `{pushed, pulled, last_sync_at}`: sincroniza ya. Con `token` (una sesión del remoto, p. ej. de un login hecho directo contra el server) la verifica (`GET /api/me` del remoto: tiene que ser la misma cuenta —por el id del remoto ya vinculado, o por el nombre si todavía no hay vínculo—, si no 409) y la guarda para las próximas. Con `remote_url` además (requiere `token`) pasa a sincronizar por esa dirección: si es otra dirección del mismo server (mismo `server_id`), sigue donde estaba; si es otro server, sincronización completa. 409 si la cuenta nunca se vinculó, 401 si la sesión del remoto venció (hace falta login online), 502 si no se pudo |
| `*` | `/api/admin/**` | | 403 |
| PUT | `/api/me/password` | | 403 |
| PUT | `/api/me/username` | | 403. Si renombran la cuenta en el server, cada sincronización (`GET {remoto}/api/me`) le pone el nombre nuevo a la cuenta local vinculada (por el id del remoto, no por el nombre): el login sin conexión acepta el nuevo y un login online con el nuevo vuelve a la misma cuenta local |

```ts
type LocalStatus = {
  mode: "local"; username: string;
  remote_url: string | null;       // vinculado o el de la config
  remote_signed_in: boolean;       // hay sesión del remoto guardada (si no: login online)
  remote_reachable: boolean | null;// null = sin remoto configurado (se comprueba con
                                   // GET /api/health del remoto, 3 s, cacheado 15 s)
  last_sync_at: number | null;     // ms unix de la última sincronización buena
  last_sync_error: string | null;
  last_sync_error_code: string | null; // ver "Errores": sync_unreachable, sync_failed...
  pending_changes: number;         // cambios locales sin subir
  transcode_available: boolean;    // hay ffmpeg en el dispositivo
  sync_interval_s: number | null;  // null = solo a pedido
  version: string;
};
```

Además, en modo local solo se aceptan requests con `Host` de loopback (`127.0.0.1`, `localhost`,
`[::1]`, con cualquier puerto): 403 si no (protección contra DNS rebinding).

Rutas montadas desde el crate de media, con auth (Bearer o `?t=`):

| Método | Ruta | Respuesta |
|---|---|---|
| GET | `/api/media/{session}/file` | el archivo, con `Range` (206). Prioriza las piezas pedidas. Cada respuesta trae a lo sumo 32 MB (`bytes=X-` contesta `X-(X+32 MB-1)`): el navegador pide el tramo siguiente solo, y así ninguna conexión queda abierta todo el episodio (por `tailscale serve` una conexión larga se degradaba hasta trabar el video) |
| GET | `/api/media/{session}/hls/index.m3u8` | `?start=<s>&audio=<idx>`: playlist HLS que arranca en `start` (para buscar en modo transcode la UI recarga con otro `start` y suma el offset) |
| GET | `/api/media/{session}/hls/{segment}` | segmentos `.ts` o `.m4s` |
| GET | `/api/media/{session}/subs/{index}.vtt` | subtítulo interno en WebVTT. Si la extracción no terminó en ~20 s (o antes, si el archivo está a medio bajar y ffmpeg lleva ~3 s esperando piezas) devuelve lo que hay (VTT válido desde el principio) con el header `X-Mythos-Partial: true`; la UI vuelve a pedirlo cada 5 s para completarlo. Se guarda como completo solo si terminó con el archivo entero bajado y la sesión abierta |
| GET | `/api/media/{session}/status` | `MediaStatus` |
| GET | `/api/media/{session}/thumbs` | `ThumbsIndex` (abajo): las miniaturas para la vista previa al adelantar. La primera vez lanza la generación de fondo. 404 si todavía no se sabe la duración (volver a pedir), 501 sin ffmpeg (la barra sin miniatura) |
| GET | `/api/media/{session}/thumbs/{i}.jpg` | la miniatura `i` (JPEG, `Cache-Control: private, max-age=86400`); 404 si todavía no está |
| DELETE | `/api/media/{session}` | 204 (para ffmpeg; el torrent queda en caché) |
| GET | `/api/media/subs/external` | `?u=<url de OpenSubtitles>` → VTT (gz/srt → vtt) — lo hace el server |
| GET | `/api/media/img` | `?u=<url https de *.metahub.space>&w=<ancho>` → la imagen achicada a 320/480/640/960/1280 px de ancho (el primero ≥ `w`, JPEG), o la original si ya es chica o tiene transparencia. Cacheada en disco (`<data>/img-cache`, tope 512 MB en server y 96 MB en local, se borran las menos usadas) con `Cache-Control: private, max-age=2592000, immutable`. 400 si no es de metahub, 502 si metahub falla |

**Preferencias aprendidas** (`server/src/taste.rs`, tabla `plays`): cada `/api/play` con `meta_id`
guarda el grupo del release (`[Grupo] ...` o `...-GRUPO`, y el `bingeGroup` de Torrentio), calidad,
tamaño, audio dual, los idiomas de audio y subtítulos reales del archivo (ffprobe), si se eligió a
mano (`manual`) y cuál ofrecía la app (`top_key`). Cómo salió se completa solo: `finished` (el
progreso pasó el 90 %), `switched` (antes de 15 min y con menos de 5 vistos se abrió otro stream
del mismo episodio) o `stalled` (`after_stall`). Se guardan las últimas 600 por usuario.

En `/api/streams` se infiere (de las últimas 300): el grupo de la última reproducción buena de esa
serie (confianza 0,6 / 0,8 / 1 con 1 / 2 / 3+ que lo confirman; el que se trabó o se cambió
enseguida no se prefiere y resta), los grupos que se repiten en otras series, los idiomas de
subtítulos buscados (el `subLang` de Settings, o el que tienen de verdad al menos dos elecciones a
mano buenas, sin contar inglés) y el audio dual (dos elecciones a mano). Los subtítulos de un stream
que no se abrió se adivinan por su nombre y por los que tuvo su grupo. El score suma: mismo grupo
de la serie +60 × confianza, grupo que repite +15, grupo malo −40, subtítulos buscados +50, audio
dual +25, en total entre −60 y +110: la salud del torrent sigue mandando (un preferido con 2
seeders no le gana a uno sano). Sin historial no cambia nada.

**Vista previa al adelantar (`/api/media/{session}/thumbs`)**, `media/src/thumbs.rs`:

```
ThumbsIndex { interval_s: number, count: number, width: number, height: number,
              ready: string,   // un carácter por miniatura: "1" lista, "0" todavía no
              done: boolean }  // no van a aparecer más (todas listas o se dejó de intentar)
```

- La miniatura `i` es del momento `i * interval_s + interval_s / 2` y representa el tramo
  `[i * interval_s, (i + 1) * interval_s)`: para el segundo `x`, `i = floor(x / interval_s)`.
- `interval_s` = duración / 300 redondeado para arriba, nunca menos de 5 s (un episodio de 20 min:
  cada 5 s; una película de 2 h: cada 24 s, 300 miniaturas). `width` 240 px; `height` según la
  relación de aspecto (par). ~5-10 KB cada una.
- Generación (de fondo, por sesión, empieza con el primer pedido del índice): solo con la sesión
  reproduciendo (no mientras arranca o tras un salto; un link directo además sin `starving`,
  porque leer el archivo es bajarlo del origen), un ffmpeg a la vez en todo el server, con un hilo,
  sin NVENC, prioridad más baja del sistema y 15 s de tope (si se queda esperando piezas no cuenta
  como fallo: se reintenta en la pasada siguiente); cada una decodifica solo el cuadro
  clave más cercano (`-skip_frame nokey -noaccurate_seek`). ffmpeg lee con el rol pasivo y solo para
  momentos cuyo tramo del archivo (estimado por el bitrate medio: 8 s antes y 12 s después, entre
  2 y 24 MB) ya está bajado: con el
  archivo a medio bajar hay miniaturas de lo que está y las demás aparecen en las pasadas
  siguientes (cada 5-20 s). Orden por pasadas (una cada 16, después cada 8, 4, 2, 1). 8 fallos
  seguidos con el tramo bajado: se abandona (`done: true`).
- Cache: `<data>/thumbs/{hash}_{archivo}_{ancho}x{alto}_{intervalo}/` para torrents (para siempre;
  se borra con el torrent), la carpeta temporal de la sesión para links directos.
- La UI pide el índice al empezar a reproducir y cada 15 s hasta `done`; muestra la miniatura del
  tramo si está lista y si no solo el tiempo. Un server viejo (404) o sin ffmpeg (501): sin
  miniatura.

**Top rated (`/api/top`)**: lo mejor calificado de IMDb, de los datasets públicos
(`datasets.imdbws.com`: `title.ratings.tsv.gz` ~9 MB + `title.basics.tsv.gz` ~230 MB, uso
personal y no comercial, sin clave). Se eligieron porque el catálogo `imdbRating` de Cinemeta no
está ordenado por rating (se parece a "popular") y acepta un solo género, y los addons de terceros
con "IMDb Top" dependen de un server ajeno. El server los baja al arrancar si no tiene nada (a
los ~20 s, o enseguida si alguien pide la lista) y después cada 7 días (si falla, a las 6 h, y
sigue con lo que tenía); los procesa en streaming (nunca guarda el archivo grande) y deja solo
películas (`movie`) y series (`tvSeries`, `tvMiniSeries`) no adultas con ≥ 1000 votos en
`<data>/imdb/top-v1.tsv.gz` (~1,3 MB, ~50 mil películas y ~13 mil series). Pósters y fondos: los de
metahub por id de IMDb (`/poster/small/{id}/img`, los mismos de Cinemeta); los títulos sin póster
caen en el póster tipográfico. Páginas cacheadas 1 h. En modo local está apagado (230 MB por
semana en un celular no): responde `state: "unavailable"` y la UI lo pide al server de casa.
`MYTHOS_IMDB=0` lo apaga también en el server.

**Imágenes achicadas (`/api/media/img`)**: metahub da los fondos en 480 px (`background/small`) o
1920 px (`background/medium`, ~800 KB y ~8 MB decodificado). La UI (`apps/web/src/lib/images.ts`)
pide el tamaño justo para cada lugar: hasta 480 px `small` directo de metahub; entre 480 y 1280 px
esta ruta; más, la original (en equipos flojos, `lib/perf.ts`, el tope es 1280). Las miniaturas
de episodios (`episodes.metahub.space/…/w780.jpg`) se piden en el tamaño de TMDB que alcanza
(`w185`…`w780`) sin pasar por acá, y los pósters ya vienen chicos en WebP. Si la ruta falla (un
server viejo), la UI vuelve a la URL original. `GET /api/health` trae `images: true`.

**Precarga del próximo episodio** (`POST /api/prefetch` con `session_id`, `Media::prefetch_next`).
La UI la pide cuando al episodio que se está reproduciendo le quedan 10 min o pasó el 70 % (lo
que llegue primero), con el stream del siguiente: el mismo torrent si es un pack de temporada que
lo trae (otro archivo), si no el que elegiría al abrirlo (idioma de audio preferido, después el
score); y al pasar de episodio abre exactamente ese stream (`/watch/series/<id>?s=<key>`). En el
mismo momento la UI baja la lista de subtítulos del siguiente y el de OpenSubtitles del idioma de
Settings (`apps/web/src/lib/subcache.ts`), así aparece apenas arranca; los internos no se pueden
adelantar (están repartidos por todo el archivo y el server los extrae a medida que baja). El
server:

1. **Nunca perjudica al episodio actual.** No agrega ni baja nada hasta que el actual tiene ≥ 5 min
   bajados por delante del lector (o ya está todo lo que falta hasta el final) y ninguna sesión
   pasa hambre (arrancando, tras un salto, o con < 20 s de colchón). Si el actual baja de 2,5 min
   de colchón, se frena al instante: el archivo sale de la selección del torrent (mismo pack) o el
   torrent se pausa (otro torrent), y vuelve cuando se recupera. Prioridad mínima: un solo
   "fantasma" de 32 MB a la vez (primero el final, después el principio) contra las ventanas del
   actual.
2. **Si se deja de mirar, se cancela y se borra**: la sesión se cierra y en 45 s no se abre el
   siguiente; se abre otra cosa (otro título, otro stream) con esa sesión ya cerrada; la sesión no
   pide nada en 10 min (pausa larga); o llega otra precarga para la misma sesión. Torrent nuevo
   (lo agregó la precarga) → se quita con sus archivos; mismo pack, o un torrent que ya estaba en
   caché → su archivo vuelve a prioridad 0 (sale de la selección) y queda como estaba. La
   precarga nunca anota uso, así que ni la LRU ni el vencimiento la retienen; si el motor se cierra
   en el medio, al arrancar borra los torrents de precargas que nunca se abrieron
   (`<data>/next-prefetch.json`).
3. **Si empieza el siguiente episodio** (`/api/play` con la misma fuente, también en esos 45 s),
   lo precargado se reusa: el torrent queda como cualquiera que se está viendo.
4. **Tope**: el final del archivo (índice/moov/cues, 4–16 MB) y 90 s de video del principio
   (bitrate supuesto por nombre y tamaño), entre 48 y 128 MB. Cuando está, se frena como en 1: no
   baja el episodio entero. Mientras baja, librqbit les da a los peers sin trabajo piezas en orden
   natural, así que se pasa un poco (medido con ~200 Mbit: 52–55 MB pedidos, 77–108 MB en disco).
   Medido (`backend/media/examples/warm.rs`): el siguiente pasa a `ready` en 0,7–0,9 s contra
   7–9 s en frío.

**Velocidad con el puerto abierto** (detalle en la cabecera de `backend/media/src/engine.rs`,
"Peers y NAT"): hasta 200 peers por torrent en el server (`MYTHOS_PEER_LIMIT`, 160 en modo local),
subida sin tope, DHT + PEX + LSD + uTP, ~60 trackers públicos (ngosang "best" + newTrackon
"stable", refrescados cada 12 h) y el puerto de peers anunciado tal cual. La sesión que pasa
hambre tiene prioridad: las precargas le ceden (fantasmas fuera, torrent en pausa) y los torrents
sin uso se pausan enseguida.

**HLS espera, no da 404**: un segmento o la playlist que ffmpeg todavía no generó se queda
esperando a que exista (hasta 120 s la playlist y 90 s un segmento) en vez de responder 404, que
haría que hls.js lo dé por perdido. Los timeouts del cliente (hls.js `manifestLoadingTimeOut`,
`fragLoadingTimeOut`, etc.) tienen que ser de al menos ~120 s.

**HLS se reanuda solo**: sin pedidos durante 60 s se mata ffmpeg pero los segmentos quedan. El
próximo pedido de la playlist (con el mismo `start`/`audio`) o de un segmento que falta relanza
ffmpeg desde el final de lo ya generado, con la misma numeración y timestamps continuos: la UI no
tiene que hacer nada (ni recalcular `start`). Cambiar `start` o `audio` arranca otro trabajo; los
segmentos llevan la clave del trabajo en el nombre (`s<ms>a<pista|d>_00012.m4s`) y uno de otro
trabajo da 404. La playlist es EVENT y termina en `#EXT-X-ENDLIST` cuando ffmpeg llega al final;
en hls.js hay que pasar `startPosition: 0` (si no, trata la EVENT como en vivo y arranca cerca del
final de lo generado).

**`MediaStatus.buffered_ahead_s`** es una estimación: segundos bajados desde donde arrancó la
reproducción (o el último salto) menos lo que se supone ya reproducido desde que pasó a `ready`.
Sirve para la pantalla de carga; durante la reproducción la UI tiene que fiarse de los eventos del
`<video>` (`waiting`/`playing`), porque una pausa del usuario el server no la ve.

Los headers expuestos por CORS son `Content-Range`, `Accept-Ranges`, `Content-Length` y
`X-Mythos-Partial`.

## Embeber el server (crate `mythos`, `backend/server`)

`backend/server` es una biblioteca (`mythos`) más un binario fino (`main.rs`: lee las variables
`MYTHOS_*`, comandos `adduser`/`passwd`/`health` y Ctrl+C/SIGTERM). Las apps (Tauri) dependen de la
biblioteca: `mythos = { path = "../../backend/server" }` (con `default-features` no hace falta
nada más; compila para `x86_64-pc-windows-msvc` y `aarch64-linux-android`, ver
`docs/local-mode.md`).

```rust
pub enum Mode { Server, Local }
pub struct Config {
    pub mode: Mode,
    pub bind: SocketAddr,              // Config::local: 127.0.0.1:0 (puerto libre)
    pub data_dir: PathBuf,             // mythos.db + caché de torrents
    pub web_dir: Option<PathBuf>,      // None = no sirve UI (local)
    pub cache_gb: f64,                 // 0 = sin tope (local)
    pub peer_limit: usize,             // peers por torrent: server 200 (MYTHOS_PEER_LIMIT), local 160
    pub cache_max_age: Option<Duration>, // local: 24 h desde el último uso; server: None
    pub peer_port: u16 /* 0 = efímero */, pub hwaccel: HwAccel,
    pub admin_user: Option<String>, pub admin_password: Option<String>, pub dev_users: bool, // solo server
    pub torrentio_url: String,
    pub ffmpeg: PathBuf, pub ffprobe: PathBuf,   // ruta vacía = sin ffmpeg (solo directo)
    pub extra_trackers: bool,
    pub metrics: bool,                 // local: false
    pub remote_url: Option<String>,    // local: server remoto por defecto
    pub sync_interval: Option<Duration>, // local: Some(60 s); None = solo a pedido
    pub imdb_top: bool,                // "Top rated" (datasets de IMDb); local: false
}
impl Config {
    pub fn server(data_dir) -> Self;   // los mismos valores que el binario sin variables
    pub fn local(data_dir) -> Self;    // 127.0.0.1:0, sin UI, sin métricas, caché 20 GB,
                                       // peers en puerto efímero, sync cada 60 s, sin Top rated
}
pub async fn run(cfg: Config) -> anyhow::Result<Handle>;   // vuelve cuando ya escucha
pub struct Handle;
impl Handle {
    pub fn addr(&self) -> SocketAddr;  pub fn port(&self) -> u16;
    pub fn base_url(&self) -> String;  // "http://127.0.0.1:<puerto>"
    pub fn mode(&self) -> Mode;  pub fn media_tools(&self) -> MediaTools;
    pub async fn stopped(&mut self) -> anyhow::Result<()>;  // si el server se cae solo
    pub async fn shutdown(self) -> anyhow::Result<()>;      // cierre ordenado (≤ ~10 s; ~1 s típico)
}
pub async fn cli(cfg: &Config, args: &[String]) -> anyhow::Result<()>;  // adduser / passwd
pub use mythos_media::HwAccel;
```

La biblioteca no inicializa `tracing` (lo hace quien la usa). Necesita un runtime de tokio
multihilo (el de Tauri, `tauri::async_runtime`, sirve).

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
    // Precarga del próximo episodio atada a la sesión que se ve (reglas en "Precarga del próximo
    // episodio"); no bloquea; error si la sesión no existe. `next_status`: estado (diagnóstico).
    pub fn prefetch_next(&self, session_id: &str, src: Source) -> anyhow::Result<()>;
    pub fn next_status(&self, session_id: &str) -> Option<NextStatus>; // {state: waiting|active|done, info_hash, file_idx, same_torrent, have_bytes, target_bytes}
    pub fn announce_port(&self) -> Option<u16>;    // el puerto que se anuncia a trackers y DHT
    pub fn is_downloaded(&self, info_hash: &str, file_idx: Option<usize>) -> bool; // para Stream.cached
    pub fn status(&self, session_id: &str) -> Option<MediaStatus>; // lo mismo que GET /{session}/status
    pub async fn close(&self, session_id: &str);   // lo mismo que DELETE /{session}: mata ffmpeg
    // Para experimentos (examples/sim.rs); el server usa `new` = `Tuning::default()`:
    pub async fn with_tuning(cfg: MediaConfig, tuning: Tuning) -> anyhow::Result<Self>;
    pub fn set_download_limit(&self, bps: Option<u32>); // tope de bajada en caliente
    pub fn tools(&self) -> MediaTools;   // {ffmpeg, ffprobe}: detectados al arrancar (`-version`)
    pub async fn shutdown(&self);        // cierra sesiones (mata ffmpeg), para el endpoint interno
                                         // y la sesión de librqbit (suelta puertos y archivos)
}
pub fn http_client_builder() -> reqwest::ClientBuilder; // TLS según plataforma (Android: raíces
                                                        // de Mozilla embebidas, ver local-mode.md)
pub enum Source { Torrent { info_hash: String, file_idx: Option<usize>, trackers: Vec<String> }, Url(String) }
```

**ffmpeg opcional**: si `ffmpeg`/`ffprobe` no están (o la ruta es vacía), el motor arranca igual.
Sin ffprobe, el sondeo lo hace un lector propio de cabeceras MP4 (`moov`) y Matroska/WebM
(`Info`/`Tracks`, con SeekHead) por el mismo endpoint interno (así las piezas que lee se bajan con
prioridad); si falla, se adivina por el nombre. Sin ffmpeg: `PlayInfo.mode` siempre `direct`,
`hls_url` null, sin subtítulos internos ni miniaturas, y `/hls/*`, `/subs/*` y `/thumbs*` responden 501 con un mensaje
claro. `/api/streams` además hunde en la lista lo que según `caps` no se reproduce directo.

Los tipos `ClientCaps`, `PlayInfo`, `Track`, `MediaStatus`, `TorrentStat`, `MediaStats`, `MediaTools`, `ProbeSource` viven en

`mythos-media` (serde) y el server los reexporta tal cual. `Track` implementa `Default`. Ojo: en
`PlayInfo` los opcionales salen como `null` (no se omiten), así que la UI tiene que tratar `null`
como ausente.
