<script lang="ts">
  // Barra de titulo propia del desktop (como Ethereon): plegar la barra lateral, la marca, un
  // hueco para arrastrar (doble clic maximiza), "Update" si hay una version lista, a que server se
  // habla y los controles de la ventana.
  import { ArrowDownToLine, Menu, Search } from "@lucide/svelte";
  import { go } from "../lib/router.svelte";
  import { installUpdate, shell } from "../lib/shell.svelte";
  import { toast } from "../lib/toast.svelte";
  import ConnBadge from "./ConnBadge.svelte";
  import Logo from "./Logo.svelte";
  import WindowControls from "./WindowControls.svelte";

  let { ontoggle, bare = false }: { ontoggle?: () => void; bare?: boolean } = $props();

  let installing = $state(false);
  async function update() {
    installing = true;
    // Si instala, la app se reinicia y vuelve aca (lib/shell.svelte.ts).
    if (!(await installUpdate().catch(() => false))) {
      installing = false;
      toast("Couldn't install the update. It will try again later.", true);
    }
  }
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
  {#if shell.update?.status === "ready"}
    <button class="upd" onclick={update} disabled={installing} title={`Restart into Mythos ${shell.update.version}`}>
      <ArrowDownToLine size={13} /><span>{installing ? "Updating…" : "Update"}</span>
    </button>
  {/if}
  {#if !bare}
    <!-- Server de casa o el del dispositivo (modo local). -->
    <ConnBadge />
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
  /* data-tauri-drag-region no se hereda: el logo tiene que dejar pasar el clic al .brand. */
  .brand :global(svg) {
    pointer-events: none;
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
  .upd {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-ink);
    background: var(--primary-bg);
    transition:
      filter 0.12s,
      opacity 0.12s;
  }
  .upd:hover {
    filter: brightness(1.1);
  }
  .upd:disabled {
    opacity: 0.7;
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
