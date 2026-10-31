<script lang="ts" generics="T">
  // Fila horizontal con scroll suave y flechas. Carga sus datos recien cuando se acerca a la
  // pantalla (las de abajo no piden nada hasta que se baja). Mientras, esqueletos con shimmer.
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";
  import { tick, type Snippet } from "svelte";
  import { scrollBehavior } from "../lib/motion";

  let {
    title,
    load,
    items: given,
    card,
    wide = false,
    hideEmpty = true,
  }: {
    title: string;
    load?: () => Promise<T[]>;
    items?: T[];
    card: Snippet<[T, number]>;
    /** Tarjetas apaisadas (seguir viendo). */
    wide?: boolean;
    hideEmpty?: boolean;
  } = $props();

  let fetched = $state<T[] | null>(null);
  /** Ya se acerco a la pantalla (y se esta cargando): recien ahi brillan los esqueletos. */
  let near = $state(false);
  let failed = $state(false);
  const items = $derived(given ?? fetched);
  let root = $state<HTMLElement>();
  let track = $state<HTMLDivElement>();
  let canLeft = $state(false);
  let canRight = $state(false);
  // Los catalogos traen ~50 titulos por fila: se dibujan de a tandas a medida que se scrollea a
  // la derecha (un centinela al final avisa), asi el inicio no arma cientos de tarjetas de golpe.
  const BATCH = 16;
  let shown = $state(BATCH);
  const visible = $derived(items && items.length > shown ? items.slice(0, shown) : items);
  let sentinel = $state<HTMLDivElement>();

  $effect(() => {
    if (!sentinel || !track) return;
    const io = new IntersectionObserver((es) => es.some((e) => e.isIntersecting) && (shown += BATCH), {
      root: track,
      rootMargin: "0px 100% 0px 0px",
    });
    io.observe(sentinel);
    return () => io.disconnect();
  });

  $effect(() => {
    if (!load || !root) return;
    const io = new IntersectionObserver(
      (es) => {
        if (!es.some((e) => e.isIntersecting)) return;
        io.disconnect();
        near = true;
        load!()
          .then((l) => (fetched = l))
          .catch(() => ((failed = true), (fetched = [])));
      },
      { rootMargin: "400px 0px" },
    );
    io.observe(root);
    return () => io.disconnect();
  });

  function sync() {
    if (!track) return;
    canLeft = track.scrollLeft > 4;
    canRight = track.scrollLeft + track.clientWidth < track.scrollWidth - 4;
  }
  // Al scrollear se mide una vez por cuadro (leer scrollWidth en cada evento fuerza el layout).
  let syncRaf = 0;
  function onScroll() {
    if (!syncRaf) syncRaf = requestAnimationFrame(() => ((syncRaf = 0), sync()));
  }

  $effect(() => {
    void items;
    if (!track) return;
    sync();
    const ro = new ResizeObserver(sync);
    ro.observe(track);
    return () => ro.disconnect();
  });

  // Teclado: flechas izquierda/derecha entre tarjetas de la fila; arriba/abajo a la fila vecina
  // (a la tarjeta en la misma posicion).
  function onKey(e: KeyboardEvent) {
    if (!track || !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) return;
    const cards = (t: Element) => [...t.querySelectorAll<HTMLElement>(":scope > a")];
    const mine = cards(track);
    const i = mine.indexOf((document.activeElement as HTMLElement)?.closest("a") as HTMLElement);
    if (i < 0) return;
    let next: HTMLElement | undefined;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      next = mine[i + (e.key === "ArrowRight" ? 1 : -1)];
      // Al final de lo dibujado: la tanda siguiente y despues el foco.
      if (!next && e.key === "ArrowRight" && items && shown < items.length) {
        e.preventDefault();
        shown += BATCH;
        tick().then(() => {
          const n = track && cards(track)[i + 1];
          n?.focus({ preventScroll: true });
          n?.scrollIntoView({ behavior: scrollBehavior(), block: "nearest", inline: "nearest" });
        });
        return;
      }
    } else {
      const shelves = [...document.querySelectorAll(".shelf .track")];
      const other = shelves[shelves.indexOf(track) + (e.key === "ArrowDown" ? 1 : -1)];
      if (other) {
        const theirs = cards(other);
        next = theirs[Math.min(i, theirs.length - 1)];
        if (!next) {
          // La fila de al lado todavia no cargo: se la trae a la vista y se enfoca cuando llega.
          e.preventDefault();
          other.scrollIntoView({ behavior: scrollBehavior(), block: "center" });
          const mo = new MutationObserver(() => {
            const l = cards(other);
            if (!l.length) return;
            mo.disconnect();
            l[Math.min(i, l.length - 1)].focus({ preventScroll: true });
          });
          mo.observe(other, { childList: true });
          setTimeout(() => mo.disconnect(), 4000);
          return;
        }
      }
    }
    if (!next) return;
    e.preventDefault();
    next.focus({ preventScroll: true });
    next.scrollIntoView({ behavior: scrollBehavior(), block: "nearest", inline: "nearest" });
  }

  $effect(() => {
    const t = track;
    if (!t) return;
    t.addEventListener("keydown", onKey);
    return () => t.removeEventListener("keydown", onKey);
  });

  function page(dir: number) {
    if (!track) return;
    track.scrollBy({ left: dir * track.clientWidth * 0.85, behavior: scrollBehavior() });
  }
</script>

{#if !(hideEmpty && items && items.length === 0)}
  <section class="shelf" class:wide bind:this={root}>
    <h2 class="row-title">{title}</h2>
    <div class="viewport">
      <button class="arrow left" class:show={canLeft} onclick={() => page(-1)} aria-label="Scroll left" tabindex="-1">
        <ChevronLeft size={22} />
      </button>
      <div class="track" bind:this={track} onscroll={onScroll}>
        {#if visible}
          {#each visible as it, i (i)}
            {@render card(it, i)}
          {/each}
          {#if items && shown < items.length}<div class="more" bind:this={sentinel} aria-hidden="true"></div>{/if}
        {:else}
          <!-- Las filas lejanas, quietas: un brillo animado dentro de una fila que no se dibuja
               (content-visibility) obliga a recalcular estilos en cada cuadro. -->
          {#each Array(wide ? 4 : 8) as _, i (i)}
            <div class="ph" class:still={!near && !!load}>
              <div class="skel frame"></div>
              <div class="skel line" style="width: {55 + ((i * 17) % 35)}%"></div>
            </div>
          {/each}
        {/if}
      </div>
      <button class="arrow right" class:show={canRight} onclick={() => page(1)} aria-label="Scroll right" tabindex="-1">
        <ChevronRight size={22} />
      </button>
    </div>
    {#if failed}<div class="faint err">Couldn't load this row.</div>{/if}
  </section>
{/if}

<style>
  .shelf {
    --card-w: clamp(118px, 11vw, 168px);
    margin-bottom: 26px;
    /* Las filas fuera de pantalla no se dibujan ni se recalculan (el inicio tiene diez): el
       tamaño estimado es el de una fila de posters y despues se recuerda el real. */
    content-visibility: auto;
    contain-intrinsic-size: auto calc(var(--card-w) * 1.5 + 80px);
  }
  .shelf.wide {
    --card-w: clamp(230px, 22vw, 320px);
  }
  .row-title {
    font-family: var(--display);
    font-size: 16px;
    font-weight: 650;
    letter-spacing: -0.01em;
    margin: 0 0 10px;
    padding: 0 var(--gutter);
  }
  .viewport {
    position: relative;
  }
  .track {
    display: flex;
    gap: 14px;
    overflow-x: auto;
    overflow-y: visible;
    padding: 10px var(--gutter) 14px;
    margin: -10px 0 -6px;
    scroll-snap-type: x proximity;
    scroll-padding: 0 var(--gutter);
    scrollbar-width: none;
  }
  .track::-webkit-scrollbar {
    display: none;
  }
  .track > :global(*) {
    scroll-snap-align: start;
  }
  .more {
    flex: none;
    width: 1px;
  }
  .ph {
    width: var(--card-w);
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 9px;
  }
  .still :global(.skel)::after {
    animation: none;
  }
  .frame {
    aspect-ratio: 2 / 3;
    border-radius: var(--radius);
  }
  .wide .frame {
    aspect-ratio: 16 / 9;
  }
  .arrow {
    position: absolute;
    top: 0;
    bottom: 34px;
    z-index: 3;
    width: calc(var(--gutter) + 18px);
    display: grid;
    place-items: center;
    color: var(--text);
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.2s;
  }
  .arrow.left {
    left: 0;
    background: linear-gradient(90deg, var(--bg) 20%, transparent);
  }
  .arrow.right {
    right: 0;
    background: linear-gradient(270deg, var(--bg) 20%, transparent);
  }
  .viewport:hover .arrow.show {
    opacity: 1;
    pointer-events: auto;
  }
  .arrow :global(svg) {
    transition: transform 0.18s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .arrow:hover :global(svg) {
    transform: scale(1.25);
  }
  /* Con fondo de imagen o gradiente, la flecha lleva su propia pastilla. */
  :global([data-theme="warp"]) .arrow,
  :global([data-theme="haze"]) .arrow,
  :global([data-theme="poolsuite"]) .arrow {
    background: none;
  }
  :global([data-theme="warp"]) .arrow :global(svg),
  :global([data-theme="haze"]) .arrow :global(svg),
  :global([data-theme="poolsuite"]) .arrow :global(svg) {
    box-sizing: content-box;
    padding: 8px;
    border-radius: 50%;
    background: var(--sidebar);
    border: var(--border) solid var(--line-strong);
  }
  .err {
    padding: 0 var(--gutter);
    font-size: 12px;
  }
  @media (hover: none) {
    .arrow {
      display: none;
    }
  }
</style>
