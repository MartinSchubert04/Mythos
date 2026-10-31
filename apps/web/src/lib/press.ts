// Accion `use:press`: clic derecho o mantener apretado con el dedo (550 ms) llama a `cb` con
// el punto, para abrir un menu ahi. El toque que sigue a mantener apretado no cuenta como clic
// (si no, se abriria el menu y a la vez se elegiria lo que estaba debajo).
type Cb = ((x: number, y: number) => void) | undefined;

export function press(node: HTMLElement, cb: Cb) {
  let f = cb;
  let timer = 0;
  let fired = false;
  let touch = false;
  let x0 = 0;
  let y0 = 0;
  const down = (e: PointerEvent) => {
    fired = false;
    touch = e.pointerType === "touch";
    if (!f || !touch) return;
    x0 = e.clientX;
    y0 = e.clientY;
    clearTimeout(timer);
    timer = window.setTimeout(() => {
      fired = true;
      f?.(x0, y0);
    }, 550);
  };
  const move = (e: PointerEvent) => {
    if (Math.hypot(e.clientX - x0, e.clientY - y0) > 10) clearTimeout(timer);
  };
  const cancel = () => clearTimeout(timer);
  const context = (e: MouseEvent) => {
    if (!f) return;
    e.preventDefault();
    clearTimeout(timer);
    // En Android mantener apretado tambien manda contextmenu: una sola vez.
    if (touch && fired) return;
    if (touch) fired = true;
    f(e.clientX, e.clientY);
  };
  const click = (e: MouseEvent) => {
    if (!fired) return;
    fired = false;
    e.preventDefault();
    e.stopImmediatePropagation();
  };
  node.addEventListener("pointerdown", down);
  node.addEventListener("pointermove", move);
  node.addEventListener("pointerup", cancel);
  node.addEventListener("pointercancel", cancel);
  node.addEventListener("contextmenu", context);
  node.addEventListener("click", click, true);
  return {
    update(cb: Cb) {
      f = cb;
    },
    destroy() {
      clearTimeout(timer);
      node.removeEventListener("pointerdown", down);
      node.removeEventListener("pointermove", move);
      node.removeEventListener("pointerup", cancel);
      node.removeEventListener("pointercancel", cancel);
      node.removeEventListener("contextmenu", context);
      node.removeEventListener("click", click, true);
    },
  };
}
