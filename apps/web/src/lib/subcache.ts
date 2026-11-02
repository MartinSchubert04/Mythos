// Subtitulos de OpenSubtitles ya bajados (en memoria, mientras la app esta abierta). La precarga
// del proximo episodio deja aca el que se va a elegir, asi al pasar de episodio aparece enseguida
// en vez de esperar la lista y la descarga. Los internos no se guardan: el server los va
// extrayendo a medida que baja el archivo (X-Mythos-Partial) y cambian entre un pedido y otro.
import { api, mediaUrl } from "./api";
import { normLang, prefs } from "./prefs.svelte";
import type { MediaType } from "./types";

const MAX = 8;
const texts = new Map<string, Promise<string>>();

/** El VTT de un subtitulo externo (`/api/media/subs/external?u=…`), de la cache si ya se bajo. */
export function externalSub(url: string): Promise<string> {
  const hit = texts.get(url);
  if (hit) return hit;
  const p = fetch(mediaUrl(url)).then((r) => {
    if (!r.ok) throw new Error(String(r.status));
    return r.text();
  });
  texts.set(url, p);
  p.catch(() => texts.delete(url));
  while (texts.size > MAX) texts.delete(texts.keys().next().value!);
  return p;
}

/** Deja listos la lista de subtitulos de un titulo y el primero del idioma de Settings (el que
 *  elegiria el reproductor si el video no trae uno interno). Sin idioma elegido, solo la lista. */
export async function prefetchSubs(type: MediaType, id: string): Promise<void> {
  const l = await api.subtitles(type, id);
  const want = prefs.subLang;
  if (!want) return;
  const s = l.find((x) => normLang(x.lang) === want);
  if (s) await externalSub(s.url);
}
