<script lang="ts">
  // Tarjeta de "New episodes": la imagen del episodio (o el fondo de la serie si se esconden los
  // spoilers), la serie, el episodio y hace cuanto salio. Clic: al reproductor.
  import { Play } from "@lucide/svelte";
  import { epCode, sized } from "../lib/format";
  import type { NewEp } from "../lib/newEpisodes";
  import { prefs } from "../lib/prefs.svelte";
  import { go, loadView } from "../lib/router.svelte";
  import Img from "./Img.svelte";

  let { n, i = 0 }: { n: NewEp; i?: number } = $props();

  const img = $derived(prefs.hideSpoilers || !n.ep.thumbnail ? sized(n.series.background ?? n.series.poster, "medium") : n.ep.thumbnail);
  const href = $derived(`/watch/series/${n.ep.id}`);
  const ago = $derived.by(() => {
    const d = Math.floor((Date.now() - Date.parse(n.ep.released)) / 86_400_000);
    return d <= 0 ? "Out today" : d === 1 ? "Out yesterday" : `${d} days ago`;
  });
</script>

<a
  class="ne"
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
    <Img src={img} alt="" fallback={n.series.name} />
    <div class="shade"></div>
    <span class="play"><Play size={20} fill="currentColor" /></span>
    <span class="new">New{n.more ? ` · ${n.more + 1}` : ""}</span>
    <div class="info">
      <span class="name ellipsis">{n.series.name}</span>
      <span class="sub ellipsis">{epCode(n.ep.season, n.ep.episode)}{n.ep.name ?` · ${n.ep.name}` : ""} · {ago}</span>
    </div>
  </div>
</a>

<style>
  .ne {
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
  .ne:hover .frame,
  .ne:focus-visible .frame {
    transform: translateY(-3px) scale(1.025);
    border-color: color-mix(in srgb, var(--accent) 55%, var(--line));
    box-shadow:
      0 18px 34px -16px var(--media-scrim),
      0 10px 40px -14px color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .ne:focus-visible {
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
  }
  .ne:hover .play,
  .ne:focus-visible .play {
    transform: translate(-50%, -50%) scale(1);
    opacity: 1;
  }
  .new {
    position: absolute;
    top: 7px;
    left: 7px;
    padding: 2px 7px;
    border-radius: 999px;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--accent-ink);
    background: var(--primary-bg);
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
  }
</style>
