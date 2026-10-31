<script lang="ts">
  // Tarjeta de "Continue watching": el fondo del titulo, la barra de progreso y el episodio.
  // Clic: vuelve al reproductor (con el mismo stream); la X lo saca de la fila.
  import { Play, X } from "@lucide/svelte";
  import { api } from "../lib/api";
  import { clock, sized } from "../lib/format";
  import { fit } from "../lib/images";
  import { go, loadView } from "../lib/router.svelte";
  import type { Progress } from "../lib/types";
  import Img from "./Img.svelte";

  let { p, i = 0, onremove }: { p: Progress; i?: number; onremove?: (p: Progress) => void } = $props();

  // Si el progreso se guardo sin imagenes, se toman del catalogo (queda en cache).
  let fromMeta = $state<string | null | undefined>(null);
  const img = $derived(p.background ?? p.poster ?? fromMeta);
  $effect(() => {
    if (p.background || p.poster) return;
    api
      .meta(p.type, p.series_id ?? p.meta_id.split(":")[0])
      .then((m) => (fromMeta = m.background ?? m.poster))
      .catch(() => {});
  });

  const pct = $derived(p.duration_s ? Math.min(100, (p.position_s / p.duration_s) * 100) : 0);
  const left = $derived(Math.max(0, p.duration_s - p.position_s));
  // Desde aca se sigue sin preguntar, con el mismo stream.
  const href = $derived(`/watch/${p.type}/${p.meta_id}?resume=1${p.stream_key ? `&s=${encodeURIComponent(p.stream_key)}` : ""}`);
</script>

<a
  class="cw"
  href="#{href}"
  style="--i: {i}"
  onpointerenter={() => loadView("player").catch(() => {})}
  onclick={(e) => {
    if (e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    go(href);
  }}
>
  <div class="frame">
    <Img src={fit(sized(img, "medium"), 320)} alt="" fallback={p.name} />
    <div class="shade"></div>
    <span class="play"><Play size={20} fill="currentColor" /></span>
    {#if onremove}
      <button
        class="rm"
        title="Remove from Continue watching"
        onclick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onremove(p);
        }}><X size={14} /></button
      >
    {/if}
    <div class="info">
      <span class="name ellipsis">{p.name}</span>
      <span class="sub ellipsis">{p.episode_label ? `${p.episode_label} · ` : ""}{clock(left)} left</span>
      <div class="progress"><div class="progress-fill" style="width: {pct}%"></div></div>
    </div>
  </div>
</a>

<style>
  .cw {
    width: var(--card-w);
    flex: none;
    animation: rise 0.36s cubic-bezier(0.2, 0.8, 0.2, 1) backwards;
    animation-delay: calc(min(var(--i, 0), 8) * 40ms);
  }
  .frame {
    position: relative;
    aspect-ratio: 16 / 9;
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--panel-2);
    border: var(--border) solid var(--line);
    box-shadow: var(--card-shadow);
    transition:
      transform 0.28s cubic-bezier(0.2, 0.8, 0.2, 1),
      box-shadow 0.28s,
      border-color 0.2s;
  }
  .cw:hover .frame,
  .cw:focus-visible .frame {
    transform: translateY(-3px) scale(1.025);
    border-color: color-mix(in srgb, var(--accent) 55%, var(--line));
    box-shadow:
      0 18px 34px -16px var(--media-scrim),
      0 10px 40px -14px color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .cw:focus-visible {
    outline: none;
  }
  .shade {
    position: absolute;
    inset: 0;
    background: linear-gradient(0deg, var(--media-scrim) 10%, transparent 70%);
  }
  .play {
    position: absolute;
    left: 50%;
    top: 42%;
    display: grid;
    place-items: center;
    width: 46px;
    height: 46px;
    border-radius: 50%;
    color: var(--accent-ink);
    background: var(--primary-bg);
    transform: translate(-50%, -50%) scale(0.7);
    opacity: 0;
    transition:
      transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1),
      opacity 0.2s;
    box-shadow: 0 6px 24px -6px color-mix(in srgb, var(--accent) 70%, transparent);
  }
  .cw:hover .play,
  .cw:focus-visible .play {
    transform: translate(-50%, -50%) scale(1);
    opacity: 1;
  }
  .rm {
    position: absolute;
    top: 6px;
    right: 6px;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    color: var(--on-media);
    background: var(--media-scrim);
    opacity: 0;
    transition:
      opacity 0.15s,
      transform 0.12s;
  }
  .cw:hover .rm {
    opacity: 1;
  }
  .rm:hover {
    transform: scale(1.1);
  }
  .info {
    position: absolute;
    left: 10px;
    right: 10px;
    bottom: 9px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    color: var(--on-media);
  }
  .name {
    font-weight: 650;
    font-size: 13.5px;
  }
  .sub {
    font-size: 11.5px;
    color: var(--on-media-2);
    margin-bottom: 5px;
  }
  .progress {
    background: var(--media-line);
  }
  @media (hover: none) {
    .rm {
      opacity: 1;
    }
  }
  /* TV: el foco tiene que verse a 3 m (anillo del acento y un poco mas grande). */
  :global(html[data-tv]) .cw:focus-visible .frame {
    transform: scale(1.06);
    border-color: var(--accent);
    box-shadow:
      0 0 0 3px var(--accent),
      0 18px 34px -16px var(--media-scrim);
  }
  :global([data-lite]) .cw {
    animation: none;
  }
</style>
