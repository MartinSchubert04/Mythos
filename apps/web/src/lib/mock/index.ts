// Modo mock: un server de mentira dentro del navegador, solo para desarrollo (VITE_MOCK=1 o
// `npm run dev:mock`). Contesta el contrato con datos reales de Cinemeta guardados en
// fixtures.json y un video de prueba publico. El build de produccion no lo incluye.
import fixtures from "./fixtures.json";
import { setMockAvatar } from "../api";
import { mockAudit, mockLogs, mockMetrics, mockSessions, sessionsOf } from "./traffic";
import type { MetricsRange } from "../types";
import type {
  AdminStats,
  Avatar,
  Episode,
  MediaStatus,
  Meta,
  MetaPreview,
  PlayInfo,
  Progress,
  Settings,
  Stream,
  TorrentStat,
  User,
} from "../types";

// Big Buck Bunny entero (Blender Foundation, CC BY), de Wikimedia Commons.
const VIDEO_DIRECT = "https://upload.wikimedia.org/wikipedia/commons/transcoded/c/c0/Big_Buck_Bunny_4K.webm/Big_Buck_Bunny_4K.webm.720p.vp9.webm";
const VIDEO_HLS = "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8";
const DURATION = 634; // Big Buck Bunny (la version de Wikimedia)

type Fx = { catalogs: Record<string, MetaPreview[]>; metas: Record<string, Meta> };
const fx = fixtures as unknown as Fx;

const KEY = "mythos.mock";
type Db = {
  users: User[];
  passwords: Record<string, string>;
  progress: Progress[];
  library: MetaPreview[];
  settings: Record<string, Settings>;
  nextId: number;
  /** Galeria de avatares: las imagenes como data: URL. */
  avatars?: (Avatar & { src: string })[];
};

/** Tres avatares de muestra (circulos de color con una figura), como data: URL. */
function sampleAvatars(): (Avatar & { src: string })[] {
  const svg = (bg: string, fg: string, shape: string) =>
    "data:image/svg+xml," +
    encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="${bg}"/><g fill="${fg}">${shape}</g></svg>`);
  const t = Math.floor(Date.now() / 1000);
  return [
    { id: 1, name: "Moon", created_at: t, src: svg("#1e293b", "#fde68a", '<circle cx="34" cy="30" r="16"/><circle cx="42" cy="24" r="14" fill="#1e293b"/>') },
    { id: 2, name: "Leaf", created_at: t, src: svg("#14532d", "#86efac", '<path d="M16 46C16 24 32 14 50 14c0 22-12 34-32 34z"/>') },
    { id: 3, name: "Wave", created_at: t, src: svg("#1e3a8a", "#93c5fd", '<path d="M6 40c10-10 16-10 26 0s16 10 26 0v18H6z"/>') },
  ];
}
function load(): Db {
  try {
    const d = JSON.parse(localStorage.getItem(KEY) ?? "");
    if (d?.users) return d;
  } catch {}
  const now = new Date().toISOString();
  const got = fx.metas.tt0944947;
  const inception = fx.metas.tt1375666;
  return {
    users: [
      { id: 1, username: "admin", is_admin: true, disabled: false, created_at: now },
      { id: 2, username: "test", is_admin: false, disabled: false, created_at: now },
      { id: 3, username: "guest", is_admin: false, disabled: true, created_at: now },
    ],
    passwords: { admin: "admin", test: "test", guest: "guest" },
    progress: [
      {
        meta_id: "tt0944947:1:3",
        type: "series",
        series_id: "tt0944947",
        name: got.name,
        poster: got.poster,
        background: got.background,
        episode_label: "S01E03 · Lord Snow",
        position_s: 130,
        duration_s: 634,
        updated_at: now,
      },
      {
        meta_id: "tt1375666",
        type: "movie",
        name: inception.name,
        poster: inception.poster,
        background: inception.background,
        position_s: 210,
        duration_s: 634,
        updated_at: now,
      },
    ],
    library: [fx.catalogs["movie/top"][1], fx.catalogs["series/top"][0], fx.catalogs["movie/imdbRating"][0]],
    settings: {},
    nextId: 4,
  };
}
const db = load();
const save = () => localStorage.setItem(KEY, JSON.stringify(db));

const tokens = new Map<string, string>(); // token -> username
const avatarsOf = () => (db.avatars ??= sampleAvatars());

function userOf(req: Request): User | null {
  const url = new URL(req.url);
  const t = req.headers.get("Authorization")?.replace(/^Bearer /, "") ?? url.searchParams.get("t") ?? "";
  // Los tokens del mock son "mock-<usuario>": sobreviven a recargar la pagina.
  const name = tokens.get(t) ?? (t.startsWith("mock-") ? t.slice(5) : "");
  return db.users.find((u) => u.username === name && !u.disabled) ?? null;
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const err = (status: number, error: string) => json({ error }, status);
const none = (status = 204) => new Response(null, { status });
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

function allPreviews(): MetaPreview[] {
  const seen = new Set<string>();
  const out: MetaPreview[] = [];
  for (const l of Object.values(fx.catalogs))
    for (const m of l)
      if (!seen.has(m.id)) {
        seen.add(m.id);
        out.push(m);
      }
  return out;
}

function metaFor(type: string, id: string): Meta | null {
  if (fx.metas[id]) return fx.metas[id];
  const p = allPreviews().find((m) => m.id === id);
  if (!p) return null;
  const m: Meta = {
    ...p,
    runtime: type === "movie" ? `${95 + (id.charCodeAt(5) % 60)} min` : "55 min",
    cast: ["Alex Morgan", "Sam Rivera", "Jordan Lee", "Taylor Kim"],
    director: ["Chris Doe"],
    trailer_youtube_id: "YoHD9XEInc0",
  };
  if (type === "series") {
    const eps: Episode[] = [];
    for (let s = 1; s <= 3; s++)
      for (let e = 1; e <= 8; e++)
        eps.push({
          id: `${id}:${s}:${e}`,
          season: s,
          episode: e,
          name: `Episode ${e}`,
          overview: "An episode of the series. In mock mode every episode plays the same test video.",
          thumbnail: p.background,
          // Los dos ultimos de la temporada 3: uno de hace 3 dias y otro en 4 (para "New episodes").
          released:
            s === 3 && e >= 7
              ? new Date(Date.now() + (e === 7 ? -3 : 4) * 86_400_000).toISOString()
              : `20${10 + s}-0${Math.min(9, e)}-01T00:00:00.000Z`,
        });
    m.videos = eps;
  }
  return m;
}

const QUALITIES = ["2160p", "1080p", "1080p", "720p", "1080p", "480p", "720p", "2160p"] as const;
function streamsFor(id: string): Stream[] {
  const base = id.replace(/\W/g, "").padEnd(40, "0").slice(0, 40).toLowerCase();
  const names = [
    ["2160p.UHD.BluRay.x265.10bit.HDR.DTS-HD.MA.5.1-GRP", ["HDR", "HEVC", "10bit", "5.1"], 31.2e9, 212, "ThePirateBay", ["🇬🇧"]],
    ["1080p.BluRay.x264.DTS-HD.MA.5.1-FGT", ["x264", "5.1", "BluRay"], 12.4e9, 845, "1337x", ["🇬🇧"]],
    ["1080p.WEB-DL.DDP5.1.H.264-NTb", ["WEB-DL", "x264", "5.1"], 6.1e9, 1290, "EZTV", ["🇬🇧", "🇪🇸"]],
    ["720p.BluRay.x264-SPARKS", ["x264", "BluRay"], 4.4e9, 96, "YTS", ["🇬🇧"]],
    ["1080p.BluRay.REMUX.AVC.TrueHD.Atmos-FraMeSToR", ["REMUX", "Atmos"], 34.8e9, 31, "RARBG", ["multi"]],
    ["480p.DVDRip.XviD-AMIABLE", ["XviD"], 0.7e9, 6, "ThePirateBay", ["🇬🇧"]],
    ["720p.HDTV.x264-KILLERS", ["x264", "HDTV"], 1.1e9, 3, "EZTV", ["🇬🇧"]],
    ["2160p.WEB-DL.DV.HEVC.Atmos-FLUX", ["DV", "HDR", "HEVC", "Atmos"], 18.9e9, 47, "TorrentGalaxy", ["🇬🇧", "🇫🇷"]],
  ] as const;
  return names.map(([t, tags, size, seeders, source, languages], i) => ({
    key: `${base.slice(0, 39)}${i}:0`,
    info_hash: `${base.slice(0, 39)}${i}`,
    file_idx: 0,
    title: `Mock.Title.${t}`,
    file_name: `Mock.Title.${t}.mkv`,
    quality: QUALITIES[i],
    size_bytes: size,
    seeders,
    source,
    tags: [...tags],
    languages: [...languages],
    cached: i === 2,
    score: 100 - i * 9,
  }));
}

// ---- sesiones de reproduccion simuladas

type Sess = { started: number; total: number; peersMax: number; slow: boolean };
const sessions = new Map<string, Sess>();

function status(s: Sess): MediaStatus {
  const t = (Date.now() - s.started) / 1000;
  const state: MediaStatus["state"] = t < 1.2 ? "metadata" : t < 2 ? "probing" : t < 3.2 ? "buffering" : "ready";
  const bps = s.slow ? 90e3 + Math.random() * 60e3 : Math.min(9e6, 1.5e6 * t) * (0.85 + Math.random() * 0.3);
  const done = Math.min(s.total, (s.slow ? 1e5 : 4e6) * t * t);
  return {
    state,
    down_bps: state === "metadata" ? 0 : bps,
    up_bps: bps * 0.08,
    peers: Math.min(s.peersMax, Math.floor(t * 6)),
    downloaded_bytes: done,
    total_bytes: s.total,
    progress: done / s.total,
    buffered_ahead_s: state === "ready" ? 120 : state === "buffering" ? (t - 2) * 6 : 0,
  };
}

function vtt(lang: string): string {
  const lines: string[] = ["WEBVTT", ""];
  const say: Record<string, string[]> = {
    eng: ["Mock subtitles in English.", "Big Buck Bunny wakes up.", "<i>Something is wrong in the forest.</i>", "These cues follow the real time."],
    spa: ["Subtítulos de prueba en español.", "Big Buck Bunny se despierta.", "<i>Algo pasa en el bosque.</i>", "Estas líneas siguen el tiempo real."],
  };
  const l = say[lang] ?? say.eng;
  for (let i = 0; i < 120; i++) {
    const a = i * 5 + 1;
    const f = (s: number) => `00:${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}.000`;
    lines.push(`${f(a)} --> ${f(a + 3)}`, l[i % l.length], "");
  }
  return lines.join("\n");
}

const torrents: TorrentStat[] = [
  {
    info_hash: "a".repeat(40),
    name: "Mock.Title.1080p.WEB-DL.DDP5.1.H.264-NTb",
    size_bytes: 6.1e9,
    downloaded_bytes: 2.2e9,
    down_bps: 5.2e6,
    up_bps: 0.4e6,
    peers: 37,
    state: "live",
    last_used_at: new Date().toISOString(),
  },
  {
    info_hash: "b".repeat(40),
    name: "Game.of.Thrones.S01E03.1080p.BluRay.x264",
    size_bytes: 2.4e9,
    downloaded_bytes: 2.4e9,
    down_bps: 0,
    up_bps: 0.2e6,
    peers: 4,
    state: "paused",
    last_used_at: new Date(Date.now() - 36e5).toISOString(),
  },
];

async function route(req: Request): Promise<Response> {
  const url = new URL(req.url);
  const path = url.pathname;
  const m = req.method;
  const body = async <T>() => (await req.json()) as T;
  // Un poco de latencia para ver los esqueletos.
  await wait(path.includes("/status") ? 40 : 160 + Math.random() * 380);

  if (path === "/api/health") return json({ ok: true, version: "mock" });
  if (path === "/api/auth/login" && m === "POST") {
    const { username, password } = await body<{ username: string; password: string }>();
    const u = db.users.find((x) => x.username === username);
    if (!u || db.passwords[username] !== password || u.disabled) return err(401, "Wrong username or password");
    const token = `mock-${username}`;
    tokens.set(token, username);
    return json({ token, user: u });
  }
  const me = userOf(req);
  if (!me) return err(401, "Not signed in");

  let r: RegExpMatchArray | null;
  if (path === "/api/auth/logout") return none();
  if (path === "/api/me" && m === "PATCH") {
    const b = await body<{ avatar_id?: number | null }>();
    if (b.avatar_id !== undefined) {
      if (b.avatar_id !== null && !avatarsOf().some((a) => a.id === b.avatar_id)) return err(404, "Avatar not found");
      me.avatar_id = b.avatar_id;
      save();
    }
    return json(me);
  }
  if (path === "/api/me") return json(me);
  if (path === "/api/me/username" && m === "PUT") {
    const { username } = await body<{ username: string }>();
    const name = username.trim();
    if (!/^[A-Za-z0-9_.-]{1,32}$/.test(name)) return err(400, "Username must be 1-32 characters: letters, numbers, '_', '-' or '.'");
    if (db.users.some((u) => u.id !== me.id && u.username.toLowerCase() === name.toLowerCase())) return err(409, "Username already exists");
    db.passwords[name] = db.passwords[me.username];
    // La sesion sigue: el token viejo ahora es de este nombre (hasta recargar la pagina).
    tokens.set(`mock-${me.username}`, name);
    me.username = name;
    save();
    return json(me);
  }
  if (path === "/api/avatars") return json(avatarsOf().map(({ src: _, ...a }) => a));
  if (path === "/api/admin/avatars" && m === "POST") {
    if (!me.is_admin) return err(403, "Admin access required");
    // Como el server: un formulario multipart con `file` y `name`.
    const form = await req.formData();
    const blob = form.get("file");
    if (!(blob instanceof Blob)) return err(400, "Send the image in a `file` field");
    if (blob.size > 2 * 1024 * 1024) return err(413, "Images can be up to 2 MB");
    if (!/^image\/(png|jpeg|webp|gif)$/.test(blob.type)) return err(415, "Use a PNG, JPEG, WebP or GIF image");
    const src = await new Promise<string>((ok) => {
      const r = new FileReader();
      r.onload = () => ok(String(r.result));
      r.readAsDataURL(blob);
    });
    const list = avatarsOf();
    const a = { id: Math.max(0, ...list.map((x) => x.id)) + 1, name: String(form.get("name") ?? "") || "Avatar", created_at: Math.floor(Date.now() / 1000), src };
    list.push(a);
    save();
    const { src: _, ...out } = a;
    return json(out, 201);
  }
  if ((r = path.match(/^\/api\/admin\/avatars\/(\d+)$/)) && m === "DELETE") {
    if (!me.is_admin) return err(403, "Admin access required");
    const id = Number(r[1]);
    db.avatars = avatarsOf().filter((a) => a.id !== id);
    for (const u of db.users) if (u.avatar_id === id) u.avatar_id = null;
    save();
    return none();
  }
  if (path.startsWith("/api/me/sessions")) {
    const s = mockSessions(me.username, m, path);
    if (s) return s.status === 204 ? none() : json(s.body, s.status);
  }
  if (path === "/api/me/password") {
    const b = await body<{ current: string; new: string }>();
    if (db.passwords[me.username] !== b.current) return err(400, "The current password is wrong");
    db.passwords[me.username] = b.new;
    save();
    return none();
  }
  if (path === "/api/me/settings") {
    const cur: Settings = db.settings[me.username] ?? { ui: {}, has_rd_key: false, preferred_quality: "1080p", max_size_gb: 20 };
    if (m === "PUT") {
      const b = await body<Partial<Settings> & { rd_key?: string }>();
      if (b.ui) cur.ui = { ...cur.ui, ...b.ui };
      if (b.preferred_quality !== undefined) cur.preferred_quality = b.preferred_quality;
      if (b.max_size_gb !== undefined) cur.max_size_gb = b.max_size_gb;
      if (b.rd_key !== undefined) cur.has_rd_key = b.rd_key !== "";
      db.settings[me.username] = cur;
      save();
    }
    return json(cur);
  }
  if ((r = path.match(/^\/api\/catalog\/(movie|series)\/(\w+)$/))) {
    const genre = url.searchParams.get("genre");
    const skip = Number(url.searchParams.get("skip") ?? 0);
    let list = fx.catalogs[`${r[1]}/${r[2]}${genre ? `/${genre}` : ""}`];
    if (!list) {
      list = (fx.catalogs[`${r[1]}/top`] ?? []).filter((x) => !genre || x.genres?.includes(genre));
      if (r[2] === "imdbRating") list = [...list].sort((a, b) => Number(b.imdb_rating ?? 0) - Number(a.imdb_rating ?? 0));
    }
    return json(list.slice(skip, skip + 30));
  }
  if ((r = path.match(/^\/api\/top\/(movie|series)$/))) {
    // Como el server (imdb.rs): por rating, con todos los generos pedidos y el conteo de cada
    // genero sumado al filtro. Los votos son inventados (los fixtures no los traen).
    await wait(250);
    const want = (url.searchParams.get("genres") ?? "").split(",").filter(Boolean);
    const minVotes = Math.max(1000, Number(url.searchParams.get("min_votes") ?? (r[1] === "movie" ? 25000 : 10000)));
    const skip = Number(url.searchParams.get("skip") ?? 0);
    const limit = Number(url.searchParams.get("limit") ?? 50);
    const votes = (id: string) => (Number(id.replace(/\D/g, "")) % 97) * 9000 + 1000;
    const all = allPreviews()
      .filter((x) => x.type === r![1] && x.imdb_rating)
      .sort((a, b) => Number(b.imdb_rating) - Number(a.imdb_rating));
    const hits = all.filter((x) => votes(x.id) >= minVotes && want.every((g) => x.genres?.includes(g)));
    const names = [...new Set(all.flatMap((x) => x.genres ?? []))].sort();
    const genres = names.map((name) => ({ name, count: hits.filter((x) => x.genres?.includes(name)).length }));
    return json({
      state: "ready",
      items: hits.slice(skip, skip + limit),
      total: hits.length,
      more: skip + limit < hits.length,
      genres,
      min_votes: minVotes,
      updated_at: Math.floor(Date.now() / 1000) - 86400,
    });
  }
  if ((r = path.match(/^\/api\/genres\/(movie|series)$/)))
    return json(["Action", "Adventure", "Animation", "Comedy", "Crime", "Drama", "Fantasy", "Horror", "Sci-Fi", "Thriller"]);
  if (path === "/api/search") {
    const q = (url.searchParams.get("q") ?? "").toLowerCase();
    const hits = allPreviews().filter((x) => x.name.toLowerCase().includes(q));
    return json({ movies: hits.filter((x) => x.type === "movie"), series: hits.filter((x) => x.type === "series") });
  }
  if ((r = path.match(/^\/api\/meta\/(movie|series)\/(.+)$/))) {
    const meta = metaFor(r[1], decodeURIComponent(r[2]));
    return meta ? json(meta) : err(404, "Not found");
  }
  if ((r = path.match(/^\/api\/streams\/(movie|series)\/(.+)$/))) {
    await wait(500);
    return json(streamsFor(decodeURIComponent(r[2])));
  }
  if ((r = path.match(/^\/api\/subtitles\/(movie|series)\/(.+)$/)))
    return json([
      { id: "os-1", lang: "eng", url: "/api/media/subs/external?u=mock-eng" },
      { id: "os-2", lang: "spa", url: "/api/media/subs/external?u=mock-spa" },
      { id: "os-3", lang: "por", url: "/api/media/subs/external?u=mock-por" },
      // Mas de una version por idioma (y una variante): para probar el menu por pasos.
      { id: "os-4", lang: "eng", url: "/api/media/subs/external?u=mock-eng" },
      { id: "os-5", lang: "eng", url: "/api/media/subs/external?u=mock-eng" },
      { id: "os-6", lang: "spl", url: "/api/media/subs/external?u=mock-spa" },
    ]);
  if (path === "/api/prefetch") return none(202);
  if (path === "/api/play" && m === "POST") {
    const b = await body<{ stream_key: string }>();
    await wait(700);
    const idx = Number(b.stream_key.split(":")[0].slice(-1));
    const session = Math.random().toString(36).slice(2) + Math.random().toString(36).slice(2);
    // El stream 3 se traba (para probar "Try another stream"); el 4 va por HLS (hls.js).
    sessions.set(session, { started: Date.now(), total: 6e9, peersMax: idx === 3 ? 2 : 40, slow: idx === 3 });
    const hls = idx === 4;
    const info: PlayInfo = {
      session_id: session,
      mode: hls ? "remux" : "direct",
      // Una IP que no contesta: el video se queda esperando datos (como un torrent sin peers).
      direct_url: idx === 3 ? "http://10.255.255.1/never.mp4" : VIDEO_DIRECT,
      hls_url: hls ? VIDEO_HLS : undefined,
      file_name: `Mock.${b.stream_key}.mkv`,
      size_bytes: 6e9,
      duration_s: DURATION,
      container: hls ? "mp4" : "webm",
      video_codec: hls ? "h264" : "vp9",
      width: 1280,
      height: 720,
      audio_tracks: [
        { index: 1, codec: hls ? "aac" : "opus", lang: "eng", title: "English 5.1", default: true },
        { index: 2, codec: hls ? "aac" : "opus", lang: "spa", title: "Spanish" },
      ],
      subtitle_tracks: [
        { index: 3, codec: "subrip", lang: "eng", title: "English" },
        { index: 4, codec: "subrip", lang: "spa", title: "Spanish (Forced)" },
      ],
    };
    return json(info);
  }
  if ((r = path.match(/^\/api\/media\/subs\/external$/))) return new Response(vtt((url.searchParams.get("u") ?? "").slice(5)));
  if ((r = path.match(/^\/api\/media\/([^/]+)\/subs\/(\d+)\.vtt$/))) return new Response(vtt(r[2] === "4" ? "spa" : "eng"));
  if ((r = path.match(/^\/api\/media\/([^/]+)\/status$/))) {
    const s = sessions.get(r[1]);
    return s ? json(status(s)) : err(404, "No such session");
  }
  if ((r = path.match(/^\/api\/media\/([^/]+)$/)) && m === "DELETE") {
    sessions.delete(r[1]);
    return none();
  }
  if (path === "/api/progress/continue")
    return json(
      [...db.progress]
        .filter((p) => p.position_s / p.duration_s < 0.92)
        .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
        .slice(0, 20),
    );
  if (path === "/api/progress" && m === "GET") return json(db.progress.filter((p) => p.series_id === url.searchParams.get("series_id")));
  if (path === "/api/progress" && m === "PUT") {
    const p = await body<Progress>();
    db.progress = db.progress.filter((x) => x.meta_id !== p.meta_id);
    db.progress.push({ ...p, updated_at: new Date().toISOString() });
    save();
    return none();
  }
  if ((r = path.match(/^\/api\/progress\/(.+)$/))) {
    const id = decodeURIComponent(r[1]);
    if (m === "DELETE") {
      db.progress = db.progress.filter((x) => x.meta_id !== id);
      save();
      return none();
    }
    const p = db.progress.find((x) => x.meta_id === id);
    return p ? json(p) : err(404, "No progress");
  }
  if (path === "/api/library") return json(db.library);
  if ((r = path.match(/^\/api\/library\/(movie|series)\/(.+)$/))) {
    const id = decodeURIComponent(r[2]);
    db.library = db.library.filter((x) => x.id !== id);
    if (m === "PUT") db.library.unshift(await body<MetaPreview>());
    save();
    return none();
  }
  if (path.startsWith("/api/admin/") && !me.is_admin) return err(403, "Admins only");
  if (path === "/api/admin/users" && m === "GET") return json(db.users);
  if (path === "/api/admin/users" && m === "POST") {
    const b = await body<{ username: string; password: string; is_admin: boolean }>();
    if (db.users.some((u) => u.username === b.username)) return err(409, "That username is taken");
    const u: User = { id: db.nextId++, username: b.username, is_admin: b.is_admin, disabled: false, created_at: new Date().toISOString() };
    db.users.push(u);
    db.passwords[u.username] = b.password;
    save();
    return json(u);
  }
  if ((r = path.match(/^\/api\/admin\/users\/(\d+)\/sessions(?:\/(\d+))?$/))) {
    const u = db.users.find((x) => x.id === Number(r![1]));
    if (!u) return err(404, "User not found");
    const l = sessionsOf(u.username);
    if (!r[2] && m === "GET") return json([...l].sort((a, b) => b.last_used_at - a.last_used_at).map((x) => ({ ...x, current: x.current && u.id === me.id })));
    if (r[2] && m === "DELETE") {
      const i = l.findIndex((x) => x.id === Number(r![2]));
      if (i < 0) return err(404, "Session not found");
      l.splice(i, 1);
      return none();
    }
  }
  if ((r = path.match(/^\/api\/admin\/users\/(\d+)$/))) {
    const u = db.users.find((x) => x.id === Number(r![1]));
    if (!u) return err(404, "No such user");
    if (m === "DELETE") {
      if (u.id === me.id) return err(400, "You can't delete yourself");
      db.users = db.users.filter((x) => x !== u);
      save();
      return none();
    }
    const b = await body<{ username?: string; password?: string; is_admin?: boolean; disabled?: boolean }>();
    if (b.username !== undefined && b.username.trim() !== u.username) {
      const name = b.username.trim();
      if (!/^[A-Za-z0-9_.-]{1,32}$/.test(name)) return err(400, "Username must be 1-32 characters: letters, numbers, '_', '-' or '.'");
      if (db.users.some((x) => x !== u && x.username.toLowerCase() === name.toLowerCase())) return err(409, "Username already exists");
      db.passwords[name] = db.passwords[u.username];
      tokens.set(`mock-${u.username}`, name);
      u.username = name;
    }
    if (b.password) db.passwords[u.username] = b.password;
    if (b.is_admin !== undefined) u.is_admin = b.is_admin;
    if (b.disabled !== undefined) u.disabled = b.disabled;
    save();
    return json(u);
  }
  if (path === "/api/admin/torrents") {
    for (const t of torrents)
      if (t.state === "live") {
        t.down_bps = 3e6 + Math.random() * 5e6;
        t.downloaded_bytes = Math.min(t.size_bytes, t.downloaded_bytes + t.down_bps * 2);
        t.peers = 30 + Math.floor(Math.random() * 15);
      }
    return json(torrents);
  }
  if ((r = path.match(/^\/api\/admin\/torrents\/(\w+)$/))) {
    const i = torrents.findIndex((t) => t.info_hash === r![1]);
    if (i >= 0) torrents.splice(i, 1);
    return none();
  }
  if (path === "/api/admin/stats") {
    const s: AdminStats = {
      cache_bytes: 182e9,
      cache_limit_bytes: 300e9,
      disk_free_bytes: 640e9,
      active_sessions: sessions.size,
      users: db.users.length,
    };
    return json(s);
  }
  if (path === "/api/admin/metrics") {
    const range = (url.searchParams.get("range") ?? "24h") as MetricsRange;
    if (!["24h", "7d", "30d", "90d"].includes(range)) return err(400, "range must be 24h, 7d, 30d or 90d");
    return json(mockMetrics(range));
  }
  if (path === "/api/admin/logs") return json(mockLogs(url.searchParams));
  if (path === "/api/admin/audit") return json(mockAudit(url.searchParams));
  return err(404, `Mock: no route for ${m} ${path}`);
}

export function installMock() {
  // Las imagenes de los avatares van por <img>, no por fetch.
  setMockAvatar((id) => avatarsOf().find((a) => a.id === id)?.src ?? "data:,");
  const real = window.fetch.bind(window);
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const req = new Request(input, init);
    const url = new URL(req.url, location.href);
    if (!url.pathname.startsWith("/api/") || url.origin !== location.origin) return real(input, init);
    try {
      return await route(req);
    } catch (e) {
      return err(500, `Mock error: ${e}`);
    }
  };
  console.info("%cMythos mock mode", "color:#6ee7b7;font-weight:bold", "users: admin/admin, test/test");
}
