// Tooltips propios en toda la interfaz (portado de Ethereon). Los del navegador (atributo `title`)
// salen enseguida y su demora no se puede cambiar. Cada `title` se pasa a `data-tip` apenas
// aparece (asi el del navegador nunca sale) y aca se muestra uno propio si el mouse se queda
// quieto un rato. Los componentes siguen usando `title={...}` como siempre.

/** Cuanto hay que dejar el mouse encima para que aparezca. */
const DELAY = 700;

let tip: HTMLDivElement | null = null;
let timer = 0;
/** El elemento bajo el mouse que tiene tooltip. */
let current: HTMLElement | null = null;

/** Pasa el `title` a `data-tip`. Un boton que solo tiene un icono conserva un nombre accesible. */
function adopt(el: Element) {
  const t = el.getAttribute("title");
  if (t === null || !(el instanceof HTMLElement)) return;
  el.removeAttribute("title");
  el.dataset.tip = t;
  if (t && !el.hasAttribute("aria-label") && !el.textContent?.trim()) el.setAttribute("aria-label", t);
}

function sweep(root: ParentNode) {
  if (root instanceof Element) adopt(root);
  root.querySelectorAll("[title]").forEach(adopt);
}

function hide() {
  clearTimeout(timer);
  tip?.remove();
  tip = null;
}

function show(el: HTMLElement) {
  const text = el.dataset.tip;
  if (!text || !el.isConnected) return;
  hide();
  tip = document.createElement("div");
  tip.className = "hover-tip";
  tip.setAttribute("role", "tooltip");
  tip.textContent = text;
  document.body.appendChild(tip);
  // Debajo del elemento y centrado; arriba si no entra, y siempre dentro de la ventana.
  const r = el.getBoundingClientRect();
  const w = tip.offsetWidth;
  const h = tip.offsetHeight;
  const left = Math.min(Math.max(6, r.left + r.width / 2 - w / 2), innerWidth - w - 6);
  const top = r.bottom + 6 + h > innerHeight - 6 ? r.top - 6 - h : r.bottom + 6;
  tip.style.left = `${left}px`;
  tip.style.top = `${Math.max(6, top)}px`;
}

let installed = false;
export function installTooltips() {
  if (installed) return;
  installed = true;
  sweep(document.body);
  new MutationObserver((list) => {
    for (const m of list) {
      if (m.type === "attributes") adopt(m.target as Element);
      else m.addedNodes.forEach((n) => n instanceof Element && sweep(n));
    }
  }).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["title"] });

  document.addEventListener(
    "pointerover",
    (e) => {
      if (e.pointerType === "touch") return;
      const el = (e.target as Element | null)?.closest?.("[data-tip]") as HTMLElement | null;
      if (el === current) return;
      hide();
      current = el;
      if (el?.dataset.tip) timer = window.setTimeout(() => show(el), DELAY);
    },
    true,
  );
  document.addEventListener(
    "pointerout",
    (e) => {
      if (current && !(e.relatedTarget instanceof Node && current.contains(e.relatedTarget))) {
        hide();
        current = null;
      }
    },
    true,
  );
  // Un clic, una tecla o la rueda lo esconden; no vuelve hasta salir y entrar de nuevo.
  for (const ev of ["pointerdown", "keydown", "wheel"]) document.addEventListener(ev, hide, true);
  window.addEventListener("blur", hide);
}
