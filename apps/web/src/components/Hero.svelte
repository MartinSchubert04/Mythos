<script lang="ts">
  // El destacado del inicio: fondo grande, logo o titulo, datos y Play / More info. Rota cada
  // 8 s con un fundido cruzado (y un zoom lento del fondo); se pausa con la pestaña oculta. En TV
  // rota solo si el control lleva un rato quieto: mientras se recorren las filas, cambiar un fondo
  // de pantalla entera le cuesta a la tele los cuadros de las filas.
  import { Info, Play, Star } from "@lucide/svelte";
  import { onMount } from "svelte";
  import { sized } from "../lib/format";
  import { handOff } from "../lib/handoff";
  import { coverWidth, fit, retryOriginal } from "../lib/images";
  import { fadeQuick, fadeSlow, rise } from "../lib/motion";
  import { go, loadView } from "../lib/router.svelte";
  import { tv } from "../lib/tv.svelte";
  import type { MetaPreview } from "../lib/types";

  let { items, playHref }: { items: MetaPreview[] | null; playHref: (m: MetaPreview) => string } = $props();

  let index = $state(0);
  const cur = $derived(items?.[index % (items?.length || 1)]);
  let logoOk = $state<Record<string, boolean>>({});
  let bgReady = $state<Record<string, boolean>>({});
  let timer = 0;
  // El fondo cubre todo el ancho y clamp(400px, 66vh, 720px) de alto: el tamaño justo para eso
  // (se calcula una vez; un cambio de ventana no vale otra descarga).
  const bgWidth = coverWidth(innerWidth, Math.min(720, Math.max(400, innerHeight * 0.66)));
  const bgSrc = (m: MetaPreview) => fit(sized(m.background, "medium"), bgWidth);
  /** Con el mouse (o el foco) sobre el texto o los botones no rota (se esta por apretar algo). */
  let holding = false;
  /** TV: la ultima tecla del control. */
  let lastKey = 0;

  function schedule() {
    clearTimeout(timer);
    if (!items || items.length < 2) return;
    timer = window.setTimeout(() => {
      if (document.hidden || holding || (tv.on && performance.now() - lastKey < 8000)) return schedule();
      // La siguiente imagen ya tiene que estar bajada: si no, se espera un poco mas.
      const n = items![(index + 1) % items!.length];
      if (n.background && !bgReady[n.id]) {
        preload(n);
        return (timer = window.setTimeout(schedule, 1500));
      }
      index = (index + 1) % items!.length;
      schedule();
    }, 8000);
  }

  function preload(m?: MetaPreview) {
    if (!m?.background || bgReady[m.id] !== undefined) return;
    bgReady[m.id] = false;
    const im = new Image();
    im.onload = () => (bgReady[m.id] = true);
    im.onerror = () => (bgReady[m.id] = true);
    im.src = bgSrc(m)!;
  }

  $effect(() => {
    if (!items?.length) return;
    preload(items[index]);
    preload(items[(index + 1) % items.length]);
  });

  $effect(() => {
    void items;
    schedule();
    return () => clearTimeout(timer);
  });

  function pick(i: number) {
    index = i;
    schedule();
  }

  function info() {
    if (!cur) return;
    handOff(cur);
    go(`/title/${cur.type}/${cur.id}`);
  }

  onMount(() => {
    loadView("detail").catch(() => {});
  });
</script>

<svelte:window onkeydown={() => (lastKey = performance.now())} />

<section class="hero" aria-roledescription="carousel">
  {#if cur}
    {#key cur.id}
      <div class="bg" in:fadeSlow={{ duration: 900 }} out:fadeSlow={{ duration: 900 }}>
        {#if cur.background}
          <img class="kb" src={bgSrc(cur)} alt="" fetchpriority="high" decoding="async" onerror={(e) => retryOriginal(e.currentTarget)} />
        {/if}
      </div>
    {/key}
  {:else}
    <div class="bg skel"></div>
  {/if}
  <div class="shade"></div>

  <!-- Con el foco en los botones (control remoto de la TV) tampoco: rotar se llevaria el foco. -->
  <div
    class="content"
    role="presentation"
    onpointerenter={() => (holding = true)}
    onpointerleave={() => (holding = false)}
    onfocusin={() => (holding = true)}
    onfocusout={() => (holding = false)}
  >
    {#if cur}
      {#key cur.id}
        <div class="text" in:rise={{ y: 14, delay: 250 }} out:fadeQuick>
          {#if cur.logo && logoOk[cur.id] !== false}
            <img
              class="logo"
              src={sized(cur.logo, "medium")}
              alt={cur.name}
              onerror={() => (logoOk[cur.id] = false)}
              onload={() => (logoOk[cur.id] = true)}
              class:shown={logoOk[cur.id]}
            />
          {/if}
          {#if !cur.logo || logoOk[cur.id] === false}
            <h1 class="display-title title">{cur.name}</h1>
          {/if}
          <div class="facts">
            {#if cur.imdb_rating}<span class="imdb"><Star size={13} fill="currentColor" />{cur.imdb_rating}</span>{/if}
            {#if cur.year}<span>{cur.year}</span>{/if}
            <span class="kind">{cur.type === "movie" ? "Movie" : "Series"}</span>
            {#if cur.genres?.length}<span class="genres">{cur.genres.slice(0, 3).join(" · ")}</span>{/if}
          </div>
          {#if cur.description}<p class="desc">{cur.description}</p>{/if}
          <div class="actions">
            <button class="btn primary lg" onclick={() => go(playHref(cur))}><Play size={17} fill="currentColor" />Play</button>
            <button class="btn glass lg" onclick={info}><Info size={17} />More info</button>
          </div>
        </div>
      {/key}
    {:else}
      <div class="text">
        <div class="skel" style="width: min(380px, 70%); height: 70px; margin-bottom: 18px"></div>
        <div class="skel line" style="width: 220px"></div>
        <div class="skel line" style="width: min(480px, 80%); margin-top: 16px"></div>
        <div class="skel line" style="width: min(420px, 70%); margin-top: 8px"></div>
      </div>
    {/if}
  </div>

  {#if items && items.length > 1}
    <div class="dots" role="tablist">
      {#each items as m, i (m.id)}
        <button
          role="tab"
          aria-selected={i === index}
          aria-label={m.name}
          class:on={i === index}
          onclick={() => pick(i)}
          onpointerenter={() => preload(m)}><span></span></button
        >
      {/each}
    </div>
  {/if}
</section>

<style>
  .hero {
    position: relative;
    height: clamp(400px, 66vh, 720px);
    overflow: hidden;
    background: var(--media-bg);
    isolation: isolate;
  }
  .bg {
    position: absolute;
    inset: 0;
  }
  .bg img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 22%;
  }
  /* Zoom lento (Ken Burns): da vida sin distraer. En equipos flojos (lib/perf.ts) queda quieto:
     escalar un fondo de pantalla entera en cada cuadro es caro para esas GPUs. */
  .kb {
    animation: kb 12s ease-out both;
    will-change: transform;
  }
  :global([data-lite]) .kb {
    animation: none;
    will-change: auto;
  }
  @keyframes kb {
    from {
      transform: scale(1.08);
    }
    to {
      transform: scale(1);
    }
  }
  /* El hero es siempre de cine (oscuro, texto claro) en todos los temas: los logos de las
     peliculas son claros. Abajo funde corto con el fondo de la app, donde empiezan las filas. */
  .shade {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(0deg, var(--bg) 0%, color-mix(in srgb, var(--bg) 55%, transparent) 9%, transparent 22%),
      linear-gradient(0deg, color-mix(in srgb, var(--media-bg) 70%, transparent) 0%, transparent 50%),
      linear-gradient(90deg, color-mix(in srgb, var(--media-bg) 80%, transparent) 0%, color-mix(in srgb, var(--media-bg) 40%, transparent) 42%, transparent 72%);
  }
  .content {
    position: absolute;
    left: var(--gutter);
    right: var(--gutter);
    bottom: clamp(80px, 14vh, 130px);
    color: var(--on-media);
    --shadow: var(--media-scrim);
  }
  /* El texto viejo y el nuevo ocupan la misma celda mientras uno sale y el otro entra. */
  .content {
    display: grid;
    align-items: end;
  }
  .text {
    grid-area: 1 / 1;
    max-width: 560px;
  }
  .logo {
    display: block;
    max-width: min(420px, 80%);
    max-height: 140px;
    object-fit: contain;
    object-position: left bottom;
    margin-bottom: 16px;
    filter: drop-shadow(0 6px 22px var(--shadow));
    opacity: 0;
    transition: opacity 0.5s ease;
  }
  .logo.shown {
    opacity: 1;
  }
  .title {
    font-size: clamp(30px, 4.6vw, 58px);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    margin-bottom: 14px;
    text-shadow: 0 4px 30px var(--shadow);
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
    font-size: 13.5px;
    font-weight: 560;
    color: var(--on-media-2);
  }
  .imdb {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--on-media);
    font-weight: 700;
  }
  .imdb :global(svg) {
    color: var(--warn);
  }
  .kind {
    padding: 1px 7px;
    border: 1px solid var(--media-line);
    border-radius: 5px;
    font-size: 11.5px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .desc {
    margin: 12px 0 0;
    font-size: 14.5px;
    line-height: 1.55;
    color: var(--on-media-2);
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    text-shadow: 0 2px 12px var(--shadow);
  }
  .actions {
    display: flex;
    gap: 10px;
    margin-top: 22px;
    flex-wrap: wrap;
  }
  .dots {
    position: absolute;
    right: var(--gutter);
    bottom: clamp(84px, 14vh, 134px);
    display: flex;
  }
  .dots button {
    padding: 8px 2px;
  }
  /* Solo transform y color: el punto activo se estira con scaleX (sin recalcular layout). */
  .dots span {
    display: block;
    width: 34px;
    height: 3px;
    border-radius: 3px;
    background: var(--media-line);
    transform: scaleX(0.53);
    transform-origin: center;
    transition:
      transform 0.35s cubic-bezier(0.2, 0.8, 0.2, 1),
      background 0.3s;
  }
  /* Cada punto mide 34 px y se ve de 18: se superponen un poco para quedar juntos. */
  .dots button + button {
    margin-left: -12px;
  }
  .dots button:hover span {
    background: var(--on-media-2);
  }
  .dots .on span {
    transform: none;
    background: var(--on-media);
  }
  @media (max-width: 700px) {
    .dots {
      display: none;
    }
    .desc {
      -webkit-line-clamp: 2;
      line-clamp: 2;
    }
  }
</style>
