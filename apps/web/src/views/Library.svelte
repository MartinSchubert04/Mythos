<script lang="ts">
  // Arriba lo ultimo visto (fila horizontal, terminados incluidos) y abajo los favoritos en
  // grilla. El filtro por tipo vale para las dos.
  import { Heart } from "@lucide/svelte";
  import ContinueCard from "../components/ContinueCard.svelte";
  import PosterCard from "../components/PosterCard.svelte";
  import Row from "../components/Row.svelte";
  import Seg from "../components/Seg.svelte";
  import { api, errText } from "../lib/api";
  import { rise } from "../lib/motion";
  import { uniq } from "../lib/format";
  import { go } from "../lib/router.svelte";
  import { keepScroll } from "../lib/scroll";
  import type { MetaPreview, Progress } from "../lib/types";

  let items = $state<MetaPreview[] | null>(null);
  let error = $state("");
  let kind = $state<"all" | "movie" | "series">("all");

  api
    .library()
    .then((l) => (items = uniq(l)))
    .catch((e) => ((error = errText(e)), (items = [])));

  let recent = $state<Progress[] | null>(null);
  api
    .recentlyWatched()
    .then((l) => (recent = l))
    .catch(() => (recent = []));

  const shown = $derived(items?.filter((m) => kind === "all" || m.type === kind) ?? null);
  const recentShown = $derived(recent?.filter((p) => kind === "all" || p.type === kind) ?? null);
</script>

<div class="page" use:keepScroll={"library"}>
  <div class="page-head">
    <h1>Library</h1>
    {#if items?.length}<span class="sub">{items.length} saved</span>{/if}
    <div class="grow"></div>
    {#if items?.length || recent?.length}
      <Seg
        bind:value={kind}
        options={[
          { value: "all", label: "All" },
          { value: "movie", label: "Movies" },
          { value: "series", label: "Series" },
        ]}
      />
    {/if}
  </div>

  {#if recentShown === null || recentShown.length}
    <div class="recent">
      <Row title="Recently watched" items={recentShown ?? undefined} wide>
        {#snippet card(p, i)}<ContinueCard {p} {i} />{/snippet}
      </Row>
    </div>
    <h2 class="section-title">Saved</h2>
  {/if}

  {#if !shown}
    <div class="poster-grid">
      {#each Array(12) as _, i (i)}<div class="skel ph"></div>{/each}
    </div>
  {:else if error}
    <div class="empty"><h3>Couldn't load your library</h3><span>{error}</span></div>
  {:else if !shown.length}
    <div class="empty" in:rise>
      <Heart size={34} />
      <h3>{items?.length ? "Nothing here" : "Your library is empty"}</h3>
      <span>Use the heart on any movie or series to save it here.</span>
      <button class="btn primary" onclick={() => go("/")}>Browse</button>
    </div>
  {:else}
    <div class="poster-grid">
      {#each shown as m, i (m.id)}<PosterCard item={m} {i} />{/each}
    </div>
  {/if}
</div>

<style>
  .page {
    padding: 26px var(--gutter) 40px;
  }
  /* La fila va de borde a borde (pone su propio margen lateral, como en el inicio). */
  .recent {
    margin: 0 calc(-1 * var(--gutter));
  }
  .ph {
    aspect-ratio: 2 / 3;
    border-radius: var(--radius);
  }
  .empty :global(svg) {
    color: var(--text-3);
  }
</style>
