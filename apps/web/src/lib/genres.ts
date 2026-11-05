// Nombres de los generos para mostrar. Cinemeta e IMDb los mandan en inglés ("Sci-Fi", "Crime") y
// asi se guardan y se mandan al server (Top rated, "More like this"): aca solo se traduce lo que se
// ve. Uno que no esta en el diccionario (browse.genre.*) se muestra tal cual.
import { has, t } from "./i18n.svelte";

export function genreName(g: string): string {
  const k = `browse.genre.${g}`;
  return has(k) ? t(k) : g;
}
