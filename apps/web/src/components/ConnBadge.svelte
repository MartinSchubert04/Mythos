<script lang="ts">
  // A que server le habla la app: el de casa ("Server") o el que va adentro del dispositivo
  // ("Local", modo local). Siempre a la vista en desktop (barra de titulo) y en Android (arriba a
  // la derecha, `floating`); en la web no existe. El detalle (cambios sin sincronizar, ultima
  // sincronizacion) va en el tooltip; un clic lleva a Settings.
  import { Cloud, HardDrive } from "@lucide/svelte";
  import { conn, connDetail } from "../lib/local.svelte";
  import { go } from "../lib/router.svelte";

  let { floating = false }: { floating?: boolean } = $props();
  const local = $derived(conn.on === "local");
  const pending = $derived(local ? (conn.status?.pending_changes ?? 0) : 0);
</script>

{#if conn.available}
  <button
    class="conn"
    class:local
    class:floating
    class:warn={conn.needsSignIn || !!conn.error}
    title={connDetail()}
    aria-label={connDetail()}
    onclick={() => go("/settings")}
  >
    {#if local}<HardDrive size={13} />{:else}<Cloud size={13} />{/if}
    <span>{local ? "Local" : "Server"}</span>
    {#if pending > 0}<span class="count">{pending}</span>{/if}
  </button>
{/if}

<style>
  .conn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 9px 0 8px;
    margin-right: 6px;
    border-radius: 999px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-2);
    background: var(--panel-2);
    border: var(--border) solid var(--line);
    white-space: nowrap;
    transition:
      border-color 0.15s,
      color 0.15s;
  }
  .conn:hover {
    color: var(--text);
    border-color: var(--line-strong);
  }
  .conn :global(svg) {
    flex: none;
    color: var(--ok);
  }
  .conn.local :global(svg) {
    color: var(--warn);
  }
  .conn.local {
    border-color: color-mix(in srgb, var(--warn) 45%, var(--line));
  }
  .conn.warn :global(svg) {
    color: var(--danger);
  }
  .count {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 10.5px;
    color: var(--text-3);
  }
  /* Android: flota arriba a la derecha, debajo de la barra del sistema. */
  .floating {
    position: fixed;
    top: calc(8px + env(safe-area-inset-top));
    right: calc(10px + env(safe-area-inset-right));
    z-index: 40;
    margin: 0;
    height: 22px;
    padding: 0 8px 0 7px;
    font-size: 11px;
    background: color-mix(in srgb, var(--sidebar) 82%, transparent);
    backdrop-filter: var(--blur);
    box-shadow: var(--pop-shadow);
  }
</style>
