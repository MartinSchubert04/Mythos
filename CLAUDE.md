# CLAUDE.md

Mythos: a Stremio-like client for personal use. The server (on `MRTN-SERVER`, Docker) downloads the
torrents Torrentio returns with its own engine (librqbit) and sends the video to the clients over
HTTP; if the browser doesn't support the format, it converts it live with ffmpeg + NVENC. Users with
login (nobody gets in without an account), admin mode to create them. Web, desktop and Android
(Tauri 2) interfaces are identical.

## Where each decision lives

| Question | File |
|---|---|
| HTTP contract between UI and server, API of the media crate | `docs/API.md` |
| What the interface has to look like | `DESIGN.md` (ported from Ethereon) |
| Deployment on the server, ports | `deploy/README.md` |
| Android app: shell, optional server interface, updates, publishing | `docs/mobile.md` |
| Local mode (embedded server on desktop and Android, sync, Auto/Server only/Local only) | `docs/local-mode.md` |
| Android TV / TV box: 32 and 64 bits, remote control, 10-foot scale, profiles, minimum WebView | `docs/mobile.md` ("TV") |
| Samsung (Tizen): the TV app, Samsung certificates, building, installing over WiFi | `docs/tizen.md` |
| Desktop: updates from the Releases (when it installs, relaunching) | `apps/desktop/src-tauri/src/updater.rs` |
| Desktop: frosted glass (Acrylic + opacity over any theme) | `apps/web/src/lib/glass.svelte.ts` |
| "Top rated": where it comes from (IMDb datasets), how often it's downloaded, where it's stored | `backend/server/src/imdb.rs`, `docs/API.md` ("Top rated") |
| Performance: image sizes (`/api/media/img`), lite mode on weak devices, what to avoid | `apps/web/src/lib/images.ts`, `apps/web/src/lib/perf.ts`, `DESIGN.md` (5b and 7) |
| Recommendations ("para vos" / "for you", home featured item, "Similares" / "More like this"): tastes, rotation every 6 h, with no tastes random from the Top 300 | `backend/server/src/recommend.rs`, `docs/API.md` ("Recommendations") |
| Which preferences are per user (theme, palettes, glass, language, subtitles) and which per device (quality, volume, TV mode, lite mode), theme migration | `apps/web/src/lib/prefs.svelte.ts`, `apps/web/src/lib/quality.svelte.ts`, `apps/web/src/lib/themes.svelte.ts` |
| Interface language (Spanish by default, English), how to add strings, translated server errors | `apps/web/src/lib/i18n.svelte.ts`, `apps/web/src/lib/i18n/server.ts` |

## Structure

```
backend/                  Rust workspace (the only thing that goes into the container)
  server/                 `mythos` binary: axum, SQLite (rusqlite), auth, admin, catalog, progress,
                          serves the compiled UI and mounts the media router
  media/                  `mythos-media` crate: torrent engine (librqbit), streaming with Range,
                          ffmpeg (HLS, NVENC), internal subtitles. Knows nothing about users.
apps/web/                 the interface (Svelte 5 + Vite + TS). The same for web, desktop and Android.
apps/desktop/             Tauri 2: just the window; loads the apps/web build
apps/mobile/              Tauri 2 for Android: loads the apps/web build; plugin/ (Rust + Kotlin)
                          with fullscreen, back, APK updates
apps/local-server/        Tauri plugin `mythos-local` (desktop and Android): the embedded server of
                          local mode (docs/local-mode.md)
apps/tizen/               Tizen web app (.wgt) for Samsung TVs: picks the server and opens its
                          interface (docs/tizen.md)
deploy/                   Dockerfile, docker-compose.yml, deployment README
```

## Ports (MRTN-SERVER)

- `127.0.0.1:7700` the app (API + UI). Outside only via `tailscale serve --https=7700`
  → `https://<tu-server>.ts.net:7700` (tailnet only). Desktop and Android ask for that URL at login
  the first time and save it: no build carries the address (the Releases are public).
  `VITE_DEFAULT_SERVER` (`apps/web/.env.local`, outside the repo) is only for testing on the PC.
- `42069` TCP/UDP: BitTorrent peers (open it on the router for more speed).
- Don't use: 22, 53, 443, 1883, 3000, 4000, 4517, 5678, 6052, 6080, 8080, 8090, 8123, 8443, 8765,
  8787, 8788, 10200, 10300, 11987, 51515, 61208 (already taken).

## Commands

```powershell
# backend
cd backend; cargo run -p mythos      # http://127.0.0.1:7700 (uses ./data)
cd backend; cargo test
# interface
cd apps/web; npm run dev             # http://localhost:5173, proxies /api to :7700
cd apps/web; npm run check
# desktop
cd apps/desktop; npm run tauri dev
# android (ANDROID_HOME, NDK_HOME and JAVA_HOME: see docs/mobile.md)
cd apps/mobile; npm run build:emulator
```

## Versions: commit, tag and push

Repo: https://github.com/MartinSchubert04/Mythos (public: no secrets, `.env` and `data/` kept out).

Every finished fix or feature (tests and check green) is published without waiting to be asked:
bump the version, commit, tag, and push `main` and the tag. The tag triggers the workflows that
build the Release (Windows installer + `latest.json`, APK + `latest-android.json`), and from there
the installed apps update themselves (desktop: `apps/desktop/src-tauri/src/updater.rs`;
Android: `docs/mobile.md`).

- **Which number goes up** (semver, `MAJOR.MINOR.PATCH`):
  - `x.y.Z` (patch): fixes and small tweaks to something that already existed (a string, a width,
    a bug, a default value).
  - `x.Y.0` (minor): something new that can be used (an option, a screen, a feature), without
    breaking anything.
  - `X.0.0` (major): breaks something that already exists (the API between apps and server without
    compatibility, the database without a migration, removing a feature). Ask before bumping it.
- **Where the version lives** (all the same): `backend/server/Cargo.toml`,
  `backend/media/Cargo.toml`, `apps/web/package.json`, `apps/desktop/package.json`,
  `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
  `apps/mobile/package.json`, `apps/mobile/src-tauri/Cargo.toml`,
  `apps/mobile/src-tauri/tauri.conf.json`, `apps/mobile/plugin/Cargo.toml`,
  `apps/local-server/Cargo.toml`, `apps/tizen/config.xml` (`version` of the `<widget>`), and those packages' entries in the `Cargo.lock` and
  `package-lock.json` files (careful with blind replacing: other dependencies may have the same
  number).
- **Commit**: `vX.Y.Z: <summary>` and in the body one line per change, in Spanish, ending
  with the version (see `git log`). Tag `vX.Y.Z` on that commit.
- Changes that are neither fix nor feature (docs, this file, CI) are committed and pushed without a tag.
- The signing keys (APK and desktop updates) live outside the repo, in
  `~/.mythos-signing/`, and as repo secrets. If they're lost, the installed apps can't be
  updated without uninstalling.

## Rules

- **Test without disturbing.** Every video played in a test is muted from the start.
  When the test is over, always remove the mute, without waiting to be asked: on the TV (or
  any real page) the mute forced through the inspector stays until the page is reloaded.
  Restore the native `muted` of `HTMLMediaElement.prototype` (save it when patching) instead of
  reloading, because reloading cuts off whatever is being watched. Then confirm that `muted`
  is the native one again and that `mythos.prefs` (volume, mute) is as it was.
  Never close the installed Mythos app (it may be playing something): tests go in a
  separate instance (e.g. `tauri dev --config` with another `identifier`, which doesn't clash with
  the installed one's single-instance) or in the browser.
- **Pending tasks in `TODO.md`** (if it exists): read it at the start. Each task is deleted as soon
  as it's completed, in the same commit that resolves it; when none are left, the file is deleted.
- **Themes only with variables** (same as Ethereon): colors in `apps/web/src/styles/themes.css`,
  never hardcoded in components.
- **Animations only through `apps/web/src/lib/motion.ts`** (they respect "reduce motion").
- **No `/api` route without login**, except `POST /api/auth/login` and `GET /api/health`.
  Media routes also accept the token in `?t=` because `<video>` doesn't send headers.
- Real-Debrid keys and passwords never come back in any response.
- Docs in English; code comments in Spanish.
- **Rust disk space.** In November 2026 the `target/` folders added up to ~130 GB (backend
  88, desktop 20, mobile 20) and filled up C:. There are three separate `target/` folders (`backend/`,
  `apps/desktop/src-tauri/`, `apps/mobile/src-tauri/`) and each one compiles all the dependencies
  again. So it doesn't happen again:
  - `--release` only to publish or to test the installer/APK.
  - In a worktree (`.claude/worktrees/`) build with `CARGO_TARGET_DIR` pointing to the `target/`
    of the main repo (e.g. `$env:CARGO_TARGET_DIR="<repo>\backend\target"`), don't create a
    new one. When the task is done, `git worktree remove` the worktree.
  - When the tests of a task that compiled are done (before the push that publishes it), `cargo clean`
    in every `target/` that was used: they end up at 0. Rebuilding takes a few minutes, but the disk
    doesn't fill up. Before that, close the test instance running from there (Windows won't delete
    its .exe; the installed app doesn't run from there and isn't touched).
- **Interface in Spanish by default, English as an option** (Settings → Appearance, "Idioma /
  Language"; travels with the user's preferences). No hardcoded visible text in a component:
  each string is a key in `apps/web/src/lib/i18n/es/<zona>.ts` (common, browse, player,
  account, admin) with its translation in `en/<zona>.ts` (TypeScript flags any missing or extra one), and
  it's used with `t("zona.clave", { n })` from `lib/i18n.svelte.ts` (`{n}` gets replaced; plurals with
  `{ one, other }`). Numbers, dates and times with `num` / `date` / `ago` / `dur` from that module.
  Module-level option lists: `t()` inside a getter (`get label()`), never at load time.
  Neutral Rioplatense Spanish, short, with voseo ("Probá otro stream"); stream, torrent, seeders,
  HDR, 4K, REMUX stay in English. The strings in `apps/web/index.html` (old-browser notice)
  and in `apps/tizen` are ES5 without modules: they have their own es/en pair right there.
