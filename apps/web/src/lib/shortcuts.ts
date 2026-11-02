// Atajos de teclado: la lista de Settings → Keyboard shortcuts y la del "?" en el reproductor.
// Cada fila es [teclas, que hace]; varias teclas para lo mismo van separadas por "  ·  ".

export type Shortcut = [keys: string, what: string];

export const GENERAL_KEYS: Shortcut[] = [
  ["/  ·  Ctrl K", "Search"],
  ["← → ↑ ↓", "Move between posters in a row and between rows"],
  ["Esc", "Close a panel or menu · in the player: leave full screen, then go back"],
];

export const PLAYER_KEYS: Shortcut[] = [
  ["Space  ·  K", "Play or pause"],
  ["Hold Space", "Play at 2× while held"],
  ["F", "Full screen"],
  ["M", "Mute"],
  ["← →", "Back or forward 5 seconds"],
  ["J  ·  L", "Back or forward 10 seconds"],
  ["↑ ↓", "Volume"],
  ["[  ·  ]", "Slower or faster"],
  ["V", "Video scale: Fit, Fill (stretched), Crop (zoomed, no bars)"],
  ["C", "Subtitles on or off"],
  ["G  ·  H", "This subtitle 0.5 s earlier or later (Shift: 0.1 s; remembered per subtitle)"],
  ["-  ·  =", "Subtitles smaller or larger"],
  ["N", "Next episode"],
  ["E", "Episodes and seasons (series)"],
  ["D", "Playback stats"],
  ["?", "Show these shortcuts"],
  ["Enter", "Resume (when the player asks)"],
];

/** Gestos del reproductor en pantallas tactiles. */
export const PLAYER_TOUCH: Shortcut[] = [
  ["Tap", "Show or hide the controls"],
  ["Double tap a side", "Back or forward 10 seconds (keep tapping for more)"],
  ["Touch and hold", "Play at 2× while held"],
  ["Pinch", "Out: crop to fill the screen · in: fit"],
  ["Swipe up or down on the right", "Volume"],
];

/** Para Settings: todo junto; las flechas del reproductor aclaran donde (en la grilla son otra cosa). */
export const ALL_KEYS: Shortcut[] = [
  ...GENERAL_KEYS,
  ...PLAYER_KEYS.map(([k, what]): Shortcut => [k, k === "← →" || k === "↑ ↓" || k === "?" ? `${what} (in the player)` : what]),
];

/** "Space  ·  K" → ["Space", "K"]. */
export const keyParts = (k: string) => k.split("  ·  ");
