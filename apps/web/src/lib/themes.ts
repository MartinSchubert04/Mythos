// Temas y paletas (portados de Ethereon). Los colores base estan en src/styles/themes.css y las
// paletas en scripts/palettes.py (que genera palettes.css y palettes.gen.ts).
import { PALETTES } from "./palettes.gen";
import { read, write } from "./store";

export const THEMES = [
  { id: "default", name: "Default" },
  { id: "warp", name: "Warp" },
  { id: "poolsuite", name: "Poolsuite FM" },
  { id: "haze", name: "Haze" },
  { id: "athanor", name: "Athanor" },
  { id: "neovim", name: "Neovim" },
] as const;

export type ThemeId = (typeof THEMES)[number]["id"];

export function palettesOf(t: ThemeId) {
  return PALETTES[t] ?? [];
}

// Mismas claves que el script de index.html (aplica el tema antes de pintar).
const KEY = "mythos.theme";
const PKEY = (t: ThemeId) => `mythos.palette.${t}`;

export function isTheme(t: unknown): t is ThemeId {
  return THEMES.some((x) => x.id === t);
}

export function currentTheme(): ThemeId {
  const t = read(KEY);
  return isTheme(t) ? t : "default";
}

/** La paleta elegida para un tema (cada tema recuerda la suya). */
export function currentPalette(t: ThemeId): string {
  const p = read(PKEY(t));
  const list = palettesOf(t);
  return p && list.some((x) => x.id === p) ? p : (list[0]?.id ?? "");
}

/** Todas las paletas elegidas (para guardarlas en el server). */
export function allPalettes(): Record<string, string> {
  return Object.fromEntries(THEMES.map((t) => [t.id, currentPalette(t.id)]));
}

export function applyTheme(t: ThemeId, palette = currentPalette(t)) {
  const el = document.documentElement;
  el.dataset.theme = t;
  el.dataset.palette = palette;
  write(KEY, t);
  write(PKEY(t), palette);
}

/** Lo que viene del server (Settings.ui) pisa lo local. */
export function applyRemoteTheme(theme: unknown, palettes: unknown) {
  if (palettes && typeof palettes === "object") {
    for (const [t, p] of Object.entries(palettes as Record<string, unknown>)) {
      if (isTheme(t) && typeof p === "string" && palettesOf(t).some((x) => x.id === p)) write(PKEY(t), p);
    }
  }
  applyTheme(isTheme(theme) ? theme : currentTheme());
}
