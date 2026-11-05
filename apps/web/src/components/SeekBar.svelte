<script lang="ts">
  // Barra de progreso del reproductor: lo visto, lo bufferizado (en tramos), y al pasar el mouse
  // el tiempo de ese punto. Se arrastra (la posicion se aplica al soltar) y anda con flechas.
  // En TV, con el foco encima se nota de lejos (mas gruesa, con el anillo del acento y un punto
  // grande) y el tiempo queda arriba del punto: izquierda/derecha lo mueven (el reproductor
  // arma el salto y lo pasa en `time`).
  // Con `preview` (las miniaturas del server, docs/API.md) el tiempo de arriba lleva la imagen de
  // ese momento si ya esta; si no, solo el tiempo.
  import { clock } from "../lib/format";
  import { tv } from "../lib/tv.svelte";
  import type { ThumbsIndex } from "../lib/types";

  let {
    time,
    duration,
    buffered,
    downloaded = [],
    onseek,
    ondrag,
    preview = null,
  }: {
    time: number;
    duration: number;
    /** Tramos que tiene el video en memoria, en segundos reales. */
    buffered: [number, number][];
    /** Tramos ya bajados en el server (mas tenues, debajo): el navegador puede tirar partes de
     *  su memoria y dejar huecos, pero eso ya esta y se pide al instante. */
    downloaded?: [number, number][];
    onseek: (t: number) => void;
    ondrag?: (dragging: boolean) => void;
    /** Miniaturas para la vista previa (null: sin). */
    preview?: (ThumbsIndex & { url: (i: number) => string }) | null;
  } = $props();

  let el = $state<HTMLDivElement>();
  let hover = $state<number | null>(null);
  let drag = $state<number | null>(null);
  /** TV: el foco del control esta en la barra. */
  let focused = $state(false);

  const shown = $derived(drag ?? time);
  /** Donde va el tiempo de arriba: el mouse, el arrastre o (TV, con el foco) la posicion. */
  const tipAt = $derived(drag ?? hover ?? (tv.on && focused ? shown : null));
  /** La miniatura del tramo de `tipAt`, si ya esta. */
  const thumb = $derived.by(() => {
    if (!preview || tipAt === null || preview.interval_s <= 0) return null;
    const i = Math.min(preview.count - 1, Math.max(0, Math.floor(tipAt / preview.interval_s)));
    return preview.ready[i] === "1" ? preview.url(i) : null;
  });
  const pct = (t: number) => (duration > 0 ? Math.max(0, Math.min(100, (t / duration) * 100)) : 0);

  function at(e: PointerEvent) {
    const r = el!.getBoundingClientRect();
    return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * duration;
  }

  function down(e: PointerEvent) {
    if (e.button !== 0 || !duration) return;
    el!.setPointerCapture(e.pointerId);
    drag = at(e);
    ondrag?.(true);
  }
  function move(e: PointerEvent) {
    if (!duration) return;
    hover = at(e);
    if (drag !== null) drag = hover;
  }
  function up() {
    if (drag === null) return;
    const t = drag;
    drag = null;
    ondrag?.(false);
    onseek(t);
  }

  function key(e: KeyboardEvent) {
    // En TV las flechas las maneja el reproductor (saltos de 10 s que se aceleran manteniendo).
    if (tv.on) return;
    const step = e.shiftKey ? 30 : 5;
    if (e.key === "ArrowRight") onseek(Math.min(duration, time + step));
    else if (e.key === "ArrowLeft") onseek(Math.max(0, time - step));
    else if (e.key === "Home") onseek(0);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<div
  class="seek"
  class:dragging={drag !== null}
  bind:this={el}
  role="slider"
  tabindex="0"
  aria-label="Seek"
  aria-valuemin={0}
  aria-valuemax={Math.round(duration)}
  aria-valuenow={Math.round(shown)}
  aria-valuetext={clock(shown)}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  onpointerleave={() => drag === null && (hover = null)}
  onkeydown={key}
  onfocus={() => (focused = true)}
  onblur={() => (focused = false)}
>
  <div class="rail">
    {#each downloaded as [a, b], i (i)}
      <div class="buf dl" style="transform: translateX({pct(a)}%) scaleX({(pct(b) - pct(a)) / 100})"></div>
    {/each}
    {#each buffered as [a, b], i (i)}
      <div class="buf" style="transform: translateX({pct(a)}%) scaleX({(pct(b) - pct(a)) / 100})"></div>
    {/each}
    {#if hover !== null}<div class="hov" style="width: {pct(hover)}%"></div>{/if}
    <!-- Lo visto y el pulgar se mueven con transform (cambia en cada cuadro: nada de layout). -->
    <div class="played" style="transform: scaleX({pct(shown) / 100})"></div>
  </div>
  <div class="thumb-track" style="transform: translateX({pct(shown)}%)"><div class="thumb"></div></div>
  {#if tipAt !== null}
    {#if thumb && preview}
      <div class="tip pv" style="left: clamp(calc(var(--pw) / 2 + 4px), {pct(tipAt)}%, calc(100% - var(--pw) / 2 - 4px)); --ar: {preview.width} / {preview.height}">
        <img src={thumb} alt="" draggable="false" />
        <span>{clock(tipAt)}</span>
      </div>
    {:else}
      <div class="tip" style="left: clamp(28px, {pct(tipAt)}%, calc(100% - 28px))">{clock(tipAt)}</div>
    {/if}
  {/if}
</div>

<style>
  .seek {
    position: relative;
    height: 22px;
    display: flex;
    align-items: center;
    cursor: pointer;
    touch-action: none;
  }
  .rail {
    position: relative;
    width: 100%;
    height: 7px;
    border-radius: 4px;
    background: var(--media-line);
    overflow: hidden;
    /* Se engrosa con scaleY (sin layout). */
    transform: scaleY(0.58);
    transition: transform 0.15s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .seek:hover .rail,
  .seek:focus-visible .rail,
  .dragging .rail {
    transform: none;
  }
  .buf,
  .hov,
  .played {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
  }
  .buf {
    width: 100%;
    transform-origin: left center;
    background: color-mix(in srgb, var(--on-media) 32%, transparent);
  }
  .dl {
    background: color-mix(in srgb, var(--on-media) 16%, transparent);
  }
  .hov {
    background: color-mix(in srgb, var(--on-media) 22%, transparent);
  }
  .played {
    width: 100%;
    transform-origin: left center;
    background: var(--primary-bg);
    box-shadow: 0 0 12px color-mix(in srgb, var(--accent) 60%, transparent);
  }
  .thumb-track {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .thumb {
    position: absolute;
    left: 0;
    top: 50%;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--accent);
    border: 2px solid var(--on-media);
    transform: translate(-50%, -50%) scale(0);
    transition: transform 0.18s cubic-bezier(0.34, 1.56, 0.64, 1);
    pointer-events: none;
  }
  .seek:hover .thumb,
  .seek:focus-visible .thumb,
  .dragging .thumb {
    transform: translate(-50%, -50%) scale(1);
  }
  .seek:focus-visible {
    outline: none;
  }
  .tip {
    position: absolute;
    bottom: 22px;
    transform: translateX(-50%);
    padding: 3px 8px;
    border-radius: 6px;
    font: 600 12px var(--mono);
    font-variant-numeric: tabular-nums;
    color: var(--on-media);
    background: var(--media-scrim);
    backdrop-filter: blur(8px);
    border: 1px solid var(--media-line);
    pointer-events: none;
    white-space: nowrap;
  }
  /* TV: con el foco, a 3 m tiene que saltar a la vista. Sin animar (modo liviano de la tele). */
  :global(html[data-tv]) .seek {
    height: 34px;
  }
  :global(html[data-tv]) .seek:focus-visible .rail {
    height: 10px;
    border-radius: 5px;
    box-shadow: 0 0 0 2px var(--accent);
    transition: none;
  }
  :global(html[data-tv]) .seek:focus-visible .thumb {
    width: 22px;
    height: 22px;
    border-width: 3px;
    box-shadow: 0 0 0 3px var(--accent);
    transition: none;
  }
  :global(html[data-tv]) .tip {
    bottom: 32px;
    padding: 4px 10px;
    font-size: 15px;
    border-color: var(--accent);
  }
  /* Con miniatura: la imagen arriba y el tiempo abajo, en la misma tarjeta. Ancho fijo (el alto
     sale de la relacion de aspecto): la tarjeta no salta al cambiar de imagen. */
  .seek {
    --pw: 168px;
  }
  .tip.pv {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 4px 4px 3px;
  }
  .pv img {
    display: block;
    width: var(--pw);
    aspect-ratio: var(--ar);
    border-radius: 4px;
    object-fit: cover;
    background: var(--media-bg);
  }
  @media (max-width: 700px) {
    .seek {
      --pw: 128px;
    }
  }
  :global(html[data-tv]) .seek {
    --pw: 200px;
  }
  :global([data-theme="athanor"]) .rail,
  :global([data-theme="athanor"]) .thumb {
    border-radius: 0;
  }
</style>
