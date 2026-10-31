// Varias direcciones del MISMO server de casa (desktop y la pagina del APK): p. ej. la de
// Tailscale (`https://<server>.ts.net:7700`, desde cualquier lado) y la de la red de casa
// (`http://192.168.x.x:7700`, mas rapida, solo en casa). Ver docs/local-mode.md ("Varias
// direcciones del server").
//
// - La lista es de este dispositivo (localStorage, `platform.ts`); no se sube al server.
// - Todas tienen que ser el mismo server: se compara el `server_id` de GET /api/health. Una que
//   contesta con otro id no se usa ("Different server"). El token sirve en todas (es la misma base).
// - Cual se usa: se prueban todas a la vez (timeout corto) y gana la que contesta primero; la
//   marcada como preferida gana si contesta (se la espera un poco mas). Se elige al arrancar, al
//   volver de segundo plano, cuando cambia la red y cuando un pedido no llega (y mientras no
//   llegue ninguna, cada pocos segundos: net.svelte.ts y el modo local).
// - Si ninguna contesta, sigue el modo local de siempre (lib/local.svelte.ts).
// - Las sesiones de video quedan en la direccion donde se abrieron (lib/api.ts, `pins`): cambiar
//   de direccion no corta la pelicula; si se traba, el reproductor la reabre en la activa.
// Con una sola direccion no se prueba nada de mas (todo como antes).
import { net, setRecover } from "./net.svelte";
import { addrLabel, loadServers, pickServer, probeServer, remoteBase, saveServers, serverBase, type Probe, type ServerAddr } from "./platform";

/** Resultado de la ultima prueba de una direccion: ms, `null` = no contesto, "other" = es otro
 *  server, "local" = es el server de una app (no el de casa). */
export type Ping = number | null | "other" | "local";

const saved = loadServers();

export const servers = $state({
  list: saved.list,
  preferred: saved.preferred ?? null,
  /** La direccion que se usa ahora ("" = ninguna guardada). */
  active: saved.active && saved.list.some((a) => a.url === saved.active) ? saved.active : (saved.list[0]?.url ?? ""),
  ping: {} as Record<string, Ping>,
  probing: false,
});

function persist() {
  saveServers({ list: $state.snapshot(servers.list), preferred: servers.preferred, active: servers.active || null });
}

/** Hay mas de una direccion (si no, nada de esto hace falta). */
export const multi = () => pickServer && servers.list.length > 1;

/** El nombre que se muestra: el del usuario o el deducido. */
export const labelOf = (a: ServerAddr) => a.label?.trim() || addrLabel(a.url);

/** El de la direccion activa ("" si hay una sola o ninguna). */
export function activeLabel(): string {
  if (!multi()) return "";
  const a = servers.list.find((x) => x.url === servers.active);
  return a ? labelOf(a) : "";
}

/** El `server_id` que se conoce del server de casa (el de la activa, o el de cualquiera). */
function knownId(): string | null {
  return servers.list.find((a) => a.url === servers.active)?.id ?? servers.list.find((a) => a.id)?.id ?? null;
}

/** Lo que contesto en esa direccion, como `Ping`. Un server viejo (sin `server_id`) se acepta. */
function pingOf(p: Probe | null, known: string | null): Ping {
  if (!p) return null;
  if (p.mode === "local") return "local";
  if (known && p.id && p.id !== known) return "other";
  return p.ms;
}

function record(url: string, p: Probe | null, known: string | null) {
  servers.ping[url] = pingOf(p, known);
  const a = servers.list.find((x) => x.url === url);
  // La primera vez que contesta una que se agrego sin poder probarla: queda su id.
  if (a && p?.id && !a.id && typeof servers.ping[url] === "number") {
    a.id = p.id;
    persist();
  }
}

const TIMEOUT = 2500;
/** Cuanto mas se espera a la preferida si otra contesto antes. */
const GRACE = 700;

/** Prueba todas a la vez y resuelve con la que hay que usar (o null si no contesto ninguna). Las
 *  demas siguen llenando `ping` aunque ya se haya elegido. */
function race(): Promise<string | null> {
  const urls = servers.list.map((a) => a.url);
  const pref = servers.preferred && urls.includes(servers.preferred) ? servers.preferred : null;
  const known = knownId();
  servers.probing = true;
  return new Promise((resolve) => {
    let done = false;
    let left = urls.length;
    let first: string | null = null;
    let prefFailed = false;
    let grace = 0;
    const finish = (url: string | null) => {
      if (done) return;
      done = true;
      clearTimeout(grace);
      resolve(url);
    };
    for (const url of urls)
      probeServer(url, TIMEOUT).then((p) => {
        left--;
        record(url, p, known);
        if (!left) servers.probing = false;
        const ok = typeof servers.ping[url] === "number";
        if (ok && url === pref) finish(url);
        else if (ok && !first) {
          first = url;
          if (!pref || prefFailed) finish(url);
          else grace = window.setTimeout(() => finish(first), GRACE);
        } else if (!ok && url === pref) {
          prefFailed = true;
          if (first) finish(first);
        }
        if (!left) finish(first);
      });
  });
}

type Reason = "start" | "resume" | "network" | "failure" | "check" | "manual";

let onSwitch: (url: string) => void = () => {};
/** Lo registra lib/local.svelte.ts: el server del dispositivo sincroniza por la nueva direccion. */
export function onAddrSwitch(f: (url: string) => void) {
  onSwitch = f;
}

function use(url: string, reason: Reason) {
  const old = servers.active;
  if (url === old) return;
  servers.active = url;
  persist();
  // La anterior no contesto: lo que la vista ya pidio ahi (imagenes, listas) se pide de nuevo en
  // la nueva. Si contestaba (habia una mejor), lo ya cargado queda como esta.
  if (reason !== "start" && old && servers.ping[old] === null) net.epoch++;
  onSwitch(url);
}

let running: Promise<string | null> | null = null;
let lastAt = 0;
let lastUrl: string | null = null;

/** Elige direccion ya: devuelve la que quedo en uso si alguna contesta (null si ninguna). Con una
 *  sola direccion, solo dice si contesta. Varias llamadas a la vez comparten la prueba, y por un
 *  pedido que fallo no se repite si recien se probo. */
export function choose(reason: Reason): Promise<string | null> {
  if (!pickServer || !servers.list.length) {
    const base = remoteBase();
    return probeServer(base, 4000).then((p) => (p ? base : null));
  }
  if (servers.list.length === 1) {
    const url = servers.list[0].url;
    return probeServer(url, 4000).then((p) => {
      record(url, p, knownId());
      return typeof servers.ping[url] === "number" ? url : null;
    });
  }
  if (running) return running;
  if ((reason === "failure" || reason === "check") && Date.now() - lastAt < 3000) return Promise.resolve(lastUrl);
  running = race()
    .then((url) => {
      lastAt = Date.now();
      lastUrl = url;
      if (url) use(url, reason);
      return url;
    })
    .finally(() => (running = null));
  return running;
}

// ---- editar la lista (Settings → Account → Server, y el login)

const norm = (u: string) => u.trim().replace(/\/+$/, "");

/** Agrega una direccion (ya probada o no: `probe` null = se la prueba cuando conteste). */
export function addAddress(url: string, probe: Probe | null, label = "") {
  url = norm(url);
  if (!url || servers.list.some((a) => a.url === url)) return;
  servers.list.push({ url, label: label.trim() || undefined, id: probe?.id ?? undefined });
  servers.ping[url] = probe ? probe.ms : null;
  if (!servers.active) servers.active = url;
  persist();
}

export function removeAddress(url: string) {
  if (servers.list.length < 2) return;
  servers.list = servers.list.filter((a) => a.url !== url);
  if (servers.preferred === url) servers.preferred = null;
  delete servers.ping[url];
  const wasActive = servers.active === url;
  if (wasActive) servers.active = servers.list[0].url;
  persist();
  if (wasActive) choose("manual");
}

export function setPreferred(url: string | null) {
  servers.preferred = servers.preferred === url ? null : url;
  persist();
  if (servers.preferred) choose("manual");
}

export function setLabel(url: string, label: string) {
  const a = servers.list.find((x) => x.url === url);
  if (!a) return;
  a.label = label.trim() || undefined;
  persist();
}

/** Mueve una direccion un lugar (`dir` -1 arriba, 1 abajo). El orden es el de la lista: la
 *  primera es con la que se arranca si nunca se eligio otra. */
export function moveAddress(url: string, dir: -1 | 1) {
  const i = servers.list.findIndex((a) => a.url === url);
  const j = i + dir;
  if (i < 0 || j < 0 || j >= servers.list.length) return;
  const l = [...servers.list];
  [l[i], l[j]] = [l[j], l[i]];
  servers.list = l;
  persist();
}

/** El login: la lista que cargo el usuario (con lo que contesto cada una). Las que ya estaban
 *  conservan su nombre; si es otro server, la lista se reemplaza entera. Usa la que corresponde
 *  segun lo que contesto (la preferida, o la mas rapida). */
export function setAddresses(entries: { url: string; probe: Probe | null }[]) {
  const id = entries.find((e) => e.probe?.id)?.probe?.id ?? null;
  const sameServer = !id || !knownId() || knownId() === id;
  const prev = sameServer ? servers.list : [];
  servers.list = entries.map(({ url, probe }) => {
    const old = prev.find((a) => a.url === url);
    return { url, label: old?.label, id: probe?.id ?? (sameServer ? old?.id : undefined) };
  });
  if (!sameServer || !servers.list.some((a) => a.url === servers.preferred)) servers.preferred = null;
  for (const { url, probe } of entries) servers.ping[url] = probe ? probe.ms : null;
  const ok = entries.filter((e) => e.probe);
  const pref = ok.find((e) => e.url === servers.preferred);
  const fastest = [...ok].sort((a, b) => a.probe!.ms - b.probe!.ms)[0];
  servers.active = (pref ?? fastest ?? entries[0])?.url ?? "";
  persist();
}

/** Es una de las direcciones del server de casa. */
export const isOurs = (url: string) => !!url && servers.list.some((a) => a.url === norm(url));

/** Ir a ese server (un perfil de la TV guardado con su direccion): si es una de las de la lista no
 *  cambia nada; si es otro server, queda solo esa. */
export function useServer(url: string) {
  url = norm(url);
  if (!url || isOurs(url)) return;
  servers.list = [{ url }];
  servers.preferred = null;
  servers.active = url;
  persist();
}

/** Para que el aviso de "no se llega al server" pruebe todas antes de seguir esperando. */
// (Solo mientras la API le habla al de casa: en el modo local, la vuelta la maneja local.svelte.ts.)
setRecover(async () => multi() && serverBase() === remoteBase() && (await choose("failure")) !== null);

// ---- cuando probar de nuevo

if (pickServer) {
  // Al volver de segundo plano (el celular salio de casa con la app abierta, la PC desperto).
  let hiddenAt = 0;
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) hiddenAt = Date.now();
    else if (multi() && hiddenAt && Date.now() - hiddenAt > 15_000) choose("resume");
  });
  // Cambio de red (wifi ↔ datos, volvio la conexion).
  let netTimer = 0;
  const onNet = () => {
    clearTimeout(netTimer);
    netTimer = window.setTimeout(() => multi() && choose("network"), 1200);
  };
  window.addEventListener("online", onNet);
  (navigator as Navigator & { connection?: EventTarget }).connection?.addEventListener?.("change", onNet);
}
