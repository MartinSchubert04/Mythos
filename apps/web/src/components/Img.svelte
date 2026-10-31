<script lang="ts">
  import { untrack } from "svelte";
  // Imagen que aparece en fade al cargar. El tamaño lo reserva el contenedor (aspect-ratio),
  // asi nada salta de lugar. Si falla, queda el fondo (o el texto de respaldo).
  let {
    src,
    alt = "",
    eager = false,
    fit = "cover",
    fallback = "",
    fallbackSub = "",
    style = "",
    vt = "",
  }: {
    src?: string | null;
    alt?: string;
    eager?: boolean;
    fit?: "cover" | "contain";
    fallback?: string;
    /** Segunda linea del respaldo (el año). */
    fallbackSub?: string;
    style?: string;
    /** view-transition-name (el poster que vuela al detalle). */
    vt?: string;
  } = $props();

  let loaded = $state(false);
  let failed = $state(false);
  let el = $state<HTMLImageElement>();

  // Si cambia la imagen, vuelve a aparecer en fade (si ya estaba en cache, sin fade). Depende
  // solo de `src`: si dependiera de `el`, un error (que desmonta el <img>) lo volveria a montar
  // y fallaria otra vez, en bucle.
  $effect(() => {
    void src;
    untrack(() => {
      failed = false;
      loaded = !!el?.complete && el.naturalWidth > 0;
    });
  });
</script>

{#if src && !failed}
  <img
    bind:this={el}
    class="fade-img"
    class:loaded
    src={src}
    {alt}
    loading={eager ? "eager" : "lazy"}
    decoding="async"
    fetchpriority={eager ? "high" : "auto"}
    draggable="false"
    style="object-fit: {fit}; {vt ? `view-transition-name: ${vt};` : ''} {style}"
    onload={() => (loaded = true)}
    onerror={() => (failed = true)}
  />
{:else if fallback}
  <span class="fb" style={vt ? `view-transition-name: ${vt}` : ""}>
    <span class="fb-name">{fallback}</span>
    {#if fallbackSub}<span class="fb-sub">{fallbackSub}</span>{/if}
  </span>
{/if}

<style>
  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }
  /* Sin imagen: un degradado del tema con el nombre (y el año), como un poster tipografico. */
  .fb {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 4px;
    padding: 14px 12px;
    text-align: left;
    background:
      radial-gradient(120% 70% at 0% 0%, color-mix(in srgb, var(--accent) 28%, transparent), transparent 60%),
      radial-gradient(90% 60% at 100% 100%, color-mix(in srgb, var(--accent) 14%, transparent), transparent 70%),
      linear-gradient(160deg, var(--panel-2), var(--panel));
  }
  .fb-name {
    font-family: var(--display);
    font-weight: 750;
    font-size: 15px;
    line-height: 1.15;
    letter-spacing: -0.01em;
    color: var(--text);
    display: -webkit-box;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .fb-sub {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-3);
  }
</style>
