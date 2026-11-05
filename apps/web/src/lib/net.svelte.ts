// Conexion con el server. Si un pedido no llega (server caido, sin red), se avisa arriba y se
// prueba /api/health cada pocos segundos; al volver, la vista actual se recarga sola.
import { serverBase } from "./platform";

export const net = $state({
  down: false,
  /** Cambia cada vez que vuelve la conexion: App lo usa para recargar la vista. */
  epoch: 0,
  checking: false,
});

let timer = 0;

/** Lo registra lib/servers.svelte.ts: con varias direcciones del server, probarlas todas (y pasar
 *  a la que conteste) antes de seguir esperando a la de ahora. */
let recover: () => Promise<boolean> = async () => false;
export function setRecover(f: () => Promise<boolean>) {
  recover = f;
}

export function markDown() {
  if (net.down) return;
  net.down = true;
  schedule(3000);
}

export function markUp() {
  if (!net.down) return;
  net.down = false;
  net.epoch++;
  clearTimeout(timer);
}

function schedule(ms: number) {
  clearTimeout(timer);
  timer = window.setTimeout(check, ms);
}

/** Prueba ya (el boton Retry) y sigue probando cada 5 s mientras no responda. */
export async function check() {
  clearTimeout(timer);
  net.checking = true;
  try {
    if (await recover()) return markUp();
    const r = await fetch(serverBase() + "/api/health", { signal: AbortSignal.timeout(5000) });
    if (r.ok) return markUp();
  } catch {
  } finally {
    net.checking = false;
  }
  if (net.down) schedule(5000);
}

/** Un server que no contesta ni rechaza (apagado, o la red descarta los paquetes) no da error: el
 *  navegador espera minutos y la interfaz no avisaba nada (probado en la tele: la vista nueva no
 *  llegaba nunca y no aparecia la barra). Si un pedido o el codigo de una vista tarda mas de 8 s, se
 *  prueba /api/health con 5 s de tope; si tampoco contesta, caido. Lo lento de verdad (buscar
 *  streams, preparar un torrent) sigue esperando: el server contesta /api/health enseguida. */
let probing = false;
export function watchSlow<T>(p: Promise<T>, ms = 8000): Promise<T> {
  const id = window.setTimeout(probe, ms);
  const done = () => clearTimeout(id);
  p.then(done, done);
  return p;
}
async function probe() {
  if (net.down || probing) return;
  probing = true;
  try {
    const r = await fetch(serverBase() + "/api/health", { signal: AbortSignal.timeout(5000) });
    if ([502, 503, 504].includes(r.status)) markDown();
  } catch {
    markDown();
  } finally {
    probing = false;
  }
}

// El navegador avisa cuando vuelve la red: probar enseguida.
window.addEventListener("online", () => net.down && check());
