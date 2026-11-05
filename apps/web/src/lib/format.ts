// Formatos para mostrar: tiempos, tamaños, velocidades, imagenes. Los decimales y los tiempos
// relativos siguen el idioma de la interfaz (lib/i18n.svelte.ts).
import { ago, num } from "./i18n.svelte";

const ONE = { minimumFractionDigits: 1, maximumFractionDigits: 1 };

/** 42:10 o 1:05:09. */
export function clock(s: number): string {
  if (!isFinite(s) || s < 0) s = 0;
  s = Math.floor(s);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const mm = h ? String(m).padStart(2, "0") : String(m);
  return (h ? `${h}:` : "") + `${mm}:${String(sec).padStart(2, "0")}`;
}

export function bytes(n?: number | null): string {
  if (n === undefined || n === null || !isFinite(n)) return "";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1000 && i < u.length - 1) {
    n /= 1000;
    i++;
  }
  return `${n >= 100 || i === 0 ? n.toFixed(0) : num(n, ONE)} ${u[i]}`;
}

export function speed(bps?: number | null): string {
  if (!bps) return "0 MB/s";
  return bps >= 1e6 ? `${num(bps / 1e6, ONE)} MB/s` : `${Math.round(bps / 1e3)} KB/s`;
}

/** Tamaño de las imagenes de metahub: chico en grillas para no gastar. */
export function sized(url: string | null | undefined, size: "small" | "medium" | "large"): string | undefined {
  return url?.replace(/\/(poster|background|logo)\/(small|medium|large)\//, `/$1/${size}/`);
}

/** "S01E02". */
export function epCode(season: number, episode: number): string {
  return `S${String(season).padStart(2, "0")}E${String(episode).padStart(2, "0")}`;
}

/** Color de los seeders: muchos (ok), pocos (warn), casi nada (danger). */
export function seedTone(n?: number | null): "ok" | "warn" | "bad" {
  if (n == null) return "warn";
  return n >= 50 ? "ok" : n >= 10 ? "warn" : "bad";
}

/** "hace 5 min" / "5 min ago". */
export function relTime(iso: string): string {
  return ago(iso);
}

/** Sin repetidos (mismo tipo e id): los catalogos a veces traen el mismo titulo dos veces. */
export function uniq<T extends { id: string; type: string }>(l: T[]): T[] {
  const seen = new Set<string>();
  return l.filter((m) => {
    const k = `${m.type}/${m.id}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}
