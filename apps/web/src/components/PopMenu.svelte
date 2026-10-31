<script lang="ts" module>
  import type { Component } from "svelte";
  export type PopItem = {
    label: string;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    icon?: Component<any>;
    title?: string;
    /** Si devuelve una promesa, el item queda "trabajando" hasta que termine y despues se cierra. */
    run: () => unknown;
  };
</script>

<script lang="ts">
  // Menu chico que flota donde se pidio (clic derecho, mantener apretado o un boton ⋯). Se cuelga
  // de <body>: dentro de un panel con blur o transform, "position: fixed" se mediria desde ahi.
  import { LoaderCircle } from "@lucide/svelte";
  import { onMount, tick } from "svelte";
  import { pop } from "../lib/motion";

  let {
    x,
    y,
    items,
    onclose,
    onmedia = false,
    label = "Options",
  }: { x: number; y: number; items: PopItem[]; onclose: () => void; onmedia?: boolean; label?: string } = $props();

  let el = $state<HTMLDivElement>();
  // svelte-ignore state_referenced_locally
  let pos = $state({ left: x, top: y });
  let busy = $state(-1);

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }

  onMount(async () => {
    await tick();
    if (!el) return;
    // Que entre en la ventana: si no hay lugar abajo, va para arriba; si no a la derecha, a la izquierda.
    const r = el.getBoundingClientRect();
    const left = Math.max(8, Math.min(x, innerWidth - r.width - 8));
    const top = y + r.height > innerHeight - 8 ? Math.max(8, y - r.height) : y;
    pos = { left, top };
    el.querySelector<HTMLElement>("button")?.focus({ preventScroll: true });
  });

  async function choose(i: number) {
    if (busy >= 0) return;
    const r = items[i].run();
    if (r instanceof Promise) {
      busy = i;
      try {
        await r;
      } finally {
        busy = -1;
      }
    }
    onclose();
  }

  function outside(e: PointerEvent) {
    if (el && !el.contains(e.target as Node)) onclose();
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const bs = [...(el?.querySelectorAll<HTMLElement>("button") ?? [])];
      const i = bs.indexOf(document.activeElement as HTMLElement);
      bs[(i + (e.key === "ArrowDown" ? 1 : -1) + bs.length) % bs.length]?.focus();
      e.preventDefault();
      e.stopPropagation();
    }
  }
</script>

<svelte:window onpointerdown={outside} onresize={onclose} onkeydowncapture={onKey} />

<div
  bind:this={el}
  use:portal
  class="popmenu"
  class:onmedia
  role="menu"
  aria-label={label}
  tabindex="-1"
  style="left: {pos.left}px; top: {pos.top}px"
  in:pop={{ start: 0.94 }}
>
  {#each items as it, i (it.label)}
    {@const Icon = it.icon}
    <button class="pi" role="menuitem" title={it.title} disabled={busy >= 0 && busy !== i} onclick={() => choose(i)}>
      <span class="ic">
        {#if busy === i}<LoaderCircle size={15} class="spin" />{:else if Icon}<Icon size={15} />{/if}
      </span>
      <span class="ellipsis">{it.label}</span>
    </button>
  {/each}
</div>

<style>
  .popmenu {
    position: fixed;
    z-index: 95;
    min-width: 200px;
    max-width: min(320px, calc(100vw - 16px));
    padding: 5px;
    border-radius: var(--radius);
    background: var(--sidebar);
    border: var(--border) solid var(--line-strong);
    box-shadow: var(--pop-shadow);
    color: var(--text);
    transform-origin: top left;
  }
  .pi {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    min-height: 36px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    text-align: left;
    font-size: 13.5px;
    color: var(--text);
    transition: background 0.1s;
  }
  .pi:hover,
  .pi:focus-visible {
    background: var(--hover);
  }
  .pi:disabled {
    opacity: 0.45;
  }
  .ic {
    display: grid;
    place-items: center;
    width: 18px;
    flex: none;
    color: var(--text-2);
  }
  /* Encima del video: vidrio oscuro como los menus del reproductor. */
  .onmedia {
    background: color-mix(in srgb, var(--media-bg) 86%, transparent);
    border-color: var(--media-line);
    backdrop-filter: blur(20px);
    color: var(--on-media);
  }
  .onmedia .pi {
    color: var(--on-media);
  }
  .onmedia .pi:hover,
  .onmedia .pi:focus-visible {
    background: var(--media-line);
  }
  .onmedia .ic {
    color: var(--on-media-2);
  }
  @media (pointer: coarse) {
    .pi {
      min-height: 44px;
    }
  }
</style>
