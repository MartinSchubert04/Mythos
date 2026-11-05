// Animaciones de la interfaz (portado de Ethereon). Todas pasan por aca para respetar "reducir
// animaciones": la del sistema o la de Settings (data-reduce-motion en <html>). Con eso duran 0.
import { backOut, cubicOut } from "svelte/easing";
import { fade, fly, scale, slide } from "svelte/transition";

export const reduceMotion = () =>
  (typeof document !== "undefined" && document.documentElement.hasAttribute("data-reduce-motion")) ||
  (typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches);
export const ms = (n: number) => (reduceMotion() ? 0 : n);

/** Entrada de una vista o una tarjeta: sube unos px y aparece. */
export const rise = (node: Element, o: { delay?: number; y?: number; skip?: boolean } = {}) =>
  fly(node, { y: o.y ?? 8, duration: o.skip ? 0 : ms(220), delay: o.skip ? 0 : ms(o.delay ?? 0), easing: cubicOut });

/** Cajon lateral: entra desde la derecha con un poco de resorte. */
export const slideIn = (node: Element) => fly(node, { x: 48, duration: ms(260), easing: backOut, opacity: 0 });
export const slideOut = (node: Element) => fly(node, { x: 32, duration: ms(160), easing: cubicOut });

/** Dialogos y desplegables: crecen desde un poco mas chico. */
export const pop = (node: Element, o: { start?: number } = {}) =>
  scale(node, { start: o.start ?? 0.95, duration: ms(160), easing: cubicOut });

/** Un paso de un menu por pasos (idiomas → versiones): entra de costado, segun para donde se va
 *  (dir 1 = hacia adentro, -1 = volver). Solo entrada: el paso anterior se va sin animar. */
export const step = (node: Element, o: { dir?: number } = {}) =>
  fly(node, { x: 22 * (o.dir ?? 1), duration: ms(190), easing: cubicOut });

export const fadeQuick =(node: Element) => fade(node, { duration: ms(140) });
/** Fundido largo: el crossfade del hero y la pantalla de carga del reproductor. */
export const fadeSlow = (node: Element, o: { duration?: number } = {}) => fade(node, { duration: ms(o.duration ?? 700) });

/** Avisos: entran desde abajo. */
export const toastIn = (node: Element) => fly(node, { y: 16, duration: ms(240), easing: backOut });

export const flipMs = () => ms(200);

/** Abrir o cerrar una seccion desplegable. */
export const expand = (node: Element) => slide(node, { duration: ms(240), easing: cubicOut });

/** Desplazamiento suave (o salto, con reducir animaciones). En TV (data-tv), salto: el
 *  desplazamiento suave de una fila o de la pagina redibuja en cada cuadro y en una tele (pagina
 *  al doble, raster por CPU) eran la mayoria de los cuadros perdidos al moverse con el control. */
export const scrollBehavior = (): ScrollBehavior =>
  reduceMotion() || (typeof document !== "undefined" && document.documentElement.hasAttribute("data-tv")) ? "auto" : "smooth";
