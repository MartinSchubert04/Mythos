<script lang="ts">
  // Interruptor (en vez de una casilla). Clic en todo el renglon, Espacio o Enter.
  import type { Snippet } from "svelte";

  let {
    checked = $bindable(false),
    onchange,
    disabled = false,
    children,
  }: { checked?: boolean; onchange?: (v: boolean) => void; disabled?: boolean; children?: Snippet } = $props();

  function toggle() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<button type="button" class="switch-row" role="switch" aria-checked={checked} {disabled} onclick={toggle}>
  <span class="track" class:on={checked}><span class="thumb"></span></span>
  {#if children}<span class="label">{@render children()}</span>{/if}
</button>

<style>
  .switch-row {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    text-align: left;
    padding: 2px 0;
  }
  .switch-row:disabled {
    opacity: 0.5;
  }
  /* El pulgar se centra con top 50% + translate: queda bien con borde de 1 o 2 px. */
  .track {
    --tw: 34px;
    --th: 20px;
    --gap: 2px;
    --size: calc(var(--th) - 2 * var(--border) - 2 * var(--gap));
    position: relative;
    flex: none;
    width: var(--tw);
    height: var(--th);
    border-radius: 999px;
    background: var(--panel-2);
    border: var(--border) solid var(--line-strong);
    transition: background 0.2s, border-color 0.2s;
  }
  .track.on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .thumb {
    position: absolute;
    top: 50%;
    left: var(--gap);
    width: var(--size);
    height: var(--size);
    border-radius: 50%;
    background: var(--text-2);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.35);
    transform: translate(0, -50%);
    transition:
      transform 0.22s cubic-bezier(0.34, 1.56, 0.64, 1),
      background 0.2s,
      width 0.15s;
  }
  /* Recorrido = ancho - alto: el pulgar queda a la misma distancia de los dos bordes. */
  .on .thumb {
    transform: translate(calc(var(--tw) - var(--th)), -50%);
    background: var(--accent-ink);
  }
  /* Al apretar, el pulgar se estira (como en iOS). */
  .switch-row:active .thumb {
    width: calc(var(--size) + 4px);
  }
  .switch-row:active .on .thumb {
    transform: translate(calc(var(--tw) - var(--th) - 4px), -50%);
  }
  .switch-row:focus-visible .track {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 25%, transparent);
  }
  :global([data-theme="athanor"]) .track,
  :global([data-theme="athanor"]) .thumb {
    border-radius: 0;
  }
</style>
