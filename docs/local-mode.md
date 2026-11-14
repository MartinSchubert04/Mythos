# Local mode (server embedded in the apps)

The desktop (Tauri 2, Windows) and phone (Tauri 2, Android) apps can work without the home server.
They carry inside the same Mythos server (the `mythos` crate, `backend/server`) running on the
device itself, in `local` mode: Cinemeta catalog, Torrentio streams, torrents downloaded and played
on the device, progress, library and settings. Same account as on the server. When the server comes
back, everything syncs on its own.

HTTP contract in `docs/API.md` (sections "Sync", "Local mode" and "Embedding the server").

## Architecture

```
┌──────────────────────── app (Tauri) ─────────────────────────┐
│  webview (apps/web)                                          │
│    │  API base = remote server  (online)                     │
│    │           = http://127.0.0.1:<port>  (local)            │
│    ▼                                                         │
│  Rust shell ── mythos::run(Config::local(dir)) ──► Handle    │
│                 │  axum on 127.0.0.1:0, own SQLite,          │
│                 │  librqbit, ffmpeg if present               │
│                 └── sync (push/pull) ──────────┐             │
└────────────────────────────────────────────────│─────────────┘
                                                 ▼ (Tailscale)
                              home server (server mode, Docker)
                              GET/POST /api/sync, /api/auth/login
```

- **The shell** starts the local server when the app opens (`mythos::run`) and hands the URL to
  the UI (`Handle::base_url()`, port chosen by the system). On close, `Handle::shutdown()`. In
  **Auto** it starts ~8 s later (the UI doesn't request it earlier either): on a TV box or a weak
  phone it would compete for CPU and network with the first render. If it's needed earlier (login
  through the local one, home server down), the `info` command starts it on the spot. In Local only
  it starts right away.
- **The UI** talks to the remote server while it can be reached (streaming with NVENC, big cache)
  and to the local one when not. It doesn't implement sync: it only picks whom to talk to.
- **The local server** stores data in its own SQLite and syncs with the remote on its own: every
  60 s (`sync_interval`) if the remote responds, ~5 s after each local change, and on demand
  (`POST /api/local/sync`).

## Modes the user chooses

| Mode | Shell | UI |
|---|---|---|
| **Auto** (recommended) | starts the local server with `remote_url` = home server | login through the local one (`POST {local}/api/auth/login` with `remote_url`); uses the remote if `offline: false`, the local otherwise; switches on its own (below) |
| **Server only** | no need to start the local one | as before: everything against the remote. If it doesn't respond, error |
| **Local only** | starts the local one (with `remote_url` to sync, or `None` to not touch the server's network) | everything against the local one, always (streaming on the device). The login still validates online if it can (`offline: false`) |

On **Android TV / TV box, or with less than 2 GB of RAM**, local mode doesn't exist: the shell
never starts the embedded server, the mode is always "Server only" (regardless of what's saved) and
`set_mode` rejects the others. `info` says so with `unavailable: "tv" | "low_ram"` and Settings →
Connection explains it instead of offering Auto / Local only. Why: librqbit + SQLite + the WebView
don't fit in 1-2 GB, and a TV box processor can't handle downloading and serving the torrent at the
same time. Detection is in `apps/local-server/src/android.rs` (`limit`): system TV mode,
`leanback` or no touchscreen; `MemTotal` < 1.5 GiB (a "2 GB" device reports ~1.8) or
`isLowRamDevice()`. See `docs/mobile.md`, "TV".

Changing modes doesn't lose data: the local and the remote converge through sync. Going from
"Server only" to "Auto"/"Local only" asks for the password once (an online login through the local
one is needed to enroll the account on the device).

## Flows

### First login (online)

1. The UI sends `POST {local}/api/auth/login {username, password, device, remote_url}`.
2. The local one logs in against the remote. If it accepts: it creates/updates the local account
   (same name, its own password verifier), saves a remote session for syncing (`local_link`), opens
   a local session and starts the first sync (downloads progress, library and settings).
3. It responds `{token, user, offline: false, remote: {url, token, user}}`. The UI stores both
   tokens and uses the remote.

If the remote rejects the password → 401 with its message (the local one is not tried). If the
account is disabled on the server, it's also disabled on the device.

### Server outage ("Server unavailable, continuing locally")

- At sign-in: if the remote can't be reached (network, 4 s connection timeout, 5xx), the local one
  validates against its verifier and responds `offline: true`. The UI shows the notice and uses the
  local one.
- During use: if a request to the remote fails **because of the network** (not because of an HTTP
  status) or `GET {remote}/api/health` fails twice in a row, the UI shows the notice and switches to
  the local one with the local token. Recommended: don't switch in the middle of a movie; switch on
  the next navigation (the movie keeps going as long as the remote serves it).
- The local one has the data up to its last sync (≤ 60 s behind). What the user does afterwards
  (progress, library, settings) stays in the local one and is uploaded on return.

### Server comes back (reconnection)

1. While on local, the UI queries `GET {local}/api/local/status` every 15–30 s.
2. When `remote_reachable` is `true`: `POST {local}/api/local/sync` (uploads what was done offline
   and downloads what's new). With `200`, the UI goes back to the remote with the remote token it
   already had.
3. If `remote_signed_in` is `false` (the remote session expired or was revoked) or the sync returns
   401: an online login (asking for the password) is needed to keep syncing. Meanwhile it can stay
   on local.

The local server also syncs on its own in the background: even if the UI does nothing, changes
reach the remote as soon as it can be reached.

### Multiple server addresses

The same home server can be reached through more than one address: through Tailscale
(`https://<tu-server>.ts.net:7700`, from anywhere) and through the home network
(`http://192.168.x.x:7700` with `MYTHOS_EXTRA_BIND`, faster, only at home). Desktop and the APK page
keep a **list of addresses of the same server** (`lib/servers.svelte.ts`, in this device's
`localStorage`: it isn't uploaded to the server or synced).

- **Adding them**: at login ("Add another address") or later in Settings → Account → Server
  (add, remove, name them, mark the preferred one with the star, "Test" with each one's latency).
  Without a name it's inferred: "Tailscale" (`*.ts.net`, `100.64.0.0/10`), "Home network" (private
  IP, `*.local`, `*.lan`), "This computer" (loopback).
- **Same server**: each one is validated with `GET /api/health` and the `server_id` is compared
  (`docs/API.md`). An address that doesn't answer when added (the home one while away) is saved
  anyway and verified when it answers; if it answers with another `server_id` it isn't used
  ("Different server"). Adding in Settings an address of another server offers to switch servers
  (signs out of the account and keeps only that one). Since it's the same database, **the same
  token works on all of them**.
- **Which one is used**: all are tried in parallel (`/api/health`, 2.5 s). The preferred one wins if
  it answers (it's waited for up to 0.7 s more if another answered first); otherwise, the one that
  answers first. With no preferred one marked, the home network one counts as preferred: at home
  that one is always used. Through Tailscale the video goes through the `tailscale serve` proxy
  (HTTP/2 through the tunnel), and in tests a long playback with pauses ended up with the connection
  nearly stalled in that proxy (the server with data ready and `tailscaled` not forwarding it);
  through the home network it never happened.
  It's chosen at startup (with a single address nothing extra is tried), when coming back from the
  background (more than 15 s), when the network changes (`online`, `navigator.connection`), when a
  request doesn't get through (before switching to local the others are tried) and, while none
  answers, every few seconds (the reconnection in `net.svelte.ts`, or local mode's 20 s check).
- **Indicator**: the name of the one in use ("Home network", "Tailscale") goes in the `ConnBadge`
  tooltip and in the details of Settings → Connection.
- **If none answers**: the usual failover to local mode (above). To go back it's enough for any of
  them to answer: the UI tries them itself (the local one only knows the one it has saved) and does
  `POST /api/local/sync {token, remote_url}` with the one that answered.
- **The device's server** syncs through the address in use: when it changes, the UI sends it
  `POST /api/local/sync {token, remote_url}`, and a few seconds after opening it checks
  `/api/local/status` → `remote_url`. Since the link stores the remote's `server_id`
  (`local_link.remote_server_id`), another address of the same server doesn't restart the sync
  (it continues at the same cursor); another server does.
- **Media**: URLs (the `<video>`'s `?t=`, `/api/media/img`, avatars) are built with the address
  active at the time. Each video session stays on the address where it was opened: switching
  address in the middle of a movie doesn't cut it; if it stalls, the player reopens it on the active
  one at the same second (same as when switching to local). If it switched because the previous one
  stopped answering, the views reload (`net.epoch`) so images and lists are requested from the new
  one.
- **TV profiles**: a profile saved with any of the addresses belongs to the same server; its
  avatars and its logout go through the address in use.

### Username change

The name is changed on the server (`PUT /api/me/username` or the admin); on the device it returns
403. The local account is tied to the server's by its id (`local_link.remote_user_id`), not by
name: each sync starts with `GET {remote}/api/me` and, if the name changed, the local account takes
the new one (if another old local account had that name, `~<id>` is appended to it so they don't
clash). From then on the offline login accepts the new name (and no longer the old one), and an
online login with the new one goes back to the same local account, with its data. Until the first
sync after the change, the old name still works offline.

### Conflicts

Last-write-wins per record with `updated_ms` (device clock), tombstones for deletions, idempotent,
incremental by cursor (see `docs/API.md` → "Sync"). If the same episode was watched on two devices
offline, the position of the last one that wrote stays. A clock far ahead on a device makes its
changes win until that time passes; each new write uses `max(now, previous + 1)` so that the last
thing the user did on a device doesn't lose against what that device already had.

## Security

### Offline credentials: own verifier, not the server's hash

To validate the password without network, something was needed on the device. Options:

1. Download the user's argon2 hash from the server ("credentials bundle").
2. **Compute an own verifier on the device** (chosen).

The local server logs in against the remote with the password the user typed; only if the remote
accepts it does it compute a new argon2id, with its own salt, and store it in its SQLite. Why:

- **The server's hash never leaves the server**: there is no route that exports it (less attack
  surface, the rule "passwords never come back in any response" holds without exceptions) and a
  change of argon2 parameters on the server doesn't break clients.
- Stealing the device gives the same as the bundle would (a verifier for offline brute force) but
  allows nothing against the server: the server never accepts hashes, only passwords.
- No new route on the server is needed for this: the normal login is enough.

Consequences: the first login on each device has to be online. A password change on the server
reaches the device on the next online login (until then the old one works offline); that's why
`PUT /api/me/password` returns 403 in local mode.

### The local API on 127.0.0.1

On Android any app on the phone can connect to `127.0.0.1:<port>`. That's why:

- All routes require login just like on the server (except `POST /api/auth/login` and
  `GET /api/health`), with the same rate limit (10 failures / 5 min).
- Only requests with a loopback `Host` are accepted: a web page that points its domain to
  127.0.0.1 (DNS rebinding) gets 403. CORS only for the Tauri and Vite origins.
- No admin (403 on `/api/admin/**`), no metrics.

### What's stored on the device

In `data_dir` (the app's private folder): `mythos.db` with the argon2 password verifier, the local
sessions (only the sha256 of each token) and the remote session used by sync
(`local_link.remote_token`, in plain text: it's a session revocable from the server's
`/api/me/sessions`, it shows up as the device that signed in). The local logout, if it's that
account's last session on the device, also closes that remote session.

### Real-Debrid

Project rule: the RD key never comes back in any response. That's why **it isn't synced**
(`SettingsRec` doesn't have it, and applying remote settings doesn't touch the local one). Local
mode works without RD (regular torrents on the device); if the user wants RD offline too, they enter
it in the app (`PUT /api/me/settings {rd_key}` against the local one) and it stays only on that
device.

## Playback on the device and ffmpeg

- Torrents are downloaded on the device (`data_dir/torrents`). No size cap: each movie is deleted
  24 h after its last use (`cache_max_age`; never the one being watched, and whatever expired while
  the app was closed is deleted when it opens). With less than 2 GB free they're deleted earlier,
  and with less than 1 GB the download is paused. The first play takes the same as on the server;
  there's no NVENC.
- ffmpeg is optional. The shell can pass the path to an ffmpeg (on Windows, if it's in the PATH,
  `Config::local` already uses it). Without ffmpeg: direct playback only, probing with the in-house
  MP4/MKV header reader, `PlayInfo.transcode_available: false` and `direct_ok` says whether the
  player will be able to open it. `/api/streams` with `?caps=` sinks what isn't direct. The UI
  should send the device's real `caps` (Android: the WebView's `<video>` usually does
  h264/hevc/vp9/av1 + aac/opus/flac in mp4/webm; mkv depends on the device) and, if `direct_ok` is
  `false`, offer the next stream.
- External subtitles (OpenSubtitles) work the same; internal ones need ffmpeg.

## Android

- **Data folder**: the app's private one (`app.path().app_data_dir()` in Tauri =
  `/data/user/0/<package>/files`, or `cache_dir` for the torrents if you want the system to be able
  to free them). Don't use shared storage.
- **Ports**: the server on `127.0.0.1:0` (the shell hands the real port to the UI); BitTorrent
  peers on an ephemeral port (`peer_port = 0`); on mobile networks UPnP does nothing and it doesn't
  matter. The UI talks to `http://127.0.0.1:<port>` from `http://tauri.localhost`: cleartext HTTP
  must be allowed **only** for loopback with a `network_security_config.xml`
  (`<domain-config cleartextTrafficPermitted="true">` with `127.0.0.1` and `localhost`), besides
  the `INTERNET` permission.
- **TLS**: the own HTTP clients use the embedded Mozilla roots
  (`mythos_media::http_client_builder`), because rustls's platform verifier needs to be initialized
  through JNI. librqbit has its own HTTP client (`https://` trackers); without that initialization
  those trackers fail, but UDP ones and the DHT work, which is what Torrentio uses most. If you want
  it complete: the shell calls `rustls_platform_verifier::android::init_hosted` at startup and adds
  the Gradle component `rustls-platform-verifier-android`.
- **Battery and background**: Android freezes or kills background processes. While something is
  playing or downloading, the shell should have a *foreground service* (`mediaPlayback`/`dataSync`)
  with a notification; without it, turning off the screen can cut the download. Without playback,
  it's not needed: the engine pauses idle torrents on its own (15 min) and sync is a small request
  every 60 s only if the server responds. When going to the background for a long time, the shell
  can call `shutdown()` and start again on return (takes ~0.5 s; the data stays).
- **Tailscale**: the remote can only be reached with the Tailscale VPN on; otherwise, Auto mode
  falls back to local (it's the normal "server unavailable" case).
- **Logs**: the library uses `tracing` and initializes nothing; on Android the shell can send the
  events to logcat (e.g. `tracing-android` or `android_logger`).

## Building for Windows and Android

Windows (MSVC) is the native target: `cargo build -p mythos --lib`.

Android (`aarch64-linux-android`), from Windows with Android Studio's NDK
(`%LOCALAPPDATA%\Android\Sdk\ndk\<28.x or 30.x>`):

```bash
NDK="$LOCALAPPDATA/Android/Sdk/ndk/28.0.13004108"
TC="$NDK/toolchains/llvm/prebuilt/windows-x86_64/bin"
export ANDROID_NDK_HOME="$NDK"
export CC_aarch64_linux_android="$TC/aarch64-linux-android24-clang.cmd"
export CXX_aarch64_linux_android="$TC/aarch64-linux-android24-clang++.cmd"
export AR_aarch64_linux_android="$TC/llvm-ar.exe"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TC/aarch64-linux-android24-clang.cmd"
rustup target add aarch64-linux-android
cd backend && cargo build --target aarch64-linux-android -p mythos --lib
```

The same for `armv7-linux-androideabi` (TV boxes and phones with 32-bit Android; the clang is
`armv7a-linux-androideabi24-clang`): the release APK carries both (`docs/mobile.md`).

Nothing else was needed: SQLite (`rusqlite` with `bundled`), `ring` and `aws-lc-sys` (the rustls
providers that librqbit and reqwest bring) compile with the NDK's clang without CMake or OpenSSL.
The only Android-specific thing in the code is TLS verification (above). With Tauri, `tauri android
build` sets these variables on its own.

## How it's implemented in the apps

**Shell** (`apps/local-server`, Tauri plugin `mythos-local`, the same on desktop and Android):

- Stores the mode in `<app_local_data_dir>/local-mode` (`auto` by default). In `auto` and `local`
  it starts `mythos::run(Config::local(<app_local_data_dir>/local))` in the background as soon as
  the app opens (the window doesn't wait; ~1–2 s in debug, the local server is ready by the time
  it's needed). In `server` it starts nothing.
- Commands: `info` → `{mode, base_url, transcode_available, error, unavailable}` (waits if it's
  starting) and `set_mode(mode)` (saves it; `server` shuts down the local one, `auto`/`local` start
  it; with `unavailable`, only `server`).
- ffmpeg: doesn't ship with the app. On Windows, if `ffmpeg` and `ffprobe` are in the PATH they're
  used; otherwise, empty paths (direct only, the engine doesn't even try to launch them). On
  Android, never. ffmpeg processes are launched without a console window (`CREATE_NO_WINDOW`,
  `mythos_media::util::tool_command`).
- On exit (`RunEvent::Exit`): `Handle::shutdown()`.
- `tracing`: to stderr on desktop, to logcat (tag `mythos`) on Android (`init_tracing`).
- Android: `rustls_platform_verifier::android::init_with_env` with the app's context (from
  `ndk-context`, set by tao) and the Kotlin part `org.rustls:rustls-platform-verifier` in Gradle
  (same version as `rustls-platform-verifier-android` in `Cargo.lock`, ProGuard rule in
  `proguard-rules.pro`). Without it the first librqbit `https://` tracker panicked (and in release,
  with `panic = "abort"`, closed the app).

**UI** (`apps/web/src/lib/local.svelte.ts`):

- Settings → Connection: Auto / Server only / Local only, "Show a notice when switching to local",
  status (unsynced changes, last sync, "Sync now"). Switching to Auto/Local without the account on
  the device asks for the password right there (login through the local one).
- Always-visible indicator, icon only (cloud = server, disk = local, a dot if there are pending
  changes; the details in the tooltip): in the desktop title bar and at the top right on Android
  (`components/ConnBadge.svelte`).
- Two tokens in `localStorage`: `mythos.token` (home server) and `mythos.localToken` (local).
  `lib/api.ts` uses the active server's. Logout and "Switch account" close both.
- Outage: a request to the home one that doesn't get through (or 502/503/504) and a
  `GET /api/health` that doesn't either (on none of its addresses, `lib/servers.svelte.ts`) →
  switches to local, repeats that request there and notifies. On local it checks
  `/api/local/status` every 20 s; with `remote_reachable` → `POST /api/local/sync {token}` → goes
  back to the home one ("Back on the server · synced") and reloads the view. Without a remote
  session (`remote_signed_in: false` or a login made offline) it stays on local and asks to sign in
  again to sync.
- Each video session stays tied to the server (and token) where it was opened: `direct_url`, HLS,
  internal subtitles and `/status` keep going there even if the API changes. If that video stalls
  and the API is already on another server, the player reopens it on the active one at the same
  second.
- Without ffmpeg (`transcode_available: false`): if `direct_ok` is `false` it tries the next
  stream on its own (up to 3); if the `<video>` can't decode it, the same instead of degrading.

## What's missing

- Android: *foreground service* while something is playing or downloading on the local one (with
  the screen off Android can freeze the process).
- Real `caps` of the Android WebView for `/api/streams` (today it uses the same detection as the
  web, which is already the device's).
