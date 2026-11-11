// Calidad preferida: es de cada dispositivo (nunca viaja con la cuenta). Va en cada pedido de
// streams (`?quality=`) y el server ordena con esa.
//
// Sin elegir nada en este aparato: en la TV, 4K; en el resto, la de la cuenta (`preferred_quality`
// de Settings, lo que se elegía antes de que fuera por dispositivo; sin `?quality=` el server usa
// esa) o 1080p.
import { read, write } from "./store";
import { tv } from "./tv.svelte";
import type { PreferredQuality } from "./types";

const KEY = "mythos.quality";
const valid = (v: string | null): v is PreferredQuality => v === "2160p" || v === "1080p" || v === "720p";
const saved = read(KEY);

export const quality = $state<{ chosen: PreferredQuality | null }>({ chosen: valid(saved) ? saved : null });

/** La que va en `?quality=` (undefined: que decida la cuenta). */
export const streamQuality = (): PreferredQuality | undefined => quality.chosen ?? (tv.on ? "2160p" : undefined);

/** La que se muestra en Settings (`account`: la de la cuenta). */
export const shownQuality = (account: PreferredQuality | null | undefined): PreferredQuality => streamQuality() ?? account ?? "1080p";

export function setQuality(q: PreferredQuality) {
  quality.chosen = q;
  write(KEY, q);
}
