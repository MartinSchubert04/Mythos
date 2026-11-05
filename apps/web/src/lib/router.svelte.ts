// Rutas con hash (#/title/movie/tt0111161): andan igual en la web (el server sirve index.html)
// y en Tauri. Cada vista se carga recien cuando hace falta (import dinamico).
import type { Component } from "svelte";
import { tick } from "svelte";
import { reduceMotion } from "./motion";
import { markDown } from "./net.svelte";

export type ViewName = "home" | "search" | "top" | "detail" | "player" | "library" | "settings" | "admin";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Loader = () => Promise<{ default: Component<any> }>;
const loaders: Record<ViewName, Loader> = {
  home: () => import("../views/Home.svelte"),
  search: () => import("../views/Search.svelte"),
  top: () => import("../views/Top.svelte"),
  detail: () => import("../views/Detail.svelte"),
  player: () => import("../views/Player.svelte"),
  library: () => import("../views/Library.svelte"),
  settings: () => import("../views/Settings.svelte"),
  admin: () => import("../views/Admin.svelte"),
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const loaded = $state<Partial<Record<ViewName, Component<any>>>>({});
const pending = new Map<ViewName, Promise<void>>();

/** Carga el codigo de una vista (se puede llamar antes, por ejemplo al pasar el mouse). */
export function loadView(v: ViewName): Promise<void> {
  if (loaded[v]) return Promise.resolve();
  let p = pending.get(v);
  if (!p) {
    p = loaders[v]().then((m) => {
      loaded[v] = m.default;
    });
    // Sin el codigo de la vista (en la web lo sirve el mismo server: si se cayo, no llega) quedaba
    // el logo girando para siempre y sin aviso (probado en la PC con el server apagado). Se avisa
    // como cualquier pedido que no llega; al volver, App la carga de nuevo.
    p.catch(() => {
      pending.delete(v);
      markDown();
    });
    pending.set(v, p);
  }
  return p;
}

export function viewComponent(v: ViewName) {
  return loaded[v];
}

type Route = { path: string; view: ViewName; params: string[]; query: URLSearchParams; key: string; vt: boolean };

function parse(hash: string, vt = false): Route {
  const raw = hash.replace(/^#/, "") || "/";
  const [path, qs = ""] = raw.split("?");
  const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
  const query = new URLSearchParams(qs);
  let view: ViewName = "home";
  let params: string[] = [];
  switch (parts[0]) {
    case "search":
      view = "search";
      break;
    case "title":
      view = "detail";
      params = parts.slice(1, 3);
      break;
    case "watch":
      view = "player";
      params = parts.slice(1, 3);
      break;
    case "top":
    case "library":
    case "settings":
    case "admin":
      view = parts[0];
      break;
  }
  // La busqueda no se vuelve a montar al escribir (cambia solo ?q=), ni "Top rated" al cambiar
  // los filtros (?type=&genres=&votes=).
  const key = view === "search" || view === "top" ? view : path;
  return { path, view, params, query, key, vt };
}

export const route = $state<Route>(parse(location.hash));

function apply(r: Route) {
  Object.assign(route, r);
}

function sync() {
  const r = parse(location.hash);
  if (r.path === route.path && r.query.toString() === route.query.toString()) return;
  loadView(r.view).finally(() => apply(r));
}
/** "Atras" del navegador con algo abierto (modo TV en la web: el boton atras del control en
 *  Firefox para Android navega el historial): si devuelve true ya lo uso para cerrar eso, y la
 *  ruta no cambia. Lo pone lib/spatial.svelte.ts. */
let backGuard: (() => boolean) | null = null;
export function setBackGuard(f: () => boolean) {
  backGuard = f;
}

function onPop() {
  const r = parse(location.hash);
  if (r.path !== route.path && backGuard?.()) {
    // Se vuelve a poner la URL de la vista que sigue en pantalla.
    const q = route.query.toString();
    history.pushState(null, "", "#" + route.path + (q ? "?" + q : ""));
    return;
  }
  sync();
}
// Atras/adelante y la URL escrita a mano (hashchange) re-parsean la ruta.
window.addEventListener("popstate", onPop);
window.addEventListener("hashchange", sync);

// El estado de la ruta es uno solo: si Vite recargara este modulo en caliente quedarian dos
// copias (App con una y go() con otra). Mejor recargar la pagina entera.
if (import.meta.hot) import.meta.hot.accept(() => location.reload());

/** Ir a una ruta. Con `vt`, View Transitions (el poster vuela de la tarjeta al detalle). */
export async function go(path: string, o: { replace?: boolean; vt?: boolean } = {}) {
  const url = "#" + path;
  if (o.replace) history.replaceState(null, "", url);
  else if (location.hash !== url) history.pushState(null, "", url);
  const r = parse(url, false);
  const vt = o.vt && !!document.startViewTransition && !reduceMotion();
  if (!vt) {
    await loadView(r.view).catch(() => {});
    apply(r);
    return;
  }
  // El codigo de la vista se carga antes: la captura del estado nuevo espera al callback.
  await loadView(r.view).catch(() => {});
  r.vt = true;
  document.startViewTransition!(async () => {
    apply(r);
    await tick();
  });
}

/** Cambia la query sin agregar una entrada al historial (la busqueda en vivo). */
export function setQuery(params: Record<string, string>) {
  const q = new URLSearchParams(params).toString();
  const url = "#" + route.path + (q ? "?" + q : "");
  history.replaceState(null, "", url);
  route.query = new URLSearchParams(q);
}

export function back(fallback = "/") {
  if (history.length > 1 && history.state !== "root") history.back();
  else go(fallback, { replace: true });
}

// La primera entrada queda marcada: "atras" desde ahi va al inicio en vez de salir de la app.
if (!history.state) history.replaceState("root", "", location.href);

/** "/" o Ctrl K: la busqueda se carga recien ahi, asi que lo que se escribe mientras tanto se
 *  guarda aca y el campo lo toma (con el foco) al montarse. */
export const searchIntent = { pending: false, typed: "" };
