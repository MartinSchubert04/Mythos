// Temas y paletas (portados de Ethereon). Los colores base estan en src/styles/themes.css y las
// paletas en scripts/palettes.py (que genera palettes.css y palettes.gen.ts).
//
// Son de cada usuario: el tema y la paleta elegida para cada tema viajan en sus preferencias del
// server (`Settings.ui.theme` y `ui.palettes`, docs/API.md) y se aplican al entrar o al cambiar de
// perfil (lib/prefs.svelte.ts, `applyRemoteUi`). Lo que queda en localStorage es solo una copia
// del ultimo usuario que entro en este dispositivo: la usa el script de index.html para pintar el
// tema antes de montar (sin parpadeo), se ve en el login y en "¿Quién está viendo?", y es lo que
// hereda un usuario que todavia no tiene tema guardado (la migracion, en prefs.svelte.ts).
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

const isPalette = (t: ThemeId, p: unknown): p is string => typeof p === "string" && palettesOf(t).some((x) => x.id === p);

/** Sin elegir: Neovim (su paleta oscura, la primera), el que va con el logo. */
export const DEFAULT_THEME: ThemeId = "neovim";

/** El tema en uso (reactivo: Settings lo sigue si cambia de usuario con la pantalla abierta). */
export const look = $state<{ theme: ThemeId; palettes: Partial<Record<ThemeId, string>> }>({
  theme: ((t) => (isTheme(t) ? t : DEFAULT_THEME))(read(KEY)),
  palettes: Object.fromEntries(THEMES.map((t) => [t.id, read(PKEY(t.id)) ?? ""])),
});

export function currentTheme(): ThemeId {
  return look.theme;
}

/** La paleta elegida para un tema (cada tema recuerda la suya). */
export function currentPalette(t: ThemeId): string {
  const p = look.palettes[t];
  return isPalette(t, p) ? p : (palettesOf(t)[0]?.id ?? "");
}

/** Todas las paletas elegidas (para guardarlas en el server). */
export function allPalettes(): Record<string, string> {
  return Object.fromEntries(THEMES.map((t) => [t.id, currentPalette(t.id)]));
}

export function applyTheme(t: ThemeId, palette = currentPalette(t)) {
  const el = document.documentElement;
  look.theme = t;
  look.palettes[t] = palette;
  el.dataset.theme = t;
  el.dataset.palette = palette;
  write(KEY, t);
  write(PKEY(t), palette);
}

/** El tema de un usuario (Settings.ui). Las paletas que no trae vuelven a la primera de su tema:
 *  no quedan las que eligio otro usuario en este dispositivo. Devuelve si habia tema guardado. */
export function applyRemoteTheme(theme: unknown, palettes: unknown): boolean {
  if (!isTheme(theme)) return false;
  const got = palettes && typeof palettes === "object" ? (palettes as Record<string, unknown>) : {};
  for (const t of THEMES) {
    const p = got[t.id];
    look.palettes[t.id] = isPalette(t.id, p) ? p : (palettesOf(t.id)[0]?.id ?? "");
    write(PKEY(t.id), look.palettes[t.id]!);
  }
  applyTheme(theme);
  return true;
}
