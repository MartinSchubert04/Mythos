// Idioma de audio preferido: en que idioma viene cada stream (por las banderas de Torrentio y
// las palabras del nombre del release) y cual pista de audio elegir. /api/streams no recibe un
// idioma, asi que el orden se ajusta aca, sin tocar el del server dentro de cada grupo.
// Codigos: ISO 639-2 ("eng", "spa") con la variante cuando se sabe: "spa-419" (Latinoamerica),
// "spa-ES" (España), "por-BR" (Brasil).
import { normLang } from "./prefs.svelte";
import type { Stream, Track } from "./types";

/** Las opciones de Settings → Playback. */
export const AUDIO_LANGS: { value: string; label: string }[] = [
  { value: "", label: "Original" },
  { value: "eng", label: "English" },
  { value: "spa-419", label: "Spanish (Latin America)" },
  { value: "spa-ES", label: "Spanish (Spain)" },
  { value: "por-BR", label: "Portuguese (Brazil)" },
  { value: "por", label: "Portuguese (Portugal)" },
  { value: "fre", label: "French" },
  { value: "ger", label: "German" },
  { value: "ita", label: "Italian" },
  { value: "rus", label: "Russian" },
  { value: "pol", label: "Polish" },
  { value: "tur", label: "Turkish" },
  { value: "jpn", label: "Japanese" },
  { value: "kor", label: "Korean" },
  { value: "chi", label: "Chinese" },
  { value: "hin", label: "Hindi" },
];

// Banderas que pone Torrentio (🇲🇽 = "Latino", 🇪🇸 = Castellano).
const FLAGS: Record<string, string> = {
  "🇬🇧": "eng", "🇺🇸": "eng", "🇪🇸": "spa-ES", "🇲🇽": "spa-419", "🇦🇷": "spa-419", "🇧🇷": "por-BR", "🇵🇹": "por",
  "🇫🇷": "fre", "🇩🇪": "ger", "🇮🇹": "ita", "🇷🇺": "rus", "🇵🇱": "pol", "🇹🇷": "tur", "🇯🇵": "jpn", "🇰🇷": "kor",
  "🇨🇳": "chi", "🇹🇼": "chi", "🇮🇳": "hin", "🇳🇱": "dut", "🇺🇦": "ukr", "🇸🇦": "ara",
};

// Palabras del nombre del release o del titulo de la pista. El orden importa: las variantes
// primero ("Latino" antes que "Spanish"). VOSTFR/VOSE son subtitulos, no audio: no cuentan.
const WORDS: [RegExp, string][] = [
  [/\b(latino|latam|lat|es[-_. ]?(419|mx|la)|latinoam[eé]rica|mexican[oa]?)\b/i, "spa-419"],
  [/\b(castellano|castilian|esp|es[-_. ]?es|espa[ñn]a|spain)\b/i, "spa-ES"],
  [/\b(spanish|espa[ñn]ol|spa)\b/i, "spa"],
  [/\b(dublado|nacional|pt[-_. ]?br|brazilian|brasil(eiro)?)\b/i, "por-BR"],
  [/\b(portuguese|portugu[eê]s|por)\b/i, "por"],
  [/\b(english|eng)\b/i, "eng"],
  [/\b(truefrench|french|fran[cç]ais|vff|vfq|vf2?)\b/i, "fre"],
  [/\b(german|deutsch|ger)\b/i, "ger"],
  [/\b(italian|italiano|ita)\b/i, "ita"],
  [/\b(russian|rus)\b/i, "rus"],
  [/\b(japanese|jpn)\b/i, "jpn"],
  [/\b(korean|kor)\b/i, "kor"],
  [/\b(hindi)\b/i, "hin"],
  [/\b(multi|dual)\b/i, "multi"],
];

/** Los idiomas que dice traer un stream (puede no decir ninguno). */
export function streamLangs(s: Stream): Set<string> {
  const out = new Set<string>();
  for (const l of s.languages ?? []) {
    const f = FLAGS[l];
    if (f) out.add(f);
    else if (/^multi/i.test(l)) out.add("multi");
    else if (/^[a-z]{2,3}([-_][a-z0-9]{2,3})?$/i.test(l)) out.add(variantOf(l));
  }
  // El nombre del release con los separadores como espacios ("Movie.2010.LATINO.1080p").
  const name = `${s.title} ${s.file_name ?? ""}`.replace(/[._]/g, " ");
  for (const [re, code] of WORDS) if (re.test(name)) out.add(code);
  return out;
}

/** Un codigo suelto ("es", "spa", "es-419", "pt-BR") con su variante. */
function variantOf(code: string): string {
  const c = code.toLowerCase();
  if (/^(es|spa)[-_](419|mx|la|ar)$/.test(c) || c === "spl" || c === "spn" || c === "ea") return "spa-419";
  if (/^(es|spa)[-_]es$/.test(c)) return "spa-ES";
  if (/^(pt|por)[-_]br$/.test(c) || c === "pob" || c === "pb") return "por-BR";
  return normLang(c);
}

/** Que tan bien cumple `have` con `want`: 3 la misma variante, 2 el mismo idioma sin variante,
 *  1 el idioma con otra variante (o "multi"), 0 nada. */
function fit(have: Set<string>, want: string): number {
  const base = want.split("-")[0];
  let best = 0;
  for (const h of have) {
    const hb = h.split("-")[0];
    if (h === want) return 3;
    if (hb === base) best = Math.max(best, h === hb || want === base ? 2 : 1);
    else if (h === "multi") best = Math.max(best, 1);
  }
  return best;
}

/** Los streams en ese idioma primero (los mejores del idioma arriba, el orden del server
 *  adentro de cada grupo). Solo suben los que tienen con que arrancar: con pocos seeders o CAM
 *  se quedan donde estaban, asi el idioma nunca elige uno que se va a trabar. */
export function sortByLang(list: Stream[], want: string): Stream[] {
  if (!want || list.length < 2) return list;
  const healthy = (s: Stream) => s.quality !== "CAM" && (!!s.cached || (s.seeders ?? 0) >= 5);
  const tier = (s: Stream) => {
    if (!healthy(s)) return 0;
    const have = streamLangs(s);
    // Sin ninguna marca casi siempre es el audio original en ingles.
    if (!have.size || (have.size === 1 && have.has("multi") && want === "eng")) return want === "eng" ? 2 : 0;
    return fit(have, want);
  };
  return list
    .map((s, i) => ({ s, i, t: tier(s) }))
    .sort((a, b) => b.t - a.t || a.i - b.i)
    .map((x) => x.s);
}

/** Los idiomas de una pista de audio, por su codigo y su titulo ("Español Latino", "Castellano"). */
function trackLangs(t: Track): Set<string> {
  const out = new Set<string>();
  const title = (t.title ?? "").replace(/[._]/g, " ");
  for (const [re, code] of WORDS) if (code !== "multi" && re.test(title)) out.add(code);
  if (t.lang && t.lang !== "und") {
    const v = variantOf(t.lang);
    // "spa" sin variante en el codigo: la del titulo, si la dice.
    if (![...out].some((x) => x.startsWith(v + "-"))) out.add(v);
  }
  return out;
}

/** La pista en el idioma preferido (la de la misma variante antes), o null si no hay. */
export function preferredAudio(tracks: Track[], want: string): Track | null {
  if (!want || tracks.length < 2) return null;
  let best: Track | null = null;
  let bestFit = 0;
  for (const t of tracks) {
    const f = fit(trackLangs(t), want);
    // A igual idioma, la que viene por defecto en el archivo.
    if (f > bestFit || (f === bestFit && f > 0 && t.default && !best?.default)) {
      best = t;
      bestFit = f;
    }
  }
  return best;
}
