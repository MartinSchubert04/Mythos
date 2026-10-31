// localStorage con try/catch: en una ventana privada o con los datos bloqueados puede tirar.

export function read(k: string): string | null {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
}

export function write(k: string, v: string | null) {
  try {
    if (v === null) localStorage.removeItem(k);
    else localStorage.setItem(k, v);
  } catch {}
}

export function readJson<T>(k: string, def: T): T {
  try {
    const v = read(k);
    return v ? { ...def, ...JSON.parse(v) } : def;
  } catch {
    return def;
  }
}
