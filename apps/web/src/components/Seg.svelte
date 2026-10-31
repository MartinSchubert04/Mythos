<script lang="ts" generics="T">
  // (Portado de Ethereon.) Control segmentado con una pildora que se desliza a la opcion elegida.
  let {
    value = $bindable(),
    options,
    onchange,
  }: { value: T; options: { value: T; label: string }[]; onchange?: (v: T) => void } = $props();

  const index = $derived(Math.max(0, options.findIndex((o) => o.value === value)));

  // La pildora copia la posicion y el ancho del boton elegido: con textos largos las columnas
  // no quedan iguales y un ancho calculado como 100%/n se desalinea.
  let root: HTMLDivElement;
  let buttons: HTMLButtonElement[] = $state([]);
  let left = $state(0);
  let width = $state(0);
  $effect(() => {
    const b = buttons[index];
    if (!b) return;
    const place = () => {
      left = b.offsetLeft;
      width = b.offsetWidth;
    };
    place();
    const ro = new ResizeObserver(place);
    ro.observe(root);
    ro.observe(b);
    return () => ro.disconnect();
  });
</script>

<div class="seg2" role="radiogroup" bind:this={root} style="--n: {options.length}">
  <span class="pill" aria-hidden="true" style="transform: translateX({left}px); width: {width}px"></span>
  {#each options as o, i}
    <button
      bind:this={buttons[i]}
      type="button"
      role="radio"
      aria-checked={o.value === value}
      class:on={o.value === value}
      onclick={() => {
        // Con onchange el que decide es el padre (puede pedir confirmacion y no cambiar).
        if (onchange) onchange(o.value);
        else value = o.value;
      }}>{o.label}</button
    >
  {/each}
</div>

<style>
  .seg2 {
    position: relative;
    display: grid;
    grid-template-columns: repeat(var(--n), minmax(max-content, 1fr));
    height: 36px;
    padding: 3px;
    background: var(--panel-2);
    border: var(--border) solid var(--line);
    border-radius: var(--radius-sm);
  }
  .pill {
    position: absolute;
    top: 3px;
    bottom: 3px;
    left: 0;
    background: var(--panel);
    border-radius: calc(var(--radius-sm) - 2px);
    box-shadow:
      0 0 0 var(--border) var(--line-strong),
      0 2px 8px -2px rgba(0, 0, 0, 0.35);
    transition:
      transform 0.28s cubic-bezier(0.34, 1.3, 0.64, 1),
      width 0.28s cubic-bezier(0.34, 1.3, 0.64, 1);
  }
  button {
    position: relative;
    z-index: 1;
    font-size: 12.5px;
    font-weight: 560;
    color: var(--text-2);
    transition: color 0.2s;
    white-space: nowrap;
    padding: 0 8px;
  }
  button:hover {
    color: var(--text);
  }
  button.on {
    color: var(--text);
  }
  :global([data-theme="athanor"]) .pill,
  :global([data-theme="poolsuite"]) .pill {
    background: var(--accent);
    box-shadow: none;
  }
  :global([data-theme="athanor"]) button.on,
  :global([data-theme="poolsuite"]) button.on {
    color: var(--accent-ink);
  }
  :global([data-theme="athanor"]) .pill {
    transition-timing-function: steps(4);
  }
</style>
