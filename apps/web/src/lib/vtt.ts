// Subtitulos WebVTT dibujados por la app (no con <track>): asi siguen al tiempo real cuando el
// modo transcode arranca la playlist mas adelante, y el tamaño sale de las preferencias.

export type Cue = { start: number; end: number; text: string };

function ts(s: string): number {
  const p = s.trim().replace(",", ".").split(":");
  let t = 0;
  for (const x of p) t = t * 60 + parseFloat(x);
  return t;
}

/** Texto plano con saltos de linea; se quedan solo <i> y <b> (se escapa lo demas). */
function clean(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/&lt;(\/?)(i|b|u)&gt;/gi, "<$1$2>")
    .replace(/&lt;[^&]*?&gt;/g, "")
    .replace(/\{\\[^}]*\}/g, "")
    .replace(/&amp;(nbsp|amp|lt|gt);/g, "&$1;")
    .replace(/\n/g, "<br>");
}

export function parseVtt(src: string): Cue[] {
  const out: Cue[] = [];
  const blocks = src.replace(/\r/g, "").split(/\n\n+/);
  for (const b of blocks) {
    const lines = b.split("\n");
    const i = lines.findIndex((l) => l.includes("-->"));
    if (i < 0) continue;
    const [a, z] = lines[i].split("-->");
    const start = ts(a);
    const end = ts(z.trim().split(/\s+/)[0]);
    const text = lines.slice(i + 1).join("\n").trim();
    if (text && isFinite(start) && isFinite(end)) out.push({ start, end, text: clean(text) });
  }
  return out.sort((x, y) => x.start - y.start);
}

/** Las lineas que tocan en `t` (busqueda binaria; puede haber varias superpuestas). */
export function cuesAt(cues: Cue[], t: number): string {
  let lo = 0;
  let hi = cues.length - 1;
  let first = cues.length;
  while (lo <= hi) {
    const m = (lo + hi) >> 1;
    if (cues[m].start <= t) lo = m + 1;
    else {
      first = m;
      hi = m - 1;
    }
  }
  const hits: string[] = [];
  // Hacia atras desde el ultimo que empezo; los subtitulos largos raramente pasan de 10 lineas.
  for (let j = first - 1; j >= 0 && j >= first - 10; j--) if (cues[j].end > t) hits.unshift(cues[j].text);
  return hits.join("<br>");
}
