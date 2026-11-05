<script lang="ts">
  // Lista de streams: calidad, tamaño, seeders (con color segun cuantos), fuente, tags como chips,
  // idiomas. Los que estan en cache (instantaneos) van destacados. Con `linkFor`, un menu chico
  // (⋯ al pasar el mouse, clic derecho o mantener apretado) para copiar el link directo o abrirlo
  // en VLC (desktop). En TV no hay menu: copiar un link o abrir VLC no tiene sentido ahi.
  // Si el server lo recomienda por lo que aprendio del usuario (`reasons`, server/src/taste.rs),
  // una linea dice por que ("Same group as last time · Spanish subs").
  import { Check, Copy, Ellipsis, ExternalLink, HardDrive, Users, Zap } from "@lucide/svelte";
  import { errText } from "../lib/api";
  import { canVlc, copyText, openInVlc } from "../lib/external";
  import { bytes, seedTone } from "../lib/format";
  import { press } from "../lib/press";
  import { langName } from "../lib/prefs.svelte";
  import { toast } from "../lib/toast.svelte";
  import { tv } from "../lib/tv.svelte";
  import type { Stream } from "../lib/types";
  import PopMenu, { type PopItem } from "./PopMenu.svelte";

  let {
    streams,
    current = "",
    onpick,
    onmedia = false,
    linkFor,
  }: {
    streams: Stream[];
    current?: string;
    onpick: (s: Stream) => void;
    onmedia?: boolean;
    /** El link directo (con el token) de un stream: prende el menu de cada uno. */
    linkFor?: (s: Stream) => Promise<string>;
  } = $props();

  let menu = $state<{ s: Stream; x: number; y: number } | null>(null);
  const withMenu = $derived(!!linkFor && !tv.on);
  const WARN = "The link includes your session: anyone who has it can watch as you. Don't share it.";
  const items = (s: Stream): PopItem[] => [
    ...(canVlc
      ? [
          {
            label: "Open in VLC",
            icon: ExternalLink,
            title: WARN,
            run: async () => {
              try {
                await openInVlc(await linkFor!(s));
              } catch (e) {
                toast(String(e).includes("VLC") ? String(e) : `Couldn't open VLC: ${errText(e)}`, true);
              }
            },
          },
        ]
      : []),
    {
      label: "Copy stream link",
      icon: Copy,
      title: WARN,
      run: async () => {
        try {
          await copyText(await linkFor!(s));
          toast("Link copied. It includes your session, so don't share it.");
        } catch (e) {
          toast(errText(e), true);
        }
      },
    },
  ];

  function openAt(s: Stream, x: number, y: number) {
    menu = { s, x, y };
  }
  function fromButton(e: MouseEvent, s: Stream) {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    if (menu?.s.key === s.key) menu = null;
    else openAt(s, r.right - 200, r.bottom + 4);
  }

  /** Los motivos del server en palabras. */
  function why(rs: string[] | undefined): string {
    return (rs ?? [])
      .map((r) =>
        r === "same_group"
          ? "Same group as last time"
          : r === "liked_group"
            ? "A group you often pick"
            : r === "dual_audio"
              ? "Dual audio"
              : r.startsWith("subs:")
                ? `${langName(r.slice(5))} subs`
                : "",
      )
      .filter(Boolean)
      .join(" · ");
  }

  const qClass = (q: string) => (q === "2160p" ? "uhd" : q === "1080p" ? "fhd" : q === "720p" ? "hd" : "sd");
</script>

<div class="list" class:onmedia>
  {#each streams as s, i (s.key)}
    <div class="item">
      <button
        class="stream"
        class:cached={s.cached}
        class:on={s.key === current}
        class:has-more={withMenu}
        style="--i: {i}"
        onclick={() => onpick(s)}
        use:press={withMenu ? (x, y) => openAt(s, x, y) : undefined}
      >
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
          {#if why(s.reasons)}<span class="why">{why(s.reasons)}</span>{/if}
          {#if s.tags.length}
            <span class="tags">
              {#each s.tags as t (t)}<span class="chip" class:accent={t === "HDR" || t === "DV" || t === "REMUX"}>{t}</span>{/each}
            </span>
          {/if}
        </span>
        {#if s.key === current}<span class="cur"><Check size={16} /></span>{:else if i === 0}<span class="best">Best</span>{/if}
      </button>
      {#if withMenu}
        <button class="more" class:open={menu?.s.key === s.key} title="More" aria-label="More options" aria-haspopup="menu" onclick={(e) => fromButton(e, s)}>
          <Ellipsis size={16} />
        </button>
      {/if}
    </div>
  {:else}
    <div class="none">
      <b>No streams found</b>
      <span>It may be too new, or the size and quality limits in Settings leave nothing out. Try again in a while.</span>
    </div>
  {/each}
</div>

{#if menu}
  <PopMenu x={menu.x} y={menu.y} items={items(menu.s)} {onmedia} label="Stream options" onclose={() => (menu = null)} />
{/if}

<style>
  .item {
    position: relative;
  }
  .item > .stream {
    width: 100%;
  }
  /* El ⋯ aparece al pasar el mouse, arriba a la derecha (no tapa "Best" ni el tilde). */
  .more {
    position: absolute;
    top: 4px;
    right: 4px;
    display: grid;
    place-items: center;
    width: 28px;
    height: 24px;
    border-radius: var(--radius-sm);
    color: var(--text-2);
    background: var(--panel-2);
    border: var(--border) solid var(--line);
    opacity: 0;
    transition:
      opacity 0.15s,
      background 0.15s;
  }
  .item:hover .more,
  .more:focus-visible,
  .more.open {
    opacity: 1;
  }
  .more:hover {
    color: var(--text);
    background: var(--hover);
  }
  /* En pantallas tactiles se mantiene apretado el stream: sin boton extra. */
  @media (hover: none) {
    .more {
      display: none;
    }
    .stream.has-more {
      -webkit-touch-callout: none;
    }
  }
  .onmedia .more {
    color: var(--on-media-2);
    background: var(--media-scrim);
    border-color: var(--media-line);
  }
  .onmedia .more:hover {
    color: var(--on-media);
    background: var(--media-line);
  }
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
    animation: rise 0.3s cubic-bezier(0.2, 0.8, 0.2, 1) backwards;
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
  .why {
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
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
