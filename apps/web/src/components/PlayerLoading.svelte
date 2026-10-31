<script lang="ts">
  // Pantalla de carga del reproductor con datos vivos del server: peers, velocidad, cuanto hay
  // listo. Antes del primer cuadro ocupa todo (con el fondo del titulo); si el video se queda
  // esperando a mitad de camino, es un velo encima del cuadro congelado.
  import { Users, Gauge, Download } from "@lucide/svelte";
  import { speed } from "../lib/format";
  import { fadeSlow } from "../lib/motion";
  import type { Snippet } from "svelte";
  import type { MediaStatus, PlayMode } from "../lib/types";

  let {
    full,
    background,
    logo,
    title,
    subtitle = "",
    text,
    pct,
    status,
    mode,
    children,
  }: {
    full: boolean;
    background?: string;
    logo?: string;
    title: string;
    subtitle?: string;
    text: string;
    /** 0..100, o null si no se sabe todavia (gira sin parar). */
    pct: number | null;
    status: MediaStatus | null;
    mode?: PlayMode;
    /** En lugar del anillo y el texto (el dialogo de retomar va debajo del logo). */
    children?: Snippet;
  } = $props();

  let logoOk = $state(true);
  const R = 26;
  const C = 2 * Math.PI * R;
</script>

<div class="loading" class:full transition:fadeSlow={{ duration: full ? 500 : 220 }}>
  {#if full}
    {#if background}<img class="bg" src={background} alt="" />{/if}
    <div class="veil"></div>
  {/if}
  <div class="center">
    {#if full}
      {#if logo && logoOk}
        <img class="logo" src={logo} alt={title} onerror={() => (logoOk = false)} />
      {:else}
        <h1 class="display-title name">{title}</h1>
      {/if}
      {#if subtitle}<div class="sub">{subtitle}</div>{/if}
    {/if}

    {#if children}
      {@render children()}
    {:else}
    <div class="ring" class:indet={pct === null}>
      <svg viewBox="0 0 64 64" width="64" height="64" aria-hidden="true">
        <circle cx="32" cy="32" r={R} class="track" />
        <circle
          cx="32"
          cy="32"
          r={R}
          class="arc"
          stroke-dasharray={C}
          stroke-dashoffset={pct === null ? C * 0.72 : C * (1 - Math.max(0.08, pct / 100))}
        />
      </svg>
      {#if pct !== null}<span class="pct">{Math.round(pct)}</span>{/if}
    </div>
    <div class="text" aria-live="polite">{text}</div>
    {/if}

    {#if status && status.state !== "metadata"}
      <div class="stats">
        <span title="Connected peers"><Users size={13} />{status.peers} peer{status.peers === 1 ? "" : "s"}</span>
        <span title="Download speed"><Gauge size={13} />{speed(status.down_bps)}</span>
        {#if status.total_bytes}
          <span title="Downloaded"><Download size={13} />{Math.round(status.progress * 100)}%</span>
        {/if}
        {#if mode && mode !== "direct"}<span class="mode">{mode === "remux" ? "Remux" : "Converting"}</span>{/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .loading {
    position: absolute;
    inset: 0;
    z-index: 4;
    display: grid;
    place-items: center;
    color: var(--on-media);
    background: color-mix(in srgb, var(--media-scrim) 60%, transparent);
    pointer-events: none;
  }
  .loading.full {
    background: var(--media-bg);
    pointer-events: auto;
  }
  .bg {
    position: absolute;
    inset: -40px;
    width: calc(100% + 80px);
    height: calc(100% + 80px);
    object-fit: cover;
    filter: blur(28px) saturate(1.2);
    opacity: 0.55;
    animation: breathe 9s ease-in-out infinite alternate;
  }
  @keyframes breathe {
    to {
      transform: scale(1.06);
    }
  }
  .veil {
    position: absolute;
    inset: 0;
    background: radial-gradient(ellipse at center, transparent 0%, var(--media-scrim) 75%);
  }
  .center {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    padding: 20px;
    text-align: center;
    animation: rise 0.5s cubic-bezier(0.2, 0.8, 0.2, 1) both;
  }
  .logo {
    max-width: min(440px, 70vw);
    max-height: 140px;
    object-fit: contain;
    margin-bottom: 8px;
    filter: drop-shadow(0 8px 30px var(--media-scrim));
  }
  .name {
    font-size: clamp(26px, 4vw, 46px);
    text-shadow: 0 4px 30px var(--media-scrim);
  }
  .sub {
    margin-top: -6px;
    color: var(--on-media-2);
    font-size: 14px;
  }
  .ring {
    position: relative;
    width: 64px;
    height: 64px;
    margin-top: 12px;
  }
  .ring svg {
    transform: rotate(-90deg);
  }
  .indet svg {
    animation: spin 1.1s linear infinite;
  }
  circle {
    fill: none;
    stroke-width: 4;
  }
  .track {
    stroke: var(--media-line);
  }
  .arc {
    stroke: var(--accent);
    stroke-linecap: round;
    transition: stroke-dashoffset 0.6s cubic-bezier(0.2, 0.8, 0.2, 1);
    filter: drop-shadow(0 0 6px color-mix(in srgb, var(--accent) 70%, transparent));
  }
  .pct {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    font: 700 13px var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .text {
    font-size: 15px;
    font-weight: 560;
    min-height: 1.4em;
    text-shadow: 0 2px 14px var(--media-scrim);
  }
  .stats {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px 16px;
    font-size: 12.5px;
    color: var(--on-media-2);
    font-variant-numeric: tabular-nums;
  }
  .stats span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .mode {
    padding: 0 6px;
    border: 1px solid var(--media-line);
    border-radius: 5px;
    font-size: 11px;
  }
</style>
