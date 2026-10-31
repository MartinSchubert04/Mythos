// Retraso de cada subtitulo, recordado por archivo y pista ("hash:archivo|i3" → segundos).
// Solo en este dispositivo (localStorage con el try/catch de store.ts); se guardan los distintos
// de cero y, pasado el tope, se olvidan los mas viejos.
import { read, write } from "./store";

const KEY = "mythos.subDelays";
const MAX = 400;

export function loadDelays(): Record<string, number> {
  try {
    const raw = JSON.parse(read(KEY) ?? "{}");
    const out: Record<string, number> = {};
    if (raw && typeof raw === "object")
      for (const [k, v] of Object.entries(raw)) if (typeof v === "number" && isFinite(v) && v) out[k] = v;
    return out;
  } catch {
    return {};
  }
}

/** Guarda el mapa (lo recorta si paso el tope: las claves mantienen el orden en que se agregaron). */
export function saveDelays(map: Record<string, number>) {
  const keys = Object.keys(map);
  for (const k of keys.slice(0, Math.max(0, keys.length - MAX))) delete map[k];
  write(KEY, JSON.stringify(map));
}
