<script lang="ts">
  // Busqueda en vivo (con debounce): peliculas y series por separado. "/" o Ctrl K vienen aca.
  import { LoaderCircle, Search, SearchX, X } from "@lucide/svelte";
  import PosterCard from "../components/PosterCard.svelte";
  import { api, errText } from "../lib/api";
  import { uniq } from "../lib/format";
  import { rise } from "../lib/motion";
  import { onMount, tick } from "svelte";
  import { route, searchIntent, setQuery } from "../lib/router.svelte";
  import { keepScroll } from "../lib/scroll";
  import type { MetaPreview } from "../lib/types";

  const initial = route.query.get("q") ?? "";
  let q = $state(initial);
  let res = $state<{ movies: MetaPreview[]; series: MetaPreview[] } | null>(null);
  let busy = $state(false);
  let error = $state("");
  let input = $state<HTMLInputElement>();
  let timer = 0;
  let ctl: AbortController | null = null;

  // Foco al montar (y lo que se escribio mientras la vista cargaba, si vino por "/" o Ctrl K).
  onMount(async () => {
    const typed = searchIntent.pending ? searchIntent.typed : "";
    searchIntent.pending = false;
    searchIntent.typed = "";
    if (typed) {
      q = typed;
      onInput();
    }
    await tick();
    input?.focus();
    requestAnimationFrame(() => {
      input?.focus();
      input?.setSelectionRange(q.length, q.length);
    });
  });

  async function run(text: string) {
    ctl?.abort();
    const t = text.trim();
    if (t.length < 2) {
      res = null;
      busy = false;
      return;
    }
    ctl = new AbortController();
    busy = true;
    error = "";
    try {
      const r = await api.search(t, ctl.signal);
      res = { movies: uniq(r.movies ?? []), series: uniq(r.series ?? []) };
    } catch (e) {
      if (!(e instanceof DOMException && e.name === "AbortError")) error = errText(e);
    } finally {
      busy = false;
    }
  }

  function onInput() {
    clearTimeout(timer);
    setQuery(q ? { q } : {});
    timer = window.setTimeout(() => run(q), 260);
  }

  if (initial) run(initial);

  // Sin busqueda: lo popular, para que la pantalla no quede vacia (sale de la cache del inicio).
  let popular = $state<MetaPreview[]>([]);
  Promise.all([api.catalog("movie", "top"), api.catalog("series", "top")])
    .then(([m, s]) => (popular = uniq(m.slice(0, 9).flatMap((x, i) => (s[i] ? [x, s[i]] : [x])))))
    .catch(() => {});

  const empty = $derived(res && !res.movies.length && !res.series.length);
</script>

<div class="page" use:keepScroll={"search"}>
  <div class="head">
    <div class="inbox big" class:busy>
      {#if busy}<LoaderCircle size={18} class="spin" />{:else}<Search size={18} />{/if}
      <input
        id="search-input"
        bind:this={input}
        bind:value={q}
        oninput={onInput}
        placeholder="Movies and series"
        autocomplete="off"
        spellcheck="false"
        onkeydown={(e) => e.key === "Escape" && q && (e.preventDefault(), (q = ""), onInput())}
      />
      {#if q}
        <button class="icon-btn" aria-label="Clear" onclick={() => ((q = ""), onInput(), input?.focus())}><X size={16} /></button>
      {/if}
    </div>
  </div>

  {#if error}
    <div class="empty"><h3>Search failed</h3><span>{error}</span></div>
  {:else if !res}
    {#if busy}
      <div class="poster-grid">
        {#each Array(12) as _, i (i)}<div class="skel ph"></div>{/each}
      </div>
    {:else}
      <div class="hint-line faint" in:rise>Press <kbd>/</kbd> or <kbd>Ctrl K</kbd> from anywhere to search.</div>
      {#if popular.length}
        <h2 class="section-title">Popular right now</h2>
        <div class="poster-grid">
          {#each popular as m, i (m.type + m.id)}<PosterCard item={m} {i} />{/each}
        </div>
      {/if}
    {/if}
  {:else if empty}
    <div class="empty" in:rise>
      <SearchX size={34} />
      <h3>Nothing found for "{q.trim()}"</h3>
      <span>Try another title, the original title, or check the spelling.</span>
    </div>
  {:else}
    {#each [{ t: "Movies", l: res.movies }, { t: "Series", l: res.series }] as g (g.t)}
      {#if g.l.length}
        <h2 class="section-title">{g.t} <span class="n">{g.l.length}</span></h2>
        <div class="poster-grid">
          {#each g.l as m, i (m.id)}<PosterCard item={m} {i} />{/each}
        </div>
      {/if}
    {/each}
  {/if}
</div>

<style>
  .page {
    padding: 26px var(--gutter) 40px;
  }
  .head {
    position: sticky;
    top: -26px;
    z-index: 5;
    margin: -26px calc(-1 * var(--gutter)) 8px;
    padding: 26px var(--gutter) 14px;
    background: linear-gradient(180deg, var(--bg) 70%, transparent);
  }
  :global([data-theme="warp"]) .head,
  :global([data-theme="haze"]) .head,
  :global([data-theme="poolsuite"]) .head {
    background: none;
  }
  .inbox.big {
    height: 50px;
    padding: 0 14px;
    gap: 11px;
    border-radius: var(--radius);
    font-size: 16px;
    max-width: 720px;
    background: var(--panel);
    box-shadow: var(--card-shadow);
  }
  .inbox.big input {
    height: 48px;
    font-size: 16px;
  }
  .section-title .n {
    color: var(--text-3);
    font-weight: 500;
    margin-left: 4px;
  }
  .hint-line {
    font-size: 12.5px;
    margin: -2px 0 6px;
  }
  .ph {
    aspect-ratio: 2 / 3;
    border-radius: var(--radius);
  }
  .empty :global(svg) {
    color: var(--text-3);
  }
</style>
