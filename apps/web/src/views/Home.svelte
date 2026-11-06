<script lang="ts">
  // Inicio: el hero y las filas. Las filas de abajo piden sus datos recien al acercarse.
  import ContinueCard from "../components/ContinueCard.svelte";
  import Hero from "../components/Hero.svelte";
  import NewEpisodeCard from "../components/NewEpisodeCard.svelte";
  import PosterCard from "../components/PosterCard.svelte";
  import Row from "../components/Row.svelte";
  import { api } from "../lib/api";
  import { t, type Key } from "../lib/i18n.svelte";
  import { lastNewEpisodes, newEpisodes, type NewEp } from "../lib/newEpisodes";
  import { keepScroll } from "../lib/scroll";
  import { attempt } from "../lib/toast.svelte";
  import type { MediaType, MetaPreview, Progress } from "../lib/types";

  let featured = $state<MetaPreview[] | null>(null);
  let cont = $state<Progress[] | null>(null);

  /** "Para vos" (GET /api/recommended): por los gustos del usuario, o al azar de lo mejor de IMDb
   *  si todavia no vio lo suficiente; cambia cada 6 h. `null` si el server no lo tiene (bajando los
   *  datos de IMDb, el embebido de las apps, uno viejo sin la ruta): ahi se usa lo de antes. */
  const forYou = (type: MediaType) =>
    api
      .recommended(type)
      .then((r) => (r.state === "ready" && r.items.length ? r.items : null))
      .catch(() => null);
  // Pedidos ya: el destacado los usa y las filas "para vos" toman lo mismo de la cache.
  const forYouMovies = forYou("movie");
  const forYouSeries = forYou("series");

  // El destacado: lo recomendado (los primeros traen descripcion y logo del server) o, sin eso,
  // lo popular de Cinemeta (que queda en cache para las filas "Popular", que piden lo mismo).
  Promise.all([forYouMovies, forYouSeries])
    .then(([m, s]) =>
      m || s
        ? [m ?? [], s ?? []]
        : Promise.all([api.catalog("movie", "top"), api.catalog("series", "top").catch(() => [] as MetaPreview[])]),
    )
    .then(([movies, series]) => {
      // Peliculas y series intercaladas, solo las que tienen fondo.
      const pick = (l: MetaPreview[]) => l.filter((m) => m.background).slice(0, 4);
      const a = pick(movies);
      const b = pick(series);
      featured = (a.length ? a.flatMap((m, i) => (b[i] ? [m, b[i]] : [m])) : b).slice(0, 7);
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

  /** Lo mejor de IMDb (GET /api/top). Si el server todavia no lo tiene (bajando, o el embebido
   *  de las apps, o uno viejo sin la ruta), el catalogo "imdbRating" de Cinemeta. */
  const topRated = (type: MediaType) => () =>
    api
      .top(type, { limit: 40 })
      .then((r) => (r.state === "ready" && r.items.length ? r.items : Promise.reject()))
      .catch(() => api.catalog(type, "imdbRating"));

  // `title` es la clave del texto (se traduce al pintar).
  // Las "para vos" sin datos quedan vacias y no se dibujan (Row las esconde).
  const ROWS: { title: Key; load: () => Promise<MetaPreview[]>; href?: string }[] = [
    { title: "browse.home.forYouMovies", load: () => forYouMovies.then((l) => l ?? []) },
    { title: "browse.home.forYouSeries", load: () => forYouSeries.then((l) => l ?? []) },
    { title: "browse.home.popularMovies", load: () => api.catalog("movie", "top") },
    { title: "browse.home.popularSeries", load: () => api.catalog("series", "top") },
    { title: "browse.home.topMovies", load: topRated("movie"), href: "/top" },
    { title: "browse.genre.Action", load: () => api.catalog("movie", "top", { genre: "Action" }) },
    { title: "browse.home.topSeries", load: topRated("series"), href: "/top?type=series" },
    { title: "browse.genre.Comedy", load: () => api.catalog("movie", "top", { genre: "Comedy" }) },
    { title: "browse.genre.Sci-Fi", load: () => api.catalog("movie", "top", { genre: "Sci-Fi" }) },
    { title: "browse.genre.Animation", load: () => api.catalog("movie", "top", { genre: "Animation" }) },
    { title: "browse.home.crimeSeries", load: () => api.catalog("series", "top", { genre: "Crime" }) },
    { title: "browse.genre.Horror", load: () => api.catalog("movie", "top", { genre: "Horror" }) },
  ];
</script>

<div class="page home" use:keepScroll={"home"}>
  <Hero items={featured} {playHref} />
  <div class="rows">
    {#if cont === null || cont.length}
      <Row title={t("browse.home.continue")} items={cont ?? undefined} wide>
        {#snippet card(p, i)}<ContinueCard {p} {i} onremove={remove} />{/snippet}
      </Row>
    {/if}
    {#if fresh.length}
      <Row title={t("browse.home.newEpisodes")} items={fresh} wide>
        {#snippet card(n, i)}<NewEpisodeCard {n} {i} />{/snippet}
      </Row>
    {/if}
    {#each ROWS as r, ri (r.title)}
      <Row title={t(r.title)} load={r.load} href={r.href}>
        {#snippet card(m, i)}<PosterCard item={m} {i} eager={ri === 0 && i < 6} />{/snippet}
      </Row>
    {/each}
  </div>
</div>

<style>
  .home {
    padding: 0 0 40px;
  }
  /* TV: la ultima fila tambien queda arriba, donde quedan todas (lib/spatial.svelte.ts, ROW_TOP):
     espacio debajo para poder llevarla ahi. */
  :global(html[data-tv]) .home {
    padding-bottom: max(40px, calc(100vh - 320px));
  }
  .rows {
    position: relative;
    margin-top: clamp(-70px, -7vh, -40px);
    z-index: 2;
  }
</style>
