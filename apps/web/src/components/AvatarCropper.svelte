<script lang="ts">
  // Recorte de una imagen antes de sumarla a la galeria de avatares (Admin → Avatars): se arrastra
  // para centrarla y se acerca con el deslizador, la rueda o +/-. Sale un cuadrado de 512 px (WebP,
  // o PNG si el navegador no sabe WebP). El circulo es la guia: los avatares se ven redondos.
  // "Upload original" la sube tal cual (por ejemplo, un GIF animado, que al recortarlo se quedaria
  // quieto).
  import { Minus, Plus } from "@lucide/svelte";
  import { t } from "../lib/i18n.svelte";
  import { fadeQuick, pop } from "../lib/motion";

  let {
    file,
    index = 1,
    total = 1,
    ondone,
  }: {
    file: File;
    index?: number;
    total?: number;
    /** `null` = saltear esta imagen. */
    ondone: (r: { blob: Blob; name: string } | null) => void;
  } = $props();

  const VIEW = 300; // lado del recuadro en px
  const OUT = 512; // lado de la imagen que se sube
  const MAX_ZOOM = 5;

  let img = $state<HTMLImageElement | null>(null);
  let failed = $state(false);
  let name = $state("");
  let zoom = $state(1);
  let x = $state(0);
  let y = $state(0);
  let saving = $state(false);
  let box = $state<HTMLDivElement>();

  // Escala para que el lado corto llene el recuadro (zoom 1) y la actual.
  const base = $derived(img ? VIEW / Math.min(img.naturalWidth, img.naturalHeight) : 1);
  const scale = $derived(base * zoom);
  const w = $derived(img ? img.naturalWidth * scale : VIEW);
  const h = $derived(img ? img.naturalHeight * scale : VIEW);

  $effect(() => {
    name = file.name.replace(/\.[^.]+$/, "");
    const url = URL.createObjectURL(file);
    const im = new Image();
    im.onload = () => {
      img = im;
      zoom = 1;
      const s = VIEW / Math.min(im.naturalWidth, im.naturalHeight);
      x = (VIEW - im.naturalWidth * s) / 2;
      y = (VIEW - im.naturalHeight * s) / 2;
      box?.focus();
    };
    im.onerror = () => (failed = true);
    im.src = url;
    return () => URL.revokeObjectURL(url);
  });

  /** La imagen siempre cubre el recuadro (sin bordes vacios). */
  function clamp() {
    x = Math.min(0, Math.max(VIEW - w, x));
    y = Math.min(0, Math.max(VIEW - h, y));
  }

  /** Cambia el zoom dejando quieto el punto `(cx, cy)` del recuadro (el centro por defecto). */
  function setZoom(z: number, cx = VIEW / 2, cy = VIEW / 2) {
    const nz = Math.min(MAX_ZOOM, Math.max(1, z));
    const k = nz / zoom;
    x = cx - (cx - x) * k;
    y = cy - (cy - y) * k;
    zoom = nz;
    clamp();
  }

  let drag = $state<{ id: number; px: number; py: number; x0: number; y0: number } | null>(null);
  function down(e: PointerEvent) {
    if (!img) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { id: e.pointerId, px: e.clientX, py: e.clientY, x0: x, y0: y };
  }
  function move(e: PointerEvent) {
    if (!drag || drag.id !== e.pointerId) return;
    x = drag.x0 + e.clientX - drag.px;
    y = drag.y0 + e.clientY - drag.py;
    clamp();
  }
  function up(e: PointerEvent) {
    if (drag?.id === e.pointerId) drag = null;
  }
  function wheel(e: WheelEvent) {
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setZoom(zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1), e.clientX - r.left, e.clientY - r.top);
  }
  function key(e: KeyboardEvent) {
    const step = e.shiftKey ? 40 : 10;
    const k = e.key;
    if (k === "ArrowLeft") x += step;
    else if (k === "ArrowRight") x -= step;
    else if (k === "ArrowUp") y += step;
    else if (k === "ArrowDown") y -= step;
    else if (k === "+" || k === "=") return (e.preventDefault(), setZoom(zoom * 1.1));
    else if (k === "-") return (e.preventDefault(), setZoom(zoom / 1.1));
    else return;
    e.preventDefault();
    clamp();
  }

  async function save() {
    if (!img || saving) return;
    saving = true;
    const c = document.createElement("canvas");
    c.width = c.height = OUT;
    const ctx = c.getContext("2d")!;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(img, -x / scale, -y / scale, VIEW / scale, VIEW / scale, 0, 0, OUT, OUT);
    const as = (type: string) => new Promise<Blob | null>((r) => c.toBlob(r, type, 0.92));
    let blob = await as("image/webp");
    if (!blob || blob.type !== "image/webp") blob = await as("image/png");
    saving = false;
    if (blob) ondone({ blob, name: name.trim() || "Avatar" });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      ondone(null);
    }
  }
</script>

<svelte:window onkeydown={onKey} />
<div class="scrim" role="presentation" onclick={() => ondone(null)} transition:fadeQuick></div>
<div class="center">
  <div class="dialog card" role="dialog" aria-modal="true" aria-label={t("account.crop.aria")} transition:pop>
    <div class="top">
      <h3>{t("account.crop.title")}</h3>
      {#if total > 1}<span class="faint">{t("account.crop.of", { i: index, n: total })}</span>{/if}
    </div>
    {#if failed}
      <p class="muted">{t("account.crop.failed")}</p>
    {:else}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div
        bind:this={box}
        class="view"
        class:grabbing={!!drag}
        style="width: {VIEW}px; height: {VIEW}px"
        tabindex="0"
        role="application"
        aria-label={t("account.crop.drag")}
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        onpointercancel={up}
        onwheel={wheel}
        onkeydown={key}
      >
        {#if img}
          <img src={img.src} alt="" draggable="false" style="width: {w}px; height: {h}px; transform: translate({x}px, {y}px)" />
        {:else}
          <span class="skel fill"></span>
        {/if}
        <span class="ring" aria-hidden="true"></span>
      </div>
      <div class="zoom">
        <button class="icon-btn" title={t("account.crop.zoomOut")} aria-label={t("account.crop.zoomOut")} onclick={() => setZoom(zoom / 1.2)} disabled={zoom <= 1}><Minus size={15} /></button>
        <input type="range" min="1" max={MAX_ZOOM} step="0.01" value={zoom} aria-label={t("account.crop.zoom")} oninput={(e) => setZoom(+e.currentTarget.value)} />
        <button class="icon-btn" title={t("account.crop.zoomIn")} aria-label={t("account.crop.zoomIn")} onclick={() => setZoom(zoom * 1.2)} disabled={zoom >= MAX_ZOOM}><Plus size={15} /></button>
      </div>
    {/if}
    <label class="field">
      <span class="faint">{t("account.crop.name")}</span>
      <input class="field-in" bind:value={name} maxlength="60" onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), save())} />
    </label>
    <div class="row actions">
      <button class="btn" onclick={() => ondone(null)}>{total > 1 ? t("account.crop.skip") : t("common.cancel")}</button>
      <div class="grow"></div>
      <button class="btn" title={t("account.crop.originalHint")} onclick={() => ondone({ blob: file, name: name.trim() || "Avatar" })}>{t("account.crop.original")}</button>
      <button class="btn primary" disabled={!img || saving} onclick={save}>{t("common.save")}</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: var(--media-scrim);
    z-index: 75;
  }
  .center {
    position: fixed;
    inset: 0;
    z-index: 76;
    display: grid;
    place-items: center;
    pointer-events: none;
  }
  .dialog {
    pointer-events: auto;
    width: min(380px, calc(100vw - 32px));
    padding: 20px;
    background: var(--sidebar);
    box-shadow: var(--pop-shadow);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .top {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  h3 {
    margin: 0;
    font-size: 15px;
  }
  p {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
  }
  .view {
    position: relative;
    align-self: center;
    max-width: 100%;
    overflow: hidden;
    border-radius: var(--radius);
    background: var(--panel-2);
    cursor: grab;
    touch-action: none;
    user-select: none;
  }
  .view.grabbing {
    cursor: grabbing;
  }
  .view:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .view img {
    position: absolute;
    top: 0;
    left: 0;
    max-width: none;
    transform-origin: 0 0;
    pointer-events: none;
  }
  .fill {
    position: absolute;
    inset: 0;
  }
  /* Guia: lo que queda afuera del circulo se oscurece (el avatar se ve redondo). */
  .ring {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    box-shadow: 0 0 0 999px color-mix(in srgb, var(--bg) 55%, transparent);
    outline: 2px solid color-mix(in srgb, var(--text) 70%, transparent);
    pointer-events: none;
  }
  .zoom {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .zoom input {
    flex: 1;
    accent-color: var(--accent);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12.5px;
  }
  .actions {
    align-items: center;
    gap: 8px;
  }
  .grow {
    flex: 1;
  }
</style>
