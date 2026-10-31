// Formatos y cuentas del panel de trafico (Admin → Traffic / Logs) y de la lista de dispositivos.
import type { Metrics, MetricsRange } from "./types";

/** 1234 → "1,234"; 1.2 M para numeros grandes. */
export function count(n?: number | null): string {
  if (n == null || !isFinite(n)) return "–";
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e7 ? 0 : 1)} M`;
  return Math.round(n).toLocaleString("en-US");
}

/** Version corta para ejes: 1.2k, 3 M. */
export function compact(n: number): string {
  if (n >= 1e6) return `${+(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${+(n / 1e3).toFixed(1)}k`;
  return String(Math.round(n));
}

/** 840 ms, 2.5 s. */
export function dur(ms?: number | null): string {
  if (ms == null || !isFinite(ms)) return "–";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${+(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)} s`;
}

export function pct(part: number, total: number): string {
  if (!total) return "0%";
  const p = (part / total) * 100;
  return p === 0 ? "0%" : p < 0.1 ? "<0.1%" : `${p < 10 ? p.toFixed(1) : Math.round(p)}%`;
}

/** Hace cuanto, desde ms unix: "just now", "5 min ago", "3 h ago", "2 d ago". */
export function agoMs(ms: number, now = Date.now()): string {
  const d = (now - ms) / 1000;
  if (!isFinite(d)) return "";
  if (d < 45) return "just now";
  if (d < 3600) return `${Math.max(1, Math.round(d / 60))} min ago`;
  if (d < 86400) return `${Math.floor(d / 3600)} h ago`;
  if (d < 86400 * 60) return `${Math.floor(d / 86400)} d ago`;
  return new Date(ms).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
}

/** Hora de un log: "14:03:22" si es de hoy, "Oct 29 14:03" si no. */
export function stamp(ms: number): string {
  const d = new Date(ms);
  const today = new Date().toDateString() === d.toDateString();
  const time = d.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", second: today ? "2-digit" : undefined });
  return today ? time : `${d.toLocaleDateString("en-US", { month: "short", day: "numeric" })} ${time}`;
}

export const fullDate = (ms: number) =>
  new Date(ms).toLocaleString("en-US", { dateStyle: "medium", timeStyle: "medium" });

// ---- serie del grafico

/** Cuanto mide cada barra segun el rango: con 30 dias, una por hora serian 720 barras. */
const BUCKET_H: Record<MetricsRange, number> = { "24h": 1, "7d": 4, "30d": 24, "90d": 24 };

export type Bucket = { start: number; end: number; requests: number; errors: number; bytes_out: number; p95_ms: number | null };

/** Agrupa la serie por hora en baldes parejos (incluye los vacios: un hueco tambien es un dato).
 *  Los baldes de un dia arrancan a la medianoche local. */
export function buckets(m: Metrics, range: MetricsRange): { list: Bucket[]; hours: number } {
  const hours = BUCKET_H[range] ?? 1;
  const size = hours * 3_600_000;
  const align = (t: number) => {
    if (hours >= 24) {
      const d = new Date(t);
      d.setHours(0, 0, 0, 0);
      return d.getTime();
    }
    // Baldes de varias horas alineados a la hora local (00, 04, 08...).
    const d = new Date(t);
    d.setMinutes(0, 0, 0);
    d.setHours(d.getHours() - (d.getHours() % hours));
    return d.getTime();
  };
  const first = align(m.from_ms + 1);
  const list: Bucket[] = [];
  const idx = new Map<number, Bucket>();
  for (let t = first; t <= m.to_ms; ) {
    const b: Bucket = { start: t, end: 0, requests: 0, errors: 0, bytes_out: 0, p95_ms: null };
    // Con cambio de horario un dia puede durar 23 o 25 h: se calcula con Date.
    let next: number;
    if (hours >= 24) {
      const d = new Date(t);
      d.setDate(d.getDate() + 1);
      next = d.getTime();
    } else next = t + size;
    b.end = next;
    list.push(b);
    idx.set(t, b);
    t = next;
  }
  for (const p of m.series) {
    const b = idx.get(align(p.hour_ms)) ?? list.find((x) => p.hour_ms >= x.start && p.hour_ms < x.end);
    if (!b) continue;
    b.requests += p.requests;
    b.errors += p.errors;
    b.bytes_out += p.bytes_out;
    // p95 del balde: el peor de sus horas (no se puede promediar un percentil).
    if (p.p95_ms != null) b.p95_ms = Math.max(b.p95_ms ?? 0, p.p95_ms);
  }
  return { list, hours };
}

/** Tope "lindo" para el eje y lineas intermedias: 1, 2, 2.5, 5 × 10^n. */
export function niceMax(v: number): number {
  if (v <= 0) return 1;
  const p = 10 ** Math.floor(Math.log10(v));
  for (const k of [1, 2, 2.5, 5, 10]) if (v <= k * p) return k * p;
  return 10 * p;
}

// ---- dispositivos

/** "Chrome on Windows", "Mythos app on Android", "curl"... a partir del User-Agent. */
export function prettyUa(ua?: string | null): { name: string; kind: "desktop" | "phone" | "tv" | "tool" } {
  if (!ua) return { name: "Unknown device", kind: "desktop" };
  const os = /Android/.test(ua)
    ? "Android"
    : /iPhone|iPad|iPod/.test(ua)
      ? "iOS"
      : /Windows/.test(ua)
        ? "Windows"
        : /Mac OS X|Macintosh/.test(ua)
          ? "macOS"
          : /CrOS/.test(ua)
            ? "ChromeOS"
            : /Linux/.test(ua)
              ? "Linux"
              : "";
  const phone = /Mobile|iPhone|Android(?!.*(TV|AFT))/.test(ua);
  const tv = /SmartTV|AFT|Android TV|BRAVIA|Tizen|Web0S/i.test(ua);
  // La app (Tauri) usa el WebView del sistema: WebView2 en Windows, wv en Android.
  const app = /Tauri|; wv\)/.test(ua);
  const tool = ua.match(/^(curl|Wget|python-requests|PostmanRuntime|okhttp|Go-http-client|node-fetch|undici)/i);
  if (tool) return { name: tool[1], kind: "tool" };
  let browser = "";
  if (app) browser = "Mythos app";
  else if (/Edg\//.test(ua)) browser = "Edge";
  else if (/OPR\/|Opera/.test(ua)) browser = "Opera";
  else if (/Firefox\//.test(ua)) browser = "Firefox";
  else if (/SamsungBrowser/.test(ua)) browser = "Samsung Internet";
  else if (/Chrome\//.test(ua)) browser = "Chrome";
  else if (/Safari\//.test(ua)) browser = "Safari";
  const name = browser && os ? `${browser} on ${os}` : browser || os || ua.slice(0, 40);
  return { name, kind: tv ? "tv" : phone ? "phone" : "desktop" };
}
