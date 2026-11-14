# Mythos for Samsung (Tizen)

The Samsung TV app (`apps/tizen`): a minimal Tizen web app (`.wgt`) that acts as a shell,
like the Android one with "server interface". It asks for the server address the first time,
saves it, checks that it's a Mythos and navigates to the interface the server serves, which enters
TV mode on its own (`lib/tv.svelte.ts` recognizes "Tizen" in the user agent).

## Where everything goes

```
apps/tizen/
  config.xml          the app: id MythosTV01.Mythos (the package has to be 10 characters),
                      version (the repo's), privileges, navigating to any origin
  index.html          the local page: pick a server ("Conectando a ..." / "Cambiar de servidor"; in
                      Spanish, or in English if the TV is in English)
  shell.js, shell.css (ES5: on an old TV it says it's not enough instead of staying black)
  icon.png            the Android app's (512x512)
  build.ps1           builds a signed build\Mythos.wgt and, with -Tv <ip>, installs and opens it
  tools/samsung-cert.py  the Samsung certificates (below)
apps/web/src/lib/tizen.ts   the Samsung-specific bits in the interface: key names, media keys,
                            exiting the app
```

## How it works

- **The server address.** Same as on Android, no build carries it. The TV doesn't have
  Tailscale: it uses the home network one (`http://<ip-del-server>:7700`, with `MYTHOS_EXTRA_BIND` on the
  server). It's typed with the TV keyboard (the remote's center button opens it; the IP is enough:
  without `http://` it uses `http://`, and without a port, `:7700`) or found with **Buscar en esta red** ("Find on this network"): it probes
  `http://<x.y.z.1-254>:7700/icon.svg` on the TV's network (the IP comes from `tizen.systeminfo`).
  It stays saved; on open "Conectando a ..." ("Connecting to ...") shows for a moment with **Cambiar de servidor** ("Change server").
- **Checking that it's a Mythos.** `GET /api/health` (`ok`, and that it's not another
  app's embedded server). If the local page can't read it (CORS: the server only lets its known
  origins read), it's enough that `/icon.svg` loads from that address.
- **Navigate, not iframe.** The server sends `frame-ancestors 'none'`, so the local page navigates
  to the server's (`<tizen:allow-navigation>*`: the address is chosen by the user). Before leaving
  it marks its history entry (`#away`).
- **Keys.** `lib/tizen.ts` translates Samsung's codes to the standard names already used by
  `lib/spatial.svelte.ts` and the player: back (10009) = `BrowserBack`, and the media keys
  (play/pause, play, pause, stop, fast-forward, rewind). Media keys only arrive if they're
  registered with `tizen.tvinputdevice.registerKey`: the local page does it before navigating and the
  interface on startup (if that page has `tizen`). Stop pauses.
- **Back** (`tizenBack` in `lib/spatial.svelte.ts`). The remote sends `XF86Back` (keyCode 10009)
  and the TV doesn't navigate on its own. Same as the Android back button: first an Esc to whatever is focused
  (closes drawer, menu, dialog, trailer, player menus; in the player with nothing open, it
  exits); if nobody uses it, the previous view. On the home screen: if the rows were being browsed,
  it goes back up (to the featured item's Play); at the very top, it closes the app without asking (the
  "go back up" step already prevents exiting by accident, and a confirmation would be one more key press every time).
  Closing: with `tizen.application` if the server's page has it; if not, `history.back()` goes back to
  the local page, which closes itself when it sees `#away`. Tested with the real remote on home,
  title page, stream drawer, ⋯ menu, trailer, player (with and without a menu) and exit.
- **TV mode always.** On Samsung there's no option (Settings doesn't show "Modo TV" ("TV mode")): when off, the
  remote didn't move anything and there was no way to turn it back on (it stayed saved).
- **The TV keyboard.** It opens with the center button on a field (search opens it on its own when
  entering from the sidebar). "Done" moves to the next field and on the last one submits. Back with the
  keyboard open closes it and doesn't change view; sometimes the keyboard swallows the first back and
  a second one is needed. While it's open the page is half as tall (`resize`): nothing that's
  open should close because of that (the `Select` with a search box used to close: on TV it no longer has one). On a
  **password** field the keyboard doesn't respect `inputmode="none"` and opened just by passing
  over it (login, Real-Debrid): while navigating, the fields are also read-only
  (`lib/spatial.svelte.ts`, like the local page).
- **Held media keys.** The TV doesn't repeat fast-forward/rewind (one keydown on press and
  one keyup on release, unlike the arrows): the jump grows with a timer until the keyup.
- **Server down.** If the server neither answers nor refuses (off, or the network drops the packets),
  the browser waits for minutes: a request or a view that takes more than 8 s probes `/api/health`
  (5 s) and, if that doesn't answer either, the "No se puede conectar con el servidor" ("Can't reach the server") bar appears and it
  retries on its own (`lib/net.svelte.ts`). In the player it waits without "try another stream" and resumes
  on its own when it's back. If the server already wasn't answering when the app was opened, the local page says so ("No hay
  un servidor de Mythos en ..." ("Can't reach a Mythos server at ..."), with the address to correct).
- **Trailer.** Plays on the TV (youtube-nocookie iframe) and is controlled with the remote from
  outside the iframe (`lib/trailer.ts`, docs/mobile.md "TV"). Some can't be embedded (the
  owner decides on YouTube): custom notice.

## Playback: AC-3 / E-AC-3

The TV accepts AC-3 and E-AC-3 on their own in MSE but **not muxed with the video** in the same
SourceBuffer (`video/mp4;codecs="hvc1…,ec-3"` → not supported), and the server's HLS segments carry
both tracks together. Previously the interface said yes (`ClientCaps`), the server copied the audio and
hls.js couldn't even create the buffer: "The stream stopped responding" with almost any
WEB-DL (DDP5.1) release. Now `lib/caps.ts` also asks for the muxed combination (the server converts the
audio) and, if it still happens (`bufferAddCodecError`), the player downgrades the audio and continues.

## Chromium 120: what's different from the PC

- `<button>`s come with `align-items: flex-start` in the browser's styles (current Chrome,
  `normal`): a button with `display: flex` in a column left its children at content width
  (each theme's preview in Settings was 2 px wide). `styles/base.css` sets it to `normal`.
- `Intl` in es-AR: the compact format gives "1 K" but "10 k" (the votes list in Calificadas, "Top rated", now
  goes in thousands by hand: "25 mil+ votos").

## Performance

The page is drawn at double size (960 px in 1920) and the UN50DU7000 rasterizes on the CPU: what's costly is
repainting, not the JavaScript. On TV (`data-tv`):

- Always lite mode (`data-lite`, `lib/perf.ts`): no blurs, no slow hero zoom,
  no staggered appearance of the cards, backdrops at 1280 px at most.
- Scrolling in one jump (`scrollBehavior()` from `lib/motion.ts`, `styles/tv.css`): smooth scrolling
  repainted every frame and accounted for most of the dropped frames.
- Cards grow on focus without animating and with only the ring (no soft shadow): animated,
  each card became a layer while it lasted and when it ended the page below was repainted.
- The hero only rotates with the remote idle for 8 s; rows don't measure their arrows (they're not shown);
  sideways, navigation only searches within the row (`lib/spatial.svelte.ts`).

Measured on the TV (home screen with the rows already loaded, 25 arrow presses): before ~30 frames/s, 70–82
frames over 50 ms, key→frame p90 390–440 ms, 62–67 long tasks; after ~55 frames/s,
16–18, 234–241 ms and 13. Idle/navigating raster dropped from ~128 % to ~46 % of one core.

## Tools (once, on the PC)

- **Tizen Studio CLI** (`web-cli_Tizen_Studio_6.1_windows-64.exe` from
  `https://download.tizen.org/sdk/Installer/tizen-studio_6.1/`). The installer asks for administrator;
  without admin it installs into the profile:
  `$env:__COMPAT_LAYER='RunAsInvoker'; .\web-cli.exe --accept-license $env:USERPROFILE\tizen-studio`
  It brings `tools\sdb.exe`, `tools\ide\bin\tizen.bat` and its own Java 8 (`jdk\`).
  Careful: the package manager (`package-manager-cli.exe install ...`) tries to update WebCLI
  first, fails ("Cannot install updates") and leaves `jdk\` and part of the package manager deleted:
  it's not needed for any of this (if it was used, reinstall).
- **Python 3** with `cryptography` and `requests` (for `samsung-cert.py`).

## TV developer mode

A factory TV doesn't install a `.wgt` from a USB drive or from just anywhere: only from the
Samsung store or, in developer mode, via `sdb` from the chosen PC and signed with a
Samsung certificate that names that TV.

1. On the TV: Apps → press `1 2 3 4 5` → Developer mode **On** and **Host PC IP** = the PC's IP.
2. Really restart the TV (hold the power button until the logo appears):
   only then does it open the `sdb` port (26101).
3. `sdb connect <ip-de-la-tele>` and `sdb -s <ip-de-la-tele>:26101 shell 0 getduid` (the DUID).

## Samsung certificates

`apps/tizen/tools/samsung-cert.py` does the same as Tizen Studio's Certificate Manager with the
Samsung Certificate Extension, without Eclipse: it opens the Samsung account login in the browser
(answers on `http://localhost:4794/signin/callback`), requests the author and
distributor certificates from Samsung (with the TV's DUID, Public level), builds the `.p12` files with Samsung's CAs and creates
the `Mythos` signing profile in Tizen Studio:

```
python apps\tizen\tools\samsung-cert.py --duid <DUID> --tizen $env:USERPROFILE\tizen-studio
```

They end up in `~/.mythos-signing/tizen/` (outside the repo, like the Android keys):
`author.p12`, `distributor.p12`, `password.txt` (the one for both `.p12` files) and `ca/` (Samsung's public
CAs, downloaded from the extension). The signing profile lives in
`~/tizen-studio-data/profile/profiles.xml`.

- **The author is forever**: it signs every version, and the TV won't install over it one signed by
  another author (you have to uninstall). It isn't requested again: the script refuses if there's already an
  `author.p12`.
- **Another TV**: `--reuse-author --duid <DUID-1> --duid <DUID-2>` requests only a new distributor
  with all the TVs.

## Build, sign and install

```
cd apps/tizen
.\build.ps1                       # build\Mythos.wgt signed with the "Mythos" profile
.\build.ps1 -Tv 192.168.0.139     # also: sdb connect, tizen install and open it
```

`build.ps1` copies only the app's files to `build\` (signing writes there) and runs
`tizen package -t wgt -s Mythos`, `tizen install` and `tizen run -p MythosTV01.Mythos`. To
debug: `sdb -s <ip>:26101 shell 0 debug MythosTV01.Mythos` gives an inspector port
(`sdb forward tcp:<puerto> tcp:<puerto>` and `chrome://inspect`).

## Testing with the network remote

The TV's network remote (wss `:8002`, `samsung.remote.control`) is useful for testing with the
real keys, with a few quirks:

- `Click` sends press and release together: with a text field focused, the center button sometimes opens the
  keyboard and also types the highlighted letter. Separate `Press` and `Release` behave like the
  physical remote.
- `Press` alone, without repeating it, releases on its own after ~1 s; to hold longer, send `Press` every
  ~0.5 s and `Release` at the end.
- A connection idle for more than a minute loses keys silently: reconnect before an
  important key.
- `SendInputString` types into the open keyboard and `SendInputEnd` is its "Done" (keyCode 65376).
- DevTools screenshots don't show the video (iframes yes): check it via the `<video>`'s state.

## Updating

- **The interface** comes from the server: every deploy updates it on its own, without touching the TV.
- **The app** (the local page, `config.xml`) almost never changes: it's updated by reinstalling with
  `build.ps1 -Tv <ip>` (same author; the saved address stays). There's no automatic update.

## Limitations

- It needs developer mode with this PC as Host PC and the distributor certificate with the
  TV's DUID (to install and to reinstall each version).
- **It doesn't go to the Releases**: a signed `.wgt` only works on the TVs of its distributor
  certificate, and it can't be installed without `sdb` (not even from a USB drive). That's why there's no CI workflow:
  building a `.wgt` in CI is no use to anyone without their TV's certificate. Publishing it for any
  TV would mean going through the Samsung store (Seller Office).
- The interface requires Chromium 111 (like on Android): Samsung TVs with Tizen 8 or newer (2024
  onwards; Tizen 7 ships Chromium 94). On an older one the local page says so.
- Server only: the TV doesn't run the local mode's embedded server.
