// (.svelte.ts por el $effect que mira la ruta.)
// Navegacion espacial con el control remoto (solo en modo TV, lib/tv.svelte.ts). Las flechas
// mueven el foco al elemento que se puede apretar mas cercano en esa direccion, mirando donde
// esta dibujado cada uno; el centro del control (Enter) lo aprieta; "atras" es el Esc de siempre
// (lib/shell.svelte.ts). Sin librerias: unas decenas de elementos por pantalla, cada tecla mide
// los rectangulos de nuevo.
//
// - Primero deciden los componentes: si alguno ya uso la tecla (preventDefault: un menu, un
//   Select abierto, el reproductor) aca no se hace nada. El reproductor maneja sus flechas
//   solo y llama a `moveFocus` cuando muestra los controles.
// - Con algo flotando (dialogo, cajon, menu: `[data-nav-scope]`, `[aria-modal]`, `[role=menu]`,
//   `[role=dialog]`) solo se navega adentro de eso.
// - Si no hay nada en esa direccion, se desplaza el contenedor (texto largo, filas que cargan
//   recien al acercarse).
// - Al cambiar de vista el foco va a lo principal (el boton primario o lo primero del
//   contenido) o, al volver, a lo ultimo que tenia en esa vista.
// - La tecla de menu del control abre el menu contextual del elemento (el mismo del clic
//   derecho o de mantener apretado, lib/press.ts).
// - Campos de texto: pasar por encima no abre el teclado del sistema (inputmode="none" mientras
//   se navega); el centro lo abre.
import { scrollBehavior } from "./motion";
import { isAndroid } from "./platform";
import { profiles } from "./profiles.svelte";
import { back, route, setBackGuard } from "./router.svelte";
import { exitTizenApp, isTizen, keyName } from "./tizen";
import { tv } from "./tv.svelte";

export type Dir = "up" | "down" | "left" | "right";
const DIRS: Record<string, Dir> = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" };

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
const SCOPES = '[data-nav-scope], [aria-modal="true"], [role="menu"], [role="dialog"], [role="alertdialog"]';
const TEXT = 'input:not([type]), input[type="text"], input[type="search"], input[type="password"], input[type="email"], input[type="url"], input[type="number"], textarea';

const hasBox = (el: Element) => {
  const r = el.getBoundingClientRect();
  return r.width >= 1 && r.height >= 1;
};

/** Donde se navega ahora: lo que flota encima (el ultimo abierto) o toda la pagina. */
function scope(): HTMLElement {
  const open = [...document.querySelectorAll<HTMLElement>(SCOPES)].filter(hasBox);
  return open[open.length - 1] ?? document.body;
}

function usable(el: HTMLElement): boolean {
  if (el.closest("[inert], [aria-hidden='true'], [data-nav-skip]")) return false;
  const s = getComputedStyle(el);
  if (s.visibility === "hidden" || s.pointerEvents === "none") return false;
  // Transparente a proposito (lo que aparece solo con el mouse) no; entrando con su animacion
  // (las listas aparecen escalonadas), si: si no, una flecha apretada enseguida lo saltearia.
  return Number(s.opacity) > 0.05 || el.getAnimations().some((a) => a.playState !== "finished");
}

function candidates(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(hasBox);
}

/** Puntaje de ir de `a` a `b` en esa direccion (menos es mejor); null si `b` no esta para ese
 *  lado. Distancia de borde a borde en el eje de la flecha, mas tres veces lo que se corre en el
 *  otro eje (0 si se superponen): asi gana lo que esta "enfrente". `aligned`: se superponen en el
 *  otro eje (a los costados solo se va a lo que esta a la misma altura: de una fila corta no se
 *  salta a la de arriba). */
function score(a: DOMRect, b: DOMRect, d: Dir): { s: number; aligned: boolean } | null {
  const tol = 8;
  let main: number;
  let lo1: number, hi1: number, lo2: number, hi2: number;
  if (d === "right" || d === "left") {
    if (d === "right" ? b.left < a.right - tol && b.left + b.width / 2 <= a.right : b.right > a.left + tol && b.left + b.width / 2 >= a.left) return null;
    main = d === "right" ? Math.max(0, b.left - a.right) : Math.max(0, a.left - b.right);
    [lo1, hi1, lo2, hi2] = [a.top, a.bottom, b.top, b.bottom];
  } else {
    if (d === "down" ? b.top < a.bottom - tol && b.top + b.height / 2 <= a.bottom : b.bottom > a.top + tol && b.top + b.height / 2 >= a.top) return null;
    main = d === "down" ? Math.max(0, b.top - a.bottom) : Math.max(0, a.top - b.bottom);
    [lo1, hi1, lo2, hi2] = [a.left, a.right, b.left, b.right];
  }
  const cross = Math.max(0, lo2 - hi1, lo1 - hi2);
  // Desempate: centros alineados, o bordes de inicio (de algo ancho, como el titulo de una
  // seccion, a lo primero de la fila de abajo y no a lo que quede en el medio).
  const center = Math.min(Math.abs((lo1 + hi1) / 2 - (lo2 + hi2) / 2), Math.abs(lo1 - lo2));
  return { s: main + cross * 3 + center * 0.05, aligned: cross === 0 };
}

/** El mejor de `list` para ir desde `cur` (dibujado en `a`) para ese lado. */
function pick(list: HTMLElement[], cur: HTMLElement, a: DOMRect, d: Dir): HTMLElement | undefined {
  const side = d === "left" || d === "right";
  return list
    .filter((el) => el !== cur && !cur.contains(el))
    .map((el) => ({ el, r: score(a, el.getBoundingClientRect(), d) }))
    .filter((x): x is { el: HTMLElement; r: { s: number; aligned: boolean } } => x.r !== null && (!side || x.r.aligned))
    .sort((x, y) => x.r.s - y.r.s)
    .find((x) => usable(x.el))?.el;
}

/** Lo ultimo enfocado adentro del contenido (para volver desde la barra lateral). */
let lastMain: HTMLElement | null = null;

/** Enfoca y lo trae a la vista (la fila se desplaza de costado; la pagina, lo justo). */
export function focusEl(el: HTMLElement) {
  if (el.matches(TEXT)) quiet(el as HTMLInputElement);
  el.focus({ preventScroll: true });
  // El destacado del inicio se ve entero: la pagina vuelve arriba de todo.
  const page = el.closest(".hero") ? el.closest<HTMLElement>(".page") : null;
  if (page) page.scrollTo({ top: 0, behavior: scrollBehavior() });
  else el.scrollIntoView({ behavior: scrollBehavior(), block: "nearest", inline: "nearest" });
}

/** Mueve el foco para ese lado. `false` si no habia nada (y no se movio). */
export function moveFocus(d: Dir, root: HTMLElement = scope()): boolean {
  const cur = document.activeElement as HTMLElement | null;
  const inside = !!cur && cur !== document.body && root.contains(cur);
  if (!inside) return focusNear(root) || focusFirst(root);
  // De la barra lateral al contenido: a donde estaba.
  if (d === "right" && cur.closest(".sidebar") && lastMain?.isConnected && root.contains(lastMain) && hasBox(lastMain) && usable(lastMain)) {
    focusEl(lastMain);
    return true;
  }
  // Arriba desde algo que lleva un boton propio adentro (la X de "Continue watching"): a ese boton.
  if (d === "up") {
    const inner = cur.querySelector<HTMLElement>("[data-nav-inner]");
    if (inner && hasBox(inner) && usable(inner)) return focusEl(inner), true;
  }
  const a = cur.getBoundingClientRect();
  const side = d === "left" || d === "right";
  // A los costados dentro de una fila: solo sus tarjetas (en una tele, medir todo lo enfocable
  // de la pagina en cada flecha se nota). Al principio o al final de la fila, lo de siempre.
  const track = side ? cur.closest<HTMLElement>(".shelf .track") : null;
  if (track && root.contains(track)) {
    const near = pick(candidates(track), cur, a, d);
    if (near) return focusEl(near), true;
  }
  // Arriba/abajo no se cruza entre la barra lateral y el contenido (eso es con los costados): sin
  // nada mas abajo en el contenido, se desplaza la pagina.
  const region = (el: Element) => el.closest(".sidebar, main");
  const mine = region(cur);
  let next = pick(
    candidates(root).filter((el) => side || region(el) === mine),
    cur,
    a,
    d,
  );
  // Del contenido a la barra lateral: siempre a la seccion activa (no a lo que quede a la altura).
  const activeNav = () => document.querySelector<HTMLElement>(".sidebar .nav.on") ?? document.querySelector<HTMLElement>(".sidebar .nav");
  if (next && d === "left" && cur.closest("main") && next.closest(".sidebar")) next = activeNav() ?? next;
  if (!next) {
    // Entre la barra lateral y el contenido aunque no haya nada a la misma altura: a la seccion
    // activa de la barra, o a lo principal del contenido.
    if (d === "left" && cur.closest("main")) {
      const nav = activeNav();
      if (nav && root.contains(nav) && hasBox(nav)) return focusEl(nav), true;
    }
    const main = cur.closest(".sidebar") && d === "right" ? document.querySelector<HTMLElement>("main") : null;
    return main && root.contains(main) ? focusFirst(main) : false;
  }
  focusEl(next);
  return true;
}

/** Lo que focusFirst prefiere (si no hay nada de esto, lo primero del contenido). */
const PREFERRED = "[data-autofocus], [autofocus], [aria-current='true'], [aria-checked='true'], .sel, .btn.primary";

/** El primer foco de una pantalla: lo marcado con data-autofocus, el boton primario o lo primero
 *  del contenido. */
export function focusFirst(root: HTMLElement = scope(), onlyPreferred = false): boolean {
  const q = (sel: string) => document.querySelector<HTMLElement>(sel);
  const area = root === document.body ? (q("main") ?? q(".player-host") ?? q(".who") ?? q(".body") ?? root) : root;
  const list = candidates(area).filter(usable);
  const pick =
    list.find((el) => el.matches("[data-autofocus], [autofocus]")) ??
    list.find((el) => el.matches("[aria-current='true'], [aria-checked='true'], .sel")) ??
    list.find((el) => el.matches(".btn.primary")) ??
    // La X de un cajon no: lo primero de su contenido (la X queda arriba, a una flecha).
    (onlyPreferred ? undefined : (list.find((el) => !el.matches("[data-close]")) ?? list[0]));
  if (!pick) return false;
  focusEl(pick);
  return true;
}

/** Sin nada para ese lado: se desplaza lo que scrollea (texto largo, filas que cargan al
 *  acercarse). */
function scrollAlong(d: Dir) {
  if (d !== "up" && d !== "down") return;
  let el: HTMLElement | null = (document.activeElement as HTMLElement | null) ?? null;
  while (el && el !== document.body) {
    const s = getComputedStyle(el);
    if (/(auto|scroll)/.test(s.overflowY) && el.scrollHeight > el.clientHeight + 2) break;
    el = el.parentElement;
  }
  // Cada vista scrollea en su .page (App.svelte).
  if (!el || el === document.body) el = document.querySelector<HTMLElement>("main .page");
  el?.scrollBy({ top: (d === "down" ? 1 : -1) * el.clientHeight * 0.45, behavior: scrollBehavior() });
}

// ---- campos de texto: el teclado del sistema solo con el centro

const typing = new WeakSet<HTMLElement>();
function quiet(el: HTMLInputElement) {
  if (typing.has(el) || el.dataset.navMode !== undefined) return;
  el.dataset.navMode = el.getAttribute("inputmode") ?? "";
  el.setAttribute("inputmode", "none");
  el.addEventListener("blur", () => restore(el), { once: true });
}
function restore(el: HTMLElement) {
  const m = el.dataset.navMode;
  if (m === undefined) return;
  delete el.dataset.navMode;
  if (m) el.setAttribute("inputmode", m);
  else el.removeAttribute("inputmode");
  typing.delete(el);
}
/** Centro sobre un campo: ahora si, el teclado (blur + focus dentro del gesto del usuario). */
function openKeyboard(el: HTMLInputElement) {
  restore(el);
  typing.add(el);
  el.blur();
  el.focus({ preventScroll: true });
  el.addEventListener("blur", () => typing.delete(el), { once: true });
}

// ---- el foco al cambiar de vista

/** Vista → lo ultimo enfocado (para volver con "atras" a la misma tarjeta). */
const memory = new Map<string, { sel: string; n: number }>();

function keyOf(el: HTMLElement): { sel: string; n: number } | null {
  let sel = "";
  const href = el.getAttribute("href");
  // Sin la query: "seguir viendo" lleva la posicion en el link y cambia despues de ver un rato.
  if (href) {
    const [t, p] = [el.tagName.toLowerCase(), CSS.escape(href.split("?")[0])];
    sel = `${t}[href="${p}"], ${t}[href^="${p}\\?"]`;
  }
  else if (el.getAttribute("aria-label")) sel = `${el.tagName.toLowerCase()}[aria-label="${CSS.escape(el.getAttribute("aria-label")!)}"]`;
  if (!sel) return null;
  const n = [...document.querySelectorAll(sel)].indexOf(el);
  return n < 0 ? null : { sel, n };
}

let restoring = 0;
function settle(key: string, fresh = false) {
  cancelAnimationFrame(restoring);
  // Una vista puede tardar en llegar (el codigo de la vista, los datos; en un TV box, varios
  // segundos): se mira hasta 8 s, sin costo mientras el foco este en su lugar.
  const start = performance.now();
  const until = start + 8000;
  // `fresh`: otra pantalla en la misma ruta (login, "Who's watching?"): lo guardado no esta.
  const want = fresh ? undefined : memory.get(key);
  /** El foco quedo en "lo primero que habia" porque lo principal todavia no estaba. */
  let stopgap = false;
  let lastLook = 0;
  // Se sigue mirando hasta el final: la vista puede volver a armarse (cambia la sesion, llegan
  // los datos) y llevarse el elemento enfocado.
  const step = () => {
    if (route.view === "player" || !tv.on) return;
    const cur = document.activeElement;
    const lost = !cur || cur === document.body || !cur.isConnected || !hasBox(cur);
    if (lost) {
      const saved = want ? (document.querySelectorAll<HTMLElement>(want.sel)[want.n] ?? null) : null;
      if (saved && hasBox(saved)) focusEl(saved);
      // Lo guardado puede tardar (filas que cargan): se espera un poco antes de ir a lo primero.
      // Al principio solo lo principal (el boton Play del hero llega con los datos); despues,
      // lo primero que haya.
      else if (!want || performance.now() - start > 1000) {
        const early = performance.now() - start < 1200;
        if (focusFirst(scope(), early) && !early) stopgap = !document.activeElement?.matches(PREFERRED);
      }
    } else if (stopgap && performance.now() - lastLook > 250) {
      // Llego lo principal (en una tele el hero puede tardar mas que las filas): ahi, si nadie
      // toco nada todavia (una tecla corta esta espera). Cada 250 ms: mirar toda la pantalla
      // en cada cuadro es caro en una tele.
      lastLook = performance.now();
      if (focusFirst(scope(), true)) stopgap = false;
    }
    if (performance.now() < until) restoring = requestAnimationFrame(step);
  };
  restoring = requestAnimationFrame(step);
}

/** Cambio de pantalla sin cambio de ruta (login → inicio, "Who's watching?"): el foco a lo
 *  principal. Lo llama App.svelte. */
export function settleFocus() {
  if (tv.on) settle(route.key, true);
}

// ---- teclas

function onKey(e: KeyboardEvent) {
  // Modo TV en la web (o probando en la PC con ?tv=1): Esc hace de "atras" del control. En la app
  // de Android el boton atras ya pasa por lib/shell.svelte.ts (cierra lo abierto con Esc o vuelve
  // a la vista anterior). Se mira despues de que lo vean los componentes (un menu o un dialogo lo
  // usan para cerrarse).
  if (tv.on && !isAndroid && e.key === "Escape" && e.isTrusted && route.view !== "player") {
    setTimeout(() => !e.defaultPrevented && route.view !== "home" && history.back(), 0);
  }
  if (!tv.on || e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
  // Ya se esta usando el control: lo que buscaba el foco al cambiar de vista deja de buscarlo
  // (si no, podria llevarselo despues de que la persona se movio o abrio algo).
  if (e.isTrusted) cancelAnimationFrame(restoring);
  // Algunos controles mandan "atras" como tecla (BrowserBack / GoBack; en Samsung, 10009) en vez
  // de navegar el historial: lo mismo que el atras del navegador.
  const key = keyName(e);
  if ((key === "BrowserBack" || key === "GoBack") && !isAndroid) {
    e.preventDefault();
    // Samsung: la tecla no navega el historial por su cuenta (abajo).
    if (isTizen) return tizenBack();
    if (!onBrowserBack()) history.back();
    return;
  }
  // El reproductor tiene su propio manejo (views/Player.svelte).
  if (route.view === "player" && document.querySelector(".player-host")) return;
  const t = e.target as HTMLElement;

  if (e.key === "ContextMenu") {
    const el = document.activeElement as HTMLElement | null;
    if (!el || el === document.body) return;
    const r = el.getBoundingClientRect();
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }));
    e.preventDefault();
    return;
  }

  // Samsung: "Done" del teclado de la tele (lib/tizen.ts). En un formulario pasa al campo
  // siguiente; en el ultimo, lo envia (el login: usuario → contraseña → entrar).
  if (key === "ImeDone" && t.matches?.(TEXT)) {
    e.preventDefault();
    const form = (t as HTMLInputElement).form;
    if (!form) return;
    const fields = [...form.querySelectorAll<HTMLElement>(TEXT)].filter(hasBox);
    const next = fields[fields.indexOf(t) + 1];
    if (next) focusEl(next);
    else form.requestSubmit();
    return;
  }

  if (e.key === "Enter" && t.matches?.(TEXT) && !typing.has(t)) {
    e.preventDefault();
    openKeyboard(t as HTMLInputElement);
    return;
  }

  const d = DIRS[e.key];
  if (!d) return;
  // Escribiendo: izquierda/derecha mueven el cursor hasta el borde del texto.
  if (t.matches?.(TEXT) && typing.has(t) && (d === "left" || d === "right")) {
    const i = t as HTMLInputElement;
    const at = i.selectionStart ?? 0;
    const edge = d === "left" ? at === 0 : at === i.value.length;
    if (!edge) return;
  }
  // Un rango (volumen, tamaños): izquierda/derecha cambian el valor.
  if (t.matches?.('input[type="range"]') && (d === "left" || d === "right")) return;
  if (t.closest?.("select")) return;

  e.preventDefault();
  if (!moveFocus(d)) scrollAlong(d);
}

/** Lo ultimo enfocado, en que vista y donde estaba dibujado. Si el foco se pierde sin ir a otro
 *  lado (el boton se deshabilito mientras guardaba, o se fue del DOM), la proxima flecha sigue
 *  desde ahi y no desde el principio de la pantalla. */
let last: { el: HTMLElement; view: string; x: number; y: number } | null = null;

/** Vuelve a lo ultimo enfocado o a lo mas cercano a donde estaba (misma vista, adentro de `root`). */
function focusNear(root: HTMLElement): boolean {
  const at = last;
  if (!at || at.view !== route.key || !root.contains(at.el.isConnected ? at.el : root)) return false;
  if (at.el.isConnected && root.contains(at.el) && hasBox(at.el) && usable(at.el) && !at.el.matches(":disabled")) {
    focusEl(at.el);
    return true;
  }
  if (at.el.isConnected) {
    const r = at.el.getBoundingClientRect();
    if (r.width || r.height) [at.x, at.y] = [r.left + r.width / 2, r.top + r.height / 2];
  }
  let best: HTMLElement | null = null;
  let bestD = Infinity;
  for (const el of candidates(root)) {
    if (!usable(el)) continue;
    const r = el.getBoundingClientRect();
    const d = Math.hypot(r.left + r.width / 2 - at.x, r.top + r.height / 2 - at.y);
    if (d < bestD) [best, bestD] = [el, d];
  }
  if (!best) return false;
  focusEl(best);
  return true;
}

function onFocusIn(e: FocusEvent) {
  const el = e.target as HTMLElement;
  const r = el.getBoundingClientRect();
  last = { el, view: route.key, x: r.left + r.width / 2, y: r.top + r.height / 2 };
  if (el.closest("main")) lastMain = el;
  if (route.view !== "player") {
    const k = keyOf(el);
    if (k) memory.set(route.key, k);
  }
}

/** Web en modo TV (Firefox para Android en un TV box): el boton atras del control navega el
 *  historial. Con algo abierto (cajon, dialogo, menu, un menu del reproductor, "Who's watching?",
 *  "Add profile") lo cierra como Esc en vez de cambiar de vista; si no, es el atras de siempre
 *  (sale del reproductor, vuelve a la vista anterior). En la app de Android el boton atras ya
 *  pasa por lib/shell.svelte.ts. */
function onBrowserBack(): boolean {
  if (!tv.on || isAndroid) return false;
  if (profiles.adding && profiles.list.length) {
    profiles.adding = false;
    profiles.picking = true;
    return true;
  }
  const open = scope() !== document.body || document.querySelector('[role="listbox"], .player-host .pmenu, .player-host .side');
  if (!open) return false;
  const t = (document.activeElement as HTMLElement | null) ?? document.body;
  t.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true, cancelable: true }));
  return true;
}

/** Samsung (lib/tizen.ts): el atras del control llega como tecla y no navega solo. Igual que el
 *  boton atras de Android (lib/shell.svelte.ts): primero el Esc de siempre a lo enfocado (cierra
 *  menus, cajones, dialogos, el trailer, los menus del reproductor; en el reproductor sin nada
 *  abierto, sale). Si nadie lo usa, la vista anterior. En el inicio: si se estaba recorriendo las
 *  filas, vuelve arriba (al Play del destacado); arriba de todo, cierra la app. */
function tizenBack() {
  if (profiles.adding && profiles.list.length) {
    profiles.adding = false;
    profiles.picking = true;
    return;
  }
  const t = (document.activeElement as HTMLElement | null) ?? document.body;
  const esc = new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true, cancelable: true });
  t.dispatchEvent(esc);
  if (esc.defaultPrevented || route.view === "player") return;
  // Algo flotando que no se cierra con Esc: no se cambia de vista por debajo.
  if (scope() !== document.body) return;
  if (route.view !== "home" || profiles.picking) return back("/");
  const page = document.querySelector<HTMLElement>("main .page");
  const cur = document.activeElement;
  const top = (page?.scrollTop ?? 0) < 8 && (!cur || !cur.closest("main") || !!cur.closest(".hero"));
  if (!top) {
    page?.scrollTo({ top: 0 });
    const play = document.querySelector<HTMLElement>("main .hero .btn.primary");
    if (play) focusEl(play);
    else focusFirst();
    return;
  }
  exitTizenApp();
}

let started = false;
/** Se llama siempre al arrancar; no hace nada hasta que `tv.on`. */
export function initSpatial() {
  if (started) return;
  started = true;
  window.addEventListener("keydown", onKey);
  setBackGuard(onBrowserBack);
  document.addEventListener("focusin", onFocusIn);
  $effect.root(() => {
    $effect(() => {
      const key = route.key;
      if (tv.on) settle(key);
    });
  });
}
