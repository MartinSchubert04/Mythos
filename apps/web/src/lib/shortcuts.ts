// Atajos de teclado: la lista de Settings → Keyboard shortcuts y la del "?" en el reproductor.
// Cada fila es [teclas, que hace]; varias teclas para lo mismo van separadas por "  ·  ".
// Los textos siguen el idioma de la interfaz: cada fila tiene getters (se traducen al leerlas,
// no al cargar el modulo), asi las listas son fijas y `[k, what]` se sigue desarmando igual.
import { t, type Key } from "./i18n.svelte";

export type Shortcut = [keys: string, what: string];

/** Fila con los dos textos calculados al leerlos. */
function row(keys: () => string, what: () => string): Shortcut {
  const r: string[] = [];
  Object.defineProperty(r, 0, { get: keys, enumerable: true });
  Object.defineProperty(r, 1, { get: what, enumerable: true });
  return r as unknown as Shortcut;
}

/** Teclas fijas (con "Space" traducido) y que hace (una clave). */
const key = (keys: string, what: Key) => row(() => keys.replace("Space", t("player.key.space")), () => t(what));
/** Gesto (las dos cosas son texto). */
const gesture = (keys: Key, what: Key) => row(() => t(keys), () => t(what));

export const GENERAL_KEYS: Shortcut[] = [
  key("/  ·  Ctrl K", "player.sc.search"),
  key("← → ↑ ↓", "player.sc.moveGrid"),
  key("Esc", "player.sc.esc"),
];

export const PLAYER_KEYS: Shortcut[] = [
  key("Space  ·  K", "player.sc.playPause"),
  row(() => t("player.key.holdSpace"), () => t("player.sc.hold2x")),
  key("F", "player.sc.fullscreen"),
  key("M", "player.sc.mute"),
  key("← →", "player.sc.seek5"),
  key("J  ·  L", "player.sc.seek10"),
  key("↑ ↓", "player.sc.volume"),
  key("[  ·  ]", "player.sc.speed"),
  key("V", "player.sc.scale"),
  key("C", "player.sc.subs"),
  key("G  ·  H", "player.sc.subDelay"),
  key("-  ·  =", "player.sc.subSize"),
  key("N", "player.sc.next"),
  key("E", "player.sc.episodes"),
  key("D", "player.sc.stats"),
  key("?", "player.sc.help"),
  key("Enter", "player.sc.resume"),
];

/** Gestos del reproductor en pantallas tactiles. */
export const PLAYER_TOUCH: Shortcut[] = [
  gesture("player.touch.tap", "player.touch.tapWhat"),
  gesture("player.touch.doubleTap", "player.touch.doubleTapWhat"),
  gesture("player.touch.hold", "player.sc.hold2x"),
  gesture("player.touch.pinch", "player.touch.pinchWhat"),
  gesture("player.touch.swipe", "player.sc.volume"),
];

/** Para Settings: todo junto; las flechas del reproductor aclaran donde (en la grilla son otra cosa). */
export const ALL_KEYS: Shortcut[] = [
  ...GENERAL_KEYS,
  ...PLAYER_KEYS.map((r): Shortcut => {
    const k = r[0];
    return k === "← →" || k === "↑ ↓" || k === "?" ? row(() => r[0], () => t("player.sc.inPlayer", { what: r[1] })) : r;
  }),
];

/** "Space  ·  K" → ["Space", "K"]. */
export const keyParts = (k: string) => k.split("  ·  ");
