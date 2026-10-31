<script lang="ts">
  // Lista de streams: calidad, tamaño, seeders (con color segun cuantos), fuente, tags como chips,
  // idiomas. Los que estan en cache (instantaneos) van destacados.
  import { Check, HardDrive, Users, Zap } from "@lucide/svelte";
  import { bytes, seedTone } from "../lib/format";
  import type { Stream } from "../lib/types";

  let {
    streams,
    current = "",
    onpick,
    onmedia = false,
  }: { streams: Stream[]; current?: string; onpick: (s: Stream) => void; onmedia?: boolean } = $props();

  const qClass = (q: string) => (q === "2160p" ? "uhd" : q === "1080p" ? "fhd" : q === "720p" ? "hd" : "sd");
</script>

<div class="list" class:onmedia>
  {#each streams as s, i (s.key)}
    <button class="stream" class:cached={s.cached} class:on={s.key === current} style="--i: {i}" onclick={() => onpick(s)}>
      <span class="q {qClass(s.quality)}">{s.quality === "unknown" ? "?" : s.quality.replace("p", "")}<small>{s.quality.endsWith("p") ? "p" : ""}</small></span>
      <span class="main">
        <span class="title ellipsis" title={s.title}>{s.title}</span>
        <span class="facts">
          {#if s.cached}<span class="chip ok instant"><Zap size={11} fill="currentColor" />Instant</span>{/if}
          {#if s.size_bytes}<span class="fact"><HardDrive size={12} />{bytes(s.size_bytes)}</span>{/if}
          {#if s.seeders != null}<span class="fact seed {seedTone(s.seeders)}"><Users size={12} />{s.seeders}</span>{/if}
          {#if s.source}<span class="fact src">{s.source}</span>{/if}
          {#if s.languages.length}<span class="fact langs">{s.languages.join(" ")}</span>{/if}
        </span>
        {#if s.tags.length}
          <span class="tags">
            {#each s.tags as t (t)}<span class="chip" class:accent={t === "HDR" || t === "DV" || t === "REMUX"}>{t}</span>{/each}
          </span>
        {/if}
      </span>
      {#if s.key === current}<span class="cur"><Check size={16} /></span>{:else if i === 0}<span class="best">Best</span>{/if}
    </button>
  {:else}
    <div class="none">
      <b>No streams found</b>
      <span>It may be too new, or the size and quality limits in Settings leave nothing out. Try again in a while.</span>
    </div>
  {/each}
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .stream {
    display: flex;
    align-items: center;
    gap: 13px;
    padding: 11px 13px;
    text-align: left;
    border-radius: var(--radius);
    background: var(--panel);
    border: var(--border) solid var(--line);
    animation: rise 0.3s cubic-bezier(0.2, 0.8, 0.2, 1) both;
    animation-delay: calc(min(var(--i), 14) * 28ms);
    transition:
      border-color 0.15s,
      background 0.15s,
      transform 0.18s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .stream:hover {
    border-color: var(--line-strong);
    background: var(--panel-2);
    transform: translateX(3px);
  }
  .stream:active {
    transform: scale(0.99);
  }
  .stream.cached {
    border-color: color-mix(in srgb, var(--ok) 45%, var(--line));
    background: color-mix(in srgb, var(--ok) 6%, var(--panel));
  }
  .stream.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }
  .q {
    flex: none;
    width: 52px;
    height: 40px;
    display: grid;
    place-items: center;
    grid-auto-flow: column;
    justify-content: center;
    align-items: baseline;
    padding-top: 9px;
    border-radius: var(--radius-sm);
    font-weight: 780;
    font-size: 15px;
    font-variant-numeric: tabular-nums;
    background: var(--panel-2);
    color: var(--text-2);
    border: var(--border) solid var(--line);
  }
  .q small {
    font-size: 10px;
    font-weight: 600;
    margin-left: 1px;
  }
  .q.uhd {
    color: var(--accent-ink);
    background: var(--primary-bg);
    border-color: transparent;
  }
  .q.fhd {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .title {
    font-weight: 560;
    font-size: 13px;
    color: var(--text);
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 12px;
    font-size: 12px;
    color: var(--text-2);
  }
  .fact {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .seed.ok {
    color: var(--ok);
  }
  .seed.warn {
    color: var(--warn);
  }
  .seed.bad {
    color: var(--danger);
  }
  .src {
    color: var(--text-3);
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .tags .chip {
    height: 18px;
    font-size: 10.5px;
  }
  .instant {
    height: 18px;
    font-size: 10.5px;
    border-color: color-mix(in srgb, var(--ok) 45%, transparent);
  }
  .cur {
    color: var(--accent);
    display: grid;
  }
  .best {
    flex: none;
    font-size: 10.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--accent);
  }
  .none {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 30px 10px;
    text-align: center;
    color: var(--text-2);
    line-height: 1.5;
  }
  .none b {
    color: var(--text);
  }
  /* Encima del video: vidrio oscuro. */
  .onmedia .stream {
    background: color-mix(in srgb, var(--media-scrim) 70%, transparent);
    border-color: var(--media-line);
    color: var(--on-media);
  }
  .onmedia .stream:hover {
    background: color-mix(in srgb, var(--on-media) 10%, var(--media-scrim));
  }
  .onmedia .title {
    color: var(--on-media);
  }
  .onmedia .facts,
  .onmedia .src {
    color: var(--on-media-2);
  }
  .onmedia .q {
    background: var(--media-line);
    border-color: transparent;
    color: var(--on-media);
  }
  .onmedia .q.uhd {
    background: var(--primary-bg);
    color: var(--accent-ink);
  }
</style>
