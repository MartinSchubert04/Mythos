<script lang="ts" module>
  import type { MetaPreview, TopRated } from "../lib/types";
  // Lo ya cargado de cada filtro (todas las paginas): volver del detalle deja la grilla entera y
  // el scroll donde estaba, sin pedir nada.
  const memo = new Map<string, { data: TopRated; items: MetaPreview[] }>();
  function remember(k: string, v: { data: TopRated; items: MetaPreview[] }) {
    memo.delete(k);
    memo.set(k, v);
    if (memo.size > 8) memo.delete(memo.keys().next().value!);
  }
</script>

<script lang="ts">
  // "Top rated": lo mejor calificado de IMDb (GET /api/top/{type}), peliculas o series, con uno o
  // mas generos que tienen que coincidir todos. El filtro vive en la URL (#/top?type=series&
  // genres=Crime,Drama&votes=10000): se puede compartir y "atras" vuelve con el mismo. La
  // grilla trae de a 60 y pide mas al acercarse al final (en TV, al bajar con el control).
  import { LoaderCircle, RefreshCw, SearchX, Trophy, X } from "@lucide/svelte";
  import { untrack } from "svelte";
  import PosterCard from "../components/PosterCard.svelte";
  import Seg from "../components/Seg.svelte";
  import Select from "../components/Select.svelte";
  import { api, errText } from "../lib/api";
  import { uniq } from "../lib/format";
  import { genreName } from "../lib/genres";
  import { date, num, t } from "../lib/i18n.svelte";
  import { rise } from "../lib/motion";
  import { go, route, setQuery } from "../lib/router.svelte";
  import { keepScroll } from "../lib/scroll";
  import type { MediaType } from "../lib/types";

  const PAGE = 60;
  /** IMDb da como mucho 3 generos por titulo (el server rechaza mas). */
  const MAX_GENRES = 3;
  const DEFAULT_VOTES: Record<MediaType, number> = { movie: 25000, series: 10000 };
  const TYPES: { value: MediaType; label: string }[] = $derived([
    { value: "movie", label: t("browse.filter.movies") },
    { value: "series", label: t("browse.filter.series") },
  ]);
  const VOTES = $derived([1000, 10000, 25000, 100000, 500000].map((v) => ({ value: v, label: t("browse.top.votesOpt", { n: num(v, { notation: "compact" }) }) })));

  const type = $derived<MediaType>(route.query.get("type") === "series" ? "series" : "movie");
  const picked = $derived(
    [...new Set((route.query.get("genres") ?? "").split(",").map((g) => g.trim()).filter(Boolean))].slice(0, MAX_GENRES),
  );
  const votes = $derived(Number(route.query.get("votes")) || DEFAULT_VOTES[type]);
  const fkey = $derived(`${type}|${picked.join(",")}|${votes}`);

  let data = $state<TopRated | null>(null);
  let items = $state<MetaPreview[]>([]);
  let loading = $state(true);
  let loadingMore = $state(false);
  let error = $state("");
  let page = $state<HTMLElement>();
  let sentinel = $state<HTMLElement>();
  let ctl: AbortController | null = null;
  let seq = 0;
  let retryTimer = 0;

  /** Cambia el filtro (en la URL, sin agregar entradas al historial). */
  function setFilters(o: { type?: MediaType; genres?: string[]; votes?: number }) {
    const ty = o.type ?? type;
    const g = o.genres ?? picked;
    const explicit = route.query.get("votes");
    const v = o.votes ?? (explicit ? Number(explicit) : undefined);
    const p: Record<string, string> = {};
    if (ty !== "movie") p.type = ty;
    if (g.length) p.genres = g.join(",");
    if (v && v !== DEFAULT_VOTES[ty]) p.votes = String(v);
    setQuery(p);
    page?.scrollTo({ top: 0 });
  }

  function toggle(g: string) {
    setFilters({ genres: picked.includes(g) ? picked.filter((x) => x !== g) : [...picked, g] });
  }

  async function load(k: string, more = false) {
    clearTimeout(retryTimer);
    const hit = memo.get(k);
    if (!more && hit) {
      ({ data, items } = hit);
      loading = false;
      error = "";
      return;
    }
    ctl?.abort();
    ctl = new AbortController();
    const my = ++seq;
    if (more) loadingMore = true;
    else {
      loading = true;
      items = [];
    }
    error = "";
    try {
      const r = await api.top(type, { genres: picked, min_votes: votes, skip: more ? items.length : 0, limit: PAGE }, ctl.signal);
      if (my !== seq) return;
      data = r;
      if (r.state !== "ready") {
        items = [];
        // La primera bajada de IMDb tarda un par de minutos: se vuelve a preguntar solo.
        if (r.state === "preparing") retryTimer = window.setTimeout(() => load(k), 10_000);
        return;
      }
      items = more ? uniq([...items, ...r.items]) : uniq(r.items);
      remember(k, { data: r, items });
    } catch (e) {
      if (my !== seq || (e instanceof DOMException && e.name === "AbortError")) return;
      error = errText(e);
    } finally {
      if (my === seq) loading = loadingMore = false;
    }
  }

  $effect(() => {
    const k = fkey;
    untrack(() => load(k));
  });
  $effect(() => () => (ctl?.abort(), clearTimeout(retryTimer)));

  // Mas titulos al acercarse al final (el contenedor que scrollea es la pagina).
  $effect(() => {
    if (!sentinel || !page) return;
    const io = new IntersectionObserver(
      (es) => {
        if (es.some((e) => e.isIntersecting) && data?.more && !loadingMore && !loading) load(fkey, true);
      },
      { root: page, rootMargin: "900px 0px" },
    );
    io.observe(sentinel);
    return () => io.disconnect();
  });

  const genres = $derived(data?.genres ?? []);
  const full = $derived(picked.length >= MAX_GENRES);
  const updated = $derived(
    data?.updated_at ? date(data.updated_at * 1000, { month: "short", day: "numeric", year: "numeric" }) : "",
  );
</script>

<div class="page" bind:this={page} use:keepScroll={"top"}>
  <div class="page-head">
    <h1>{t("browse.top.title")}</h1>
    {#if data?.state === "ready" && !loading}
      <span class="sub">IMDb · {t("browse.top.titles", { n: data.total, count: num(data.total) })}</span>
    {/if}
    <div class="grow"></div>
    <div class="ctl">
      {#if picked.length}
        <button class="btn ghost sm clear" title={t("browse.top.clearGenres")} aria-label={t("browse.top.clearGenres")} onclick={() => setFilters({ genres: [] })}
          ><X size={14} /><span>{t("browse.clear")}</span></button
        >
      {/if}
      {#if data?.state !== "unavailable"}
        <div class="votes"><Select value={votes} options={VOTES} onchange={(v) => setFilters({ votes: v })} /></div>
      {/if}
      <Seg value={type} onchange={(v) => setFilters({ type: v })} options={TYPES} />
    </div>
  </div>

  {#if data?.state !== "unavailable"}
    <div class="chips" role="group" aria-label={t("browse.top.genres")}>
      {#if genres.length}
        {#each genres as g (g.name)}
          {@const on = picked.includes(g.name)}
          <button
            type="button"
            class="gchip"
            class:on
            aria-pressed={on}
            disabled={!on && (g.count === 0 || full)}
            title={!on && full ? t("browse.top.upTo", { n: MAX_GENRES }) : !on && g.count === 0 ? t("browse.top.noCombo") : undefined}
            onclick={() => toggle(g.name)}
          >
            {genreName(g.name)}
            {#if picked.length && !on && g.count > 0}<span class="n">{num(g.count)}</span>{/if}
            {#if on}<X size={13} />{/if}
          </button>
        {/each}
      {:else}
        {#each Array(10) as _, i (i)}<div class="skel chip-ph" style="width: {58 + ((i * 23) % 40)}px"></div>{/each}
      {/if}
    </div>
  {/if}

  {#if error}
    <div class="empty" in:rise>
      <h3>{t("browse.top.failed")}</h3>
      <span>{error}</span>
      <button class="btn primary" onclick={() => load(fkey)}><RefreshCw size={15} />{t("common.retry")}</button>
    </div>
  {:else if data?.state === "unavailable"}
    <div class="empty" in:rise>
      <Trophy size={34} />
      <h3>{t("browse.top.needsServer")}</h3>
      <span>{t("browse.top.needsServerBody")}</span>
      <button class="btn primary" onclick={() => go("/")}>{t("browse.lib.browse")}</button>
    </div>
  {:else if data?.state === "preparing"}
    <div class="empty" in:rise>
      <LoaderCircle size={34} class="spin" />
      <h3>{t("browse.top.preparing")}</h3>
      <span>{t("browse.top.preparingBody")}</span>
    </div>
  {:else if loading}
    <div class="poster-grid">
      {#each Array(18) as _, i (i)}<div class="skel ph"></div>{/each}
    </div>
  {:else if !items.length}
    <div class="empty" in:rise>
      <SearchX size={34} />
      <h3>{t("browse.top.nothing")}</h3>
      <span>{t("browse.top.nothingHint")}</span>
      <div class="row">
        {#if picked.length}<button class="btn primary" onclick={() => setFilters({ genres: [] })}>{t("browse.top.clearGenres")}</button>{/if}
        {#if votes > 1000}<button class="btn" onclick={() => setFilters({ votes: 1000 })}>{t("browse.top.anyVotes")}</button>{/if}
      </div>
    </div>
  {:else}
    <div class="poster-grid">
      {#each items as m, i (m.id)}<PosterCard item={m} i={i % PAGE} eager={i < 12} pinRating />{/each}
      {#if loadingMore}
        {#each Array(6) as _, i (i)}<div class="skel ph"></div>{/each}
      {/if}
    </div>
    {#if !data?.more}
      <div class="foot faint">{updated ? t("browse.top.footUpdated", { date: updated }) : t("browse.top.foot")} · {t("browse.top.votesOpt", { n: num(votes) })}</div>
    {/if}
  {/if}
  <div class="sentinel" bind:this={sentinel} aria-hidden="true"></div>
</div>

<style>
  .page {
    padding: 26px var(--gutter) 40px;
  }
  .page-head {
    flex-wrap: wrap;
  }
  .ctl {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .votes {
    width: 142px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 7px;
    margin: -2px 0 22px;
  }
  .gchip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 13px;
    border-radius: 999px;
    font-size: 12.5px;
    font-weight: 560;
    white-space: nowrap;
    color: var(--text-2);
    background: var(--panel-2);
    border: var(--border) solid var(--line);
    transition:
      background 0.15s,
      color 0.15s,
      border-color 0.15s,
      transform 0.12s;
  }
  .gchip:hover {
    color: var(--text);
    border-color: var(--line-strong);
  }
  .gchip:active {
    transform: scale(0.95);
  }
  .gchip.on {
    color: var(--accent-ink);
    background: var(--primary-bg);
    border-color: transparent;
  }
  .gchip:disabled {
    opacity: 0.38;
    pointer-events: none;
  }
  .gchip .n {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  :global([data-theme="poolsuite"]) .gchip.on {
    border-color: var(--line);
    box-shadow: 2px 2px 0 var(--line);
  }
  :global([data-theme="athanor"]) .gchip {
    transition-timing-function: steps(2);
  }
  .chip-ph {
    height: 32px;
    border-radius: 999px;
  }
  .ph {
    aspect-ratio: 2 / 3;
    border-radius: var(--radius);
  }
  .empty :global(svg) {
    color: var(--text-3);
  }
  .foot {
    margin-top: 28px;
    text-align: center;
    font-size: 12px;
  }
  .sentinel {
    height: 1px;
  }

  /* Celular: tipo, votos y "Clear" (solo el icono) en una fila, y los generos en una sola linea
     que se desliza de costado (si no, ocupan media pantalla). */
  @media (max-width: 700px) {
    .ctl {
      width: 100%;
    }
    .votes {
      flex: 1;
      min-width: 0;
    }
    .ctl :global(.seg2) {
      order: -1;
    }
    .clear {
      order: 1;
    }
    .clear span {
      display: none;
    }
    .chips {
      flex-wrap: nowrap;
      overflow-x: auto;
      margin: 0 calc(-1 * var(--gutter));
      padding: 2px var(--gutter) 4px;
      scrollbar-width: none;
    }
    .chips::-webkit-scrollbar {
      display: none;
    }
  }
</style>
