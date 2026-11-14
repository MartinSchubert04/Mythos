# Mythos for Android

The Android app (`apps/mobile`): a Tauri 2 shell with the same interface as `apps/web`, the native
bits needed on the phone and automatic updates, same as in Haze.

## Where everything lives

```
apps/mobile/
  package.json        Tauri CLI and scripts (npm run build:emulator, build:debug, build)
  plugin/             `mythos` plugin (Rust + Kotlin): the native bits and the updates
    src/lib.rs        commands for the interface, permissions for the server's interface
    src/updater.rs    check / download / install (same as Haze)
    android/          Kotlin: MythosPlugin.kt (fullscreen, back, screen on,
                      bars, local network, `device`: TV and memory) and Updater.kt (GitHub Releases +
                      PackageInstaller)
  src-tauri/          the app: tauri.conf.json (loads apps/web/dist), capabilities, icons
    src/lib.rs        just the Builder (mythos and mythos-local plugins)
    gen/android/      Gradle project (from `tauri android init`, with its own signing and MainActivity)
apps/web/src/lib/
  platform.ts         isTauri / isAndroid / isDesktop, serverBase(), setActiveServer(), fullscreen
  servers.svelte.ts   multiple addresses of the same server: which to use, latency, Settings → Server
  shell.svelte.ts     everything Android on the interface side
  tv.svelte.ts        TV mode: detection, 10-foot scale (960 px viewport)
  spatial.svelte.ts   remote control navigation (arrows, center, menu)
  profiles.svelte.ts  TV profiles ("Who's watching?", views/Profiles.svelte)
```

## The server address

The repo and the Releases are public: neither the APK nor the desktop installer carries the tailnet
address. The first time, the login shows **Server URL** (`https://` is added if missing), it's
validated with `GET /api/health` and saved. Afterwards it's changed in Settings → Account → Server.
It can have **multiple addresses of the same server** (e.g. Tailscale and the home network,
`http://` with `MYTHOS_EXTRA_BIND`): they're added at login ("Add another address") or in Settings,
it's checked that they're the same server (`server_id` from `/api/health`) and the app uses the
preferred one if it answers, otherwise the fastest, and switches on its own when the network
changes. See `docs/local-mode.md`, "Multiple server addresses".
`VITE_DEFAULT_SERVER` (`apps/web/.env.local`) is only for testing on the PC; the workflows leave it
empty on purpose.

Android 17 cuts connections to the local network without the `ACCESS_LOCAL_NETWORK` permission
("Nearby devices"). If the server doesn't answer, the app requests it and tries again (a server with
a home IP, or the emulator talking to the PC at `10.0.2.2`).

## Where the interface comes from

**By default, from the APK.** The interface is the `apps/web` build that comes inside (the local
page, `http://tauri.localhost`), it talks to the server over HTTP (`serverBase()`) and updates with
the APK. It's the one local mode uses (the embedded server, at `http://127.0.0.1:<port>`):
`setActiveServer(url)` from `platform.ts` switches the server live without reloading.

**Optional: the server's interface** (Settings → App → "Load the interface from the server", off by
default). With that on, when the app opens the local page checks whether the server answers and
serves `<meta name="mythos-shell">` (an old server doesn't) and navigates to the interface the
server serves: every deploy updates it instantly, without reinstalling, and it's always the same
version as the API. If the server doesn't answer, the local one stays.

| | APK interface (default) | Server interface (optional) |
|---|---|---|
| Updating the interface | with the APK (automatic, in the background) | with every deploy, instantly |
| No server / local mode | works (reconnection, or the embedded server) | falls back to the APK's |
| Startup | immediate, all local | one extra request to the server before showing |
| Native permissions | the local page (`capabilities/default.json`) | only that exact origin, at runtime |

Security of the server's interface: `open_server` (only the local page can call it) gives the exact
origin the user chose (not `*.ts.net`) a runtime capability with the commands in
`REMOTE_PERMISSIONS` (`plugin/src/lib.rs`): never `open_server`, and no files or windows. Any other
page opened in the WebView has no IPC. The two pages have separate storage: when entering the
server's, the token is passed to it only once (`handoff`), and "Change server" or turning the option
off go back to the local one (`leave_server`).

Contract between the interface and the APK: plugin commands are only added, never changed or
removed, and the interface tolerates one being missing (an APK older than the server's interface).

## Local mode

The embedded server (`docs/local-mode.md`) is the `mythos-local` plugin (`apps/local-server`), the
same as on desktop: it starts in the background depending on the mode (Auto / Server only / Local
only, in Settings → Connection) with the app's private folder, and the interface asks it where it is
(`info`). The interface switches servers live with `useLocal()` from `lib/api.ts` (which uses
`setActiveServer(url)` from `platform.ts`); see `lib/local.svelte.ts`.

- **Network:** the release allows HTTP (`gen/android/app/src/main/res/xml/network_security_config.xml`)
  to reach the server over the home network (`http://<server-ip>:7700`, with `MYTHOS_EXTRA_BIND`
  on the server; useful on a TV box without Tailscale) and for the embedded server on `127.0.0.1`.
  Android doesn't allow limiting it to an IP range. From outside home it goes over HTTPS
  (`tailscale serve`); an address without `http://` is completed with `https://`. Loopback doesn't
  need Android 17's local network permission; the home IP does (requested when connecting).
- **TLS:** Gradle brings `org.rustls:rustls-platform-verifier` (rustls's maven repository in
  `gen/android/build.gradle.kts`); the plugin initializes it at startup.
- **Logs:** `adb logcat -s mythos` (the embedded server's).
- The server's interface (optional) doesn't work in local mode: with the server down the app stays
  on the APK's, which is the one that talks to the embedded server.

## On the phone

- **Back:** the same path as Esc (closes menus, drawers, dialogs and the trailer); in the player it
  exits right away (doesn't go through "exit fullscreen"); with nothing open it goes back to the
  previous view, and on home it sends the app to the background.
- **Player:** on entering, native fullscreen (no system bars) and landscape (rotates between the
  two). The fullscreen button removes it (free rotation) and puts it back. On exit, everything back
  to normal.
- **Screen on** while a `<video>` is playing.
- **Edge to edge** with `env(safe-area-inset-*)` (App.svelte); the system bar icons follow the theme
  (light or dark).
- **ClientCaps:** the same detection as the web. The Android WebView reports h264, hevc, av1, aac,
  opus, flac and `mkv: maybe`; no ac3/eac3/dts (the server converts the audio). If direct fails, the
  player degrades and switches to HLS (hls.js with MSE).
- Below 700 px wide, the tab bar at the bottom (DESIGN.md §8); in landscape, the side one.

## TV (Android TV and TV box)

(Samsung TVs aren't Android: they have their own app, see `docs/tizen.md`.)

The same APK works for a TV box or an Android TV: it shows up in the TV launcher with its banner
(`res/drawable-xhdpi/banner.png`, 320x180, made with the logo) and is driven with the remote
control. Nothing changes on the phone.

**Installing on a TV box** (e.g. an X96 Mini, which has no Play Store or ADB at hand):

- With the **Downloader** app (or the box's browser): open the APK link of the latest Release
  (`Mythos-X.Y.Z-android.apk` on GitHub) and install. You have to allow Downloader to "install
  unknown apps".
- Or with a **USB stick**: copy the APK, open it from the box's file manager and install.
- Afterwards it updates itself like on the phone (Settings → App).

**32 and 64 bits.** Many TV boxes (Allwinner H313, Amlogic S905W...) have a 64-bit processor but
32-bit Android (`armeabi-v7a` userspace). The APK is universal: it carries `lib/arm64-v8a` and
`lib/armeabi-v7a` (librqbit, rusqlite, ring/rustls compile just the same for armv7) and Android
picks the matching one. A single APK, same `versionCode` (Tauri builds it from the version) and the
same signature: installed apps keep updating themselves. It weighs a few MB more than the 64-bit-only
one.

**What "TV" is.** The plugin (`MythosPlugin.kt`, `device` command) says it's a TV if the system is
in TV mode (`UiModeManager`), if it has `android.software.leanback` or if it has no touchscreen.
With that the interface (`lib/tv.svelte.ts`) sets `data-tv` on `<html>`; it's saved and on the next
open it's applied before painting (`index.html` script). In the app it's automatic, with no option.

**In the browser (e.g. Firefox on the TV box).** If the box's WebView can't be updated, the
alternative is the server's web interface in Firefox for Android (its own engine, doesn't use the
WebView; works on Android 8+ and on 32 bits) over Tailscale. There TV mode is the same and is
decided like this: what was chosen in Settings → Appearance → "TV mode (remote control)" (saved in
that browser), or `?tv=1` / `?tv=0` / `?tv=auto` in the URL (also saved). With no choice,
automatic: Android without a touchscreen (`maxTouchPoints` 0 and no "coarse" pointer) or a Smart TV
browser (Android TV, Fire TV, BRAVIA, Tizen, webOS...). With TV mode the login also requests the
non-expiring session and the account is kept as a profile, same as in the app. What changes: the
remote's back button navigates the browser history; the router (`setBackGuard`) first asks
`lib/spatial.svelte.ts`, which, with something open (drawer, dialog, menu, player menu, "Who's
watching?", "Add profile"), closes it like Esc and leaves the route as it was; if nothing is open
it's the usual back (exits the player, goes back to the previous view). The `BrowserBack` /
`GoBack` keys are also accepted. Esc on the keyboard does the same as back.

**Server only.** On a TV, or with less than 2 GB of RAM (`MemTotal` < 1.5 GiB or
`isLowRamDevice()`), the local mode's embedded server never starts (it can't handle downloading
torrents there): the mode is always "Server only" and Settings → Connection explains it instead of
offering Auto / Local only (`docs/local-mode.md`). The login goes straight to the home server.

**10-foot scale.** The page is laid out at 960 px wide (Android TV's 960x540 dp canvas) with
`<meta name="viewport" content="width=960">`, and the WebView scales it up to fill the screen (the
plugin turns on `useWideViewPort` and `loadWithOverviewMode` on TV). On a 1080p TV everything is
drawn at double size, whatever density the box has. The sidebar stays as icons and opens over the
content when focus enters it.

**Lightweight.** TV boxes have little memory and a small GPU (the real X96 Mini: Hi3798MV310,
Android 9, 32-bit userspace, 1 GB of RAM with ~380 MB free, Mali-450). On TV the embedded server
doesn't run (above), nothing big is preloaded (hls.js and each view are downloaded only when used)
and `styles/tv.css` removes every `backdrop-filter` (the blur behind panels, menus and the player
controls is recalculated every frame and makes the video stutter): each theme's backgrounds remain.

**Remote control** (`lib/spatial.svelte.ts`, TV only):

- Arrows: focus goes to the nearest pressable thing in that direction, looking at where it's drawn
  (poster rows, hero, sidebar, detail, streams, Settings, menus, dialogs). Sideways it only goes to
  what's at the same height; up/down doesn't jump between the sidebar and the content; if there's
  nothing, the page scrolls (and rows that load when approached appear). The row scrolls on its own
  so the focused item is visible.
- Poster rows (home, search, "Similares" ("More like this")): each focused row keeps its top edge
  always at the same height (`ROW_TOP`, 40 px) and up/down move to the adjacent row in order, even
  if it hasn't loaded yet: the row goes to its place and focus enters when its cards arrive (holding
  the arrow keeps going row by row). Below the last one it does nothing (home has space below so the
  last one can also sit at the top). A row's "see all" title isn't a stop. Skeletons measure the
  same as the cards: nothing shifts on load. Everything else keeps a margin (56 px top, 96 px
  bottom) to see what comes next; a Settings section that opens goes to the top of the screen.
- Center (Enter): presses the focused item. On a text field it opens the system keyboard (passing
  over it doesn't open it: `inputmode="none"` while navigating). Search opens it on its own when
  entered from the sidebar; when coming back with back (from a result) it doesn't, and focus returns
  to the card (`route.pop` from `lib/router.svelte.ts`).
- On entering a view focus goes to the main thing: what's marked with `data-autofocus` or,
  otherwise, the first thing on the page among the selected one (current tab or option) and the
  primary button (on a series' detail page, Play and not the "Temporada 1" ("Season 1") tab).
- Back: the usual Esc (closes menus and dialogs, exits the player, goes back to the previous view)
  and on return focus lands on the card you left from. When a drawer closes focus returns to what
  opened it; if the focused item disappears (a button that gets disabled while saving), the next
  arrow continues from where it was.
- Remote's menu key: the element's context menu (the right-click / long-press one).
- Things with their own button inside (the X on "Continue watching", `data-nav-inner`): with focus
  on the card it's visible, and up focuses it.
- Samsung: the on-screen keyboard's "Done" moves to the next field of the form and on the last one
  submits it (login: username → password → sign in); in a dialog with one field (Admin: rename,
  password) it confirms.
- Dropdowns (`components/Select.svelte`) have no search box on TV: the arrows belong to the list
  and focusing it opened the keyboard, which shrank the page and closed the list.
- Settings: no "Atajos de teclado" ("Keyboard shortcuts") (they're useless with the remote and the
  player's arrows do something else).
- Trailer (YouTube): center = play/pause, left/right = -10/+10 s (and the media keys), back closes
  it; a hint at the bottom says so. The iframe doesn't receive focus: it's driven through the
  YouTube player's `postMessage` API (`lib/trailer.ts`). If the owner doesn't allow watching it
  outside YouTube (errors 101/150), our own notice instead of the "Watch on YouTube" that can't be
  pressed.
- Focus: 3 px ring separated from the edge, in the `--focus` color (`styles/themes.css`): on TV the
  text color, which contrasts with the background in every palette (the accent doesn't: Poolsuite
  Sunset/Mint Pool and Default Paper stayed below 3:1); in light Poolsuite the accent (everything
  has a black border); over the video and the home featured item, white (`--on-media`). Cards grow
  a bit (with "reduce motion", without animating) and their ring goes with a gap of background. The
  outer ring is only for focus: the selected item (theme, palette) is marked inside, otherwise from
  3 m away you can't tell where you are. In Athanor the focused item loses the staircase (the
  `clip-path` cropped the ring).
- No mouse: what appears on hover on the PC appears with focus (each episode's buttons: mark
  watched and pick stream). No scrollbars. No copy link / open in VLC menu on streams.
- Always lite (`data-lite`) and single-jump scrolling: see `docs/tizen.md`, "Performance".
- Player: no volume (the TV's is used; also the bar kept the arrows) and no fullscreen. Controls hide
  after 5 s. Notices (stalled, error, finished) take focus; so do the audio and speed menus, and the
  streams and episodes panels enter at the current one. After choosing a speed or an audio track,
  focus returns to its button. Media keys also work with a menu open. With the seek preview
  (docs/API.md), the thumbnail goes above the time. Left/right bring focus to the
  progress bar, which with focus looks thicker, with the accent ring, a big dot and the time above
  it (`components/SeekBar.svelte`). center = play/pause (also with focus on the bar); left/right =
  -10/+10 s (holding, the jump grows: 10, 30 and 60 s per step; the jump, tapped or held, is applied
  only once on release: one request to the server); up/down show the controls and move between
  them (subtitles, audio, speed, streams); below everything hides them; back exits. The remote's
  media keys (play/pause, fast-forward, rewind) also work; fast-forward/rewind held grow the same
  even if the remote doesn't repeat them (the Samsung one doesn't: a timer until keyup). While
  loading, play/pause doesn't pause (it stayed at "Cargando 99%" ("Buffering 99%") forever, with
  nothing saying it was paused). With the server down "try another stream" doesn't appear: it waits
  with the top bar and continues on its own when it comes back.

**Profiles.** On the TV each account is saved as a profile (`lib/profiles.svelte.ts`): you sign in
only once with the password and the session never expires (`persistent`, `docs/API.md`). On open
it goes straight into the last used profile (no "Who's watching?"); the sidebar avatar opens
"Who's watching?" (the current one already selected; back closes it without switching profile). "Add profile" adds another
account; "Manage profiles" opens each profile to choose its avatar (with that profile's session,
without entering) or remove it (closes its session on the server). If someone revokes that session
from Settings → Signed-in devices or the admin from Admin → Users → "Signed-in devices", the profile
is deleted on its own and the picker comes back. Switching profiles reloads the page (no cache from
the previous account remains). Theme, language and subtitles belong to each account: the profile
keeps the latest copy of its preferences and, when chosen, they're applied before reloading (the
page starts with its theme already; the server's arrive afterwards). "Who's watching?" and the
login use the theme of the last profile that signed in (`lib/prefs.svelte.ts`). Only
on TV: on the phone and the PC "Switch account" is still signing out and in.

**Minimum WebView: Chromium 111.** The interface uses `color-mix()` in almost every color (111),
`:has()` (105) and `AbortSignal.timeout` (103). Many TV boxes ship an old Android System WebView
(the X96 Mini, 66 from 2018): with that the app module doesn't even compile. That's why
`index.html` has an ES5 script that checks the Chrome/Chromium version from the userAgent before
loading anything: if it's below 111 it doesn't load the app (the `gate` plugin in `vite.config.ts`
removes the `<script type="module">` from the HTML and `mythosBoot` loads it only if it's enough)
and shows "Update Android System WebView" with the steps, both versions and, as an alternative,
opening the server address in Firefox (see "In the browser"). On an old Chromium-based PC browser it
says "Update your browser"; Firefox requires 121 (`:has()`; otherwise, "Update Firefox") and Safari
always passes. To see it on the PC: `http://localhost:5173/?webviewTest=66`.

How to update a TV box's WebView: from Google Play ("Android System WebView" → Update) or, without
Play Store, installing the Android System WebView APK for the box's ABI (armeabi-v7a or
arm64-v8a) with Downloader or a USB stick and selecting it in Developer options → WebView
implementation. Careful: some firmwares ship an AOSP WebView (`com.android.webview`) signed by the
manufacturer that can't be replaced without root (and don't show "WebView implementation"); there
the app doesn't work and Firefox is what's left. Also careful with the WebView APK's minSdk: a box
with Android 8-9 (API 26-28, even if it says 10 or 13) won't install one that requires API 29.

**APK Android version.** `minSdk` 24 (Android 7.0): it works on Android 8-9 boxes. What only exists
in newer versions of the plugin has its check (`canRequestPackageInstalls` from 26,
`setRequireUserAction` and `FLAG_MUTABLE` from 31, `ACCESS_LOCAL_NETWORK` from 37); permissions the
system doesn't know are ignored.

**Testing on the PC** (without the box): `npm run dev:mock` and `http://localhost:5173/?tv=1` (or
the Settings → Appearance switch; `?tv=auto` goes back to automatic), with arrows, Enter and Esc
(Esc acts as back). The desktop browser ignores the meta viewport width: to see the scale, a
960x540 window (or iframe).

Not yet tested on the real device: detection (`device`) and the 960 px viewport depend on the box's
WebView.

## APK updates

Same as Haze: a while after opening and every six hours it checks whether the latest Release of
`MartinSchubert04/Mythos` has a newer version in `latest-android.json`, downloads it in the
background and verifies the sha256. Android verifies the signature: an APK signed with another key
doesn't install over it.

It installs itself when you leave the app with nothing playing, if Android allows doing it without
asking: Android 12 or newer, with "Install unknown apps" and Mythos as the installer of the current
version (`setRequireUserAction(USER_ACTION_NOT_REQUIRED)`, which also needs the
`UPDATE_PACKAGES_WITHOUT_USER_ACTION` permission; without it Android asks for confirmation anyway).
Otherwise, "Update now" stays in Settings → App: the first time it opens the permission setting and
then the system dialog. On opening the new version the interface returns to the same screen (and the
player to the same second, with `?resume=1`). The downloaded APK is deleted on the next startup.

Tested on the emulator (Android 17): check, download and verify from a test `latest-android.json`;
"Update now" with the permission and the dialog; automatic install on exit (0.2.1 → 0.2.2 without a
dialog) and returning to the same screen.

To test without publishing (debug builds only): serve `latest-android.json` and the APK on the PC
and point the app there:

```
adb shell run-as dev.mrtn.mythos sh -c 'echo http://10.0.2.2:8000/latest-android.json > files/update-url.txt'
```

## Building

You need the Android SDK and NDK, Java 21 and the Rust targets
(`rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`). The Tauri
CLI reads:

```
ANDROID_HOME=%LOCALAPPDATA%\Android\Sdk
NDK_HOME=%ANDROID_HOME%\ndk\28.0.13004108
JAVA_HOME=C:\Program Files\Java\jdk-21
```

```
cd apps/mobile
npm run build:emulator     # x86_64 debug APK for the emulator
npm run build:debug        # arm64 + armv7 debug APK (phone or TV box)
npm run build              # arm64 + armv7 release APK (the one that gets published)
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

On Windows `tauri android build` needs developer mode (it creates a symlink of the `.so` in
`jniLibs`). Without it, it fails after compiling Rust; you can continue by hand: copy
`src-tauri/target/x86_64-linux-android/debug/libmythos_mobile_lib.so` to
`gen/android/app/src/main/jniLibs/x86_64/` and run in `gen/android`
`gradlew assembleUniversalDebug -PabiList=x86_64 -ParchList=x86_64 -PtargetList=x86_64 -x rustBuildUniversalDebug -x rustBuildX86_64Debug`.
With two ABIs (the release), the same for each one:

```
copy src-tauri\target\aarch64-linux-android\release\libmythos_mobile_lib.so gen\android\app\src\main\jniLibs\arm64-v8a\
copy src-tauri\target\armv7-linux-androideabi\release\libmythos_mobile_lib.so gen\android\app\src\main\jniLibs\armeabi-v7a\
cd src-tauri\gen\android
gradlew.bat assembleUniversalRelease -PabiList=arm64-v8a,armeabi-v7a -ParchList=arm64,arm -PtargetList=aarch64,armv7 -x rustBuildUniversalRelease -x rustBuildArm64Release -x rustBuildArmRelease
```

CI runs on Linux and doesn't have the problem (it compiles both and verifies the APK carries them).

Native logs come out in `adb logcat` (filter by `mythos` or `Tauri`). In debug the WebView is
inspected from `chrome://inspect`.

## Publishing

Desktop and Android ship together with the same version: the `vX.Y.Z` tag triggers `desktop.yml`
and `android.yml`, and both upload to the same Release with `.github/scripts/release.sh` (the first
one creates it, marked as latest). Android uploads `Mythos-X.Y.Z-android.apk` and
`latest-android.json`.

The signing comes from four repo secrets (Settings → Secrets and variables → Actions). To create
them, just once:

```
keytool -genkeypair -keystore mythos.jks -alias mythos -keyalg RSA -keysize 4096 -validity 36500
base64 -w0 mythos.jks   # on Windows: certutil -encode mythos.jks mythos.b64 (without the BEGIN/END lines)
```

| Secret | Value |
|---|---|
| `ANDROID_KEYSTORE` | `mythos.jks` in base64 |
| `ANDROID_KEYSTORE_PASSWORD` | the keystore password |
| `ANDROID_KEY_ALIAS` | `mythos` |
| `ANDROID_KEY_PASSWORD` | the key's password (if it's the same, it can be omitted) |

All versions are signed with the same keystore. If it's lost, nobody can update without
uninstalling: keep it in two places, outside the repo. Without `keystore.properties` the local
release is signed with the debug key (good for testing, not for publishing; CI fails if the secrets
are missing). For an APK signed on the PC, create
`apps/mobile/src-tauri/gen/android/keystore.properties` (it's in `.gitignore`) with `storeFile`,
`storePassword`, `keyAlias` and `keyPassword`.

The workflows (`.github/`) need a token with the `workflow` scope to be pushed.
