<script lang="ts">
  // A que server le habla la app: el de casa o el que va adentro del dispositivo (modo local).
  // Siempre a la vista en desktop (barra de titulo) y en Android (arriba a la derecha,
  // `floating`); en la web no existe. El detalle (cambios sin sincronizar, ultima
  // sincronizacion, que direccion del server se usa) va en el tooltip; un clic lleva a Settings. Es
  // solo el icono: nube = server, disco = este dispositivo; un punto si hay cambios sin sincronizar.
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
    {#if pending > 0}<span class="dot"></span>{/if}
  </button>
{/if}

<style>
  .conn {
    position: relative;
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    margin-right: 6px;
    border-radius: 999px;
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
  .dot {
    position: absolute;
    top: 2px;
    right: 2px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--warn);
  }
  /* Android: flota arriba a la derecha, debajo de la barra del sistema. */
  .floating {
    position: fixed;
    top: calc(8px + env(safe-area-inset-top));
    right: calc(10px + env(safe-area-inset-right));
    z-index: 40;
    margin: 0;
    width: 26px;
    height: 26px;
    background: color-mix(in srgb, var(--sidebar) 82%, transparent);
    backdrop-filter: var(--blur);
    box-shadow: var(--pop-shadow);
  }
</style>
