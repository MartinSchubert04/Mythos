// El trailer de YouTube (views/Detail.svelte): el iframe de youtube-nocookie y, en la TV, el
// control remoto. El iframe no recibe el foco del control (las flechas son de lib/spatial.svelte.ts),
// asi que en la TV se lo maneja de afuera con la API de postMessage del reproductor de YouTube
// (`enablejsapi=1`, sin cargar ningun script): centro = play/pausa, izquierda/derecha = -10/+10 s,
// atras cierra. Probado en la Samsung (Tizen 8): reproduce y obedece.
//
// Algunos trailers no se pueden ver fuera de YouTube (el dueño lo prohibe: errores 101 y 150): la
// pagina de YouTube muestra "Mirar en YouTube", que con el control no se puede apretar. `blocked`
// lo avisa para mostrar un cartel propio.

const ORIGIN = "https://www.youtube-nocookie.com";

export function trailerUrl(id: string): string {
  return `${ORIGIN}/embed/${encodeURIComponent(id)}?autoplay=1&rel=0&enablejsapi=1&origin=${encodeURIComponent(location.origin)}`;
}

export type TrailerCtl = {
  /** YouTube no deja verlo aca (o no existe). */
  readonly blocked: boolean;
  toggle(): void;
  /** Salto relativo en segundos. */
  skip(s: number): void;
  destroy(): void;
};

/** Escucha al reproductor del iframe y lo maneja. `onchange` cuando cambia `blocked`. */
export function trailerCtl(frame: HTMLIFrameElement, onchange: () => void): TrailerCtl {
  let state = -1; // -1 sin empezar, 0 termino, 1 reproduciendo, 2 pausa, 3 cargando, 5 listo
  let time = 0;
  let blocked = false;
  const send = (func: string, args: unknown[] = []) =>
    frame.contentWindow?.postMessage(JSON.stringify({ event: "command", func, args }), ORIGIN);
  // Sin "listening" el reproductor no manda su estado.
  const listen = () => frame.contentWindow?.postMessage(JSON.stringify({ event: "listening", id: "mythos", channel: "widget" }), ORIGIN);
  frame.addEventListener("load", listen);

  function onMessage(e: MessageEvent) {
    if (e.origin !== ORIGIN || e.source !== frame.contentWindow || typeof e.data !== "string") return;
    let m: { event?: string; info?: unknown };
    try {
      m = JSON.parse(e.data);
    } catch {
      return;
    }
    if (m.event === "onError") {
      // 2: id invalido, 5: el navegador no puede, 100: no existe, 101/150: no se puede embeber.
      blocked = true;
      onchange();
    } else if (m.event === "onStateChange" && typeof m.info === "number") state = m.info;
    else if (m.event === "infoDelivery" && m.info && typeof m.info === "object") {
      const i = m.info as { playerState?: number; currentTime?: number };
      if (typeof i.playerState === "number") state = i.playerState;
      if (typeof i.currentTime === "number") time = i.currentTime;
    }
  }
  window.addEventListener("message", onMessage);

  return {
    get blocked() {
      return blocked;
    },
    toggle() {
      send(state === 1 || state === 3 ? "pauseVideo" : "playVideo");
    },
    skip(s: number) {
      time = Math.max(0, time + s);
      send("seekTo", [time, true]);
    },
    destroy() {
      frame.removeEventListener("load", listen);
      window.removeEventListener("message", onMessage);
    },
  };
}
