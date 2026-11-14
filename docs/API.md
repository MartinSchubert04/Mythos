# Mythos contract

This is the source of truth between the three modules (server, media, web). If something changes,
it changes here first.

## Priorities (user's request, in this order)

1. **The movie must not stall.** As much as possible from the code: prioritize pieces, initial
   buffer, preloading, recovery from drops, picking the right stream, switching streams without
   leaving.
2. **Lightweight**: little RAM, small bundle, no heavy dependencies without a reason.
3. **Pretty and animated**, user friendly, good "feel".

## General

- All JSON, `snake_case`. Errors: HTTP status + `{"error": "human-readable text in English"}` (also
  for malformed JSON and non-existent `/api` routes) and, for the ones the UI shows, `code` and
  `params` (see "Errors", below). 502 = an external service (Cinemeta, Torrentio, OpenSubtitles)
  or media failed; if there is an old cached response, the server uses it instead of failing.
- Auth: `Authorization: Bearer <token>`. Routes under `/api/media/**` also accept `?t=<token>`
  (for `<video>`, hls.js and `<track>`). Sessions last **180 days from last use** (they renew
  themselves; login is a one-time gate per device). Logout revokes the session; each user sees
  and closes their own in `/api/me/sessions`.
- Sessions that **never expire** (`persistent: true` in the login; the TV app requests them, where
  each account is a saved profile and the password is never asked again): `expires_at` stays at
  `253402300799` (9999-12-31) and is not renewed; only the last use is recorded, once a day. They
  are revoked like the rest: logout, `/api/me/sessions` (from any device), password change,
  disabling the account, or the admin (Admin → Users → "Signed-in devices",
  `/api/admin/users/{id}/sessions`). The usual ones (web, phone, PC) don't change.
- 401 → the UI goes back to login. 403 → not an admin.
- CORS: allow `http://localhost:5173`, `tauri://localhost`, `http://tauri.localhost`,
  `https://tauri.localhost` (desktop and Android) with `Authorization` and `Range`; expose
  `Content-Range`, `Accept-Ranges`, `Content-Length`. Answers Private Network Access preflights
  (`Access-Control-Allow-Private-Network: true`) so the Tauri window can talk to the local server
  on `127.0.0.1`.
- The server serves the compiled UI (`MYTHOS_WEB_DIR`, `./web` by default) at `/` with fallback to
  `index.html` (SPA). Embedded in the apps (local mode) it serves no UI: the app brings it.
- There are two modes (`GET /api/health` → `mode`): **`server`** (the home one, Docker) and
  **`local`** (the server embedded in the desktop/Android app, see `docs/local-mode.md`). The
  differences are marked on each route.
- **Server identity** (`GET /api/health` → `server_id`): 32 random hex characters that the server
  creates the first time and stores in its database (`server_meta`); it survives restarts and
  updates, and a new database is a different server. The apps compare it to know that several
  addresses (Tailscale, the home network with `MYTHOS_EXTRA_BIND`) lead to the **same** server: the
  same sessions work on all of them (`docs/local-mode.md`, "Multiple server addresses"). An old
  server doesn't send it: the app accepts it without being able to compare. The embedded server
  (local mode) has its own, different one.

### Errors

```jsonc
{ "error": "Torrentio answered 503 Service Unavailable, try again in a moment",  // always, in English
  "code": "torrentio_status",          // stable, snake_case; missing on validation errors
  "params": { "status": 503 } }        // only if the text carries data
```

- `error` doesn't change shape (old clients keep showing it). The UI translates by `code`
  (`apps/web/src/lib/i18n/server.ts`: the key is `srv.` + the code in camelCase) with `params`
  for the data; if there is no code (an old server) or it doesn't know it, it recognizes the text,
  and if not that either, it shows it as is. **Changing a sentence breaks nothing; changing a code
  does**: a new code needs its `srv.*` key in `lib/i18n/es` and `lib/i18n/en`.
- API validation errors (malformed JSON, an invalid field, metrics `range`) have no code: the user
  doesn't see them.
- Codes and their `params`:
  - Login and account: `bad_login`, `bad_login_offline` (local mode without server), `disabled`,
    `too_many_logins` (`n`: minutes), `no_token`, `expired`, `bad_token`, `admin_only`,
    `admin_local`, `user_exists`, `user_not_found`, `bad_username`, `pw_short`, `pw_long`,
    `pw_wrong`, `pw_on_server`, `rename_on_server`, `other_account`, `session_not_found`.
  - Admin: `self_admin`, `self_disable`, `self_delete`, `last_admin`, `need_admin`; avatars:
    `avatar_not_found`, `avatar_too_big`, `avatar_type`, `gallery_full` (`n`).
  - External services: `torrentio_down`, `torrentio_bad`, `torrentio_status` (`status`),
    `cinemeta_down`, `cinemeta_bad`, `cinemeta_status` (`status`), `title_not_found`,
    `subs_down`, `subs_bad`, `subs_status` (`status`), `sub_download`, `sub_too_large`,
    `sub_corrupt`, `img_download`, `img_too_large`, `query_long`, `max_genres` (`n`).
  - Streams and playback: `unknown_stream`, `rd_missing`, `rd_invalid`, `only_torrentio`,
    `media_session`, `play_failed` (`e`: the detail, in English), `no_peers` (`s`: seconds it
    waited), `no_space`, `torrent_error` (`e`), `no_ffmpeg`, `buffering`, `sub_not_found`.
  - Local mode: `bad_server_url`, `server_url`, `sync_not_linked`, `sync_signed_out`,
    `sync_unreachable`, `sync_unsupported`, `sync_failed` (`e`). Local mode passes through as is
    whatever code the home server sends (e.g. `bad_login` in the online login).
  - `internal` (500): the detail stays in the server log.
- `GET /api/media/{session}/status` carries the same for the session error: `error`,
  `error_code` (`no_space`, `torrent_error`) and `error_params`. `GET /api/local/status`:
  `last_sync_error` and `last_sync_error_code`.
- In the media crate: `mythos_media::Coded { code, message, params }`, which travels inside the
  `anyhow::Error` of `Media::open` (the server extracts it with `downcast_ref`).

## Types

```ts
type User = { id: number; username: string; is_admin: boolean; disabled: boolean; created_at: string;
              avatar_id: number | null };   // the avatar chosen from the gallery (null = the initial)

type Avatar = { id: number; name: string; created_at: number };   // unix s

type MetaPreview = {           // a catalog card
  id: string;                  // imdb id, "tt0111161"
  type: "movie" | "series";
  name: string;
  poster?: string; background?: string; logo?: string;
  year?: string;               // "2010" or "2008-2013"
  imdb_rating?: string;
  genres?: string[];
  description?: string;
};

type Meta = MetaPreview & {
  runtime?: string; cast?: string[]; director?: string[];
  trailer_youtube_id?: string;
  videos?: Episode[];          // series only, sorted by season, episode
};

type Episode = {
  id: string;                  // "tt0944947:1:1"
  season: number; episode: number;
  name?: string; overview?: string; thumbnail?: string; released?: string;
};

type Stream = {
  key: string;                 // "<info_hash>:<file_idx>", "<info_hash>" if Torrentio doesn't say the
                               // file, or "url:<short hash>" (Real-Debrid). Treat it as opaque.
  info_hash?: string;          // lowercase, 40 hex
  file_idx?: number;
  url?: string;                // only with Real-Debrid: Torrentio link with the key replaced
                               // by "_RD_" (the key never leaves the server; /api/play puts it back)
  title: string;               // release name ("Movie.2010.1080p.BluRay.x264-GRP")
  file_name?: string;
  quality: "2160p" | "1080p" | "720p" | "480p" | "SD" | "CAM" | "unknown";
  size_bytes?: number;
  seeders?: number;
  source?: string;             // "ThePirateBay", "YTS", ...
  tags: string[];              // "HDR", "DV", "HEVC", "x264", "10bit", "Atmos", "5.1", "REMUX", "WEB-DL"...
  languages: string[];         // flags/languages Torrentio provides ("🇪🇸", "multi"...)
  cached?: boolean;            // in the Real-Debrid cache or already downloaded on the server
  score: number;               // higher = better. The list comes sorted by score desc.
  reasons?: string[];          // why what was learned about the user boosts it ("Learned preferences"):
                               // "same_group" | "liked_group" | "subs:<language>" | "dual_audio"
};

type TopRated = {              // GET /api/top/{type} (see "Top rated" below)
  state: "ready"               // there is data
       | "preparing"           // the first IMDb download is on its way (the UI retries on its own)
       | "unavailable";        // this server doesn't have it (local mode, or MYTHOS_IMDB=0)
  items: MetaPreview[];        // by rating and then by votes, highest first
  total: number;               // titles matching the filter
  more: boolean;               // there are more after skip + items.length
  genres: { name: string; count: number }[];  // the type's genres with that vote minimum;
                               // count = how many would remain adding it to the filter (selected ones: total)
  min_votes: number;           // the one used (with the default and the floor applied)
  updated_at: number | null;   // unix s of the IMDb download
};

type Recommended = {           // GET /api/recommended/{type} (see "Recommendations" below)
  state: "ready" | "preparing" | "unavailable";   // like TopRated: without "ready", items is empty
  basis: "taste"               // based on the user's taste
       | "top"                 // no taste yet (< 3 titles): random from the top 300
       | "like"                // similar to the `like` title
       | null;                 // without "ready"
  items: MetaPreview[];        // up to `limit`, in draw order. Without `like`, the first 4
                               // carry Cinemeta's `description` and `logo` if they arrived within 2 s
  genres: string[];            // the genres that weigh most in the taste (up to 5; empty without taste)
  rotates_at: number;          // unix s when the selection changes (6 h blocks)
};

type ClientCaps = {            // what the client's player can play directly
  h264: boolean; hevc: boolean; av1: boolean; vp9: boolean;
  mkv: boolean;                // the browser opens .mkv with <video>
  aac: boolean; ac3: boolean; eac3: boolean; dts: boolean; opus: boolean; flac: boolean;
};

type Track = { index: number; codec: string; lang?: string; title?: string; default?: boolean };

type PlayInfo = {
  session_id: string;          // 128 random bits, url-safe
  mode: "direct" | "remux" | "transcode";
  // URLs relative to the server, without the token (the UI adds ?t=)
  direct_url: string;          // /api/media/{session}/file  (Range, always available)
  hls_url?: string;            // /api/media/{session}/hls/index.m3u8  (remux or transcode)
                               // Recommended in direct too (on-demand remux): the UI uses it
                               // to switch audio track without leaving the player.
  file_name: string; size_bytes: number;
  duration_s?: number;
  container?: string; video_codec?: string; width?: number; height?: number; hdr?: boolean;
  audio_tracks: Track[];
  subtitle_tracks: Track[];    // internal; text (srt/ass/mov_text) convertible to VTT. Empty
                               // if there is no ffmpeg
  transcode_available: boolean;// the server has ffmpeg. If false: `mode` always "direct",
                               // `hls_url` null and no internal subtitles (local mode without ffmpeg)
  direct_ok: boolean;          // according to the probe, `caps` plays the file as is. With
                               // transcode_available=false and direct_ok=false, it probably
                               // won't show or won't sound: the UI should offer another stream
  probe_source: "ffprobe" | "sniff" | "guess";  // confidence of the technical data: ffprobe,
                               // MP4/MKV headers read in Rust, or guessed from the name
};

type MediaStatus = {
  state: "metadata" | "probing" | "buffering" | "ready" | "error";
  error?: string;
  error_code?: string;         // see "Errors": no_space, torrent_error
  error_params?: object;       // {"e": "..."} in torrent_error
  down_bps: number; up_bps: number;
  peers: number;               // connected
  downloaded_bytes: number; total_bytes: number;   // of the chosen file
  progress: number;            // 0..1 of the file
  buffered_ahead_s?: number;   // seconds ready after the current position, if known
  buffer_target_s?: number;    // seconds of buffer it waits for before switching to "ready" (to
                               // draw the "loading" bar: buffered_ahead_s / buffer_target_s)
  have_ranges?: [number, number][]; // ranges of the file already downloaded on the server, fractions 0..1
                               // (≤ 64, the smallest gaps get merged; torrents only). The player's
                               // bar draws them fainter under what the video has buffered
};

type Progress = {
  meta_id: string;             // "tt0111161" or "tt0944947:1:2"
  type: "movie" | "series";
  series_id?: string;          // "tt0944947" for episodes
  name: string; poster?: string; background?: string;
  episode_label?: string;      // "S01E02 · The Kingsroad"
  position_s: number; duration_s: number;
  stream_key?: string;         // to resume with the same stream
  updated_at: string;
};

type Settings = {              // per user, the UI stores whatever it wants inside `ui`
  ui: Record<string, unknown>; // theme, palettes, language, subtitles, autoplay... (see below)
  has_rd_key: boolean;         // read only
  preferred_quality?: "2160p" | "1080p" | "720p";
  max_size_gb?: number;
};

// What the UI stores in `ui` today (the server doesn't validate it: it's JSON of up to 64 KB). All of
// it belongs to the user; what depends on the device (volume, TV mode, lite mode) doesn't travel.
//   lang: "es" | "en"             interface language
//   theme: string                 "neovim", "warp"... (apps/web/src/lib/themes.svelte.ts)
//   palettes: Record<theme, id>   the palette chosen for each theme
//   glass: {on, opacity}          desktop frosted glass (opacity 0.25–1)
//   subLang, subAlways, subSize, subBg, subPos, audioLang, autoplayNext, hideSpoilers,
//   reduceMotion                  (apps/web/src/lib/prefs.svelte.ts, UiPrefs)
// Theme and glass are sent only when they change (the rest, on every PUT). If missing, the UI
// gives the user the device's ones and saves them (migration; glass, only from desktop).

type SessionInfo = {           // an open session of the user (never the token)
  id: number;
  device?: string;             // the label the UI sent at login
  user_agent?: string; last_ip?: string;
  created_at: number; last_used_at: number; expires_at: number;   // unix s
  persistent: boolean;         // never expires (expires_at = 253402300799)
  current: boolean;            // it's the one of this request
};

// ---------- sync (see "Sync" below) ----------
type ProgressRec = Omit<Progress, "updated_at"> & { updated_ms: number; deleted: boolean };
type LibraryRec = { type: "movie" | "series"; id: string; item: MetaPreview | null;  // null in tombstones
                    updated_ms: number; deleted: boolean };
type SettingsRec = { ui: Record<string, unknown>; preferred_quality: string | null;
                     max_size_gb: number | null; updated_ms: number };      // never the RD key
type Changes = { settings?: SettingsRec | null; progress: ProgressRec[]; library: LibraryRec[] };
type SyncPull = Changes & { cursor: number; more: boolean };
type SyncPush = { applied: number; ignored: number; rejected: number };
```

## Server routes (`backend/server`)

| Method | Route | Body / query | Response |
|---|---|---|---|
| GET | `/api/health` | | `{ok:true, version, mode: "server"\|"local", transcode: boolean, images: boolean, server_id: string}` (no auth; `transcode` = ffmpeg is present; `images` = `/api/media/img` exists; `server_id` = stable identity of this server, see above) |
| POST | `/api/auth/login` | `{username, password, device?, persistent?}` (`device`: label for the session list, ≤ 64; `persistent: true`: session that never expires, see General) | `{token, user}`; 401 if it fails or the account is disabled. Rate limit per IP: 10 failures / 5 min → 429. Differs in local mode: see "Local mode" |
| POST | `/api/auth/logout` | | 204 (revokes the session) |
| GET | `/api/me` | | `User` |
| PATCH | `/api/me` | `{avatar_id?: number \| null}` (`null` removes the avatar; without the field it doesn't change) | `User`; 404 if the avatar doesn't exist. Also in local mode (the embedded one has no gallery: only `null`) |
| GET | `/api/avatars` | | `Avatar[]` (the gallery; empty in local mode). The server ships 12 built-in ones (`backend/server/assets/avatars`, original drawings): they are added to the gallery only once per database (`default_avatars` flag in `server_meta`), so if the admin deletes one it doesn't come back |
| GET | `/api/media/avatars/{id}` | Bearer or `?t=` (for `<img>`) | the image, with its `Content-Type`, `nosniff` and `Cache-Control: private, max-age=31536000, immutable` (ids are not reused) |
| PUT | `/api/me/password` | `{current, new}` | 204 (closes the other sessions). In local mode: 403 (it's changed on the server) |
| PUT | `/api/me/username` | `{username}` (same rules as on creation: 1-32, letters, digits, `_ - .`) | `User`; 409 if someone else has it (case-insensitive; changing only the case of your own is fine). Sessions remain (they go by id). Audit `user_renamed` ("old → new"). In local mode: 403 (it's changed on the server and the device picks it up when syncing) |
| GET | `/api/me/sessions` | | `SessionInfo[]` (your own, most used first) |
| DELETE | `/api/me/sessions/{id}` | | 204; 404 if it's not yours |
| DELETE | `/api/me/sessions` | | `{revoked: n}`: "sign out on all other devices" (all but the current one) |
| GET | `/api/me/settings` | | `Settings` |
| PUT | `/api/me/settings` | Partial `Settings` + `rd_key?: string` (empty = delete). `ui` is merged per key (a key set to `null` is deleted); `preferred_quality`/`max_size_gb` set to `null` are deleted | `Settings` |
| GET | `/api/catalog/{type}/{catalog}` | `?genre=&skip=` (`catalog`: `top`, `imdbRating`, `year`) | `MetaPreview[]` |
| GET | `/api/genres/{type}` | | `string[]` |
| GET | `/api/top/{type}` | `?genres=Crime,Drama` (up to 3, all must match; IMDb names, case-insensitive) `&min_votes=` (default 25000 for movies and 10000 for series; floor 1000) `&skip=&limit=` (≤ 100, default 50) | `TopRated`; 400 with an unknown genre or more than 3. Always 200 even when there is no data (`state`) |
| GET | `/api/recommended/{type}` | `?limit=` (≤ 40, default 20) `&like=tt…` ("Similares" ("More like this"): similar to that title) `&genres=Crime,Drama` (the title's own, in case it's not in the IMDb data) | `Recommended`; 400 with an invalid id. Always 200 even when there is no data (`state`) |
| GET | `/api/search` | `?q=` | `{movies: MetaPreview[], series: MetaPreview[]}` |
| GET | `/api/meta/{type}/{id}` | | `Meta` |
| GET | `/api/streams/{type}/{id}` | id with episode for series (`tt…:1:2`). Optional `?caps=`: `ClientCaps` as JSON or a list `h264,hevc,aac,mkv,…`; gives a bonus to what the client plays directly. Optional `?quality=2160p|1080p|720p`: that device's preferred quality (the app always sends it: 4K by default on TV, 1080p elsewhere, or what was chosen in Settings on that device); without it (old clients), the account's `preferred_quality` | `Stream[]` |
| GET | `/api/subtitles/{type}/{id}` | | `{id, lang, url}[]` (OpenSubtitles v3 addon; `url` = `/api/media/subs/external?u=…`, returns VTT) |
| POST | `/api/prefetch` | `{info_hash?, file_idx?, url?, stream_key?, session_id?}` (`stream_key` from a recent listing is enough) | 202. Without `session_id`: starts the torrent and downloads the start and end of the file without opening a session (the UI calls it when opening the stream list, for the first one). With `session_id` (the media session being watched; send it to that session's server): **next-episode prefetch** tied to that session, with the rules below; 404 if the session doesn't exist |
| POST | `/api/play` | `{stream_key, info_hash?, file_idx?, url?, caps: ClientCaps, meta_id?, manual?, top_key?, after_stall?}` (`manual`: picked by hand; `top_key`: the one the app put first; `after_stall`: picked from the "stalled" notice; see "Learned preferences") (if `stream_key` came from `/api/streams` in the last 6 h, the rest is optional; that way the server uses the trackers Torrentio provided). Takes a few seconds (metadata + ffprobe) | `PlayInfo`; 502 with a message if it couldn't be opened |
| GET | `/api/progress/continue` | | `Progress[]` (last 20, excluding finished ones > 92 %) |
| GET | `/api/progress/recent` | | `Progress[]` (last 30, finished included, one per series: "Recently watched" in the Library; if missing, the UI uses `/continue`) |
| GET | `/api/progress` | `?series_id=tt…` | `Progress[]` of all the episodes of that series, finished included (the UI marks watched ones and each episode's progress; if missing, it uses `/continue`) |
| GET | `/api/progress/{meta_id}` | | `Progress` or 404 |
| PUT | `/api/progress` | `Progress` without `updated_at` | 204 |
| DELETE | `/api/progress/{meta_id}` | | 204 |
| GET | `/api/library` | | `MetaPreview[]` |
| PUT / DELETE | `/api/library/{type}/{id}` | PUT with `MetaPreview` | 204 |
| GET | `/api/sync` | `?since=<cursor>&limit=` (without `since` = everything; `limit` ≤ 1000 per type) | `SyncPull`: the user's changes after the cursor, with tombstones. If `more`, request again with `since=cursor` |
| POST | `/api/sync` | `Changes` (≤ 5000 records, ≤ 8 MB) | `SyncPush`. Last-write-wins per record, idempotent; invalid ones are counted in `rejected` and don't stop the batch |
| GET | `/api/admin/users` | | `User[]` |
| POST | `/api/admin/users` | `{username, password, is_admin}` | `User` |
| PATCH | `/api/admin/users/{id}` | `{username?, password?, is_admin?, disabled?}` | `User` (disabling closes their sessions; `username` as in `/api/me/username`: 400/409, audit `user_renamed`) |
| DELETE | `/api/admin/users/{id}` | | 204 (you can't delete yourself or the last admin) |
| GET | `/api/admin/users/{id}/sessions` | | `SessionInfo[]` of that user (non-expiring TV ones included; `current`: the admin's); 404 if it doesn't exist |
| DELETE | `/api/admin/users/{id}/sessions/{sid}` | | 204 (closes that session, e.g. a TV no longer in use; audit `session_revoked` "user: session N"); 404 if it doesn't belong to that user |
| POST | `/api/admin/avatars` | `multipart/form-data` with `file` (the image) and optional `name`; or the raw image in the body with `?name=` (one per request). The interface crops before uploading (center and zoom, producing a 512 px square WebP; "Upload original" uploads the file as is) | 201 `Avatar`. Only PNG, JPEG, WebP or GIF recognized by content (never SVG) → otherwise 415; over 2 MB → 413; over 200 in the gallery → 409. Stored in `<data_dir>/avatars/<id>.<ext>` (the file name is the id) |
| DELETE | `/api/admin/avatars/{id}` | | 204 (deletes the file; whoever had it is left without an avatar) |
| GET | `/api/admin/torrents` | | `TorrentStat[]` |
| DELETE | `/api/admin/torrents/{info_hash}` | `?files=true` | 204 |
| GET | `/api/admin/stats` | | `{cache_bytes, cache_limit_bytes, disk_free_bytes, active_sessions, users, watching: {username, meta_id, stream_key, session_id, started_at}[]}` (`started_at` in unix s; each user's last play in the last 6 h) |
| GET | `/api/admin/metrics` | `?range=24h\|7d\|30d\|90d` (default 24h) | `Metrics` (below) |
| GET | `/api/admin/logs` | `?limit=` (≤ 1000, default 100) `&before=<id>&user=<name or id>&ip=&status=404\|5xx&route=<part of the template>` | `{items: LogRow[], next_before: number \| null}` (newest to oldest; `next_before` → `before` of the next page) |
| GET | `/api/admin/audit` | `?limit=&before=&event=&user=` | `{items: AuditRow[], next_before}` |

All `/api/admin/**` routes return 403 in local mode.

`type TorrentStat = { info_hash, name, size_bytes, downloaded_bytes, down_bps, up_bps, peers, state, last_used_at }`

### Metrics and audit (admin)

Every request is recorded without slowing it down (middleware → bounded channel → batched write to
SQLite every ≤ 2 s; if the channel fills up it's dropped and counted in `dropped_events`). The
route **template** is stored (`/api/media/{session}/file`), never the real URL: neither `?t=` nor
any query reaches the database. Never passwords, tokens or RD keys. The IP is the socket's; only
if the socket is loopback (`tailscale serve`) are `X-Forwarded-For` (last value) and
`Tailscale-User-Login`/`Tailscale-User-Name` used. Latency is up to the response headers (a video
stream doesn't count as slow); bytes are counted when the body finishes or is cut off.
Retention: raw 14 days (cap of 1 M rows), hourly aggregates 90 days, audit 1 year.
`MYTHOS_METRICS=0` turns it off; in local mode it's off.

```ts
type Metrics = {
  range: string; from_ms: number; to_ms: number;
  totals: { requests; errors /* ≥500 */; client_errors /* 4xx */; bytes_out; p50_ms?; p95_ms? };
  series: { hour_ms; requests; errors; bytes_out; p95_ms? }[];      // one per hour with traffic
  top_ips: { ip; requests; bytes_out; users: string[]; tailscale_logins: string[] }[];
  top_users: { user_id; username?; requests; bytes_out }[];
  top_routes: { method; route; requests; errors; bytes_out; p50_ms?; p95_ms? }[];
  status: { status; requests }[];
  failed_logins: { ip; count; last_at_ms; usernames: string[] }[];  // from the audit
  dropped_events: number;
};  // percentiles approximated by histogram (5, 10, 25, 50, 100, 250, 500 ms, 1, 2.5, 5, 10 s)
type LogRow = { id; ts_ms; ip?; tailscale_login?; user_id?; username?; method; route; status; dur_ms; bytes };
type AuditRow = { id; ts_ms; event; user_id?; username?; ip?; detail? };
// event: login_ok, login_fail (username = the name tried), logout, session_revoked,
// password_changed, user_created, user_updated, user_disabled, user_deleted, user_renamed ("old → new"),
// avatar_added, avatar_removed. In the admin ones,
// user_id/username are those of the admin who did it and `detail` says to whom and what changed.
```

### Sync

Data: settings (`ui`, `preferred_quality`, `max_size_gb`; **never** the RD key), progress and
library. Each write (through the normal API or through sync) takes an increasing `seq` from the
server that receives it (the `since` cursor, independent of clocks) and an `updated_ms` (clock of
the device that made the change). Rules:

- **Last-write-wins per record** by `updated_ms`. Exact tie: the deletion wins and, otherwise, the
  greater content (canonical JSON): total order, all replicas converge.
- **Tombstones**: `DELETE /api/progress/{id}` and `DELETE /api/library/...` leave `deleted: true`
  (normal reads don't show them). An old version that arrives late doesn't resurrect anything.
- **Idempotent**: reapplying the same thing changes nothing and takes no new seq (it doesn't
  bounce back).
- A write through the normal API uses `max(now, previous updated_ms + 1)`: what the user does on a
  device always beats what that device already had.
- Settings is a single record (not merged per `ui` key).

The UI doesn't have to implement any of this: in local mode, the embedded server syncs on its own
(see below). The routes exist in both modes.

### Local mode (server embedded in the app)

Flow and security details in `docs/local-mode.md`. Changes compared to server mode:

| Method | Route | Body | Response |
|---|---|---|---|
| POST | `/api/auth/login` | `{username, password, device?, remote_url?, offline?}` | `{token, user, offline: boolean, remote: {url, token, user} \| null}`. With a remote server (from `remote_url`, from that account's link or from the config) and without `offline: true`, it validates **against the remote**: if it accepts, it enrolls/updates the local account and also returns the remote's session (`remote.token`, so the UI can talk to the server directly). If the remote rejects → 401/429 with its message (the local one is not tried; if it says "This account is disabled", it's disabled here too). If it can't be reached → offline login against the local verifier (`offline: true`, `remote: null`). Account never signed in online on this device → 401 "… sign in once while the server is reachable" |
| POST | `/api/auth/logout` | | 204. If it was that account's last local session, it also closes the saved remote session (stops syncing until another online login) |
| GET | `/api/local/status` | | `LocalStatus` |
| POST | `/api/local/sync` | optional `{remote_url?, token?}` | `{pushed, pulled, last_sync_at}`: syncs now. With `token` (a remote session, e.g. from a login made directly against the server) it verifies it (`GET /api/me` on the remote: it must be the same account —by the id of the already linked remote, or by name if there's no link yet—, otherwise 409) and saves it for next times. With `remote_url` as well (requires `token`) it switches to syncing through that address: if it's another address of the same server (same `server_id`), it continues where it was; if it's another server, full sync. 409 if the account was never linked, 401 if the remote session expired (an online login is needed), 502 if it couldn't |
| `*` | `/api/admin/**` | | 403 |
| PUT | `/api/me/password` | | 403 |
| PUT | `/api/me/username` | | 403. If the account is renamed on the server, each sync (`GET {remote}/api/me`) gives the new name to the linked local account (by the remote's id, not by name): the offline login accepts the new one and an online login with the new one goes back to the same local account |

```ts
type LocalStatus = {
  mode: "local"; username: string;
  remote_url: string | null;       // linked or the one from the config
  remote_signed_in: boolean;       // there's a saved remote session (otherwise: online login)
  remote_reachable: boolean | null;// null = no remote configured (checked with
                                   // GET /api/health on the remote, 3 s, cached 15 s)
  last_sync_at: number | null;     // unix ms of the last successful sync
  last_sync_error: string | null;
  last_sync_error_code: string | null; // see "Errors": sync_unreachable, sync_failed...
  pending_changes: number;         // local changes not yet uploaded
  transcode_available: boolean;    // ffmpeg is present on the device
  sync_interval_s: number | null;  // null = on demand only
  version: string;
};
```

Also, in local mode only requests with a loopback `Host` are accepted (`127.0.0.1`, `localhost`,
`[::1]`, with any port): 403 otherwise (protection against DNS rebinding).

Routes mounted from the media crate, with auth (Bearer or `?t=`):

| Method | Route | Response |
|---|---|---|
| GET | `/api/media/{session}/file` | the file, with `Range` (206). Prioritizes the requested pieces. Each response carries at most 32 MB (`bytes=X-` answers `X-(X+32 MB-1)`): the browser requests the next range on its own, and that way no connection stays open for the whole episode (through `tailscale serve` a long connection degraded until it stalled the video) |
| GET | `/api/media/{session}/hls/index.m3u8` | `?start=<s>&audio=<idx>`: HLS playlist starting at `start` (to seek in transcode mode the UI reloads with another `start` and adds the offset; in remux, the one from `#MYTHOS-ORIGIN:`) |
| GET | `/api/media/{session}/hls/{segment}` | `.ts` or `.m4s` segments |
| GET | `/api/media/{session}/subs/{index}.vtt` | internal subtitle as WebVTT. If extraction hasn't finished in ~20 s (or earlier, if the file is half downloaded and ffmpeg has been waiting for pieces for ~3 s) it returns what there is (valid VTT from the start) with the header `X-Mythos-Partial: true`; the UI requests it again every 5 s to complete it. It's stored as complete only if it finished with the whole file downloaded and the session open |
| GET | `/api/media/{session}/status` | `MediaStatus` |
| GET | `/api/media/{session}/thumbs` | `ThumbsIndex` (below): the thumbnails for the seek preview. The first time it starts generation in the background. 404 if the duration isn't known yet (request again), 501 without ffmpeg (the bar without thumbnail) |
| GET | `/api/media/{session}/thumbs/{i}.jpg` | thumbnail `i` (JPEG, `Cache-Control: private, max-age=86400`); 404 if it isn't there yet |
| DELETE | `/api/media/{session}` | 204 (stops ffmpeg; the torrent stays in cache) |
| GET | `/api/media/subs/external` | `?u=<OpenSubtitles url>` → VTT (gz/srt → vtt) — done by the server |
| GET | `/api/media/img` | `?u=<https url from *.metahub.space>&w=<width>` → the image scaled down to 320/480/640/960/1280 px wide (the first ≥ `w`, JPEG), or the original if it's already small or has transparency. Cached on disk (`<data>/img-cache`, cap of 512 MB on server and 96 MB in local, least used ones are deleted) with `Cache-Control: private, max-age=2592000, immutable`. 400 if it's not from metahub, 502 if metahub fails |

**Learned preferences** (`server/src/taste.rs`, `plays` table): each `/api/play` with `meta_id`
stores the release group (`[Group] ...` or `...-GROUP`, and Torrentio's `bingeGroup`), quality,
size, dual audio, the actual audio and subtitle languages of the file (ffprobe), whether it was
picked by hand (`manual`) and which one the app offered (`top_key`). How it turned out is filled in
automatically: `finished` (progress passed 90 %), `switched` (before 15 min and with less than 5
watched in that playback —from where it started, not the position: `start_s`— another stream of the
same episode was opened) or `stalled` (`after_stall`). The last 600 per user are kept.

In `/api/streams` it infers (from the last 300): the group of the last good playback of that series
(confidence 0.6 / 0.8 / 1 with 1 / 2 / 3+ confirming it, and 1 if any of those was picked by hand;
one that stalled or was switched right away is not preferred and subtracts), the groups that
repeat across other series, the subtitle languages sought (the Settings `subLang`, Spanish if never
chosen, or the one actually present in at least two good manual picks, not counting English) and
dual audio (two manual picks). The subtitles of a stream that wasn't opened are guessed from its
name, from Torrentio's flags when it says "Multi Subs" without dubbing or dual audio (there the
flags are the subtitles) and from the ones its group had. The score adds: same group of the series
+90 × confidence, recurring group +15, bad group −40, sought subtitles +50, dual audio +25, in total
between −60 and +110: torrent health still rules (a preferred one with 2 seeders doesn't beat a
healthy one; one with 15 picked by hand does beat a pack with 129, one with 7 doesn't).
Without history nothing changes.

**Seek preview (`/api/media/{session}/thumbs`)**, `media/src/thumbs.rs`:

```
ThumbsIndex { interval_s: number, count: number, width: number, height: number,
              ready: string,   // one character per thumbnail: "1" ready, "0" not yet
              done: boolean }  // no more will appear (all ready or it stopped trying)
```

- Thumbnail `i` is from the moment `i * interval_s + interval_s / 2` and represents the range
  `[i * interval_s, (i + 1) * interval_s)`: for second `x`, `i = floor(x / interval_s)`.
- `interval_s` = duration / 300 rounded up, never less than 5 s (a 20 min episode: every 5 s; a
  2 h movie: every 24 s, 300 thumbnails). `width` 240 px; `height` according to the aspect ratio
  (even). ~5-10 KB each.
- Generation (in the background, per session, starts with the first index request): only while the
  session is playing (not while it starts or after a seek; a direct link also not while `starving`,
  because reading the file means downloading it from the origin), one ffmpeg at a time across the
  whole server, with one thread, no NVENC, lowest system priority and a 15 s cap (if it gets stuck
  waiting for pieces it doesn't count as a failure: it's retried in the next pass); each one decodes
  only the nearest keyframe (`-skip_frame nokey -noaccurate_seek`). ffmpeg reads with the passive
  role and only for moments whose range of the file (estimated from the average bitrate: 8 s before
  and 12 s after, between 2 and 24 MB) is already downloaded: with the file half downloaded there
  are thumbnails for what's there and the rest appear in later passes (every 5-20 s). Order by
  passes (one every 16, then every 8, 4, 2, 1). 8 consecutive failures with the range downloaded:
  it gives up (`done: true`).
- Cache: `<data>/thumbs/{hash}_{file}_{width}x{height}_{interval}/` for torrents (forever; deleted
  with the torrent), the session's temporary folder for direct links.
- The UI requests the index when playback starts and every 15 s until `done`; it shows the range's
  thumbnail if ready and otherwise only the time. An old server (404) or one without ffmpeg (501):
  no thumbnail.

**Top rated (`/api/top`)**: IMDb's best rated, from the public datasets
(`datasets.imdbws.com`: `title.ratings.tsv.gz` ~9 MB + `title.basics.tsv.gz` ~230 MB, personal and
non-commercial use, no key). They were chosen because Cinemeta's `imdbRating` catalog isn't sorted
by rating (it looks like "popular") and accepts a single genre, and third-party addons with "IMDb
Top" depend on someone else's server. The server downloads them at startup if it has nothing (after
~20 s, or right away if someone requests the list) and then every 7 days (if it fails, after 6 h,
and keeps what it had); it processes them by streaming (never stores the big file) and keeps only
non-adult movies (`movie`) and series (`tvSeries`, `tvMiniSeries`) with ≥ 1000 votes in
`<data>/imdb/top-v1.tsv.gz` (~1.3 MB, ~50 thousand movies and ~13 thousand series). Posters and
backgrounds: metahub's by IMDb id (`/poster/small/{id}/img`, the same as Cinemeta's); titles without
a poster fall back to the typographic poster. Pages cached 1 h. In local mode it's off (230 MB a
week on a phone, no): it responds `state: "unavailable"` and the UI requests it from the home
server. `MYTHOS_IMDB=0` turns it off on the server too.

**Recommendations (`/api/recommended`)**: "Películas para vos" / "Series para vos" ("Movies for
you" / "Series for you") and the home page's featured title, and "Similares" ("More like this") on
the detail page (`backend/server/src/recommend.rs`, with the details).

- **Taste**, per account (each TV profile is an account), from what's available: there are no own
  ratings. A movie watched ≥ 60 % weighs 1, between 25 and 60 % 0.5; a series 1 for the first
  episode watched (+0.15 for each other, up to 2) or 0.5 with two started; library 0.6 (+0.2 if
  also watched). Recent stuff weighs more (half-life 90 days, floor of one quarter). From there, how
  much of what was watched has each genre and the average year, with genres and years from the Top
  rated IMDb data. Movies and series together (someone who watches crime series gets crime movies).
- **Candidates**: rating ≥ 7.0 with the Top rated default votes (25 thousand movies, 10 thousand
  series), excluding anything with progress (watched, in "Continue watching" or removed from there)
  or in the library. **Score** = affinity^1.5 × quality × era: affinity = how much more than normal
  they watch the candidate's genres (so Drama, which is in everything, weighs little); quality =
  rating and, at half weight, votes; era up to +25 %. Animation and documentaries barely watched
  (< 10 %) count at 30 %.
- **Rotation**: from the 120 best scores, 20 are drawn by weighted sampling without replacement
  (Efraimidis–Spirakis, weight = score²) with a seed (user, type, 6 h block): stateless, the same row
  within the block (going back home doesn't reshuffle it) and another every 6 h, with the best ones
  more often but not always. Watching something new changes the profile right away.
- **No taste** (fewer than 3 titles with signal): 20 at random (same seed) from the top 300 of that
  type's Top rated, excluding what's already watched.
- **Similar** (`like`): genres in common (Jaccard ≥ 0.3; animation and documentary only with
  animation and documentary), quality and era close to the title; taste adjusts ±25 %.
- Without IMDb data (the first download, local mode, `MYTHOS_IMDB=0`) it responds the `state` with no
  titles and the UI uses what it did before: the featured title goes back to Cinemeta's popular, the
  "for you" rows aren't shown and "Similares" goes back to the popular of the first genre.

**Scaled-down images (`/api/media/img`)**: metahub serves backgrounds at 480 px
(`background/small`) or 1920 px (`background/medium`, ~800 KB and ~8 MB decoded). The UI
(`apps/web/src/lib/images.ts`) requests the right size for each place: up to 480 px `small` straight
from metahub; between 480 and 1280 px this route; above that, the original (on weak devices,
`lib/perf.ts`, the cap is 1280). Episode thumbnails (`episodes.metahub.space/…/w780.jpg`) are
requested in the TMDB size that is enough (`w185`…`w780`) without going through here, and posters
already come small in WebP. If the route fails (an old server), the UI falls back to the original
URL. `GET /api/health` returns `images: true`.

**Next-episode prefetch** (`POST /api/prefetch` with `session_id`, `Media::prefetch_next`).
The UI requests it when the episode being played has 10 min left or has passed 70 % (whichever
comes first), with the next one's stream: the same torrent if it's a season pack that includes it
(another file), otherwise the one it would pick when opening it (preferred audio language, then the
score); and when moving to the next episode it opens exactly that stream
(`/watch/series/<id>?s=<key>`). At the same moment the UI downloads the next one's subtitle list
and the OpenSubtitles one in the Settings language (`apps/web/src/lib/subcache.ts`), so it appears
as soon as it starts; internal ones can't be fetched ahead (they're spread over the whole file and
the server extracts them as it downloads). The server:

1. **Never hurts the current episode.** It doesn't add or download anything until the current one
   has ≥ 5 min downloaded ahead of the reader (or everything left until the end is already there)
   and no session is starving (starting, after a seek, or with < 20 s of cushion). If the current
   one drops below 2.5 min of cushion, it stops instantly: the file leaves the torrent's selection
   (same pack) or the torrent is paused (another torrent), and it comes back when it recovers.
   Minimum priority: a single 32 MB "ghost" at a time (first the end, then the start) against the
   current one's windows.
2. **If you stop watching, it's cancelled and deleted**: the session is closed and the next one
   isn't opened within 45 s; something else is opened (another title, another stream) with that
   session already closed; the session requests nothing for 10 min (long pause); or another prefetch
   arrives for the same session. New torrent (added by the prefetch) → it's removed with its files;
   same pack, or a torrent that was already in cache → its file goes back to priority 0 (leaves the
   selection) and stays as it was. The prefetch never records use, so neither the LRU nor expiry
   keep it; if the engine closes midway, on startup it deletes the torrents of prefetches that were
   never opened (`<data>/next-prefetch.json`).
3. **If the next episode starts** (`/api/play` with the same source, also within those 45 s), what
   was prefetched is reused: the torrent stays like any other being watched.
4. **Cap**: the end of the file (index/moov/cues, 4–16 MB) and 90 s of video from the start
   (bitrate assumed from name and size), between 48 and 128 MB. Once that's there, it stops as in 1:
   it doesn't download the whole episode. While downloading, librqbit gives idle peers pieces in
   natural order, so it overshoots a bit (measured at ~200 Mbit: 52–55 MB requested, 77–108 MB on
   disk). Measured (`backend/media/examples/warm.rs`): the next one goes to `ready` in 0.7–0.9 s
   versus 7–9 s cold.

**Speed with the port open** (details in the header of `backend/media/src/engine.rs`,
"Peers y NAT"): up to 200 peers per torrent on the server (`MYTHOS_PEER_LIMIT`, 160 in local mode),
unlimited upload, DHT + PEX + LSD + uTP, ~60 public trackers (ngosang "best" + newTrackon
"stable", refreshed every 12 h) and the peer port announced as is. The starving session has
priority: prefetches yield to it (ghosts out, torrent paused) and unused torrents are paused right
away.

**HLS waits, it doesn't 404**: a segment or the playlist that ffmpeg hasn't generated yet waits
until it exists (up to 120 s for the playlist and 90 s for a segment) instead of responding 404,
which would make hls.js give it up as lost. The client timeouts (hls.js `manifestLoadingTimeOut`,
`fragLoadingTimeOut`, etc.) must be at least ~120 s.

**HLS resumes on its own**: with no requests for 60 s ffmpeg is killed but the segments remain. The
next request for the playlist (with the same `start`/`audio`) or for a missing segment relaunches
ffmpeg from the end of what was already generated, with the same numbering and continuous
timestamps: the UI doesn't have to do anything (not even recompute `start`). Changing `start` or
`audio` starts another job; segments carry the job key in their name (`s<ms>a<track|d>_00012.m4s`)
and one from another job returns 404. The playlist is EVENT and ends with `#EXT-X-ENDLIST` when
ffmpeg reaches the end; in hls.js you have to pass `startPosition: 0` (otherwise it treats the
EVENT as live and starts near the end of what was generated).

**Remux from the middle: `#MYTHOS-ORIGIN:`**: with `start` > 0 and the video copied, ffmpeg starts
at the keyframe before `start` (up to one GOP earlier), with the audio at that same keyframe
(`-noaccurate_seek`: if the audio started at `start` and the video earlier, the Samsung TV played
both tracks together and the video stayed behind for the whole session). The second of the file
that corresponds to time 0 is found by ffprobe in parallel and goes into the playlist as a comment,
`#MYTHOS-ORIGIN:<s>` (players ignore it; it may be missing in the first reloads). The UI adds that
value instead of `start` and the first time seeks to `start - origin`. In transcode it's not
there: time 0 is `start`.

**`MediaStatus.buffered_ahead_s`** is an estimate: seconds downloaded from where playback started
(or the last seek) minus what's assumed already played since it switched to `ready`. It's useful
for the loading screen; during playback the UI has to rely on the `<video>` events
(`waiting`/`playing`), because the server doesn't see a user pause.

The headers exposed through CORS are `Content-Range`, `Accept-Ranges`, `Content-Length` and
`X-Mythos-Partial`.

## Embedding the server (crate `mythos`, `backend/server`)

`backend/server` is a library (`mythos`) plus a thin binary (`main.rs`: reads the `MYTHOS_*`
variables, `adduser`/`passwd`/`health` commands and Ctrl+C/SIGTERM). The apps (Tauri) depend on
the library: `mythos = { path = "../../backend/server" }` (with `default-features` nothing else is
needed; it compiles for `x86_64-pc-windows-msvc` and `aarch64-linux-android`, see
`docs/local-mode.md`).

```rust
pub enum Mode { Server, Local }
pub struct Config {
    pub mode: Mode,
    pub bind: SocketAddr,              // Config::local: 127.0.0.1:0 (free port)
    pub data_dir: PathBuf,             // mythos.db + torrent cache
    pub web_dir: Option<PathBuf>,      // None = doesn't serve UI (local)
    pub cache_gb: f64,                 // 0 = no cap (local)
    pub peer_limit: usize,             // peers per torrent: server 200 (MYTHOS_PEER_LIMIT), local 160
    pub cache_max_age: Option<Duration>, // since last use: local 24 h; server 72 h (MYTHOS_CACHE_HOURS, 0 = None)
    pub peer_port: u16 /* 0 = ephemeral */, pub hwaccel: HwAccel,
    pub admin_user: Option<String>, pub admin_password: Option<String>, pub dev_users: bool, // server only
    pub torrentio_url: String,
    pub ffmpeg: PathBuf, pub ffprobe: PathBuf,   // empty path = no ffmpeg (direct only)
    pub extra_trackers: bool,
    pub metrics: bool,                 // local: false
    pub remote_url: Option<String>,    // local: default remote server
    pub sync_interval: Option<Duration>, // local: Some(60 s); None = on demand only
    pub imdb_top: bool,                // "Top rated" (IMDb datasets); local: false
}
impl Config {
    pub fn server(data_dir) -> Self;   // the same values as the binary without variables
    pub fn local(data_dir) -> Self;    // 127.0.0.1:0, no UI, no metrics, 20 GB cache,
                                       // peers on an ephemeral port, sync every 60 s, no Top rated
}
pub async fn run(cfg: Config) -> anyhow::Result<Handle>;   // returns once it's listening
pub struct Handle;
impl Handle {
    pub fn addr(&self) -> SocketAddr;  pub fn port(&self) -> u16;
    pub fn base_url(&self) -> String;  // "http://127.0.0.1:<port>"
    pub fn mode(&self) -> Mode;  pub fn media_tools(&self) -> MediaTools;
    pub async fn stopped(&mut self) -> anyhow::Result<()>;  // if the server goes down on its own
    pub async fn shutdown(self) -> anyhow::Result<()>;      // orderly shutdown (≤ ~10 s; ~1 s typical)
}
pub async fn cli(cfg: &Config, args: &[String]) -> anyhow::Result<()>;  // adduser / passwd
pub use mythos_media::HwAccel;
```

The library doesn't initialize `tracing` (whoever uses it does). It needs a multi-threaded tokio
runtime (Tauri's, `tauri::async_runtime`, works).

## `mythos-media` crate API (what the server uses)

```rust
pub struct MediaConfig {
    pub data_dir: PathBuf,        // torrents in data_dir/torrents, temporary HLS in data_dir/hls
    pub cache_limit_bytes: u64,   // LRU: the least used torrents that aren't in use are deleted
    pub peer_port: u16,           // 42069
    pub ffmpeg: PathBuf, pub ffprobe: PathBuf,
    pub hwaccel: HwAccel,         // Nvenc | None  (MYTHOS_HWACCEL=nvenc|none)
    pub extra_trackers: bool,     // download the public tracker list at startup
}
#[derive(Clone)] pub struct Media { /* Arc */ }
impl Media {
    pub async fn new(cfg: MediaConfig) -> anyhow::Result<Self>;
    pub async fn open(&self, src: Source, caps: ClientCaps) -> anyhow::Result<PlayInfo>;
    pub fn router(&self) -> axum::Router;          // routes relative to /api/media, WITHOUT auth (the server adds it)
    pub async fn torrents(&self) -> Vec<TorrentStat>;
    pub async fn remove_torrent(&self, info_hash: &str, delete_files: bool) -> anyhow::Result<()>;
    pub async fn stats(&self) -> MediaStats;       // cache_bytes, cache_limit_bytes, disk_free_bytes, active_sessions
    pub async fn prefetch(&self, src: Source) -> anyhow::Result<()>; // non-blocking: fires and returns
    // Next-episode prefetch tied to the session being watched (rules in "Next-episode
    // prefetch"); non-blocking; error if the session doesn't exist. `next_status`: state (diagnostics).
    pub fn prefetch_next(&self, session_id: &str, src: Source) -> anyhow::Result<()>;
    pub fn next_status(&self, session_id: &str) -> Option<NextStatus>; // {state: waiting|active|done, info_hash, file_idx, same_torrent, have_bytes, target_bytes}
    pub fn announce_port(&self) -> Option<u16>;    // the port announced to trackers and DHT
    pub fn is_downloaded(&self, info_hash: &str, file_idx: Option<usize>) -> bool; // for Stream.cached
    pub fn status(&self, session_id: &str) -> Option<MediaStatus>; // the same as GET /{session}/status
    pub async fn close(&self, session_id: &str);   // the same as DELETE /{session}: kills ffmpeg
    // For experiments (examples/sim.rs); the server uses `new` = `Tuning::default()`:
    pub async fn with_tuning(cfg: MediaConfig, tuning: Tuning) -> anyhow::Result<Self>;
    pub fn set_download_limit(&self, bps: Option<u32>); // live download cap
    pub fn tools(&self) -> MediaTools;   // {ffmpeg, ffprobe}: detected at startup (`-version`)
    pub async fn shutdown(&self);        // closes sessions (kills ffmpeg), the internal endpoint
                                         // and the librqbit session (releases ports and files)
}
pub fn http_client_builder() -> reqwest::ClientBuilder; // TLS per platform (Android: embedded
                                                        // Mozilla roots, see local-mode.md)
pub enum Source { Torrent { info_hash: String, file_idx: Option<usize>, trackers: Vec<String> }, Url(String) }
```

**Optional ffmpeg**: if `ffmpeg`/`ffprobe` aren't there (or the path is empty), the engine starts
anyway. Without ffprobe, probing is done by an in-house reader of MP4 (`moov`) and Matroska/WebM
(`Info`/`Tracks`, with SeekHead) headers through the same internal endpoint (so the pieces it reads
are downloaded with priority); if it fails, it's guessed from the name. Without ffmpeg:
`PlayInfo.mode` always `direct`, `hls_url` null, no internal subtitles or thumbnails, and `/hls/*`,
`/subs/*` and `/thumbs*` respond 501 with a clear message. `/api/streams` also sinks in the list
what, according to `caps`, doesn't play directly.

The types `ClientCaps`, `PlayInfo`, `Track`, `MediaStatus`, `TorrentStat`, `MediaStats`, `MediaTools`, `ProbeSource` live in

`mythos-media` (serde) and the server re-exports them as is. `Track` implements `Default`. Note: in
`PlayInfo` optional fields come out as `null` (they're not omitted), so the UI has to treat `null`
as absent.
