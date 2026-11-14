// Calidad preferida: es de cada dispositivo (nunca viaja con la cuenta). Va en cada pedido de
// streams (`?quality=`) y el server ordena con esa.
//
// Sin elegir nada en este aparato: en la TV, 4K; en el resto, 1080p. Siempre va `?quality=`: la
// `preferred_quality` de la cuenta (lo que se elegía antes de que fuera por dispositivo) ya no
// cuenta, si no la que se puso en la tele quedaba también en la PC.
import { read, write } from "./store";
import { tv } from "./tv.svelte";
import type { PreferredQuality } from "./types";

const KEY = "mythos.quality";
const valid = (v: string | null): v is PreferredQuality => v === "2160p" || v === "1080p" || v === "720p";
const saved = read(KEY);

export const quality = $state<{ chosen: PreferredQuality | null }>({ chosen: valid(saved) ? saved : null });

/** La de este dispositivo: la que va en `?quality=` y la que muestra Settings. */
export const streamQuality = (): PreferredQuality => quality.chosen ?? (tv.on ? "2160p" : "1080p");

export function setQuality(q: PreferredQuality) {
  quality.chosen = q;
  write(KEY, q);
}
