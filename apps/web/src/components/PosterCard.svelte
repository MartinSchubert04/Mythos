<script lang="ts">
  // Tarjeta de poster: se eleva y agranda un poco al pasar el mouse, con el brillo del tema.
  // Al pasar el mouse ya carga el codigo y los datos del detalle; al hacer clic el poster
  // vuela al detalle (View Transitions) si el navegador puede.
  import { Star } from "@lucide/svelte";
  import { tick } from "svelte";
  import { api } from "../lib/api";
  import { sized } from "../lib/format";
  import { handOff } from "../lib/handoff";
  import { go, loadView } from "../lib/router.svelte";
  import type { MetaPreview } from "../lib/types";
  import Img from "./Img.svelte";

  let { item, i = 0, eager = false }: { item: MetaPreview; i?: number; eager?: boolean } = $props();

  let flying = $state(false);
  let hoverTimer = 0;

  function warm() {
    clearTimeout(hoverTimer);
    hoverTimer = window.setTimeout(() => {
      loadView("detail").catch(() => {});
      api.meta(item.type, item.id).catch(() => {});
    }, 120);
  }

  async function open(e: MouseEvent) {
    if (e.ctrlKey || e.metaKey || e.button !== 0) return;
    e.preventDefault();
    handOff(item);
    flying = true;
    await tick();
    await go(`/title/${item.type}/${item.id}`, { vt: true });
  }
</script>

<a
  class="card-poster"
  href="#/title/{item.type}/{item.id}"
  style="--i: {i}"
  onclick={open}
  onpointerenter={warm}
  onpointerleave={() => clearTimeout(hoverTimer)}
  onfocus={warm}
  aria-label="{item.name}{item.year ? ` (${item.year})` : ''}"
>
  <div class="poster-frame">
    <Img src={sized(item.poster, "small")} alt="" {eager} fallback={item.name} fallbackSub={item.year ?? ""} vt={flying ? "poster" : ""} />
    {#if item.imdb_rating}
      <span class="rating"><Star size={10} fill="currentColor" />{item.imdb_rating}</span>
    {/if}
  </div>
  <div class="meta">
    <span class="name ellipsis">{item.name}</span>
    {#if item.year}<span class="year">{item.year}</span>{/if}
  </div>
</a>

<style>
  .card-poster {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: var(--card-w, 150px);
    flex: none;
    border-radius: var(--radius);
    animation: rise 0.36s cubic-bezier(0.2, 0.8, 0.2, 1) both;
    animation-delay: calc(min(var(--i, 0), 12) * 30ms);
  }
  .poster-frame {
    position: relative;
    aspect-ratio: 2 / 3;
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--panel-2);
    border: var(--border) solid var(--line);
    box-shadow: var(--card-shadow);
    transform: translateZ(0);
    transition:
      transform 0.28s cubic-bezier(0.2, 0.8, 0.2, 1),
      box-shadow 0.28s,
      border-color 0.2s;
  }
  /* Reflejo que cruza el poster al pasar el mouse. */
  .poster-frame::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(115deg, transparent 35%, color-mix(in srgb, var(--on-media) 16%, transparent) 50%, transparent 65%);
    transform: translateX(-110%);
    pointer-events: none;
  }
  .card-poster:hover .poster-frame,
  .card-poster:focus-visible .poster-frame {
    transform: translateY(-4px) scale(1.04);
    border-color: color-mix(in srgb, var(--accent) 55%, var(--line));
    box-shadow:
      0 18px 34px -16px var(--media-scrim),
      0 0 0 1px color-mix(in srgb, var(--accent) 35%, transparent),
      0 10px 40px -12px color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .card-poster:hover .poster-frame::after {
    transform: translateX(110%);
    transition: transform 0.75s ease;
  }
  .card-poster:active .poster-frame {
    transform: translateY(-1px) scale(1.01);
    transition-duration: 0.08s;
  }
  .card-poster:focus-visible {
    outline: none;
  }
  .rating {
    position: absolute;
    top: 6px;
    right: 6px;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 6px;
    border-radius: 999px;
    font-size: 10.5px;
    font-weight: 700;
    color: var(--on-media);
    background: var(--media-scrim);
    backdrop-filter: blur(6px);
    opacity: 0;
    transform: translateY(-3px);
    transition:
      opacity 0.2s,
      transform 0.2s;
  }
  .rating :global(svg) {
    color: var(--warn);
  }
  .card-poster:hover .rating,
  .card-poster:focus-visible .rating {
    opacity: 1;
    transform: none;
  }
  .meta {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 0 2px;
    min-width: 0;
  }
  .name {
    font-weight: 560;
    font-size: 13px;
    color: var(--text);
  }
  .year {
    font-size: 11.5px;
    color: var(--text-3);
  }
  :global([data-theme="poolsuite"]) .card-poster:hover .poster-frame {
    box-shadow: 5px 5px 0 var(--line);
    transform: translate(-2px, -3px);
  }
  :global([data-theme="athanor"]) .card-poster:hover .poster-frame {
    transform: translate(-4px, -4px);
    box-shadow: none;
    transition-timing-function: steps(2);
  }
</style>
