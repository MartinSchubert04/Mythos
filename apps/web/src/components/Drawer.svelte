<script lang="ts">
  // Panel lateral para formularios (crear/editar). Portado de Ethereon.
  import { X } from "@lucide/svelte";
  import type { Snippet } from "svelte";
  import { fadeQuick, slideIn, slideOut } from "../lib/motion";
  import { focusEl, focusFirst } from "../lib/spatial.svelte";
  import { tv } from "../lib/tv.svelte";

  let panel = $state<HTMLElement>();
  // TV: al cerrar, el foco vuelve a lo que lo abrio (si no, quedaria en el cajon que se esta
  // yendo y despues en la nada).
  const opener = document.activeElement as HTMLElement | null;
  let shown: HTMLElement | undefined;
  $effect(() => () => {
    if (!tv.on || !opener?.isConnected) return;
    const cur = document.activeElement;
    if (!cur || cur === document.body || !cur.isConnected || shown?.contains(cur)) focusEl(opener);
  });
  // TV: el foco entra al cajon apenas abre (si no, quedaria detras, en lo que lo abrio). Lo de
  // adentro puede tardar (streams): se reintenta un rato mientras el foco siga afuera.
  $effect(() => {
    if (!tv.on || !panel) return;
    const el = panel;
    shown = el;
    let n = 0;
    const id = setInterval(() => {
      const inside = el.contains(document.activeElement);
      const real = !!el.querySelector('button:not([aria-label="Close"]), input, select, textarea, a[href]');
      if ((!inside || (real && document.activeElement?.matches('[aria-label="Close"]'))) && focusFirst(el) && real) clearInterval(id);
      if (++n > 30) clearInterval(id);
    }, 150);
    return () => clearInterval(id);
  });

  let {
    title,
    onclose,
    onsave,
    children,
    footer,
  }: { title: string; onclose: () => void; onsave?: () => void; children: Snippet; footer?: Snippet } = $props();

  // Esc cierra y Ctrl S (o Ctrl Enter) guarda. Con un dialogo de confirmacion encima,
  // las teclas son de ese dialogo.
  function onKey(e: KeyboardEvent) {
    if (e.defaultPrevented || document.querySelector("[role=alertdialog]")) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (onsave && (e.ctrlKey || e.metaKey) && (e.key === "s" || e.key === "Enter")) {
      e.preventDefault();
      (document.activeElement as HTMLElement | null)?.blur?.(); // que el ultimo campo se guarde
      onsave();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="scrim" onclick={onclose} role="presentation" transition:fadeQuick|global></div>
<aside class="drawer card" data-nav-scope bind:this={panel} in:slideIn|global out:slideOut|global>
  <header class="titlebar">
    <h2>{title}</h2>
    <button class="icon-btn" onclick={onclose} aria-label="Close"><X size={16} /></button>
  </header>
  <div class="body">{@render children()}</div>
  {#if footer}
    <footer>
      {#if onsave}<span class="keys">Ctrl S saves · Esc closes</span>{/if}
      {@render footer()}
    </footer>
  {/if}
</aside>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    backdrop-filter: blur(2px);
    z-index: 40;
  }
  .drawer {
    position: fixed;
    top: 10px;
    right: 10px;
    bottom: 10px;
    width: min(520px, calc(100vw - 20px));
    z-index: 41;
    display: flex;
    flex-direction: column;
    background: var(--sidebar);
    box-shadow: var(--pop-shadow);
  }
  :global([data-theme="warp"]) .drawer,
  :global([data-theme="haze"]) .drawer {
    background: color-mix(in srgb, var(--bg) 82%, transparent);
  }
  header {
    display: flex;
    align-items: center;
    padding: 14px 16px;
    border-bottom: var(--border) solid var(--line);
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: 15px;
    font-weight: 650;
  }
  :global([data-theme="poolsuite"]) h2 {
    flex: none;
    background: var(--sidebar);
    padding: 0 10px;
    margin: 0 auto;
  }
  .body {
    flex: 1;
    overflow: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  footer {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    flex-wrap: wrap;
    align-items: center;
    padding: 10px 16px 12px;
    border-top: var(--border) solid var(--line);
  }
  .keys {
    flex-basis: 100%;
    font-size: 11px;
    color: var(--text-3);
    text-align: right;
  }
</style>
