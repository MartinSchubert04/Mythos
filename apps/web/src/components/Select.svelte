<script lang="ts" generics="T">
  // (Portado de Ethereon.) Desplegable propio (en vez del <select> nativo, que no se puede estilar en Windows).
  // Teclado: flechas, Inicio/Fin, Enter, Esc y escribir para saltar a una opcion. Con
  // muchas opciones aparece un buscador.
  import { Check, ChevronDown, Search } from "@lucide/svelte";
  import { tick } from "svelte";
  import { pop } from "../lib/motion";
  import { tv } from "../lib/tv.svelte";

  interface Option<V> {
    value: V;
    label: string;
    hint?: string;
  }

  let {
    value = $bindable(),
    options,
    placeholder = "Select...",
    disabled = false,
    onchange,
    ghost = false,
    lead,
    icon,
  }: {
    value: T;
    options: Option<T>[];
    placeholder?: string;
    disabled?: boolean;
    onchange?: (v: T) => void;
    /** Sin caja: para usarlo como titulo (por ejemplo el origen de un panel SFTP). */
    ghost?: boolean;
    /** Algo antes del texto del boton (un icono). */
    lead?: import("svelte").Snippet;
    /** Un icono por opcion (en la lista y en el boton, con la elegida). */
    icon?: import("svelte").Snippet<[T]>;
  } = $props();

  let open = $state(false);
  let active = $state(0);
  let query = $state("");
  let trigger: HTMLButtonElement;
  let list = $state<HTMLDivElement>();
  let pos = $state({ left: 0, top: 0, width: 0, up: false });
  let typed = "";
  let typedAt = 0;

  const searchable = $derived(options.length > 7);
  const selected = $derived(options.find((o) => o.value === value));
  const shown = $derived(
    query ? options.filter((o) => (o.label + " " + (o.hint ?? "")).toLowerCase().includes(query.toLowerCase())) : options,
  );

  async function show() {
    if (disabled) return;
    const r = trigger.getBoundingClientRect();
    const height = Math.min(300, options.length * 36 + (searchable ? 48 : 0) + 12);
    const up = r.bottom + height + 8 > window.innerHeight && r.top > height + 8;
    pos = { left: r.left, top: up ? r.top - 6 : r.bottom + 6, width: Math.max(r.width, ghost ? 220 : 0), up };
    query = "";
    active = Math.max(0, options.findIndex((o) => o.value === value));
    open = true;
    await tick();
    scrollActive();
    (list?.querySelector("input") as HTMLInputElement | null)?.focus();
  }

  function close(refocus = true) {
    open = false;
    if (refocus) trigger?.focus();
  }

  function choose(o: Option<T>) {
    value = o.value;
    onchange?.(o.value);
    close();
  }

  function scrollActive() {
    list?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function move(d: number) {
    if (!shown.length) return;
    active = (active + d + shown.length) % shown.length;
    scrollActive();
  }

  function onKey(e: KeyboardEvent) {
    if (!open) {
      // En TV las flechas mueven el foco a otro lado: se abre con el centro del control.
      if ((tv.on ? ["Enter", " "] : ["ArrowDown", "ArrowUp", "Enter", " "]).includes(e.key)) {
        e.preventDefault();
        show();
      }
      return;
    }
    if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Home") (active = 0), scrollActive();
    else if (e.key === "End") (active = shown.length - 1), scrollActive();
    else if (e.key === "Enter") shown[active] && choose(shown[active]);
    else if (e.key === "Escape" || e.key === "Tab") close(e.key === "Escape");
    else if (!searchable && e.key.length === 1) {
      // Escribir salta a la primera opcion que empieza asi.
      const now = Date.now();
      typed = now - typedAt < 700 ? typed + e.key.toLowerCase() : e.key.toLowerCase();
      typedAt = now;
      const i = shown.findIndex((o) => o.label.toLowerCase().startsWith(typed));
      if (i >= 0) (active = i), scrollActive();
      return;
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  // La lista se cuelga de <body>: si quedara dentro de una tarjeta con blur (Haze) o
  // con transform (las que se levantan al pasar el mouse), "position: fixed" se mide
  // desde esa tarjeta y la lista aparece corrida y recortada.
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }

  function outside(e: PointerEvent) {
    const t = e.target as Node;
    if (open && !trigger.contains(t) && !list?.contains(t)) close(false);
  }
</script>

<svelte:window onpointerdown={outside} onresize={() => open && close(false)} />

<button
  bind:this={trigger}
  type="button"
  class="select-trigger"
  class:ghost
  class:open
  class:placeholder={!selected}
  {disabled}
  aria-haspopup="listbox"
  aria-expanded={open}
  onclick={() => (open ? close() : show())}
  onkeydown={onKey}
>
  {#if lead}<span class="lead">{@render lead()}</span>{/if}
  {#if icon}<span class="lead">{@render icon(value)}</span>{/if}
  <span class="ellipsis label">{selected?.label ?? placeholder}</span>
  <span class="chev"><ChevronDown size={15} /></span>
</button>

{#if open}
  <div
    bind:this={list}
    use:portal
    class="select-pop"
    class:up={pos.up}
    role="listbox"
    tabindex="-1"
    style="left: {pos.left}px; top: {pos.top}px; min-width: {pos.width}px"
    transition:pop={{ start: 0.96 }}
    onkeydown={onKey}
  >
    {#if searchable}
      <div class="search inbox bare">
        <Search size={14} />
        <input placeholder="Search..." bind:value={query} oninput={() => (active = 0)} />
      </div>
    {/if}
    <div class="opts">
      {#each shown as o, i (o.value)}
        <div
          class="opt"
          class:active={i === active}
          class:sel={o.value === value}
          role="option"
          tabindex="-1"
          aria-selected={o.value === value}
          data-i={i}
          onpointermove={() => (active = i)}
          onclick={() => choose(o)}
          onkeydown={() => {}}
        >
          <span class="check">{#if o.value === value}<Check size={14} />{/if}</span>
          {#if icon}<span class="lead">{@render icon(o.value)}</span>{/if}
          <span class="txt">
            <span class="ellipsis">{o.label}</span>
            {#if o.hint}<span class="hint ellipsis">{o.hint}</span>{/if}
          </span>
        </div>
      {:else}
        <div class="none">Nothing matches.</div>
      {/each}
    </div>
  </div>
{/if}

<style>
  /* Variante sin caja: el texto en negrita con la flecha al lado. */
  :global(.select-trigger.ghost),
  :global(.select-trigger.ghost:hover) {
    width: auto;
    max-width: 100%;
    height: 30px;
    padding: 0 6px;
    gap: 7px;
    background: none !important;
    border-color: transparent !important;
    box-shadow: none !important;
    outline: none !important;
    font-weight: 620;
    border-radius: var(--radius-sm);
  }
  :global(.select-trigger.ghost:hover),
  :global(.select-trigger.ghost.open) {
    background: var(--hover) !important;
  }
  :global(.select-trigger.ghost:focus-visible) {
    outline: 2px solid color-mix(in srgb, var(--accent) 70%, transparent) !important;
  }
  .select-trigger {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 36px;
    padding: 0 10px 0 12px;
    text-align: left;
    background: var(--panel-2);
    border: var(--border) solid var(--line);
    border-radius: var(--radius-sm);
    transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
  }
  .select-trigger:hover {
    border-color: var(--line-strong);
  }
  .select-trigger.open,
  .select-trigger:focus-visible {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .select-trigger .label {
    flex: 1;
    min-width: 0;
  }
  .lead {
    display: grid;
    flex: none;
  }
  .placeholder .label {
    color: var(--text-3);
  }
  .chev {
    display: grid;
    color: var(--text-3);
    transition: transform 0.2s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .open .chev {
    transform: rotate(180deg);
    color: var(--accent);
  }
  .select-trigger:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .select-pop {
    position: fixed;
    z-index: 90;
    max-width: 420px;
    max-height: 300px;
    display: flex;
    flex-direction: column;
    background: var(--sidebar);
    border: var(--border) solid var(--line-strong);
    border-radius: var(--radius);
    box-shadow: var(--pop-shadow);
    overflow: hidden;
    transform-origin: top center;
  }
  /* Abre hacia arriba si abajo no entra. */
  .select-pop.up {
    transform: translateY(-100%);
    transform-origin: bottom center;
  }
  :global([data-theme="warp"]) .select-pop,
  :global([data-theme="haze"]) .select-pop {
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    backdrop-filter: blur(18px);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    color: var(--text-3);
    border-bottom: var(--border) solid var(--line);
  }
  .search input {
    padding: 10px 0;
  }
  .opts {
    overflow: auto;
    padding: 5px;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px 7px 6px;
    border-radius: calc(var(--radius-sm) - 2px);
    cursor: pointer;
    color: var(--text-2);
    transition: background 0.08s, color 0.08s;
  }
  .opt.active {
    background: var(--hover);
    color: var(--text);
  }
  .opt.sel {
    color: var(--text);
  }
  .check {
    width: 16px;
    display: grid;
    place-items: center;
    color: var(--accent);
    flex: none;
  }
  .txt {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .hint {
    font-size: 11.5px;
    color: var(--text-3);
  }
  .none {
    padding: 10px;
    color: var(--text-3);
  }
</style>
