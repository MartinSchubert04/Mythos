// Lo que una tarjeta sabe de un titulo, para que el detalle lo muestre enseguida (y el poster
// "vuele" con View Transitions) mientras llega el resto.
import type { MetaPreview } from "./types";

const map = new Map<string, MetaPreview>();

export function handOff(m: MetaPreview) {
  map.set(`${m.type}/${m.id}`, m);
  if (map.size > 200) map.delete(map.keys().next().value!);
}

export function takeOver(type: string, id: string): MetaPreview | undefined {
  return map.get(`${type}/${id}`);
}
