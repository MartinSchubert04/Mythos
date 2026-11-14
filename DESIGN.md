# DESIGN.md

Mythos design system. Format: [awesome-design-md](https://github.com/VoltAgent/awesome-design-md)
(nine sections). Ported from Ethereon (same themes, palettes, fonts, motion and components),
adapted to a movie app. Read before touching the interface (`apps/web`).

## 1. Visual theme and atmosphere

A Stremio-like client for personal use: images rule. Posters in rows and grids, a large hero
with the backdrop of something featured, the detail page with the movie's backdrop, and the player as
the absolute protagonist (black, controls that hide on their own). It has to feel premium and smooth:
every load has a shimmering skeleton, every image fades in, nothing jumps around.

- **Density:** medium. Posters 118 to 168 px wide depending on the window; landscape "Continue
  watching" cards 230 to 320 px.
- **Voice:** literal, in Spanish by default (neutral Rioplatense, with voseo) or in English if the user
  picks it (Settings → Appearance; strings in `lib/i18n/es` and `lib/i18n/en`). "Reproducir",
  "Seguir desde 42:10", "Elegir stream", "Probar otro stream", "Cargando 72%…" / "Play",
  "Resume from 42:10", "Choose stream", "Try another stream". No metaphors. Terms that are
  said in English (stream, torrent, seeders, HDR, 4K, REMUX) stay that way. In this file the quoted
  strings are the ones from the English version. Spanish is longer: buttons, the
  sidebar and the TV scale (960 px) have to hold it (`ellipsis` or the text wrapping to the next
  line, never a button getting cut off).
- **Six themes, one structure.** Colors, borders, shadows, backgrounds and fonts change; never the layout.
- **The only difference between web and desktop** is desktop's own title bar. On Android
  (`docs/mobile.md`): back button, player in landscape fullscreen, edge to edge with
  `env(safe-area-inset-*)` and Settings → App (version and updates). On Android TV / TV box
  (`docs/mobile.md`, "TV"): 10-foot scale (960 px page scaled up), large focus with the
  accent, icon sidebar that opens on focus, remote control and "Who's watching?".
  The same on the web with TV mode (Settings → Appearance, or automatic in a TV browser).

## 2. Colors and roles

All colors are variables in `apps/web/src/styles/themes.css`. No component has hardcoded
colors.

| Variable | Role |
|---|---|
| `--bg`, `--bg-image` | Window background (Warp and Haze use gradients, Poolsuite a sunset) |
| `--sidebar`, `--panel`, `--panel-2` | Surfaces, from back to front |
| `--line`, `--line-strong`, `--border` | Borders and their width (2 px in Poolsuite and Athanor) |
| `--text`, `--text-2`, `--text-3` | Primary, secondary and tertiary text |
| `--accent`, `--accent-ink`, `--primary-bg` | Accent, text on the accent, primary button (can be a gradient). Also the progress bar and the poster glow |
| `--danger`, `--warn`, `--ok` | Delete or error; IMDb stars and few seeders; many seeders, "Instant", live |
| `--media-bg`, `--on-media`, `--on-media-2`, `--media-scrim`, `--media-line` | Shared by all themes: the video's black and what goes on top (controls, subtitles, player menus) |
| `--win-close` | The Windows red of the close button (desktop) |

On the hero and the detail page the text uses the theme colors (`--text`) and the gradient blends the image
into `--bg`: in light themes the image looks washed out and the dark text is still readable. The player is
always dark (`--media-*`), with the theme accent on the progress bar.

Each theme has palettes (Settings → Appearance). They're defined in `apps/web/scripts/palettes.py`, which
generates `styles/palettes.css` and `lib/palettes.gen.ts` (`npm run palettes`); each theme remembers its own.

| Theme | Palettes |
|---|---|
| Default | Mint, Nord, Dracula, Tokyo Night, Catppuccin, Gruvbox, Paper (light), Srcery |
| Warp | Violet, Solar Flare, Pink City, Jellyfish, Leafy, Red Rock, Ember |
| Poolsuite FM | Sunset, Mint Pool, Tropicana, Lavender, Beach, Platinum, Monochrome (dark), Vellum |
| Haze | Cream, Peach, Rose, Mauve, Sky, Sage, Amber |
| Athanor | Srcery, Umber, Cinnabar, Vellum (light), Orpiment (light) |
| Neovim | Neovim, Nightshade, Oxblood, Paper (light), Moss, Kanagawa |

| Theme | Background | Accent | Trait |
|---|---|---|---|
| Default | `#0d0f14` | mint `#6ee7b7` | Flat, understated |
| Warp | black with violet and cyan lights | gradient `#8b5cf6 → #22d3ee` | Translucent panels with blur |
| Poolsuite FM | pastel sunset | pink `#ff4f9a` | Light, 2 px black borders, hard shadows, striped classic Mac bar |
| Haze | warm and cool haze | cream `#f9e2af` | Glass |
| Athanor | `#1C1B19` (Srcery) | amber `#FBB829` | Pixel clay: stepped corners, bevels, dithered shadows; bitmap font |
| Neovim | `#0c0c0c` | light gray | Everything in JetBrains Mono, little radius |

## 3. Typography

Inter Variable for everything and Geist Mono Variable for times, speeds and sizes (with
`tabular-nums`). They come bundled (`@fontsource`); the browser downloads each one only when a theme
uses it.

**Exceptions:** Athanor uses PxPlus IBM VGA 8x16 at exactly 16 px (bitmap, no bold) and titles in
Jacquard 24; Neovim uses JetBrains Mono. Video subtitles are always in Inter (`--font-sub`).

| Use | Size | Weight |
|---|---|---|
| Hero / detail title (`.display-title`) | 30 to 58 px (clamp) | 760 |
| Page title | 21 px | 650 |
| Row title | 16 px | 650 |
| Text | 13.5 px (descriptions 14.5 to 15 px) | 400 to 560 |
| Section labels | 11.5 px uppercase | 650 |
| Mono (times, MB/s) | 12.5 to 13 px | 600 |
| Subtitles | `clamp(18px, 2.4vw, 40px)` × chosen size | 600 |

## 4. Components

- **Poster** (`PosterCard`): 2:3, small image (`/poster/small/`) with `loading="lazy"` and a fade on
  load. On hover it rises 4 px, grows 4 %, the border and shadow take the accent and a sheen
  crosses it; the rating appears. Hovering already loads the detail page (code and data). On click the
  poster flies to the detail page (View Transitions).
- **Row** (`Row`): title, smooth horizontal scroll with arrows that appear on hover, loads
  its data only when it gets close to the screen, skeletons in the meantime. It renders the cards
  16 at a time (the next ones when nearing the end) and off-screen it isn't painted
  (`content-visibility: auto`): the home screen has ten rows of ~50 titles.
- **Hero** (`Hero`): backdrop with slow zoom, logo or title, rating, year, genres, description, Play /
  More info. Rotates every 8 s with a crossfade (waits for the next image to be downloaded).
- **Continue watching** (`ContinueCard`): 16:9 with progress bar, episode and time remaining.
- **New episodes** (`NewEpisodeCard`, `lib/newEpisodes.ts`): below Continue watching, one episode
  per Library series released in the last 14 days and not started ("New · 3" if there are more). No
  skeleton: the row appears only if there's something. Metas are requested 4 at a time and each series'
  recent episodes stay in `sessionStorage` for an hour.
- **Stream list** (`StreamList`): large quality (4K with the accent), size, color-coded seeders
  (`--ok` ≥ 50, `--warn` ≥ 10, `--danger` below), source, languages, tags as chips (HDR, DV, REMUX
  highlighted). "Cached" ones carry "Instant" and a green border. The first one says "Best". With an
  audio language in Settings → Playback, streams in that language move up (flags and release words:
  "Latino", "Castellano"…; never one with < 5 seeders or CAM). ⋯ menu (on hover, right
  click or long press): "Copy stream link" (the direct link with `?t=`: the tooltip warns that it
  carries the session) and, on desktop, "Open in VLC".
- **Small menu** (`PopMenu`, `lib/press.ts`): floats where it was requested (right click, long press
  550 ms or a ⋯ button), attached to `<body>`; Esc or tapping outside closes it.
- **Player** (`views/Player.svelte`): custom controls that hide after 2.8 s; bar with
  the buffered range and the time on hover (`SeekBar`); subtitle, audio and speed menus;
  stream panel on the right; loading screen with live data (`PlayerLoading`); resume
  dialog; "Try another stream" panel; "Next episode" during the credits and a countdown at the end.
  No new buttons: hold Space (or a finger) = 2× with the "2×" pill at the top; `[`/`]` the
  speed; V the scale (Fit = whole, Fill = stretched, Crop = fills by cropping; remembered for the
  session); D the stats (mode, codec, peers, MB/s, buffer, dropped frames; `/status` only
  while they're shown; on the phone, "Playback stats" in the stream panel); `?` the shortcut list.
  With a finger: tap shows/hides the controls, double tap on one side −10/+10 s with a ripple
  (keep tapping to add more), pinch out = Crop and in = Fit. Media Session: title, poster,
  position and play/pause/skips/next episode on the media keys, headphones and the
  Android notification.
- **Drawer:** forms and the detail page's stream list, on the right over a scrim.
- **Select**, **Switch**, **Seg**, **Confirm**, **Section**: Ethereon's, unchanged.
- **Notices (toasts):** come in from the bottom right; red if they fail.
- **Custom tooltips** (`lib/tooltip.ts`): every `title=` is shown with a delay and the theme's style.
- **Subtitle menu** (step by step, like Stremio): first "Off" and one entry per language with its
  name in that language ("Español (Latinoamérica)", "Português (Brasil)"), the number of options and
  "Embedded" if the file has any; the preferred one and the audio's at the top, then the ones that come
  in the file and the rest alphabetically. Picking a language selects its first option and moves (sideways)
  to its options: the internal ones first, then the OpenSubtitles ones ("Version n"). At the bottom, fixed:
  **Delay** of the chosen option (−0.5 / −0.1 / +0.1 / +0.5 s and reset; remembered per file and
  track, and the list marks the ones that have one saved), and the appearance, which is global and travels with the
  preferences: **Size** (75 to 200 %), **Position** (height from the bottom) and **Background** (None,
  Light, Dark, Solid). With a subtitle on, the menu opens directly in its language. On the phone it's
  an edge-to-edge sheet with 44 px rows. Subtitles are drawn by the app (not `<track>` or
  `::cue`), so they look the same in Chrome, WebView2 and Android and follow real time in HLS.
- **Detail page buttons**: always in one row; on the phone "Choose stream" is left with only the icon.
- **Avatar** (`Avatar.svelte`): the image the user picked from the server's gallery
  (Settings → Account; the admin puts it together in Admin → Avatars) or, with no avatar, the initial on
  `--primary-bg`. Round, in the sidebar, Settings and the TV profile picker.
- **TV focus** (`styles/tv.css`, `data-tv`): 3 px `--accent` ring offset by 3 px; the
  cards grow 6 % with the ring on the frame. Nothing depends on hover.
- **Cast** (detail page): avatars with initials on the accent (the catalog has no photos). Below,
  the "More like this" row (same main genre).
- **Top rated** (`views/Top.svelte`, `#/top?type=series&genres=Crime,Drama&votes=10000`): the
  best of IMDb (`/api/top`). At the top "Movies / Series" (`Seg`), the minimum votes (`Select`) and
  "Clear"; below, one chip per genre (multi-select, up to 3, all have to match: the
  selected one uses `--primary-bg`, the others show how many would remain and dim out if they don't combine).
  Poster grid with the rating pinned (`PosterCard pinRating`) that fetches 60 at a time when nearing the
  end; the filter lives in the URL. On the phone the chips go in a single line that scrolls. It's reached
  from the sidebar, from Search and from the title of the home screen's "Top rated" rows
  (`Row href`, with an arrow).
- **Episodes**: on wide screens they go in two or three columns; specials (season 0) are
  called "Specials" and go at the end; with more than 6 seasons, Select instead of Seg. Watched / unwatched by
  hand: the checkmark when hovering an episode, right click or long press (menu), the ⋯
  next to "Episodes" for the whole season, and for movies the checkmark button in the
  actions row. It's progress with the position at the end (or deleting it). "Hide spoilers" (Settings →
  Playback): blurred image and covered description on unstarted ones until hovered (with
  a finger, tap the description).

## 4b. States

| State | What's shown |
|---|---|
| Loading | Shimmering skeletons in rows, grids, detail page, admin; splash with the brand on open |
| Empty | Icon + sentence + action: empty Library ("Browse"), search with no results, Top rated with no matches ("Clear genres" / "Any votes"), no "Continue watching" (the row doesn't appear) |
| Top rated without data | "Getting the IMDb ratings…" with the spinner while the server downloads IMDb the first time (retries on its own every 10 s); on the apps' embedded server, "Top rated needs the home server" |
| No streams | "No streams found" with the hint (too new, or the Settings limits) and Retry |
| Server down | Pill at the top "Can't reach the server. Retrying…" with Retry; retries on its own every 5 s and when it comes back the view reloads. If it happens on open: reconnection screen (not the login) |
| Session expired (401) | Login with the notice "Your session expired" |
| Playback error (502, codec) | Panel in the player with the reason and "Try another stream" |
| No images | Typographic poster (accent gradient + name + year); detail page with the blurred poster as background |

## 5. Layout

Sidebar on the left (212 px, or 64 px collapsed with only icons; remembered) with Home,
Search, Top rated, Library, Settings and Admin (admins only), and the user at the bottom. The content scrolls in its
own container and remembers where it was when coming back.

Desktop: a 40 px bar at the top (`Titlebar`) that is the title bar of the frameless window:
collapse the sidebar, the brand, a gap to drag (double click maximizes), search box and
minimize / maximize / close.

The player takes up the whole window (no sidebar or title bar); on desktop its window
controls go in its top bar, which can also be dragged.

Hash routes: `#/`, `#/search?q=`, `#/top?type=&genres=&votes=`, `#/title/{type}/{id}`,
`#/watch/{type}/{id}?s={stream}`, `#/library`, `#/settings`, `#/admin`. Each view is loaded only when needed.

Shortcuts (listed in Settings → Keyboard shortcuts): `/` or `Ctrl K` search (what's typed
while the view loads isn't lost); arrows between the posters of a row and between rows; Esc closes
panels and menus. In the player: space/K (hold space: 2×), F, M, ←/→ (5 s), J/L (10 s),
↑/↓ volume, `[`/`]` speed, V scale, C subtitles, G/H the current subtitle 0.5 s earlier/later
(with Shift 0.1 s), -/= subtitle size, N next episode, D stats, `?` shortcuts, Esc
(closes menu → exits fullscreen → goes back). The list lives in `lib/shortcuts.ts` (Settings and
the player's `?` use the same one).

## 5b. Motion

All animations go through `apps/web/src/lib/motion.ts` (Svelte transitions) or are short CSS
from `base.css`, and last 0 with the system's or Settings' "reduce motion"
(`data-reduce-motion` on `<html>`). **Only `transform` and `opacity`**: nothing that recalculates layout on
every frame (the player bar uses `scaleX`/`translateX`, the hero dots `scaleX`, the
volume floats on top). No animated `filter: blur`. Measured with a trace (home → detail → player →
back and row scrolling): no long tasks (> 50 ms) and CLS 0.

| What | How |
|---|---|
| Changing view | rises 6 px and appears (220 ms) |
| Card → detail | the poster flies (View Transitions, 420 ms); without support, the normal view change |
| Hero | 900 ms crossfade and slow backdrop zoom (12 s) |
| Posters and episodes | appear staggered (30 ms between each); on hover, they rise and grow with a spring |
| Images | 450 ms fade when they finish loading |
| Loading | skeletons with a sheen that crosses them (1.4 s) |
| Drawer | comes in from the right with a bit of spring |
| Dialogs, menus, dropdowns | grow from 94-95 % (160 ms) |
| Buttons | rise 1 px on hover, sink when pressed; the primary one has a sheen that crosses it |
| Login | the brand glows; wrong password: the card shakes |
| Player | the controls slide outwards; play/pause/skips show an icon in the center that grows and fades |
| Notices | come in from the bottom and rearrange (flip) |

In Athanor motion is stepped (`steps()`), like pixel art.

**Weak devices** (`lib/perf.ts`: ≤ 2 GB of RAM, ≤ 2 cores, or Android with ≤ 4 cores, like the
TV boxes; forced with `localStorage["mythos.lite"]` = `"1"` or `"0"`): `data-lite` on `<html>`
removes the blurs (`--blur: none` and no `backdrop-filter`), the hero's slow zoom, the
staggered appearance of the cards and the shadow animation; backdrops are requested at 1280 px
at most. The rest (fades, hover, view transitions) stays the same.

## 6. Depth

Default, Warp, Haze and Neovim: no shadows at rest; on hover the posters take on a shadow
and an accent glow. Poolsuite: hard offset shadows (`3px 3px 0`) and on hover the
poster shifts and the shadow grows. Athanor: the poster rises one step, no soft shadow. What floats
(dialogs, menus, drawers) uses `--pop-shadow`. The player uses dark glass (`backdrop-filter`)
for menus and panels.

## 7. Dos and don'ts

- Do: never a blank screen. Always a skeleton, splash with the brand or the loading screen.
- Do: reserve the size of images (`aspect-ratio`) so nothing jumps when loading.
- Do: states with color and text at the same time (seeders in red and the number; "Instant" green).
- Do: secrets never visible; the stored Real-Debrid key shows "Stored. Type to replace."
- Do: each image at the size it's displayed at (`lib/images.ts`): `small` posters; backdrops and
  thumbnails according to the real width (480 px backdrop from metahub, downscaled by the server to
  640/960/1280, or the 1920 original only if the screen asks for it). See `docs/API.md`
  (`/api/media/img`).
- Don't: `backdrop-filter`, `transform: translateZ(0)` or `will-change` on something repeated
  (cards, rows): each one is a compositor layer and with hundreds scrolling stutters. A
  card's blur only goes on hover.
- Don't: hardcoded colors in components. If one is needed, it's a new variable in `themes.css`.
- Don't: long animations outside the hero and the poster transition.
- Don't: UI libraries. Icons from `@lucide/svelte` imported one by one.

## 8. Responsive behavior

Desktop minimum window: 900 x 560. Grids use `auto-fill`; rows, widths with `clamp`.
Up to 700 px wide (phone): the sidebar becomes a bottom bar with icons and names, the
detail page stacks poster and data, the player hides the volume bar. On touch screens there are
no permanent scrollbars and no autofocus on the login (the keyboard would cover the screen). Side margins with
`--gutter` (16 to 44 px).

## 9. Guide for agents

Before changing the interface: read this file, `styles/themes.css` and `docs/API.md`. Test in the
six themes (Settings → Appearance) and with "Reduce motion". `npm run check` with no errors or warnings and
`npm run build`. Without a server: `npm run dev:mock` (saved Cinemeta data and a test video;
users `admin`/`admin` and `test`/`test`). Comments in Spanish; interface strings with
`t()` (`lib/i18n.svelte.ts`), tested in both languages.
