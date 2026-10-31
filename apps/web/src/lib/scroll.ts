// Recuerda donde estaba el scroll de cada pagina: volver del detalle deja el inicio donde estaba.
const saved = new Map<string, number>();

export function keepScroll(node: HTMLElement, key: string) {
  const want = saved.get(key) ?? 0;
  let tries = 0;
  let raf = 0;
  // El contenido llega de a poco (filas que cargan): se reintenta hasta llegar o 1,5 s.
  const restore = () => {
    node.scrollTop = want;
    if (Math.abs(node.scrollTop - want) > 2 && tries++ < 90) raf = requestAnimationFrame(restore);
  };
  if (want) restore();
  const stop = () => cancelAnimationFrame(raf);
  // Se guarda cuando el scroll se queda quieto: leer scrollTop en cada evento (o cuadro) fuerza
  // el layout mientras se montan filas nuevas.
  let saveT = 0;
  node.addEventListener(
    "scroll",
    () => {
      clearTimeout(saveT);
      saveT = window.setTimeout(() => {
        // Ya desmontada (se fue a otra vista): scrollTop daria 0.
        if (node.isConnected) saved.set(key, node.scrollTop);
      }, 120);
    },
    { passive: true },
  );
  // Si la persona scrollea, manda ella.
  node.addEventListener("wheel", stop, { passive: true, once: true });
  node.addEventListener("touchstart", stop, { passive: true, once: true });
  return { destroy: stop };
}
