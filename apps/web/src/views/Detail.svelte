<script lang="ts">
  // Detalle de una pelicula o serie: fondo grande con degradado, poster (que llega volando desde
  // la tarjeta), datos, favoritos, trailer y Play con el mejor stream. Series: temporadas y
  // episodios con el progreso visto. Visto / no visto a mano: el boton del tilde (peliculas), al
  // pasar el mouse o mantener apretado un episodio, y el menu ⋯ de la temporada.
  import { ArrowLeft, Check, CircleCheck, Clock, Ellipsis, EyeOff, Heart, ListVideo, Play, Star, Clapperboard, X } from "@lucide/svelte";
  import Drawer from "../components/Drawer.svelte";
  import Img from "../components/Img.svelte";
  import PopMenu, { type PopItem } from "../components/PopMenu.svelte";
  import PosterCard from "../components/PosterCard.svelte";
  import Row from "../components/Row.svelte";
  import Seg from "../components/Seg.svelte";
  import Select from "../components/Select.svelte";
  import StreamList from "../components/StreamList.svelte";
  import { api, ApiError, errText } from "../lib/api";
  import { clock, epCode, sized } from "../lib/format";
  import { coverWidth, fit, retryOriginal } from "../lib/images";
  import { takeOver } from "../lib/handoff";
  import { fadeQuick, fadeSlow, pop, rise } from "../lib/motion";
  import { streamLink } from "../lib/external";
  import { prefs } from "../lib/prefs.svelte";
  import { press } from "../lib/press";
  import { back, go, loadView } from "../lib/router.svelte";
  import { keepScroll } from "../lib/scroll";
  import { sortByLang } from "../lib/streamlang";
  import { attempt, toast } from "../lib/toast.svelte";
  import type { Episode, MediaType, Meta, MetaPreview, Progress, Stream } from "../lib/types";

  let { params }: { params: string[] } = $props();
  // La vista se vuelve a montar al cambiar de titulo ({#key} en App): los params no cambian.
  // svelte-ignore state_referenced_locally
  const type = params[0] as MediaType;
  // svelte-ignore state_referenced_locally
  const id = params[1];
  const preview = takeOver(type, id);

  let meta = $state<Meta | null>(null);
  let failed = $state("");
  let inLib = $state<boolean | null>(null);
  let progress = $state<Record<string, Progress>>({});
  let season = $state(1);
  let trailer = $state(false);

  // El panel de streams: de la pelicula o de un episodio.
  let panel = $state<{ target: string; label: string } | null>(null);
  let streams = $state<Stream[] | null>(null);
  let streamsErr = $state("");

  const m = $derived<MetaPreview | null>(meta ?? preview ?? null);
  // Temporadas en orden, con los especiales (temporada 0) al final.
  const seasons = $derived(
    [...new Set((meta?.videos ?? []).map((v) => v.season))].sort((a, b) => (a === 0 ? 1 : b === 0 ? -1 : a - b)),
  );
  const seasonOpts = $derived(seasons.map((s) => ({ value: s, label: s === 0 ? "Specials" : `Season ${s}` })));
  const realSeasons = $derived(seasons.filter((s) => s > 0).length);
  const episodes = $derived((meta?.videos ?? []).filter((v) => v.season === season));

  api
    .meta(type, id)
    .then((r) => (meta = r))
    .catch((e) => (failed = e instanceof ApiError && e.status === 404 ? "This title doesn't exist." : errText(e)));

  api
    .library()
    .then((l) => (inLib = l.some((x) => x.id === id)))
    .catch(() => (inLib = false));

  loadView("player").catch(() => {});

  // Peliculas: los streams se piden ya, asi Play arranca enseguida.
  if (type === "movie") {
    api.streams(type, id).catch(() => {});
    api
      .progress(id)
      .then((p) => p && (progress = { [id]: p }))
      .catch(() => {});
  }

  // Hasta que llega el progreso no se sabe en que temporada va (ver `season` mas abajo).
  let progressReady = $state(type !== "series");
  if (type === "series") {
    api
      .seriesProgress(id)
      .catch(() => api.continueWatching().then((l) => l.filter((p) => p.series_id === id)))
      .then((l) => (progress = Object.fromEntries(l.map((p) => [p.meta_id, p]))))
      .catch(() => {})
      .finally(() => (progressReady = true));
  }

  /** El episodio para el boton grande: el ultimo que se estaba viendo, o el primero. */
  const current = $derived.by<Episode | null>(() => {
    const vids = meta?.videos;
    if (!vids?.length) return null;
    const last = Object.values(progress).sort((a, b) => b.updated_at.localeCompare(a.updated_at))[0];
    if (last) {
      const i = vids.findIndex((v) => v.id === last.meta_id);
      if (i >= 0) {
        // Si lo termino, el siguiente.
        if (last.position_s / last.duration_s > 0.92 && vids[i + 1]) return vids[i + 1];
        return vids[i];
      }
    }
    return vids.find((v) => v.season > 0) ?? vids[0];
  });

  // La temporada que se muestra arranca en la del episodio actual. Se espera al progreso: la
  // ficha suele llegar antes y, sin progreso, el actual es siempre el 1x1. Si mientras tanto se
  // elige otra temporada a mano, queda esa.
  let seasonSet = false;
  $effect(() => {
    if (!seasonSet && current && progressReady) {
      season = current.season;
      seasonSet = true;
    }
  });
  function pickSeason(s: number) {
    season = s;
    seasonSet = true;
  }

  const playTarget = $derived(type === "movie" ? id : current?.id);
  const resume = $derived(playTarget ? progress[playTarget] : undefined);
  const canResume = $derived(!!resume && resume.position_s > 30 && resume.position_s < resume.duration_s * 0.92);
  const playLabel = $derived(
    (canResume ? "Resume" : "Play") + (type === "series" && current ? ` ${epCode(current.season, current.episode)}` : ""),
  );

  /** Al reproductor. Con `resume`, sigue donde quedo sin preguntar (o arranca de cero). */
  function play(target = playTarget, key?: string, resume?: boolean, pick = false) {
    if (!target) return;
    const q = new URLSearchParams();
    if (key) q.set("s", key);
    // Elegido a mano en "Choose stream" (el server aprende que release prefiere).
    if (pick) q.set("pick", "1");
    if (resume !== undefined) q.set("resume", resume ? "1" : "0");
    const qs = q.toString();
    go(`/watch/${type}/${target}${qs ? `?${qs}` : ""}`);
  }

  // Al pasar el mouse por Play ya se arranca a bajar el mejor stream.
  function warmPlay() {
    if (!playTarget) return;
    api
      .streams(type, playTarget)
      .then((l) => {
        const first = sortByLang(l, prefs.audioLang)[0];
        if (first) api.prefetch(first);
      })
      .catch(() => {});
  }

  async function openStreams(target: string, label: string) {
    panel = { target, label };
    streams = null;
    streamsErr = "";
    try {
      const l = sortByLang(await api.streams(type, target), prefs.audioLang);
      if (panel?.target !== target) return;
      streams = l;
      if (l[0]) api.prefetch(l[0]);
    } catch (e) {
      streamsErr = errText(e);
    }
  }

  async function toggleLib() {
    if (!m || inLib === null) return;
    const was = inLib;
    inLib = !was;
    const preview: MetaPreview = {
      id: m.id,
      type: m.type,
      name: m.name,
      poster: m.poster,
      background: m.background,
      logo: m.logo,
      year: m.year,
      imdb_rating: m.imdb_rating,
      genres: m.genres,
      description: m.description,
    };
    const ok = await attempt(() => (was ? api.libraryRemove(type, id) : api.libraryAdd(preview)));
    if (!ok) inLib = was;
    else toast(was ? "Removed from Library" : "Added to Library");
  }

  // ---- visto / no visto a mano. "Visto" es lo mismo que al terminar de verlo: progreso con la
  // posicion al final (> 92 %), asi sale de "Continue watching" y el boton grande pasa al
  // siguiente. "No visto" borra el progreso.
  const watched = (mid: string) => {
    const p = progress[mid];
    return !!p && p.duration_s > 0 && p.position_s / p.duration_s > 0.92;
  };
  /** Duracion para la marca: la del progreso, la del catalogo ("148 min") o una hora. */
  function durOf(mid: string) {
    const p = progress[mid];
    if (p?.duration_s) return p.duration_s;
    const r = /(\d+)\s*min/.exec(meta?.runtime ?? "");
    return r ? Number(r[1]) * 60 : 3600;
  }
  let marking = $state(false);
  async function setWatched(ids: string[], on: boolean, done?: string) {
    if (!m || marking) return;
    // Solo lo que cambia; en orden, asi el ultimo marcado queda como el mas reciente.
    const todo = ids.filter((x) => (on ? !watched(x) : !!progress[x]));
    if (!todo.length) return;
    const before = progress;
    const next = { ...progress };
    const now = Date.now();
    todo.forEach((mid, i) => {
      if (!on) return void delete next[mid];
      const d = durOf(mid);
      const e = meta?.videos?.find((v) => v.id === mid);
      next[mid] = {
        meta_id: mid,
        type,
        series_id: type === "series" ? id : null,
        name: m!.name,
        poster: m!.poster,
        background: m!.background,
        episode_label: e ? `${epCode(e.season, e.episode)}${e.name ? ` · ${e.name}` : ""}` : null,
        position_s: d,
        duration_s: d,
        stream_key: progress[mid]?.stream_key ?? null,
        updated_at: new Date(now + i).toISOString(),
      };
    });
    progress = next;
    marking = true;
    try {
      for (const mid of todo) {
        if (on) {
          const { updated_at: _, ...rec } = next[mid];
          await api.saveProgress(rec);
        } else await api.deleteProgress(mid);
      }
      if (done) toast(done);
    } catch (e) {
      progress = before;
      toast(errText(e), true);
    } finally {
      marking = false;
    }
  }
  const toggleEp = (e: Episode) =>
    setWatched([e.id], !watched(e.id), `${epCode(e.season, e.episode)} marked as ${watched(e.id) ? "unwatched" : "watched"}`);

  // Los menus chicos: el de un episodio (clic derecho o mantener apretado) y el ⋯ de la temporada.
  let pm = $state<{ x: number; y: number; items: PopItem[]; label: string } | null>(null);
  function epMenu(e: Episode, x: number, y: number) {
    if (future(e)) return;
    pm = {
      x,
      y,
      label: `${epCode(e.season, e.episode)} options`,
      items: [
        watched(e.id)
          ? { label: "Mark as unwatched", icon: EyeOff, run: () => toggleEp(e) }
          : { label: "Mark as watched", icon: CircleCheck, run: () => toggleEp(e) },
        { label: "Choose stream", icon: ListVideo, run: () => openStreams(e.id, `${m?.name} · ${epCode(e.season, e.episode)}`) },
      ],
    };
  }
  const aired = $derived(episodes.filter((e) => !future(e)));
  const seasonWatched = $derived(aired.length > 0 && aired.every((e) => watched(e.id)));
  const seasonName = $derived(season === 0 ? "Specials" : `Season ${season}`);
  function seasonMenu(ev: MouseEvent) {
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    const ids = aired.map((e) => e.id);
    const items: PopItem[] = [];
    if (!seasonWatched) items.push({ label: `Mark ${seasonName.toLowerCase()} as watched`, icon: CircleCheck, run: () => setWatched(ids, true, `${seasonName} marked as watched`) });
    if (aired.some((e) => progress[e.id]))
      items.push({ label: `Mark ${seasonName.toLowerCase()} as unwatched`, icon: EyeOff, run: () => setWatched(ids, false, `${seasonName} marked as unwatched`) });
    pm = { x: r.left, y: r.bottom + 4, label: `${seasonName} options`, items };
  }
  /** Spoilers escondidos: el episodio no se empezo a ver. */
  const hidden = (e: Episode) => prefs.hideSpoilers && !progress[e.id];
  // Con el dedo, tocar la descripcion escondida la muestra (tocar la imagen reproduce).
  let shown = $state<Record<string, boolean>>({});
  function epClick(ev: MouseEvent, e: Episode) {
    if (hidden(e) && !shown[e.id] && (ev.target as HTMLElement).closest(".ep-text") && matchMedia("(hover: none)").matches) {
      shown[e.id] = true;
      return;
    }
    play(e.id);
  }

  function epProgress(e: Episode) {
    const p = progress[e.id];
    return p && p.duration_s ? Math.min(100, (p.position_s / p.duration_s) * 100) : 0;
  }
  /** Del mismo genero principal, sin este titulo. */
  async function moreLike(): Promise<MetaPreview[]> {
    const g = m?.genres?.[0];
    if (!g) return [];
    const l = await api.catalog(type, "top", { genre: g });
    return l.filter((x) => x.id !== id).slice(0, 20);
  }
  const initials = (n: string) =>
    n
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0])
      .join("")
      .toUpperCase();
  // Tamaños justos: el fondo cubre el ancho y min(82vh, 820px) de alto; el poster mide
  // clamp(170px, 20vw, 280px) (150 en celular) y el de 300 px alcanza salvo en pantallas densas.
  const bgWidth = coverWidth(innerWidth, Math.min(innerHeight * 0.82, 820));
  const posterHi = (innerWidth <= 700 ? 150 : Math.min(280, Math.max(170, innerWidth * 0.2))) * devicePixelRatio > 330;
  const future = (e: Episode) => !!e.released && new Date(e.released).getTime() > Date.now();
  const fmtDate = (s?: string | null) => (s ? new Date(s).toLocaleDateString("en", { year: "numeric", month: "short", day: "numeric" }) : "");
</script>

<div class="page detail" use:keepScroll={`detail:${id}`}>
  <div class="backdrop">
    {#if m?.background}
      <img
        src={fit(sized(m.background, "medium"), bgWidth)}
        alt=""
        in:fadeSlow={{ duration: 600 }}
        decoding="async"
        fetchpriority="high"
        onerror={(e) => retryOriginal(e.currentTarget)}
      />
    {:else if m?.poster}
      <!-- Sin fondo en el catalogo: el poster, grande y desenfocado. -->
      <img class="blurred" src={sized(m.poster, "small")} alt="" in:fadeSlow={{ duration: 600 }} decoding="async" />
    {/if}
    <div class="shade"></div>
  </div>

  <button class="back btn glass" onclick={() => back("/")}><ArrowLeft size={16} />Back</button>

  <div class="top">
    <div class="poster-frame poster">
      <!-- La version chica es la misma que la de la tarjeta: la transicion arranca sin espera. -->
      <Img src={sized(m?.poster, "small")} eager vt="poster" fallback={m?.name ?? ""} />
      {#if m?.poster && posterHi}<Img src={sized(m.poster, "medium")} eager />{/if}
    </div>

    <div class="info">
      {#if m}
        <h1 class="display-title title" in:rise={{ y: 10, delay: 60 }}>{m.name}</h1>
        <div class="facts" in:rise={{ y: 10, delay: 110 }}>
          {#if m.imdb_rating}<span class="imdb"><Star size={14} fill="currentColor" /><b>{m.imdb_rating}</b><span class="faint">IMDb</span></span>{/if}
          {#if m.year}<span>{m.year}</span>{/if}
          {#if meta?.runtime}<span class="fact"><Clock size={13} />{meta.runtime}</span>{/if}
          {#if type === "series" && realSeasons}<span>{realSeasons} season{realSeasons > 1 ? "s" : ""}</span>{/if}
        </div>
        {#if m.genres?.length}
          <div class="genres" in:rise={{ y: 10, delay: 140 }}>
            {#each m.genres as g (g)}<span class="chip">{g}</span>{/each}
          </div>
        {/if}

        <div class="actions" in:rise={{ y: 10, delay: 170 }}>
          <button class="btn primary lg playbtn" disabled={type === "series" && !current} onclick={() => play(playTarget, undefined, canResume || undefined)} onpointerenter={warmPlay} onfocus={warmPlay}>
            <Play size={18} fill="currentColor" />{playLabel}
          </button>
          <button
            class="btn glass lg choose"
            aria-label="Choose stream"
            disabled={!playTarget}
            onclick={() => playTarget && openStreams(playTarget, type === "movie" ? m!.name : `${m!.name} · ${epCode(current!.season, current!.episode)}`)}
          >
            <ListVideo size={17} /><span class="lbl">Choose stream</span>
          </button>
          <button class="btn glass lg icon" class:fav={inLib} onclick={toggleLib} title={inLib ? "Remove from Library" : "Add to Library"} disabled={inLib === null}>
            <Heart size={18} fill={inLib ? "currentColor" : "none"} />
          </button>
          {#if meta?.trailer_youtube_id}
            <button class="btn glass lg icon" onclick={() => (trailer = true)} title="Trailer"><Clapperboard size={18} /></button>
          {/if}
          {#if type === "movie"}
            <button
              class="btn glass lg icon"
              class:seen-on={watched(id)}
              onclick={() => setWatched([id], !watched(id), watched(id) ? "Marked as unwatched" : "Marked as watched")}
              title={watched(id) ? "Watched · click to mark as unwatched" : "Mark as watched"}
              aria-pressed={watched(id)}
              disabled={marking || !m}
            >
              <CircleCheck size={18} />
            </button>
          {/if}
        </div>
        {#if resume && resume.position_s > 30 && resume.duration_s}
          <div class="resume" in:rise={{ y: 6, delay: 200 }}>
            <div class="progress"><div class="progress-fill" style="width: {(resume.position_s / resume.duration_s) * 100}%"></div></div>
            <span class="faint">{watched(resume.meta_id) ? "Watched" : `${clock(resume.duration_s - resume.position_s)} left`}</span>
          </div>
        {/if}

        {#if m.description}<p class="desc" in:rise={{ y: 10, delay: 200 }}>{m.description}</p>{/if}
        {#if meta}
          {#if meta.director?.length}
            <div class="director" in:rise={{ y: 10, delay: 230 }}>
              <span class="faint">Directed by</span> {meta.director.join(", ")}
            </div>
          {/if}
        {:else if !failed}
          <div class="skel line" style="width: 70%; margin-top: 22px"></div>
          <div class="skel line" style="width: 50%; margin-top: 10px"></div>
        {/if}
      {:else if failed}
        <div class="empty"><h3>Couldn't open this title</h3><span>{failed}</span></div>
      {:else}
        <div class="skel" style="width: 60%; height: 48px"></div>
        <div class="skel line" style="width: 40%; margin-top: 18px"></div>
        <div class="skel line" style="width: 80%; margin-top: 30px"></div>
        <div class="skel line" style="width: 74%; margin-top: 10px"></div>
      {/if}
    </div>
  </div>

  {#if meta?.cast?.length}
    <section class="cast-sec" in:rise={{ y: 10, delay: 260 }}>
      <h2 class="sec-h">Cast</h2>
      <div class="cast">
        {#each meta.cast.slice(0, 12) as name, i (name)}
          <div class="person" style="--i: {i}">
            <span class="avatar" style="--p: {14 + ((name.charCodeAt(0) * 7 + name.length * 3) % 22)}%">{initials(name)}</span>
            <span class="pname">{name}</span>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if type === "series"}
    <section class="episodes">
      <div class="ep-head">
        <h2>Episodes</h2>
        {#if meta && aired.length}
          <button class="icon-btn season-more" onclick={seasonMenu} title="{seasonName} options" aria-label="{seasonName} options" aria-haspopup="menu" disabled={marking}>
            <Ellipsis size={18} />
          </button>
        {/if}
        <div class="grow"></div>
        {#if seasons.length > 1 && seasons.length <= 6}
          <Seg value={season} options={seasonOpts} onchange={pickSeason} />
        {:else if seasons.length > 6}
          <div class="season-select">
            <Select bind:value={() => season, pickSeason} options={seasonOpts} />
          </div>
        {/if}
      </div>

      {#if !meta}
        {#each Array(4) as _, i (i)}
          <div class="ep skel-ep"><div class="skel thumb"></div><div class="grow"><div class="skel line" style="width: 40%"></div><div class="skel line" style="width: 85%; margin-top: 12px"></div></div></div>
        {/each}
      {:else}
        {#key season}
          <div class="ep-list" in:rise={{ y: 6 }}>
            {#each episodes as e, i (e.id)}
              {@const pct = epProgress(e)}
              <div class="ep" class:now={current?.id === e.id} class:future={future(e)} class:spoiler={hidden(e) && !shown[e.id]} style="--i: {i}">
                <button class="ep-main" onclick={(ev) => epClick(ev, e)} disabled={future(e)} use:press={(x, y) => epMenu(e, x, y)}>
                  <span class="thumb">
                    <Img src={fit(e.thumbnail, 200)} alt="" fallback={epCode(e.season, e.episode)} />
                    <span class="ep-play"><Play size={18} fill="currentColor" /></span>
                    {#if pct > 0}<span class="ep-prog"><span style="width: {pct}%"></span></span>{/if}
                    {#if pct > 92}<span class="seen" title="Watched"><Check size={12} /></span>{/if}
                  </span>
                  <span class="ep-text">
                    <span class="ep-title"><span class="num">{e.episode}.</span> {e.name ?? `Episode ${e.episode}`}</span>
                    {#if e.overview}<span class="ep-desc">{e.overview}</span>{/if}
                    <span class="ep-date faint">{future(e) ? "Coming " : ""}{fmtDate(e.released)}</span>
                  </span>
                </button>
                {#if !future(e)}
                  <button
                    class="icon-btn ep-watch"
                    class:seen-on={watched(e.id)}
                    title={watched(e.id) ? "Mark as unwatched" : "Mark as watched"}
                    aria-label={watched(e.id) ? "Mark as unwatched" : "Mark as watched"}
                    onclick={() => toggleEp(e)}
                    disabled={marking}
                  >
                    <CircleCheck size={16} />
                  </button>
                  <button class="icon-btn ep-streams" title="Choose stream" onclick={() => openStreams(e.id, `${m?.name} · ${epCode(e.season, e.episode)}`)}>
                    <ListVideo size={16} />
                  </button>
                {/if}
              </div>
            {:else}
              <div class="empty"><span>No episodes in this season.</span></div>
            {/each}
          </div>
        {/key}
      {/if}
    </section>
  {/if}
  {#if m?.genres?.length}
    <div class="more">
      <Row title="More like this" load={moreLike}>
        {#snippet card(x, i)}<PosterCard item={x} {i} />{/snippet}
      </Row>
    </div>
  {/if}
</div>

{#if panel}
  <Drawer title="Choose stream" onclose={() => (panel = null)}>
    <div class="faint panel-sub">{panel.label}</div>
    {#if streamsErr}
      <div class="empty"><h3>Couldn't get streams</h3><span>{streamsErr}</span></div>
    {:else if !streams}
      {#each Array(6) as _, i (i)}<div class="skel" style="height: 66px; border-radius: var(--radius)"></div>{/each}
    {:else}
      <StreamList {streams} onpick={(s) => play(panel!.target, s.key, undefined, true)} linkFor={(s) => streamLink(s)} />
    {/if}
  </Drawer>
{/if}

{#if pm}
  <PopMenu x={pm.x} y={pm.y} items={pm.items} label={pm.label} onclose={() => (pm = null)} />
{/if}

{#if trailer && meta?.trailer_youtube_id}
  <div class="scrim" role="presentation" onclick={() => (trailer = false)} transition:fadeQuick></div>
  <div class="trailer" role="dialog" aria-modal="true" aria-label="Trailer" transition:pop>
    <button class="icon-btn close" onclick={() => (trailer = false)} aria-label="Close trailer"><X size={18} /></button>
    <iframe
      src="https://www.youtube-nocookie.com/embed/{meta.trailer_youtube_id}?autoplay=1&rel=0"
      title="Trailer"
      allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
      allowfullscreen
    ></iframe>
  </div>
{/if}

<svelte:window onkeydown={(e) => e.key === "Escape" && trailer && (e.preventDefault(), (trailer = false))} />

<style>
  .detail {
    position: relative;
    padding: 0 0 50px;
  }
  .backdrop {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: min(82vh, 820px);
    overflow: hidden;
    z-index: 0;
  }
  .backdrop img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center 20%;
  }
  .backdrop img.blurred {
    filter: blur(40px) saturate(1.3);
    transform: scale(1.2);
    opacity: 0.7;
  }
  .shade {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(0deg, var(--bg) 0%, color-mix(in srgb, var(--bg) 82%, transparent) 32%, color-mix(in srgb, var(--bg) 25%, transparent) 72%),
      linear-gradient(90deg, color-mix(in srgb, var(--bg) 80%, transparent) 10%, color-mix(in srgb, var(--bg) 30%, transparent) 60%, transparent 85%);
  }
  .back {
    position: sticky;
    top: 16px;
    left: var(--gutter);
    margin: 16px 0 0 var(--gutter);
    z-index: 6;
  }
  .top {
    position: relative;
    z-index: 2;
    display: grid;
    grid-template-columns: clamp(170px, 20vw, 280px) 1fr;
    gap: clamp(20px, 3vw, 44px);
    padding: clamp(60px, 14vh, 150px) var(--gutter) 0;
    align-items: start;
  }
  .poster {
    position: relative;
    aspect-ratio: 2 / 3;
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--panel-2);
    border: var(--border) solid var(--line);
    box-shadow:
      0 30px 60px -25px var(--media-scrim),
      var(--card-shadow);
  }
  .info {
    color: var(--text);
    --shadow: color-mix(in srgb, var(--bg) 70%, transparent);
    min-width: 0;
    padding-top: 8px;
  }
  .title {
    font-size: clamp(28px, 4vw, 52px);
    text-shadow: 0 4px 30px var(--shadow);
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    align-items: center;
    margin-top: 14px;
    font-size: 14px;
    color: var(--text-2);
  }
  .imdb,
  .fact {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .imdb b {
    color: var(--text);
  }
  .imdb :global(svg) {
    color: var(--warn);
  }
  .imdb .faint {
    font-size: 11px;
    font-weight: 700;
  }
  .genres {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 14px;
  }
  .genres .chip {
    background: color-mix(in srgb, var(--panel) 70%, transparent);
    backdrop-filter: blur(8px);
  }
  /* Siempre en una fila: Play y Choose stream con texto, favorito y trailer como iconos. Si no
     entra (celular), Choose stream tambien queda solo con el icono. */
  .actions {
    display: flex;
    flex-wrap: nowrap;
    gap: 10px;
    margin-top: 24px;
  }
  .actions .btn {
    flex: none;
  }
  @media (max-width: 520px) {
    .actions {
      gap: 8px;
    }
    .actions .btn.lg {
      padding: 0 16px;
    }
    .choose .lbl {
      display: none;
    }
    .actions .btn.choose,
    .actions .btn.icon {
      width: 42px;
      padding: 0;
    }
  }
  .btn.icon {
    width: 42px;
    padding: 0;
  }
  .fav {
    color: var(--danger);
  }
  .fav :global(svg) {
    animation: heart 0.4s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  @keyframes heart {
    40% {
      transform: scale(1.35);
    }
  }
  .resume {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 14px;
    max-width: 360px;
    font-size: 12px;
  }
  .resume .progress {
    flex: 1;
  }
  .desc {
    margin: 22px 0 0;
    max-width: 720px;
    font-size: 15px;
    line-height: 1.6;
    color: var(--text-2);
    user-select: text;
  }

  .director {
    margin-top: 16px;
    font-size: 13.5px;
    color: var(--text-2);
  }
  .sec-h {
    margin: 0 0 14px;
    font-family: var(--display);
    font-size: 19px;
  }
  .cast-sec {
    position: relative;
    z-index: 2;
    padding: 44px var(--gutter) 0;
  }
  .cast {
    display: flex;
    flex-wrap: wrap;
    gap: 14px 22px;
  }
  .person {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 92px;
    text-align: center;
    animation: rise 0.36s cubic-bezier(0.2, 0.8, 0.2, 1) backwards;
    animation-delay: calc(var(--i) * 35ms);
  }
  /* Sin fotos en el catalogo: iniciales sobre el acento, mas o menos intenso segun el nombre. */
  .avatar {
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    font-weight: 700;
    font-size: 19px;
    color: var(--text);
    background:
      linear-gradient(145deg, color-mix(in srgb, var(--accent) 30%, transparent), transparent 70%),
      color-mix(in srgb, var(--accent) var(--p), var(--panel-2));
    border: var(--border) solid var(--line);
    transition: transform 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .person:hover .avatar {
    transform: scale(1.06);
  }
  .pname {
    font-size: 12.5px;
    line-height: 1.3;
    color: var(--text-2);
  }
  .more {
    position: relative;
    z-index: 2;
    margin: 44px 0 0;
  }
  .episodes {
    position: relative;
    z-index: 2;
    padding: 40px var(--gutter) 0;
  }
  .ep-head {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 14px;
  }
  .ep-head h2 {
    margin: 0;
    font-family: var(--display);
    font-size: 19px;
  }
  .season-select {
    width: 170px;
  }
  /* En pantallas anchas los episodios van en dos (o tres) columnas: nada de media pantalla vacia. */
  .ep-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 560px), 1fr));
    gap: 6px 14px;
  }
  .ep {
    position: relative;
    display: flex;
    align-items: center;
    border-radius: var(--radius);
    animation: rise 0.32s cubic-bezier(0.2, 0.8, 0.2, 1) backwards;
    animation-delay: calc(min(var(--i, 0), 12) * 30ms);
    transition: background 0.15s;
  }
  .ep:hover {
    background: var(--hover);
  }
  .ep.now {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .ep.future {
    opacity: 0.5;
  }
  .ep-main {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 16px;
    padding: 10px;
    text-align: left;
    align-items: center;
  }
  .ep-main:disabled {
    cursor: default;
  }
  .thumb {
    position: relative;
    flex: none;
    width: clamp(130px, 16vw, 200px);
    aspect-ratio: 16 / 9;
    border-radius: var(--radius-sm);
    overflow: hidden;
    background: var(--panel-2);
  }
  .ep-play {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--on-media);
    background: var(--media-scrim);
    opacity: 0;
    transition: opacity 0.18s;
  }
  .ep-play :global(svg) {
    transform: scale(0.7);
    transition: transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .ep-main:not(:disabled):hover .ep-play {
    opacity: 1;
  }
  .ep-main:hover .ep-play :global(svg) {
    transform: scale(1.1);
  }
  .ep-prog {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 3px;
    background: var(--media-line);
  }
  .ep-prog span {
    display: block;
    height: 100%;
    background: var(--primary-bg);
  }
  .seen {
    position: absolute;
    top: 5px;
    right: 5px;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    color: var(--accent-ink);
    background: var(--accent);
  }
  .ep-text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .ep-title {
    font-weight: 600;
    font-size: 14px;
  }
  .num {
    color: var(--text-3);
  }
  .ep-desc {
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--text-2);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .ep-date {
    font-size: 11.5px;
  }
  .ep-streams {
    margin-right: 10px;
    opacity: 0;
  }
  .ep:hover .ep-streams,
  .ep-streams:focus-visible,
  .ep:hover .ep-watch,
  .ep-watch:focus-visible {
    opacity: 1;
  }
  .ep-watch {
    opacity: 0;
  }
  /* TV (sin mouse): con el foco en el episodio aparecen sus botones, y la flecha derecha llega a
     ellos (marcar visto, elegir stream). */
  :global(html[data-tv]) .ep:focus-within .ep-streams,
  :global(html[data-tv]) .ep:focus-within .ep-watch,
  :global(html[data-tv]) .ep-main:not(:disabled):focus-visible .ep-play {
    opacity: 1;
  }
  .seen-on {
    color: var(--accent);
  }
  .season-more {
    width: 34px;
    height: 34px;
  }
  /* Sin spoilers: la imagen desenfocada y la descripcion tapada hasta pasar el mouse (o tocarla).
     Sin filtro animado: aparece de una. */
  .spoiler .thumb :global(img) {
    filter: blur(14px);
    transform: scale(1.12);
  }
  .spoiler .ep-desc {
    filter: blur(5px);
  }
  @media (hover: hover) {
    .spoiler:hover .thumb :global(img),
    .spoiler:hover .ep-desc {
      filter: none;
      transform: none;
    }
  }
  .spoiler:focus-within .thumb :global(img),
  .spoiler:focus-within .ep-desc {
    filter: none;
    transform: none;
  }
  .skel-ep {
    gap: 16px;
    padding: 10px;
  }
  .skel-ep .thumb {
    width: clamp(130px, 16vw, 200px);
  }
  .panel-sub {
    margin-top: -6px;
    font-size: 12.5px;
  }
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 70;
    background: var(--media-scrim);
    backdrop-filter: blur(6px);
  }
  .trailer {
    position: fixed;
    z-index: 71;
    left: 50%;
    top: 50%;
    width: min(1100px, 92vw);
    aspect-ratio: 16 / 9;
    translate: -50% -50%;
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--media-bg);
    box-shadow: var(--pop-shadow);
  }
  .trailer iframe {
    width: 100%;
    height: 100%;
    border: 0;
  }
  .trailer .close {
    position: absolute;
    top: 8px;
    right: 8px;
    z-index: 1;
    color: var(--on-media);
    background: var(--media-scrim);
  }
  @media (max-width: 700px) {
    .top {
      grid-template-columns: 1fr;
      padding-top: 40px;
    }
    .poster {
      width: 150px;
    }
    .ep-streams {
      opacity: 1;
    }
  }
  @media (hover: none) {
    .ep-streams {
      opacity: 1;
    }
    /* Con el dedo: mantener apretado el episodio (menos botones en la fila). */
    .ep-watch {
      display: none;
    }
  }
</style>
