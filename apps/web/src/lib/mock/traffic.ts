// Mock de las metricas, logs, auditoria y sesiones (Admin → Traffic / Logs, Settings → devices).
// Datos inventados pero creibles: picos a la noche, mas trafico el fin de semana, video que
// pesa GB, un par de IPs probando contraseñas. Deterministas (misma semilla) para comparar
// capturas.
import type { AuditRow, LogRow, Metrics, MetricsRange, Page, SessionInfo } from "../types";

function rng(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const HOUR = 3_600_000;

type Who = { ip: string; user: string | null; uid: number; ts: string | null; w: number };
// Quien pega (peso = cuanto trafico hace).
const WHO: Who[] = [
  { ip: "100.101.12.4", user: "admin", uid: 1, ts: "martin@github", w: 34 },
  { ip: "100.88.40.17", user: "admin", uid: 1, ts: "martin@github", w: 14 },
  { ip: "100.88.40.17", user: "test", uid: 2, ts: "martin@github", w: 3 },
  { ip: "100.72.3.9", user: "test", uid: 2, ts: "sofi@github", w: 26 },
  { ip: "100.64.0.23", user: "test", uid: 2, ts: "living-room-tv", w: 11 },
  { ip: "127.0.0.1", user: null, uid: 0, ts: null, w: 4 },
  { ip: "100.99.1.50", user: null, uid: 0, ts: "guest@github", w: 1.5 },
];

type Rt = { method: string; route: string; w: number; ms: [number, number]; kb: [number, number]; err?: number; c4?: number };
// Rutas: peso, latencia tipica (ms) y tamaño de respuesta (KB).
const ROUTES: Rt[] = [
  { method: "GET", route: "/api/media/{session}/file", w: 26, ms: [4, 60], kb: [2000, 48000], err: 0.004 },
  { method: "GET", route: "/api/media/{session}/hls/{segment}", w: 22, ms: [8, 900], kb: [900, 6000], err: 0.01 },
  { method: "GET", route: "/api/media/{session}/status", w: 15, ms: [1, 6], kb: [0.3, 0.4] },
  { method: "GET", route: "/api/catalog/{type}/{catalog}", w: 8, ms: [3, 420], kb: [6, 30] },
  { method: "GET", route: "/api/meta/{type}/{id}", w: 7, ms: [2, 380], kb: [4, 40] },
  { method: "PUT", route: "/api/progress", w: 6, ms: [1, 9], kb: [0, 0] },
  { method: "GET", route: "/api/streams/{type}/{id}", w: 3, ms: [300, 4200], kb: [8, 60], err: 0.03 },
  { method: "POST", route: "/api/play", w: 1.6, ms: [900, 9000], kb: [1, 3], err: 0.05 },
  { method: "GET", route: "/api/subtitles/{type}/{id}", w: 1.4, ms: [150, 2500], kb: [1, 6], err: 0.02 },
  { method: "GET", route: "/api/media/{session}/subs/{index}", w: 1.2, ms: [20, 6000], kb: [40, 160] },
  { method: "GET", route: "/api/progress/continue", w: 2, ms: [1, 8], kb: [1, 8] },
  { method: "GET", route: "/api/me/settings", w: 1, ms: [1, 4], kb: [0.2, 1] },
  { method: "GET", route: "/api/search", w: 1.2, ms: [80, 900], kb: [5, 25] },
  { method: "GET", route: "/api/health", w: 2.5, ms: [0, 2], kb: [0.06, 0.06] },
  { method: "POST", route: "/api/auth/login", w: 0.25, ms: [400, 1300], kb: [0.2, 0.3], c4: 0.35 },
  { method: "GET", route: "/api/admin/stats", w: 2, ms: [2, 30], kb: [0.4, 1] },
  { method: "GET", route: "/api/{unknown}", w: 0.2, ms: [0, 1], kb: [0.05, 0.05], c4: 1 },
];

const pick = <T extends { w: number }>(l: T[], r: number): T => {
  const tot = l.reduce((a, x) => a + x.w, 0);
  let x = r * tot;
  for (const it of l) if ((x -= it.w) <= 0) return it;
  return l[l.length - 1];
};
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

/** Requests de una hora: poco de madrugada, pico de 20 a 24 h, mas el fin de semana. */
function hourLoad(t: number, r: () => number): number {
  const d = new Date(t);
  const h = d.getHours();
  const curve = [0.15, 0.08, 0.05, 0.04, 0.03, 0.04, 0.08, 0.2, 0.3, 0.3, 0.32, 0.35, 0.45, 0.4, 0.35, 0.38, 0.45, 0.55, 0.7, 0.85, 1, 1, 0.9, 0.5][h];
  const weekend = d.getDay() === 0 || d.getDay() === 6 ? 1.4 : 1;
  // Algunas horas sin nada (nadie miro nada): huecos de verdad en el grafico.
  if (curve < 0.1 && r() < 0.5) return 0;
  return Math.round(curve * weekend * (220 + r() * 260));
}

type Hour = { hour_ms: number; requests: number; errors: number; c4: number; bytes_out: number; p95_ms: number | null };
let hours: Hour[] | null = null;
function series(): Hour[] {
  if (hours) return hours;
  const r = rng(42);
  const now = Date.now();
  const end = Math.floor(now / HOUR) * HOUR;
  hours = [];
  for (let t = end - 90 * 24 * HOUR; t <= end; t += HOUR) {
    let n = hourLoad(t, r);
    // La hora actual va por la mitad.
    if (t === end) n = Math.round((n * (now - end)) / HOUR);
    if (!n) continue;
    // Una noche con el server medio caido (Torrentio fallando): muchos 502.
    const bad = Math.floor((end - t) / HOUR) === 30 || Math.floor((end - t) / HOUR) === 31;
    const errors = bad ? Math.round(n * 0.18) : r() < 0.25 ? Math.round(r() * 3) : 0;
    const streaming = n * (0.5 + r() * 0.5);
    hours.push({
      hour_ms: t,
      requests: n,
      errors,
      c4: Math.round(n * 0.01 + r() * 2),
      bytes_out: Math.round(streaming * (1.5e6 + r() * 4e6)),
      p95_ms: [250, 500, 1000, 2500][Math.min(3, Math.floor(r() * 3.2 + (bad ? 1.5 : 0)))],
    });
  }
  return hours;
}

const SPAN: Record<MetricsRange, number> = { "24h": 24, "7d": 168, "30d": 720, "90d": 2160 };

export function mockMetrics(range: MetricsRange): Metrics {
  const to = Date.now();
  const from = to - SPAN[range] * HOUR;
  const s = series().filter((h) => h.hour_ms >= Math.floor(from / HOUR) * HOUR);
  const tot = s.reduce(
    (a, h) => ((a.requests += h.requests), (a.errors += h.errors), (a.c4 += h.c4), (a.bytes += h.bytes_out), a),
    { requests: 0, errors: 0, c4: 0, bytes: 0 },
  );
  const r = rng(SPAN[range]);
  // Reparto del total por IP, usuario y ruta segun los pesos (con algo de ruido).
  const share = <T extends { w: number }>(l: T[]) => {
    const ws = l.map((x) => x.w * (0.7 + r() * 0.6));
    const sum = ws.reduce((a, b) => a + b, 0);
    return l.map((x, i) => ({ x, f: ws[i] / sum }));
  };
  const ipMap = new Map<string, { ip: string; requests: number; bytes_out: number; users: Set<string>; tailscale_logins: Set<string> }>();
  const userMap = new Map<number, { user_id: number; username: string; requests: number; bytes_out: number }>();
  for (const { x, f } of share(WHO)) {
    const e = ipMap.get(x.ip) ?? { ip: x.ip, requests: 0, bytes_out: 0, users: new Set(), tailscale_logins: new Set() };
    const req = Math.round(tot.requests * f);
    const by = Math.round(tot.bytes * f * (x.user ? 1 : 0.002));
    e.requests += req;
    e.bytes_out += by;
    if (x.user) e.users.add(x.user);
    if (x.ts) e.tailscale_logins.add(x.ts);
    ipMap.set(x.ip, e);
    if (x.user) {
      const u = userMap.get(x.uid) ?? { user_id: x.uid, username: x.user, requests: 0, bytes_out: 0 };
      u.requests += req;
      u.bytes_out += by;
      userMap.set(x.uid, u);
    }
  }
  const routes = share(ROUTES).map(({ x, f }) => {
    const req = Math.round(tot.requests * f);
    const mid = (x.kb[0] + x.kb[1]) / 2;
    return {
      method: x.method,
      route: x.route,
      requests: req,
      errors: Math.round(req * (x.err ?? 0) + (x.route === "/api/streams/{type}/{id}" ? tot.errors * 0.5 : 0)),
      bytes_out: req * mid,
      p50_ms: x.ms[0] < 5 ? 5 : x.ms[0] < 50 ? 50 : x.ms[0] < 300 ? 250 : 1000,
      p95_ms: x.ms[1] < 5 ? 5 : x.ms[1] < 50 ? 50 : x.ms[1] < 500 ? 500 : x.ms[1] < 1000 ? 1000 : x.ms[1] < 2500 ? 2500 : x.ms[1] < 5000 ? 5000 : 10000,
    };
  });
  const routeBytes = routes.reduce((a, x) => a + x.bytes_out, 0);
  const ok = tot.requests - tot.errors - tot.c4;
  const fails = audit().filter((a) => a.event === "login_fail" && a.ts_ms >= from);
  const fl = new Map<string, { ip: string; count: number; last_at_ms: number; usernames: Set<string> }>();
  for (const a of fails) {
    const e = fl.get(a.ip!) ?? { ip: a.ip!, count: 0, last_at_ms: 0, usernames: new Set() };
    e.count++;
    e.last_at_ms = Math.max(e.last_at_ms, a.ts_ms);
    if (a.username) e.usernames.add(a.username);
    fl.set(a.ip!, e);
  }
  return {
    range,
    from_ms: from,
    to_ms: to,
    totals: { requests: tot.requests, errors: tot.errors, client_errors: tot.c4, bytes_out: tot.bytes, p50_ms: 5, p95_ms: range === "24h" ? 1000 : 2500 },
    series: s.map((h) => ({ hour_ms: h.hour_ms, requests: h.requests, errors: h.errors, bytes_out: h.bytes_out, p95_ms: h.p95_ms })),
    top_ips: [...ipMap.values()]
      .map((e) => ({ ...e, users: [...e.users].sort(), tailscale_logins: [...e.tailscale_logins].sort() }))
      .sort((a, b) => b.requests - a.requests),
    top_users: [...userMap.values()].sort((a, b) => b.requests - a.requests),
    // Los bytes de cada ruta se escalan para que sumen el total (como en el server).
    top_routes: routes
      .map((x) => ({ ...x, bytes_out: Math.round((x.bytes_out / (routeBytes || 1)) * tot.bytes) }))
      .sort((a, b) => b.requests - a.requests),
    status: [
      { status: 200, requests: Math.round(ok * 0.9) },
      { status: 204, requests: Math.round(ok * 0.06) },
      { status: 206, requests: ok - Math.round(ok * 0.9) - Math.round(ok * 0.06) - Math.round(ok * 0.004) },
      { status: 304, requests: Math.round(ok * 0.004) },
      { status: 401, requests: Math.round(tot.c4 * 0.55) },
      { status: 404, requests: Math.round(tot.c4 * 0.4) },
      { status: 429, requests: tot.c4 - Math.round(tot.c4 * 0.55) - Math.round(tot.c4 * 0.4) },
      { status: 500, requests: Math.round(tot.errors * 0.1) },
      { status: 502, requests: tot.errors - Math.round(tot.errors * 0.1) },
    ].filter((x) => x.requests > 0),
    failed_logins: [...fl.values()].map((e) => ({ ...e, usernames: [...e.usernames] })).sort((a, b) => b.count - a.count),
    dropped_events: 0,
  };
}

// ---- logs crudos: los ultimos ~3 dias, del mas nuevo al mas viejo

let raw: LogRow[] | null = null;
function logs(): LogRow[] {
  if (raw) return raw;
  const r = rng(7);
  raw = [];
  let t = Date.now() - 4000;
  let id = 48_211;
  for (let i = 0; i < 2400; i++) {
    const w = pick(WHO, r());
    const rt = pick(ROUTES, r());
    let status = rt.route.includes("/media/") && rt.route.endsWith("/file") ? 206 : rt.method === "PUT" ? 204 : 200;
    if (rt.c4 && r() < rt.c4) status = rt.route.includes("unknown") ? 404 : w.user ? 401 : r() < 0.15 ? 429 : 401;
    else if (rt.err && r() < rt.err * 4) status = 502;
    if (!w.user && !["/api/health", "/api/auth/login", "/api/{unknown}"].includes(rt.route)) status = 401;
    const dur = Math.round(lerp(rt.ms[0], rt.ms[1], r() ** 3));
    const bytes = status >= 400 ? 40 + Math.round(r() * 30) : Math.round(lerp(rt.kb[0], rt.kb[1], r()) * 1000);
    raw.push({
      id: id--,
      ts_ms: t,
      ip: w.ip,
      tailscale_login: w.ts,
      user_id: w.user ? w.uid : null,
      username: w.user,
      method: rt.method,
      route: rt.route,
      status,
      dur_ms: dur,
      bytes,
    });
    t -= Math.round(r() ** 2 * 400_000);
  }
  return raw;
}

export function mockLogs(q: URLSearchParams): Page<LogRow> {
  const limit = Math.min(1000, Math.max(1, Number(q.get("limit") ?? 100)));
  const before = q.get("before") ? Number(q.get("before")) : Infinity;
  const user = q.get("user")?.trim();
  const ip = q.get("ip")?.trim();
  const st = q.get("status")?.trim();
  const route = q.get("route")?.trim();
  const out = logs().filter(
    (l) =>
      l.id < before &&
      (!user || l.username === user || String(l.user_id) === user) &&
      (!ip || l.ip === ip) &&
      (!st || (st.endsWith("xx") ? Math.floor(l.status / 100) === Number(st[0]) : l.status === Number(st))) &&
      (!route || l.route.includes(route)),
  );
  const items = out.slice(0, limit);
  return { items, next_before: out.length > limit ? items[items.length - 1].id : null };
}

// ---- auditoria

let events: AuditRow[] | null = null;
function audit(): AuditRow[] {
  if (events) return events;
  const r = rng(99);
  const now = Date.now();
  const list: Omit<AuditRow, "id">[] = [];
  const add = (ago: number, event: string, username: string | null, user_id: number | null, ip: string, detail: string | null = null) =>
    list.push({ ts_ms: now - ago, event, username, user_id, ip, detail });
  // Alguien probando contraseñas desde la misma IP hace un rato.
  for (let i = 0; i < 7; i++) add(47 * 60_000 + i * 21_000, "login_fail", ["admin", "admin", "root", "martin", "admin", "Admin", "administrator"][i], null, "100.99.1.50");
  add(3 * HOUR, "login_fail", "tset", null, "100.72.3.9");
  add(3 * HOUR - 40_000, "login_ok", "test", 2, "100.72.3.9", "Mythos app on Android");
  add(20 * 60_000, "login_ok", "admin", 1, "100.101.12.4", "Chrome on Windows");
  add(5 * HOUR, "user_updated", "admin", 1, "100.101.12.4", "test: password");
  add(5 * HOUR + 60_000, "user_disabled", "admin", 1, "100.101.12.4", "guest: disabled");
  add(26 * HOUR, "session_revoked", "test", 2, "100.72.3.9", "2 other sessions");
  add(2 * 24 * HOUR, "password_changed", "test", 2, "100.72.3.9");
  add(3 * 24 * HOUR, "user_created", "admin", 1, "100.101.12.4", "created guest (admin: false)");
  add(3 * 24 * HOUR + 1000, "logout", "admin", 1, "100.88.40.17");
  add(9 * 24 * HOUR, "user_deleted", "admin", 1, "100.101.12.4", "deleted old-laptop");
  add(12 * 24 * HOUR, "login_fail", "guest", null, "100.99.1.50", "disabled");
  for (let d = 1; d < 30; d++) {
    if (r() < 0.6) add(d * 24 * HOUR + r() * 8 * HOUR, "login_ok", "test", 2, r() < 0.5 ? "100.64.0.23" : "100.72.3.9", r() < 0.5 ? "Living room TV" : null);
    if (r() < 0.3) add(d * 24 * HOUR + r() * 8 * HOUR, "login_ok", "admin", 1, "100.88.40.17", "Pixel 8");
  }
  list.sort((a, b) => b.ts_ms - a.ts_ms);
  let id = 900;
  events = list.map((e) => ({ ...e, id: id-- }));
  return events;
}

export function mockAudit(q: URLSearchParams): Page<AuditRow> {
  const limit = Math.min(1000, Math.max(1, Number(q.get("limit") ?? 100)));
  const before = q.get("before") ? Number(q.get("before")) : Infinity;
  const ev = q.get("event");
  const user = q.get("user")?.toLowerCase();
  const out = audit().filter((a) => a.id < before && (!ev || a.event === ev) && (!user || a.username?.toLowerCase() === user));
  const items = out.slice(0, limit);
  return { items, next_before: out.length > limit ? items[items.length - 1].id : null };
}

// ---- sesiones de cada usuario (la primera es "este dispositivo")

const s = (h: number) => Math.floor(Date.now() / 1000 - h * 3600);
const UA = {
  chromeWin: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
  android: "Mozilla/5.0 (Linux; Android 15; Pixel 8; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/141.0.0.0 Mobile Safari/537.36",
  tv: "Mozilla/5.0 (Linux; Android 12; BRAVIA 4K VH2 Build/STT1.211025.001) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
  firefoxMac: "Mozilla/5.0 (Macintosh; Intel Mac OS X 14.6; rv:132.0) Gecko/20100101 Firefox/132.0",
  safariIos: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1",
};
const sessions = new Map<string, SessionInfo[]>();
export function sessionsOf(user: string): SessionInfo[] {
  let l = sessions.get(user);
  if (l) return l;
  const mk = (id: number, device: string | null, ua: string, ip: string, created: number, used: number, current = false, persistent = false): SessionInfo => ({
    id,
    device,
    user_agent: ua,
    last_ip: ip,
    created_at: s(created),
    last_used_at: s(used),
    // Las de la TV no vencen (auth::NEVER_EXPIRES).
    expires_at: persistent ? 253402300799 : s(used) + 180 * 86400,
    persistent,
    current,
  });
  l =
    user === "admin"
      ? [
          mk(41, null, UA.chromeWin, "100.101.12.4", 30 * 24, 0, true),
          mk(37, "Pixel 8", UA.android, "100.88.40.17", 20 * 24, 2),
          mk(29, null, UA.firefoxMac, "100.88.40.17", 60 * 24, 9 * 24),
          mk(12, "curl", "curl/8.6.0", "127.0.0.1", 90 * 24, 40 * 24),
        ]
      : [
          mk(44, null, UA.chromeWin, "100.72.3.9", 2, 0, true),
          mk(39, "Living room TV", UA.tv, "100.64.0.23", 14 * 24, 5, false, true),
          mk(33, null, UA.safariIos, "100.72.3.9", 25 * 24, 3 * 24),
        ];
  sessions.set(user, l);
  return l;
}

/** /api/me/sessions y /api/me/sessions/{id}. null = no es una ruta de sesiones. */
export function mockSessions(user: string, method: string, path: string): { status: number; body?: unknown } | null {
  if (path === "/api/me/sessions") {
    const l = sessionsOf(user);
    if (method === "GET") return { status: 200, body: [...l].sort((a, b) => b.last_used_at - a.last_used_at) };
    if (method === "DELETE") {
      const n = l.filter((x) => !x.current).length;
      sessions.set(user, l.filter((x) => x.current));
      return { status: 200, body: { revoked: n } };
    }
  }
  const m = path.match(/^\/api\/me\/sessions\/(\d+)$/);
  if (m && method === "DELETE") {
    const l = sessionsOf(user);
    const i = l.findIndex((x) => x.id === Number(m[1]));
    if (i < 0) return { status: 404, body: { error: "Session not found" } };
    l.splice(i, 1);
    return { status: 204 };
  }
  return null;
}
