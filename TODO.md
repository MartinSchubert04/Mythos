# TODO

What's left to do. Each task is deleted from here as soon as it's completed (in the same commit that
resolves it); when none are left, the file is deleted.

## What couldn't be tested in the second TV pass (2026-11-05)

The rest of the Samsung TV was tested with the real remote and working (login, profiles,
search, Calificadas (Top rated), Biblioteca (Library), Ajustes (Settings), player, trailer, media keys, server
down, local page in Spanish).

- [ ] **Admin with the remote**: the TV account (`tv`) isn't an admin, so it doesn't show up. Test
      with an admin account on the TV (or on the PC with `?tv=1`): navigate the tabs and the
      rename / change password dialogs (the field opens the keyboard; "Done" now confirms,
      `components/Confirm.svelte`).
- [ ] **"Continue watching": center on the X removes it**. Up focuses the X and down goes back (tested);
      it wasn't pressed so as not to remove a series from the user.
- [ ] **Android: preview when seeking by dragging with a finger** (on the PC with a mouse and on the
      TV with the remote it already works).

## TV: reload on unpause (2026-11-13)

The audio offset in remux (0.22.1) was tested on the TV with the inspector: 12 Monkeys 2160p
GalaxyRG (DDP5.1 → AAC), resuming at 40:00 and seeking to 1:05:00, both tracks of the first
segment start with the same difference as in the original file (21 ms offset, one
AAC frame); the bar shows the correct second.

- [ ] **Reload on unpause**: couldn't be reproduced. With 3 and 10 minute pauses it resumed instantly,
      with no "Cargando" ("Loading") or new playlist: the player requests the playlist every 5.5 s and the server doesn't
      close ffmpeg. If it happens again, check how long the pause lasted and whether the TV turned off the
      screen or left the app (that's when those requests stop); the player now waits 12 s
      (previously 4) before reloading.

## "For you" recommendations (2026-11-05)

Confirmed on the TV on 2026-11-06 with the server on 0.19.0: the featured item comes from the
recommendations (with description and logo), "Películas para vos" and "Series para vos" ("Movies for you" / "Series for you") above
"Populares" ("Popular"), "Similares" ("More like this") in the title page, and the home screen takes no longer than with 0.18.2 (the
"for you" requests answer in ~25 ms; the TV's heavy work on open is the same in both).

- [ ] **Check it on the PC**: the home screen and "Similares" ("More like this") with the desktop interface.

## Per-user theme (2026-11-06)

Tested on the PC (test server + `npm run dev`, headless Chrome): migration, each user
with their own theme, switching accounts without reloading, no flicker on open, and TV profiles with `?tv=1`.

- [ ] **On the TV**: with two profiles with different themes, switch in "¿Quién está viendo?" ("Who's watching?") and check
      that the new page already starts with the chosen one's theme (the first time after updating
      the previous one may show for an instant: the profile doesn't have the copy of its preferences yet).
- [ ] **Glass on desktop**: turn it on, change the opacity, log out and back in with another account (each
      with its own); and that a user without `ui.glass` inherits the PC's.

## Desktop: controls that didn't show up when moving the mouse (2026-11-09)

Fixed in 0.19.3 (`views/Player.svelte`, `.desk > video`): with a letterboxed movie
and the controls hidden for a few seconds, WebView2 sent the video to the GPU as a separate layer and
stopped painting the page. Tested in a separate desktop instance with the real server: 4 out of 4
(without the fix, 1 out of 6).

- [ ] **Does it also happen in Chrome/Edge on Windows?** Test the web app with a letterboxed
      movie, >8 s with the controls hidden, and move the mouse. If it happens, remove the `desk`
      condition (so the rule applies to everyone).
