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
    const r = await fetch(serverBase() + "/api/health", { signal: AbortSignal.timeout(5000) });
    if (r.ok) return markUp();
  } catch {
  } finally {
    net.checking = false;
  }
  if (net.down) schedule(5000);
}

// El navegador avisa cuando vuelve la red: probar enseguida.
window.addEventListener("online", () => net.down && check());
