<script lang="ts">
  // Inicio: el hero y las filas. Las filas de abajo piden sus datos recien al acercarse.
  import ContinueCard from "../components/ContinueCard.svelte";
  import Hero from "../components/Hero.svelte";
  import NewEpisodeCard from "../components/NewEpisodeCard.svelte";
  import PosterCard from "../components/PosterCard.svelte";
  import Row from "../components/Row.svelte";
  import { api } from "../lib/api";
  import { lastNewEpisodes, newEpisodes, type NewEp } from "../lib/newEpisodes";
  import { keepScroll } from "../lib/scroll";
  import { attempt } from "../lib/toast.svelte";
  import type { MetaPreview, Progress } from "../lib/types";

  let featured = $state<MetaPreview[] | null>(null);
  let cont = $state<Progress[] | null>(null);

  api
    .catalog("movie", "top")
    .then(async (movies) => {
      const series = await api.catalog("series", "top").catch(() => [] as MetaPreview[]);
      // Peliculas y series intercaladas, solo las que tienen fondo.
      const pick = (l: MetaPreview[]) => l.filter((m) => m.background).slice(0, 4);
      const a = pick(movies);
      const b = pick(series);
      featured = a.flatMap((m, i) => (b[i] ? [m, b[i]] : [m])).slice(0, 7);
    })
    .catch(() => (featured = []));

  api
    .continueWatching()
    .then((l) => (cont = l))
    .catch(() => (cont = []));

  // Episodios nuevos de las series de la Library: aparece solo si hay alguno (sin esqueleto, para
  // que la fila no se arme y desaparezca). Lo de la ultima vez se muestra enseguida.
  let fresh = $state<NewEp[]>(lastNewEpisodes());
  newEpisodes()
    .then((l) => (fresh = l))
    .catch(() => {});

  async function remove(p: Progress) {
    if (await attempt(() => api.deleteProgress(p.meta_id))) cont = cont?.filter((x) => x.meta_id !== p.meta_id) ?? null;
  }

  /** Play desde el hero: la serie sigue donde quedo (o el primer episodio). */
  function playHref(m: MetaPreview) {
    if (m.type === "movie") return `/watch/movie/${m.id}`;
    const p = cont?.find((x) => x.series_id === m.id);
    return `/watch/series/${p?.meta_id ?? `${m.id}:1:1`}`;
  }

  const ROWS: { title: string; load: () => Promise<MetaPreview[]> }[] = [
    { title: "Popular movies", load: () => api.catalog("movie", "top") },
    { title: "Popular series", load: () => api.catalog("series", "top") },
    { title: "Top rated movies", load: () => api.catalog("movie", "imdbRating") },
    { title: "Action", load: () => api.catalog("movie", "top", { genre: "Action" }) },
    { title: "Top rated series", load: () => api.catalog("series", "imdbRating") },
    { title: "Comedy", load: () => api.catalog("movie", "top", { genre: "Comedy" }) },
    { title: "Sci-Fi", load: () => api.catalog("movie", "top", { genre: "Sci-Fi" }) },
    { title: "Animation", load: () => api.catalog("movie", "top", { genre: "Animation" }) },
    { title: "Crime series", load: () => api.catalog("series", "top", { genre: "Crime" }) },
    { title: "Horror", load: () => api.catalog("movie", "top", { genre: "Horror" }) },
  ];
</script>

<div class="page home" use:keepScroll={"home"}>
  <Hero items={featured} {playHref} />
  <div class="rows">
    {#if cont === null || cont.length}
      <Row title="Continue watching" items={cont ?? undefined} wide>
        {#snippet card(p, i)}<ContinueCard {p} {i} onremove={remove} />{/snippet}
      </Row>
    {/if}
    {#if fresh.length}
      <Row title="New episodes" items={fresh} wide>
        {#snippet card(n, i)}<NewEpisodeCard {n} {i} />{/snippet}
      </Row>
    {/if}
    {#each ROWS as r, ri (r.title)}
      <Row title={r.title} load={r.load}>
        {#snippet card(m, i)}<PosterCard item={m} {i} eager={ri === 0 && i < 6} />{/snippet}
      </Row>
    {/each}
  </div>
</div>

<style>
  .home {
    padding: 0 0 40px;
  }
  .rows {
    position: relative;
    margin-top: clamp(-70px, -7vh, -40px);
    z-index: 2;
  }
</style>
