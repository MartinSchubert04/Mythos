// "New episodes" del inicio: episodios de las series de la Library que salieron en los ultimos
// 14 dias y todavia no se empezaron. Las metas (Cinemeta, via /api/meta) se piden de a pocas a la
// vez y de cada serie se guarda solo lo reciente en sessionStorage por una hora; el progreso es
// del server propio y se pide cada vez (solo de las series que tienen algo nuevo).
import { api } from "./api";
import type { Episode, Meta, MetaPreview } from "./types";

const KEY = "mythos.newEps";
const LAST = "mythos.newEps.last";
const TTL = 3600_000;
const DAYS = 14;
const CONC = 4;
const DAY = 86_400_000;

type Slim = Pick<Episode, "id" | "season" | "episode" | "name" | "thumbnail"> & { released: string };
type SeriesSlim = { name: string; poster?: string | null; background?: string | null; eps: Slim[] };
type Cache = { at: number; series: Record<string, SeriesSlim> };

export type NewEp = { series: Pick<MetaPreview, "id" | "name" | "poster" | "background">; ep: Slim; more: number };

function readJson<T>(k: string): T | null {
  try {
    return JSON.parse(sessionStorage.getItem(k) ?? "null") as T | null;
  } catch {
    return null;
  }
}
function writeJson(k: string, v: unknown) {
  try {
    sessionStorage.setItem(k, JSON.stringify(v));
  } catch {}
}

/** Lo ultimo calculado en esta sesion: para dibujar la fila enseguida al volver al inicio. */
export function lastNewEpisodes(): NewEp[] {
  const l = readJson<{ at: number; items: NewEp[] }>(LAST);
  return l && Date.now() - l.at < TTL ? l.items : [];
}

/** Corre `f` sobre la lista con a lo sumo `n` a la vez. */
async function pool<T>(list: T[], n: number, f: (x: T) => Promise<void>) {
  let i = 0;
  const worker = async () => {
    while (i < list.length) await f(list[i++]);
  };
  await Promise.all(Array.from({ length: Math.min(n, list.length) }, worker));
}

/** De la meta, los episodios recientes (y los que salen pronto: la cache dura una hora). */
function slim(m: Meta): SeriesSlim {
  const now = Date.now();
  const eps = (m.videos ?? [])
    .filter((v) => v.season > 0 && v.released && Math.abs(Date.parse(v.released) - now) < (DAYS + 1) * DAY)
    .map((v) => ({ id: v.id, season: v.season, episode: v.episode, name: v.name, thumbnail: v.thumbnail, released: v.released! }));
  return { name: m.name, poster: m.poster, background: m.background, eps };
}

export async function newEpisodes(): Promise<NewEp[]> {
  const lib = (await api.library()).filter((m) => m.type === "series");
  const old = readJson<Cache>(KEY);
  const cache: Cache = old && Date.now() - old.at < TTL ? old : { at: Date.now(), series: {} };
  const missing = lib.filter((m) => !cache.series[m.id]);
  await pool(missing, CONC, async (m) => {
    try {
      cache.series[m.id] = slim(await api.meta("series", m.id));
    } catch {}
  });
  if (missing.length) writeJson(KEY, cache);

  const now = Date.now();
  const since = now - DAYS * DAY;
  const fresh = lib.flatMap((m) => {
    const s = cache.series[m.id];
    const eps = (s?.eps ?? []).filter((e) => {
      const t = Date.parse(e.released);
      return t >= since && t <= now;
    });
    return s && eps.length ? [{ m, s, eps }] : [];
  });

  const out: NewEp[] = [];
  await pool(fresh, CONC, async ({ m, s, eps }) => {
    let seen = new Set<string>();
    try {
      seen = new Set((await api.seriesProgress(m.id)).map((p) => p.meta_id));
    } catch {}
    // Uno por serie: el primero que falta ver, y cuantos mas hay.
    const left = eps.filter((e) => !seen.has(e.id)).sort((a, b) => a.season - b.season || a.episode - b.episode);
    if (left.length)
      out.push({ series: { id: m.id, name: s.name, poster: s.poster ?? m.poster, background: s.background ?? m.background }, ep: left[0], more: left.length - 1 });
  });
  // Lo mas nuevo primero.
  out.sort((a, b) => Date.parse(b.ep.released) - Date.parse(a.ep.released));
  writeJson(LAST, { at: Date.now(), items: out });
  return out;
}
