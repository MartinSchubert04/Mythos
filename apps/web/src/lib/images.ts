// Tamaño justo de las imágenes de metahub según dónde se muestran.
//
// - Fondos (`/background/…`): metahub solo tiene 480 px (`small`) o 1920 px (`medium`, ~800 KB y
//   ~8 MB decodificado). Hasta 480 px se usa `small`; entre 480 y 1280, el server los achica y
//   cachea (`/api/media/img`, docs/API.md); más que eso, la original. En equipos flojos (lib/perf)
//   el tope es 1280 aunque la pantalla pida más.
// - Miniaturas de episodios (`episodes.metahub.space/…/w780.jpg`, tamaños de TMDB): se pide el
//   tamaño de TMDB que alcanza, sin pasar por el server.
// - Pósters: ya vienen chicos en WebP (`sized(…, "small")`), no se tocan.
//
// Si el server no tiene la ruta (una versión vieja) o falla, `retryOriginal` pasa la imagen a la
// URL original de metahub; con varios fallos seguidos y ningún acierto se deja de usar.
import { mediaUrl } from "./api";
import { lite } from "./perf";

/** Los mismos anchos que genera el server (backend/server/src/images.rs): URLs estables. */
const PROXY_W = [640, 960, 1280];
const PROXY_MAX = 1280;
const TMDB_W = [185, 300, 342, 500, 780];

/** Lo achicado → lo original (para volver atrás si falla). */
const originals = new Map<string, string>();
let proxyOn = true;
let fails = 0;
let oks = 0;

/** Pixeles reales para `css` px de ancho (más de 2× no se nota y pesa el doble). */
const px = (css: number) => Math.ceil(css * Math.min(typeof devicePixelRatio === "number" ? devicePixelRatio : 1, 2));

/** Un fondo o miniatura de metahub para mostrar a `cssWidth` px de ancho. */
export function fit(url: string | null | undefined, cssWidth: number): string | undefined {
  if (!url) return undefined;
  const need = px(cssWidth);
  // Miniatura de episodio: el tamaño de TMDB que alcanza.
  const ep = /^(https:\/\/episodes\.metahub\.space\/.+\/)w\d+(\.jpg)$/.exec(url);
  if (ep) return `${ep[1]}w${TMDB_W.find((w) => w >= need) ?? 780}${ep[2]}`;
  const bg = /^https:\/\/images\.metahub\.space\/background\/(small|medium|large)\//.exec(url);
  if (!bg) return url;
  const at = (size: string) => url.replace(/\/background\/(small|medium|large)\//, `/background/${size}/`);
  if (need <= 480) return at("small");
  const big = at("medium");
  if (!proxyOn || (need > PROXY_MAX && !lite)) return big;
  const u = mediaUrl(`/api/media/img?u=${encodeURIComponent(big)}&w=${PROXY_W.find((w) => w >= need) ?? PROXY_MAX}`);
  originals.set(u, big);
  if (originals.size > 400) originals.delete(originals.keys().next().value!);
  return u;
}

/** Ancho en px css que ocupa un fondo de 16:9 con `object-fit: cover` en una caja de w × h. */
export function coverWidth(w: number, h: number): number {
  return Math.max(w, (h * 16) / 9);
}

/** onerror de un <img>: si era una achicada por el server, prueba con la original. */
export function retryOriginal(target: EventTarget | null): boolean {
  const img = target as HTMLImageElement;
  const o = originals.get(img.src);
  if (!o) return false;
  if (!oks && ++fails >= 3) proxyOn = false;
  img.src = o;
  return true;
}

/** onload de un <img>: el server achica bien. */
export function loadedOk(target: EventTarget | null) {
  if (!oks && originals.has((target as HTMLImageElement).src)) oks++;
}
