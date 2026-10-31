<script lang="ts">
  // Barra de titulo propia del desktop (como Ethereon): plegar la barra lateral, la marca, un
  // hueco para arrastrar (doble clic maximiza) y los controles de la ventana.
  import { Menu, Search } from "@lucide/svelte";
  import { go } from "../lib/router.svelte";
  import Logo from "./Logo.svelte";
  import WindowControls from "./WindowControls.svelte";

  let { ontoggle, bare = false }: { ontoggle?: () => void; bare?: boolean } = $props();
</script>

<header class="bar" data-tauri-drag-region>
  {#if !bare && ontoggle}
    <button class="ham" title="Sidebar" onclick={ontoggle}><Menu size={17} /></button>
  {/if}
  <div class="brand" data-tauri-drag-region>
    <Logo size={18} />
    <span data-tauri-drag-region>Mythos</span>
  </div>
  <div class="drag" data-tauri-drag-region></div>
  {#if !bare}
    <button class="find" onclick={() => go("/search")} title="Search (Ctrl K)">
      <Search size={14} /><span>Search</span><kbd>/</kbd>
    </button>
  {/if}
  <WindowControls />
</header>

<style>
  .bar {
    height: 40px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    padding-left: 6px;
    background: color-mix(in srgb, var(--sidebar) 85%, transparent);
    backdrop-filter: var(--blur);
    border-bottom: var(--border) solid var(--line);
    position: relative;
    z-index: 20;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px;
    font-family: var(--display);
    font-weight: 700;
    font-size: 13.5px;
    letter-spacing: -0.01em;
  }
  .ham {
    display: grid;
    place-items: center;
    width: 32px;
    height: 30px;
    border-radius: var(--radius-sm);
    color: var(--text-2);
    transition:
      background 0.12s,
      color 0.12s;
  }
  .ham:hover {
    background: var(--hover);
    color: var(--text);
  }
  .drag {
    flex: 1;
    align-self: stretch;
  }
  .find {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding: 0 6px 0 10px;
    margin-right: 6px;
    width: 220px;
    border-radius: var(--radius-sm);
    background: var(--panel-2);
    border: var(--border) solid var(--line);
    color: var(--text-3);
    font-size: 12.5px;
    transition:
      border-color 0.15s,
      color 0.15s;
  }
  .find span {
    flex: 1;
    text-align: left;
  }
  .find:hover {
    border-color: var(--line-strong);
    color: var(--text-2);
  }
  /* Poolsuite: barra de ventana del Mac clasico (rayas). */
  :global([data-theme="poolsuite"]) .bar {
    background: var(--sidebar);
  }
  :global([data-theme="poolsuite"]) .drag {
    background: var(--titlebar);
    background-size: 100% 13px;
    background-repeat: no-repeat;
    background-position: center;
    margin: 0 8px;
  }
  @media (max-width: 720px) {
    .find {
      width: auto;
    }
    .find span,
    .find kbd {
      display: none;
    }
  }
</style>
