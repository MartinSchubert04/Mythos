<script lang="ts">
  // El reproductor. Lo mas importante de la app: que la pelicula no se trabe.
  //
  // Del lado del cliente:
  // - direct: <video src> con preload=auto (el server prioriza las piezas pedidas con Range).
  // - remux/transcode: hls.js (version liviana, import dinamico) con buffer grande. Saltar fuera
  //   de lo que ya existe recarga la playlist con ?start= y la UI suma ese offset.
  // - Si direct falla por codec o no decodifica (sin imagen o sin sonido), se pide de nuevo
  //   /api/play con menos capacidades: el server pasa a remux o transcode. Misma posicion.
  // - Mientras espera: pantalla de carga con /status (peers, MB/s, % de buffer). Si se traba mas
  //   de ~15 s con poca velocidad, ofrece otro stream ahi mismo, sin salir y en la misma posicion.
  // - Al 85 % de un episodio precarga el siguiente; al terminar, cuenta regresiva de 10 s.
  // - Progreso cada 10 s y al salir; al volver ofrece retomar.
  import {
    ArrowDown,
    ArrowLeft,
    ArrowUp,
    AudioLines,
    Captions,
    Check,
    ChevronLeft,
    ChevronRight,
    FastForward,
    Activity,
    Gauge,
    ListVideo,
    LoaderCircle,
    Maximize,
    Minimize,
    Minus,
    Pause,
    Play,
    Plus,
    RotateCcw,
    RotateCw,
    Scan,
    SkipForward,
    TriangleAlert,
    Volume1,
    Volume2,
    VolumeX,
    X,
  } from "@lucide/svelte";
  import type HlsType from "hls.js";
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import PlayerLoading from "../components/PlayerLoading.svelte";
  import SeekBar from "../components/SeekBar.svelte";
  import StreamList from "../components/StreamList.svelte";
  import WindowControls from "../components/WindowControls.svelte";
  import { api, dropCache, errText, mediaUrl, movedAway, withToken } from "../lib/api";
  import { degrade, detectCaps } from "../lib/caps";
  import { bytes, clock, epCode, sized, speed } from "../lib/format";
  import { fadeQuick, pop, rise, step } from "../lib/motion";
  import { isDesktop, isFullscreen, setFullscreen } from "../lib/platform";
  import { langName, nativeLangName, normLang, prefs, savePrefs } from "../lib/prefs.svelte";
  import { back, go, setQuery } from "../lib/router.svelte";
  import { focusEl, moveFocus, type Dir } from "../lib/spatial.svelte";
  import { tv } from "../lib/tv.svelte";
  import { keyParts, PLAYER_KEYS, PLAYER_TOUCH } from "../lib/shortcuts";
  import { preferredAudio, sortByLang } from "../lib/streamlang";
  import { streamLink } from "../lib/external";
  import { toast } from "../lib/toast.svelte";
  import type { ClientCaps, Episode, MediaStatus, MediaType, Meta, PlayInfo, Progress, Stream } from "../lib/types";
  import { loadDelays, saveDelays } from "../lib/subdelay";
  import { cuesAt, parseVtt, type Cue } from "../lib/vtt";

  let { params, query }: { params: string[]; query: URLSearchParams } = $props();
  // La vista se monta de nuevo por cada titulo o episodio ({#key} en App).
  // svelte-ignore state_referenced_locally
  const type = params[0] as MediaType;
  // svelte-ignore state_referenced_locally
  const id = params[1];
  // svelte-ignore state_referenced_locally
  const wantKey = query.get("s");
  // ?resume=1: retomar sin preguntar (Continue watching, "Resume"); ?resume=0: desde el principio.
  // svelte-ignore state_referenced_locally
  const resumeMode = query.get("resume");
  const seriesId = id.split(":")[0];

  // ---- constantes anti-trabas
  const STUCK_MS = 15_000; // esperando mas que esto y con poca velocidad: ofrecer otro stream
  const SLOW_BPS = 1.5e6; // "poca velocidad" (1,5 MB/s)
  const START_GIVEUP_MS = 45_000; // sin primer cuadro en 45 s: ofrecer otro aunque baje rapido
  const BUFFER_GOAL_S = 8; // lo que se muestra como 100 % al esperar

  // ---- estado
  let root = $state<HTMLDivElement>();
  let video = $state<HTMLVideoElement>();
  let meta = $state<Meta | null>(null);
  let streams = $state<Stream[]>([]);
  let stream = $state<Stream | null>(null);
  let info = $state<PlayInfo | null>(null);
  let caps: ClientCaps = detectCaps();
  let fatal = $state("");

  let started = $state(false); // ya se vio el primer cuadro
  let waiting = $state(false);
  let waitShown = $state(false); // el velo de espera (con una pequeña demora para no parpadear)
  let waitSince = 0;
  let loadSince = Date.now();
  let status = $state<MediaStatus | null>(null);
  let stuck = $state<"" | "slow" | "error" | "codec">("");
  let stuckMsg = $state("");
  let needsClick = $state(false);

  let offset = $state(0); // inicio de la playlist HLS actual en tiempo real
  let t = $state(0); // tiempo real (offset + currentTime)
  let vDur = $state(0);
  let buffered = $state<[number, number][]>([]);
  let paused = $state(true);
  let rate = $state(1);
  let fs = $state(false);
  let ended = $state(false);

  type FlashIcon = "play" | "pause" | "fwd" | "back" | "vol" | "subs" | "speed" | "scale";
  let ui = $state(true); // controles visibles
  let menu = $state<"" | "subs" | "audio" | "speed" | "streams">("");
  let dragging = $state(false);
  let flash = $state<{ k: number; icon: FlashIcon; text?: string } | null>(null);

  let askResume = $state<Progress | null>(null);
  let resolveResume: ((at: number) => void) | null = null;
  let countdown = $state<number | null>(null);

  /** Una pista de subtitulos. `group` es el idioma tal como se muestra ("Español (Latinoamérica)"):
   *  junta las internas y las de OpenSubtitles de ese idioma o variante. `label` es el titulo de la
   *  pista interna (puede ir vacio) y `version` su numero dentro del grupo (internas y externas por
   *  separado). El `id` es estable para recordar el retraso: `i{indice}` o `e:{id de OpenSubtitles}`. */
  type Sub = { id: string; label: string; lang: string; url: string; ext: boolean; group: string; version: number };
  let extSubs = $state<Sub[]>([]);
  let subSel = $state("");
  let cues: Cue[] = [];
  let subHtml = $state("");
  let audioSel = $state<number | null>(null);

  let hls: HlsType | null = null;
  let hlsAudio: number | undefined;
  let forceHls = false;
  let attempts = { net: 0, media: 0, fallback: 0 };
  let destroyed = false;
  let attachGen = 0;

  // ---- derivados
  const duration = $derived(info?.duration_s || (vDur ? vDur + offset : 0));
  const episodes = $derived((meta?.videos ?? []).filter((v) => v.season > 0));
  const epIndex = $derived(episodes.findIndex((v) => v.id === id));
  const ep = $derived<Episode | null>(epIndex >= 0 ? episodes[epIndex] : null);
  const nextEp = $derived.by<Episode | null>(() => {
    if (epIndex < 0) return null;
    const n = episodes[epIndex + 1];
    return n && !(n.released && new Date(n.released).getTime() > Date.now()) ? n : null;
  });
  const epLabel = $derived(ep ? `${epCode(ep.season, ep.episode)}${ep.name ? ` · ${ep.name}` : ""}` : "");
  const alternatives = $derived(streams.filter((s) => s.key !== stream?.key).slice(0, 6));
  const intSubs = $derived.by<Sub[]>(() => {
    const per: Record<string, number> = {};
    return (info?.subtitle_tracks ?? []).map((s) => {
      const group = groupName(s.lang);
      return {
        id: `i${s.index}`,
        label: s.title ?? "",
        lang: normLang(s.lang),
        url: `/api/media/${info!.session_id}/subs/${s.index}.vtt`,
        ext: false,
        group,
        version: (per[group] = (per[group] ?? 0) + 1),
      };
    });
  });
  const subs = $derived([...intSubs, ...extSubs]);
  const remaining = $derived(duration ? duration - t : 0);
  const showNextPill = $derived(!!nextEp && started && duration > 120 && remaining > 0 && remaining < 40 && countdown === null);
  const loadingFull = $derived(!started && !fatal);
  const loadText = $derived.by(() => {
    if (askResume) return "";
    if (!info) {
      if (!stream) return "Finding streams…";
      return status?.state === "metadata" || !status ? "Connecting to peers…" : "Opening the file…";
    }
    if (status?.state === "error") return status.error ?? "The server couldn't read this file";
    if (status?.state === "metadata") return "Connecting to peers…";
    if (status?.state === "probing") return "Reading the file…";
    const p = bufPct();
    return p !== null ? `Buffering ${Math.round(p)}%…` : started ? "Buffering…" : "Starting…";
  });

  // ---- inicio

  onMount(() => {
    init();
    mediaHandlers(true);
    const onHide = () => leave(true);
    window.addEventListener("pagehide", onHide);
    const onFs = () => isFullscreen().then((v) => (fs = v));
    document.addEventListener("fullscreenchange", onFs);
    window.addEventListener("resize", onFs);
    return () => {
      window.removeEventListener("pagehide", onHide);
      document.removeEventListener("fullscreenchange", onFs);
      window.removeEventListener("resize", onFs);
    };
  });

  onDestroy(() => {
    destroyed = true;
    leave(false);
    mediaHandlers(false);
    if (msess) {
      msess.metadata = null;
      msess.playbackState = "none";
      try {
        msess.setActionHandler("nexttrack", null);
        msess.setPositionState?.();
      } catch {}
    }
    if (fs) setFullscreen(root!, false);
  });

  async function init() {
    api.meta(type, seriesId).then((m) => (meta = m)).catch(() => {});
    api
      .subtitles(type, id)
      .then((l) => {
        // Agrupadas por variante de idioma (spa y spl son grupos distintos), hasta 12 versiones
        // por grupo: la lista de OpenSubtitles puede ser enorme.
        const per: Record<string, number> = {};
        const seen = new Set<string>();
        extSubs = l.flatMap((s, i) => {
          const g = groupName(s.lang);
          const sid = `e:${s.id || i}`;
          if (seen.has(sid)) return [];
          seen.add(sid);
          const v = (per[g] = (per[g] ?? 0) + 1);
          return v > 12 ? [] : [{ id: sid, label: "", lang: normLang(s.lang), url: s.url, ext: true, group: g, version: v }];
        });
        autoSub();
      })
      .catch(() => {});

    const progP = api.progress(id).catch(() => null);
    let list: Stream[];
    try {
      // Con un idioma de audio preferido, los streams en ese idioma primero (lib/streamlang.ts).
      list = sortByLang(await api.streams(type, id), prefs.audioLang);
    } catch (e) {
      fatal = `Couldn't get streams: ${errText(e)}`;
      return;
    }
    if (destroyed) return;
    streams = list;
    if (!list.length) {
      fatal = "No streams found for this title. It may be too new, or the size and quality limits in Settings leave nothing out. Try again in a while.";
      return;
    }
    const prog = await progP;
    const key = wantKey ?? prog?.stream_key;
    const chosen = list.find((s) => s.key === key) ?? list[0];

    // Retomar: se pregunta mientras el server ya va preparando el archivo.
    let startAt = Promise.resolve(0);
    if (prog && prog.position_s > 30 && prog.duration_s && prog.position_s < prog.duration_s * 0.92) {
      if (resumeMode === "1") startAt = Promise.resolve(Math.max(0, prog.position_s - 3));
      else if (resumeMode !== "0") {
        askResume = prog;
        startAt = new Promise((r) => (resolveResume = r));
      }
    }
    await open(chosen, startAt);
  }

  /** Sin streams o sin respuesta: pedir todo de nuevo (sin la cache de streams). */
  function retryAll() {
    fatal = "";
    started = false;
    loadSince = Date.now();
    dropCache(`streams:${type}/${id}`);
    init();
  }

  /** Pide la sesion al server y la conecta al <video> en `at`. */
  async function open(s: Stream, at: number | Promise<number>, newCaps?: ClientCaps) {
    const prev = info;
    // La sesion vieja deja de usarse ya: el server la cierra cuando se abre otra del mismo
    // titulo (y daria 404). El video se suelta y queda el velo de espera hasta la nueva.
    if (prev) {
      detach();
      info = null;
      if (started) {
        waiting = true;
        waitShown = true;
        waitSince = Date.now();
      }
    }
    stream = s;
    stuck = "";
    status = null;
    loadSince = Date.now();
    if (newCaps) caps = newCaps;
    forceHls = false;
    hlsAudio = undefined;
    startPolling();
    let next: PlayInfo;
    try {
      const p = api.play({ stream_key: s.key, info_hash: s.info_hash, file_idx: s.file_idx, url: s.url, caps, meta_id: id });
      next = await p;
    } catch (e) {
      if (destroyed) return;
      stuck = "error";
      stuckMsg = errText(e);
      return;
    }
    if (destroyed) {
      api.closeSession(next.session_id);
      return;
    }
    if (prev && prev.session_id !== next.session_id) api.closeSession(prev.session_id);
    // Sin ffmpeg (server local en el dispositivo) solo hay reproduccion directa: si el sondeo dice
    // que aca no se va a ver o escuchar, se prueba solo el siguiente stream (en el mismo lugar).
    if (next.transcode_available === false && next.direct_ok === false) {
      const alt = nextPlayable(s);
      if (alt) {
        api.closeSession(next.session_id);
        toast("This file can't play on this device. Trying another stream…");
        setQuery({ s: alt.key });
        return open(alt, at, newCaps);
      }
    }
    info = next;
    pickPreferredAudio(next);
    autoSub();
    const pos = await at;
    if (destroyed) return;
    attach(pos);
  }

  // ---- conectar el video

  function detach() {
    hls?.destroy();
    hls = null;
    if (video) {
      video.removeAttribute("src");
      video.load();
    }
  }

  async function attach(at: number) {
    if (!info || !video) return;
    const gen = ++attachGen;
    const wasPaused = started && paused;
    detach();
    // A mitad de camino (cambio de audio, salto fuera de lo generado, fallback): velo de espera.
    if (started) {
      waiting = true;
      waitShown = true;
      waitSince = Date.now();
    }
    ended = false;
    attempts.media = 0;
    decodeChecked = false;
    const useHls = (info.mode !== "direct" || forceHls) && !!info.hls_url;
    viaHls = useHls;
    if (!useHls) {
      offset = 0;
      pendingSeek = at > 0 ? at : null;
      video.src = mediaUrl(info.direct_url);
      video.load();
    } else {
      const start = at > 1 ? Math.floor(at) : undefined;
      offset = start ?? 0;
      const url = mediaUrl(info.hls_url!, { start, audio: hlsAudio });
      const native = !("MediaSource" in window) && video.canPlayType("application/vnd.apple.mpegurl") !== "";
      if (native) {
        video.src = url;
      } else {
        const { default: Hls } = await import("hls.js/light");
        if (gen !== attachGen || destroyed) return;
        hls = new Hls(hlsConfig());
        hls.on(Hls.Events.ERROR, (_e, d) => onHlsError(Hls, d));
        hls.loadSource(url);
        hls.attachMedia(video);
      }
    }
    if (!wasPaused) tryPlay();
  }

  function hlsConfig() {
    const policy = (ttfb: number, load: number) => ({
      default: {
        maxTimeToFirstByteMs: ttfb,
        maxLoadTimeMs: load,
        timeoutRetry: { maxNumRetry: 6, retryDelayMs: 500, maxRetryDelayMs: 4000 },
        errorRetry: { maxNumRetry: 8, retryDelayMs: 1000, maxRetryDelayMs: 8000 },
      },
    });
    return {
      // Buffer grande: lo que ya bajo no se vuelve a pedir y aguanta cortes largos.
      maxBufferLength: 120,
      maxMaxBufferLength: 300,
      maxBufferSize: 400 * 1000 * 1000,
      backBufferLength: 60,
      maxBufferHole: 0.5,
      nudgeMaxRetry: 10,
      startPosition: 0,
      enableWorker: true,
      lowLatencyMode: false,
      // Transcode: el server puede tardar en generar un segmento; mejor esperar que fallar.
      // El server hace esperar el pedido de algo que todavia no existe en vez de dar 404: hasta
      // 120 s la playlist y 90 s un segmento. Los tiempos de aca van un poco por encima.
      manifestLoadPolicy: policy(130_000, 180_000),
      playlistLoadPolicy: policy(130_000, 180_000),
      fragLoadPolicy: policy(100_000, 180_000),
      // Los segmentos son relativos a la playlist y no heredan el ?t= del token.
      xhrSetup: (xhr: XMLHttpRequest, url: string) => xhr.open("GET", withToken(url), true),
    };
  }

  type HlsErr = { fatal: boolean; type: string; details: string; response?: { code?: number } };
  let lastRestart = 0;
  function onHlsError(Hls: typeof HlsType, d: HlsErr) {
    if (!hls) return;
    // Un segmento que ya no existe (ffmpeg se cerro por inactividad, por ejemplo tras una pausa
    // larga): playlist nueva desde la posicion actual, sin esperar los reintentos.
    if (
      (d.details === Hls.ErrorDetails.FRAG_LOAD_ERROR || d.details === Hls.ErrorDetails.LEVEL_LOAD_ERROR) &&
      d.response?.code === 404 &&
      Date.now() - lastRestart > 3000
    ) {
      lastRestart = Date.now();
      attach(t);
      return;
    }
    if (!d.fatal) return;
    if (d.type === Hls.ErrorTypes.NETWORK_ERROR && attempts.net < 6) {
      attempts.net++;
      const h = hls;
      setTimeout(() => h === hls && h.startLoad(), 800 * attempts.net);
    } else if (d.type === Hls.ErrorTypes.MEDIA_ERROR && attempts.media < 2) {
      attempts.media++;
      if (attempts.media === 2) hls.swapAudioCodec();
      hls.recoverMediaError();
    } else if (attempts.fallback < 2) {
      attempts.fallback++;
      attach(t);
    } else {
      stuck = "error";
      stuckMsg = "The stream stopped responding.";
    }
  }

  async function tryPlay() {
    if (!video) return;
    try {
      await video.play();
      needsClick = false;
    } catch (e) {
      // Sin un clic reciente el navegador no deja arrancar con sonido.
      if (e instanceof DOMException && e.name === "NotAllowedError") needsClick = true;
    }
  }

  // ---- errores del <video> y "no decodifica"

  let pendingSeek: number | null = null;
  let decodeChecked = false;

  function onError() {
    const err = video?.error;
    if (!err || !info || hls) return;
    const direct = info.mode === "direct" && !forceHls;
    if (direct && (err.code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED || err.code === MediaError.MEDIA_ERR_DECODE)) {
      fallback("any");
    } else if (err.code === MediaError.MEDIA_ERR_NETWORK) {
      // Corte de red: se reintenta desde el mismo lugar; si sigue, se ofrece otro stream.
      if (attempts.net++ < 4) setTimeout(() => attach(t), 1000 * attempts.net);
      else {
        stuck = "error";
        stuckMsg = "The stream stopped responding.";
      }
    } else if (!direct || !fallback("any")) {
      stuck = "error";
      stuckMsg = err.message || "This file can't be played.";
    }
  }

  /** El modo direct no sirve en este navegador: se pide otra vez al server con menos
   *  capacidades (pasa a remux o transcode) y se sigue desde el mismo lugar. */
  function fallback(what: "video" | "audio" | "any"): boolean {
    if (!info || !stream) return false;
    // Sin ffmpeg no hay a que degradar: otro stream (o el panel para elegirlo).
    if (info.transcode_available === false) {
      const alt = nextPlayable(stream);
      if (alt) {
        toast("This file can't play on this device. Trying another stream…");
        switchTo(alt);
        return true;
      }
      stuck = "codec";
      stuckMsg = "This device can't play this file without your server. Another stream will probably work.";
      return false;
    }
    const less = degrade(caps, info, what);
    if (!less || attempts.fallback >= 3) {
      stuck = "codec";
      stuckMsg = "Neither this device nor the server could play this file. Another stream will probably work.";
      return false;
    }
    attempts.fallback++;
    toast("Switching to compatible playback…");
    open(stream, t, less);
    return true;
  }

  /** Siguiente stream de la lista que todavia no se probo (sin ffmpeg; hasta 3 saltos solos). */
  const tried = new Set<string>();
  function nextPlayable(from: Stream): Stream | null {
    tried.add(from.key);
    if (tried.size > 3) return null;
    return streams.find((x) => !tried.has(x.key)) ?? null;
  }

  /** A los ~3 s de reproducir en direct: ¿hay imagen? ¿hay sonido? */
  function checkDecode() {
    if (decodeChecked || !video || !info || info.mode !== "direct" || forceHls) return;
    decodeChecked = true;
    const v = video;
    const at = v.currentTime;
    setTimeout(() => {
      if (destroyed || v !== video || v.paused || v.currentTime - at < 1.5) {
        decodeChecked = v.currentTime - at >= 1.5;
        return;
      }
      if (v.videoWidth === 0 && (info?.width || info?.video_codec)) return void fallback("video");
      const audio = (v as HTMLVideoElement & { webkitAudioDecodedByteCount?: number }).webkitAudioDecodedByteCount;
      if (audio === 0 && info?.audio_tracks.length && !v.muted) fallback("audio");
    }, 3000);
  }

  // ---- estado del server y watchdog

  let pollTimer = 0;
  let pollCtl: AbortController | null = null;
  function startPolling() {
    clearTimeout(pollTimer);
    poll();
  }
  async function poll() {
    if (destroyed) return;
    const need = !started || waiting || !!stuck;
    if (need && info) {
      pollCtl?.abort();
      pollCtl = new AbortController();
      try {
        status = await api.status(info.session_id, pollCtl.signal);
      } catch {}
    }
    watchdog();
    pollTimer = window.setTimeout(poll, need ? 1000 : 4000);
  }

  function watchdog() {
    // La API ya le habla a otro server que el de este video (modo local: se cayo el de casa, o
    // volvio) y el video se trabo: se sigue en el activo, desde el mismo lugar.
    if (info && stream && started && (waiting || stuck === "error") && !fatal && movedAway(info.session_id) && Date.now() - waitSince > 3000) {
      stuck = "";
      toast("Reconnecting playback…");
      open(stream, t);
      return;
    }
    if (stuck || fatal || askResume || !stream) return;
    if (status?.state === "error") {
      stuck = "error";
      stuckMsg = status.error ?? "The server couldn't read this file.";
      return;
    }
    const now = Date.now();
    const slow = (status?.down_bps ?? 0) < SLOW_BPS;
    if (!started && now - loadSince > (slow ? STUCK_MS + 5000 : START_GIVEUP_MS)) {
      stuck = "slow";
    } else if (started && waiting && now - waitSince > STUCK_MS && slow) {
      stuck = "slow";
    }
  }

  function keepWaiting() {
    stuck = "";
    loadSince = Date.now();
    waitSince = Date.now();
  }

  async function switchTo(s: Stream) {
    menu = "";
    stuck = "";
    const at = started ? t : 0;
    started = false;
    attempts = { net: 0, media: 0, fallback: 0 };
    setQuery({ s: s.key });
    api.prefetch(s);
    detach();
    await open(s, at, detectCaps());
  }

  // ---- eventos del video

  function onLoadedMeta() {
    if (pendingSeek !== null && video) {
      video.currentTime = pendingSeek;
      pendingSeek = null;
    }
    vDur = isFinite(video!.duration) ? video!.duration : 0;
    applyNativeAudio();
  }

  let waitTimer = 0;
  function onWaiting() {
    waiting = true;
    waitSince = Date.now();
    clearTimeout(waitTimer);
    waitTimer = window.setTimeout(() => waiting && (waitShown = true), 400);
    clearTimeout(pollTimer);
    poll();
  }
  function onPlaying() {
    waiting = false;
    waitShown = false;
    clearTimeout(waitTimer);
    paused = false;
    if (!started) {
      started = true;
      attempts.net = 0;
    }
    checkDecode();
  }

  function onTime() {
    // Sin fuente (entre una sesion y otra) el video dice 0: no pisar la posicion real.
    if (!video || video.readyState === 0) return;
    t = offset + video.currentTime;
    // En pausa el rAF no corre: un salto tiene que cambiar la linea igual.
    if (paused) renderSub();
    readBuffered();
    syncPosition();
    if (!prefetchedNext && nextEp && duration && t / duration > 0.85) prefetchNext();
  }

  function readBuffered() {
    if (!video) return;
    const b = video.buffered;
    const out: [number, number][] = [];
    for (let i = 0; i < b.length; i++) out.push([offset + b.start(i), offset + b.end(i)]);
    buffered = out;
  }

  /** Cuanto de lo que hace falta para seguir ya esta: lo que tiene el video, lo que dice el
   *  server que hay listo despues de la posicion, y si nada de eso se sabe, lo descargado. */
  function bufPct(): number | null {
    const goal = status?.buffer_target_s || BUFFER_GOAL_S;
    let ahead = 0;
    if (video && video.readyState > 0) {
      const b = video.buffered;
      const c = video.currentTime;
      for (let i = 0; i < b.length; i++) if (b.start(i) <= c + 0.5 && b.end(i) > c) ahead = b.end(i) - c;
    }
    const server = status?.buffered_ahead_s ?? 0;
    const pct = (Math.max(ahead, server) / goal) * 100;
    if (pct > 0) return Math.min(99, pct);
    if (status && status.total_bytes > 0 && status.state !== "metadata") return Math.min(99, status.progress * 100);
    return null;
  }

  function onEnded() {
    ended = true;
    saveProgress();
    if (nextEp && prefs.autoplayNext) startCountdown();
  }

  // Tiempo y subtitulos a 60 fps solo mientras se reproduce (barato: un rAF).
  let raf = 0;
  function frame() {
    if (!video) return;
    if (video.readyState === 0) {
      raf = requestAnimationFrame(frame);
      return;
    }
    t = offset + video.currentTime;
    if (cues.length) {
      const h = cuesAt(cues, t - subDelay);
      if (h !== subHtml) subHtml = h;
    }
    raf = requestAnimationFrame(frame);
  }
  $effect(() => {
    if (!paused) raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  });

  // ---- busqueda

  function seek(to: number) {
    if (!video || !info) return;
    to = Math.max(0, Math.min(duration ? duration - 0.5 : to, to));
    ended = false;
    countdown = null;
    const local = to - offset;
    const usingHls = !!hls || ((info.mode !== "direct" || forceHls) && !!info.hls_url);
    if (!usingHls) {
      if (video.readyState === 0) pendingSeek = to;
      else video.currentTime = to;
    } else {
      const s = video.seekable;
      const end = s.length ? s.end(s.length - 1) : 0;
      // Dentro de lo que ya genero el server: salto normal. Fuera: playlist nueva desde ahi.
      if (local >= 0 && local <= end + 2) video.currentTime = local;
      else attach(to);
    }
    t = to;
    syncPosition(true);
  }

  function skip(d: number) {
    seek(t + d);
    bump(d > 0 ? "fwd" : "back", `${d > 0 ? "+" : "−"}${Math.abs(d)}s`);
  }

  // ---- reproducir / pausa / volumen / velocidad

  function toggle() {
    if (!video || !info) return;
    if (ended) {
      seek(0);
      tryPlay();
      return;
    }
    if (video.paused) {
      tryPlay();
      bump("play");
    } else {
      video.pause();
      bump("pause");
    }
  }

  let flashK = 0;
  let flashTimer = 0;
  function bump(icon: FlashIcon, text?: string) {
    flash = { k: ++flashK, icon, text };
    // Con "reducir animaciones" no hay animationend: se saca igual.
    clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => (flash = null), 700);
  }

  // ---- velocidad: [ y ] por los pasos del menu; mantener Space (o el dedo) = 2× mientras dura

  function stepRate(d: 1 | -1) {
    if (holding) return;
    const i = SPEEDS.indexOf(rate);
    const n = i >= 0 ? SPEEDS[i + d] : d > 0 ? SPEEDS.find((v) => v > rate) : [...SPEEDS].reverse().find((v) => v < rate);
    if (n !== undefined) rate = n;
    bump("speed", rate === 1 ? "Normal speed" : `${rate}×`);
  }

  let holding = $state(false);
  let holdPrev = 1;
  let holdTimer = 0;
  let spaceHeld = false;
  /** Como Stremio (usePlaybackSpeedHold): solo mientras se reproduce; al soltar, la de antes. */
  function startHold() {
    if (holding || !video || video.paused || !started) return;
    holdPrev = rate;
    holding = true;
    rate = Math.max(2, rate);
  }
  function endHold() {
    clearTimeout(holdTimer);
    if (!holding) return;
    holding = false;
    rate = holdPrev;
  }
  /** Space: en pausa arranca enseguida; reproduciendo, soltar antes de 300 ms pausa y mantener
   *  pasa a 2×. */
  function spaceDown(e: KeyboardEvent) {
    if (e.repeat || spaceHeld) return;
    if (!video || video.paused || !started || ended) return toggle();
    spaceHeld = true;
    clearTimeout(holdTimer);
    holdTimer = window.setTimeout(startHold, 300);
  }
  function onKeyUp(e: KeyboardEvent) {
    if (tv.on && (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "MediaFastForward" || e.key === "MediaRewind")) commitSeek();
    if (e.key !== " " || !spaceHeld) return;
    spaceHeld = false;
    e.preventDefault();
    if (holding) endHold();
    else {
      clearTimeout(holdTimer);
      toggle();
    }
  }
  function onBlur() {
    spaceHeld = false;
    endHold();
  }

  // ---- escala del video: Fit (entero), Fill (estirado a la pantalla), Crop (llena recortando
  // los bordes). V la cambia, pellizcar en el celular; se recuerda mientras dure la sesion.

  type Scale = "fit" | "fill" | "crop";
  const SCALES: Scale[] = ["fit", "fill", "crop"];
  const SCALE_LABEL: Record<Scale, string> = { fit: "Fit", fill: "Fill", crop: "Crop" };
  const SCALE_CSS: Record<Scale, string> = { fit: "contain", fill: "fill", crop: "cover" };
  const SCALE_KEY = "mythos.scale";
  let scale = $state<Scale>(readScale());
  function readScale(): Scale {
    try {
      const v = sessionStorage.getItem(SCALE_KEY) as Scale | null;
      return v && SCALES.includes(v) ? v : "fit";
    } catch {
      return "fit";
    }
  }
  function setScale(s: Scale) {
    scale = s;
    try {
      sessionStorage.setItem(SCALE_KEY, s);
    } catch {}
    bump("scale", SCALE_LABEL[s]);
  }
  const cycleScale = () => setScale(SCALES[(SCALES.indexOf(scale) + 1) % SCALES.length]);

  // ---- estadisticas (D, o "Playback stats" en el panel de streams): /status solo mientras se ven

  let showStats = $state(false);
  let viaHls = $state(false); // lo que se conecto en attach (direct puede ir por HLS por el audio)
  let stats = $state<MediaStatus | null>(null);
  let frames = $state<{ dropped: number; total: number } | null>(null);
  let aheadS = $state(0);
  let shownRes = $state("");
  function readLocal() {
    if (!video) return;
    const q = video.getVideoPlaybackQuality?.();
    frames = q ? { dropped: q.droppedVideoFrames, total: q.totalVideoFrames } : null;
    shownRes = video.videoWidth ? `${video.videoWidth}×${video.videoHeight}` : "";
    let ahead = 0;
    const b = video.buffered;
    const c = video.currentTime;
    for (let i = 0; i < b.length; i++) if (b.start(i) <= c + 0.5 && b.end(i) > c) ahead = b.end(i) - c;
    aheadS = ahead;
  }
  $effect(() => {
    if (!showStats) return;
    const sid = info?.session_id;
    const ctl = new AbortController();
    let timer = 0;
    let alive = true;
    const tick = async () => {
      untrack(readLocal);
      if (sid)
        try {
          stats = await api.status(sid, ctl.signal);
        } catch {}
      if (alive) timer = window.setTimeout(tick, 1000);
    };
    tick();
    return () => {
      alive = false;
      clearTimeout(timer);
      ctl.abort();
    };
  });
  const curAudio = $derived(info?.audio_tracks.find((a) => a.index === audioSel));

  // ---- gestos en pantallas tactiles (sobre el video): tocar muestra o esconde los controles,
  // doble toque a un costado = −10 / +10 s (seguir tocando suma), mantener = 2×, pellizcar
  // hacia afuera = Crop y hacia adentro = Fit. El mouse sigue igual (clic pausa, doble clic F).

  const pts = new Map<number, { x: number; y: number }>();
  let gest = { x0: 0, y0: 0, at: 0, moved: false, multi: false, pinch0: 0, pinched: false, uiAtDown: true, long: false };
  let longTimer = 0;
  let tapTimer = 0;
  let lastTap = { at: 0, zone: 0 };
  let combo = $state<{ k: number; zone: -1 | 1; total: number } | null>(null);
  let comboUntil = 0;
  let comboTimer = 0;
  let lastPointer = "mouse";
  const spread = () => {
    const [a, b] = [...pts.values()];
    return a && b ? Math.hypot(a.x - b.x, a.y - b.y) : 0;
  };

  function onVDown(e: PointerEvent) {
    lastPointer = e.pointerType;
    if (e.pointerType !== "touch") return;
    pts.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pts.size === 1) {
      // `ui` antes de que el pointerdown del reproductor muestre los controles.
      gest = { x0: e.clientX, y0: e.clientY, at: Date.now(), moved: false, multi: false, pinch0: 0, pinched: false, uiAtDown: ui, long: false };
      clearTimeout(longTimer);
      longTimer = window.setTimeout(() => {
        if (gest.moved || gest.multi) return;
        startHold();
        gest.long = holding;
      }, 450);
    } else {
      gest.multi = true;
      clearTimeout(longTimer);
      if (pts.size === 2) gest.pinch0 = spread();
    }
  }
  function onVMove(e: PointerEvent) {
    if (e.pointerType !== "touch" || !pts.has(e.pointerId)) return;
    pts.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pts.size >= 2) {
      if (!gest.pinch0 || gest.pinched) return;
      const r = spread() / gest.pinch0;
      if (r > 1.2 || r < 0.83) {
        gest.pinched = true;
        setScale(r > 1 ? "crop" : "fit");
      }
    } else if (Math.hypot(e.clientX - gest.x0, e.clientY - gest.y0) > 12) {
      gest.moved = true;
      clearTimeout(longTimer);
    }
  }
  function onVUp(e: PointerEvent) {
    if (e.pointerType !== "touch" || !pts.delete(e.pointerId) || pts.size) return;
    clearTimeout(longTimer);
    if (gest.long) return endHold();
    if (e.type === "pointercancel" || gest.multi || gest.moved || Date.now() - gest.at > 350) return;
    tap(e.clientX);
  }
  function tap(x: number) {
    if (!started || !root) return;
    const w = root.clientWidth;
    const zone = x < w / 3 ? -1 : x > (2 * w) / 3 ? 1 : 0;
    const now = Date.now();
    if (zone && combo?.zone === zone && now < comboUntil) return comboSeek(zone);
    if (zone && lastTap.zone === zone && now - lastTap.at < 300) {
      clearTimeout(tapTimer);
      lastTap = { at: 0, zone: 0 };
      return comboSeek(zone);
    }
    lastTap = { at: now, zone };
    clearTimeout(tapTimer);
    const was = gest.uiAtDown;
    // Al centro no hay doble toque: los controles enseguida. A los costados se espera un poco.
    if (!zone) toggleUi(was);
    else tapTimer = window.setTimeout(() => toggleUi(was), 300);
  }
  function toggleUi(wasShown: boolean) {
    if (wasShown) {
      clearTimeout(hideTimer);
      ui = false;
    } else activity();
  }
  function comboSeek(zone: -1 | 1) {
    const now = Date.now();
    const total = (combo?.zone === zone && now < comboUntil ? combo.total : 0) + 10;
    combo = { k: ++flashK, zone, total };
    comboUntil = now + 700;
    seek(t + zone * 10);
    clearTimeout(comboTimer);
    comboTimer = window.setTimeout(() => (combo = null), 800);
  }

  // ---- Media Session: teclas multimedia, auriculares, la notificacion de Android y la
  // pantalla bloqueada. Titulo, poster y posicion; play/pausa/saltos/siguiente episodio.

  const msess = typeof navigator !== "undefined" && "mediaSession" in navigator ? navigator.mediaSession : null;
  $effect(() => {
    if (!msess || !meta || typeof MediaMetadata === "undefined") return;
    const art = sized(ep?.thumbnail && !prefs.hideSpoilers ? ep.thumbnail : meta.poster, "medium");
    try {
      msess.metadata = new MediaMetadata({
        title: ep ? `${epCode(ep.season, ep.episode)}${ep.name ? ` · ${ep.name}` : ""}` : meta.name,
        artist: ep ? meta.name : (meta.year ?? ""),
        album: "Mythos",
        artwork: art ? [{ src: art }] : [],
      });
    } catch {}
  });
  function mediaHandlers(on: boolean) {
    if (!msess) return;
    const list: [MediaSessionAction, MediaSessionActionHandler][] = [
      ["play", () => video?.paused && toggle()],
      ["pause", () => video && !video.paused && toggle()],
      ["stop", () => video?.pause()],
      ["seekbackward", (d) => skip(-(d.seekOffset || 10))],
      ["seekforward", (d) => skip(d.seekOffset || 10)],
      ["seekto", (d) => d.seekTime != null && seek(d.seekTime)],
    ];
    for (const [a, f] of list)
      try {
        msess.setActionHandler(a, on ? f : null);
      } catch {}
  }
  $effect(() => {
    if (!msess) return;
    const next = nextEp ? () => playNext() : null;
    try {
      msess.setActionHandler("nexttrack", next);
    } catch {}
  });
  $effect(() => {
    if (msess) msess.playbackState = !started ? "none" : paused ? "paused" : "playing";
  });
  let posAt = 0;
  /** La posicion para la barra del sistema (una vez por segundo, o enseguida al saltar). */
  function syncPosition(force = false) {
    if (!msess?.setPositionState || !duration || !started) return;
    const now = Date.now();
    if (!force && now - posAt < 1000) return;
    posAt = now;
    try {
      msess.setPositionState({ duration, position: Math.max(0, Math.min(t, duration)), playbackRate: rate || 1 });
    } catch {}
  }
  $effect(() => {
    void rate;
    void paused;
    untrack(() => syncPosition(true));
  });

  // ---- "?": la lista de atajos (la misma de Settings)
  let showKeys = $state(false);
  const coarse = typeof matchMedia !== "undefined" && matchMedia("(pointer: coarse)").matches;

  function setVolume(v: number) {
    prefs.volume = Math.max(0, Math.min(1, v));
    prefs.muted = prefs.volume === 0;
    savePrefs(false);
  }
  function toggleMute() {
    prefs.muted = !prefs.muted;
    if (!prefs.muted && prefs.volume === 0) prefs.volume = 0.6;
    savePrefs(false);
    bump("vol", prefs.muted ? "Muted" : `${Math.round(prefs.volume * 100)}%`);
  }
  $effect(() => {
    if (!video) return;
    video.volume = prefs.volume;
    video.muted = prefs.muted;
  });
  $effect(() => {
    if (video) video.playbackRate = rate;
  });

  async function toggleFs() {
    if (!root) return;
    await setFullscreen(root, !fs);
    fs = await isFullscreen();
  }

  // ---- audio

  const canAudio = $derived(
    !!info && info.audio_tracks.length > 1 && (!!info.hls_url || !!(video as unknown as { audioTracks?: unknown })?.audioTracks),
  );
  type NativeTracks = { length: number; [i: number]: { enabled: boolean } };
  const nativeTracks = () => (video as unknown as { audioTracks?: NativeTracks } | undefined)?.audioTracks;

  /** Al abrir una sesion: la pista del idioma preferido (Settings → Playback) si la hay. Si no es
   *  la que viene por defecto: en directo con las pistas del navegador (al cargar, ver
   *  applyNativeAudio) y si no hay, por HLS con esa pista (igual que elegirla a mano). */
  function pickPreferredAudio(p: PlayInfo) {
    const def = p.audio_tracks.find((a) => a.default) ?? p.audio_tracks[0];
    const want = preferredAudio(p.audio_tracks, prefs.audioLang);
    audioSel = def?.index ?? null;
    if (!want || !def || want.index === def.index) return;
    if (p.mode === "direct" && nativeTracks()) audioSel = want.index;
    else if (p.hls_url) {
      audioSel = want.index;
      forceHls = true;
      hlsAudio = want.index;
    }
  }
  function applyNativeAudio() {
    const native = nativeTracks();
    if (!info || info.mode !== "direct" || forceHls || !native?.length) return;
    const pos = info.audio_tracks.findIndex((a) => a.index === audioSel);
    if (pos >= 0 && native.length === info.audio_tracks.length) for (let i = 0; i < native.length; i++) native[i].enabled = i === pos;
  }

  function pickAudio(index: number) {
    menu = "";
    if (!info || index === audioSel) return;
    audioSel = index;
    const native = (video as unknown as { audioTracks?: { length: number; [i: number]: { enabled: boolean } } }).audioTracks;
    if (info.mode === "direct" && !forceHls && native?.length) {
      const pos = info.audio_tracks.findIndex((a) => a.index === index);
      for (let i = 0; i < native.length; i++) native[i].enabled = i === pos;
      return;
    }
    if (!info.hls_url) return;
    forceHls = true;
    hlsAudio = index;
    attach(t);
  }

  // ---- subtitulos

  /** Nombre del grupo: el idioma en ese idioma ("Español (Latinoamérica)"); sin idioma, "Unknown". */
  function groupName(code?: string | null): string {
    const c = (code ?? "").toLowerCase();
    return !c || c === "und" ? "Unknown" : nativeLangName(c);
  }

  let subAuto = false;
  function autoSub() {
    if (subAuto || subSel || !info) return;
    const want = prefs.subLang;
    if (!want) return;
    // Si el audio ya esta en ese idioma, no hacen falta (salvo los "forced").
    const audioLang = normLang(info.audio_tracks.find((a) => a.index === audioSel)?.lang);
    const forced = intSubs.find((s) => s.lang === want && /forced/i.test(s.label));
    if (audioLang === want) {
      if (forced) pickSub(forced.id, false);
      subAuto = true;
      return;
    }
    const s = intSubs.find((x) => x.lang === want && !/forced/i.test(x.label)) ?? extSubs.find((x) => x.lang === want && x.version === 1);
    if (s) {
      subAuto = true;
      pickSub(s.id, false);
    }
  }

  /** Activa una pista (o ninguna con ""). `manual`: elegida a mano, se avisa si falla. */
  async function pickSub(sid: string, manual = true) {
    if (sid === subSel) return;
    subSel = sid;
    cues = [];
    subHtml = "";
    if (!sid) return;
    const s = subs.find((x) => x.id === sid);
    if (!s) return;
    try {
      const r = await fetch(mediaUrl(s.url));
      if (!r.ok) throw new Error(String(r.status));
      const text = await r.text();
      if (subSel !== sid) return;
      cues = parseVtt(text);
      renderSub();
      // El server todavia esta extrayendo la pista: se vuelve a pedir y se reemplaza.
      if (r.headers.get("X-Mythos-Partial") === "true") laterRefresh(sid);
    } catch {
      if (subSel === sid) {
        subSel = "";
        // Elegidos a mano: se avisa. Los automaticos fallan en silencio.
        if (manual) toast("Couldn't load those subtitles", true);
      }
    }
  }

  function laterRefresh(sid: string) {
    setTimeout(() => subSel === sid && !destroyed && refreshSub(sid), 5000);
  }

  async function refreshSub(sid: string) {
    const s = subs.find((x) => x.id === sid);
    // Entre una sesion y otra (fallback a HLS) la pista no esta en la lista: se espera.
    if (!s) return laterRefresh(sid);
    try {
      const r = await fetch(mediaUrl(s.url));
      if (!r.ok) return laterRefresh(sid);
      const text = await r.text();
      if (subSel !== sid) return;
      cues = parseVtt(text);
      renderSub();
      if (r.headers.get("X-Mythos-Partial") === "true") laterRefresh(sid);
    } catch {
      laterRefresh(sid);
    }
  }

  /** La linea de ahora (en pausa el rAF no corre, asi que se llama al cambiar algo). */
  function renderSub() {
    subHtml = cues.length ? cuesAt(cues, t - subDelay) : "";
  }

  // ---- retraso por pista: positivo = los subtitulos salen mas tarde. Se recuerda por archivo
  // (info_hash + archivo, o la URL) y pista: volver a ese subtitulo lo deja como estaba.

  let delays = $state<Record<string, number>>(loadDelays());
  const fileKey = $derived(
    stream ? (stream.info_hash ? `${stream.info_hash.toLowerCase()}:${stream.file_idx ?? 0}` : (stream.url ?? stream.key)) : "",
  );
  const delayOf = (sid: string) => (sid && fileKey ? (delays[`${fileKey}|${sid}`] ?? 0) : 0);
  const subDelay = $derived(delayOf(subSel));
  const fmtDelay = (v: number) => `${v > 0 ? "+" : v < 0 ? "−" : ""}${Math.abs(v).toFixed(1)} s`;

  function setSubDelay(v: number, show = false) {
    if (!subSel || !fileKey) return;
    const k = `${fileKey}|${subSel}`;
    v = Math.max(-120, Math.min(120, Math.round(v * 10) / 10));
    // Borrar y volver a poner: queda al final (el recorte olvida primero las mas viejas).
    delete delays[k];
    if (v) delays[k] = v;
    saveDelays(delays);
    renderSub();
    if (show) bump("subs", `Delay ${fmtDelay(v)}`);
  }
  function shiftSubs(d: number, show = false) {
    if (!subSel) {
      if (show) bump("subs", "Subtitles are off");
      return;
    }
    setSubDelay(subDelay + d, show);
  }

  // ---- aspecto (global: va en las preferencias y viaja al server)

  const SIZE_STEPS = [75, 90, 100, 125, 150, 175, 200];
  const BG_STEPS = [
    { value: 0, label: "None" },
    { value: 35, label: "Light" },
    { value: 65, label: "Dark" },
    { value: 90, label: "Solid" },
  ];
  const POS_MIN = 2;
  const POS_MAX = 40;

  function stepSize(d: 1 | -1, show = false) {
    const cur = prefs.subSize;
    // Desde un valor que no es de la lista (los de Settings): al siguiente de la lista.
    const next = d > 0 ? SIZE_STEPS.find((v) => v > cur) : [...SIZE_STEPS].reverse().find((v) => v < cur);
    if (next !== undefined) {
      prefs.subSize = next;
      savePrefs();
    }
    peekStyle();
    if (show) bump("subs", `Subtitles ${prefs.subSize}%`);
  }
  function setBg(v: number) {
    prefs.subBg = v;
    savePrefs();
    peekStyle();
  }
  function stepPos(d: 1 | -1) {
    prefs.subPos = Math.max(POS_MIN, Math.min(POS_MAX, prefs.subPos + d * 2));
    savePrefs();
    peekStyle();
  }

  // Al cambiar el aspecto sin ninguna linea en pantalla, una de muestra por un momento.
  let stylePeek = $state(false);
  let peekTimer = 0;
  function peekStyle() {
    stylePeek = true;
    clearTimeout(peekTimer);
    peekTimer = window.setTimeout(() => (stylePeek = false), 2500);
  }

  // ---- el menu: primero idiomas; despues las opciones de ese idioma, el retraso y el aspecto

  type SubGroup = { name: string; list: Sub[] };
  /** Un grupo por idioma: el preferido y el del audio arriba, despues los que vienen en el
   *  archivo y el resto en orden alfabetico. Adentro, primero las internas. */
  const subGroups = $derived.by<SubGroup[]>(() => {
    const audioLang = normLang(info?.audio_tracks.find((a) => a.index === audioSel)?.lang);
    const map = new Map<string, Sub[]>();
    for (const s of subs) map.set(s.group, [...(map.get(s.group) ?? []), s]);
    const rank = (l: Sub[]) =>
      l.some((x) => x.lang === prefs.subLang) ? 0 : l.some((x) => x.lang === audioLang) ? 1 : l.some((x) => !x.ext) ? 2 : 3;
    return [...map.entries()]
      .map(([name, list]) => ({ name, list }))
      .sort((a, b) => rank(a.list) - rank(b.list) || a.name.localeCompare(b.name));
  });
  const activeGroup = $derived(subs.find((x) => x.id === subSel)?.group ?? "");
  const subName = (s: Sub) => s.label || (s.ext ? `Version ${s.version}` : `Track ${s.version}`);

  let subView = $state(""); // "" = lista de idiomas; si no, el idioma abierto
  let subDir = $state(1);
  let subPanel = $state<HTMLDivElement>();
  const viewGroup = $derived(subView ? subGroups.find((g) => g.name === subView) : undefined);

  function toggleSubsMenu() {
    if (menu === "subs") {
      menu = "";
      return;
    }
    // Con un subtitulo puesto se abre directo en su idioma (retraso y tamaño a mano).
    subView = activeGroup;
    subDir = 1;
    menu = "subs";
    focusMenu();
  }
  function openGroup(g: SubGroup) {
    subDir = 1;
    subView = g.name;
    // Como Stremio: elegir el idioma ya pone su primera opcion.
    if (!g.list.some((x) => x.id === subSel)) pickSub(g.list[0].id);
    focusMenu();
  }
  function backToLangs() {
    const from = subView;
    subDir = -1;
    subView = "";
    focusMenu(from);
  }
  function subsOff() {
    pickSub("");
    menu = "";
  }

  /** Foco adentro del menu al cambiar de paso: el idioma del que se volvio, o lo elegido. */
  async function focusMenu(group?: string) {
    await tick();
    const p = subPanel;
    if (!p) return;
    const el =
      (group ? [...p.querySelectorAll<HTMLElement>("[data-group]")].find((x) => x.dataset.group === group) : undefined) ??
      p.querySelector<HTMLElement>(".mi.sel") ??
      p.querySelector<HTMLElement>("button");
    el?.focus({ preventScroll: true });
    el?.scrollIntoView({ block: "nearest" });
  }

  /** Teclas dentro de un menu del reproductor: flechas para moverse, ← para volver un paso,
   *  → para entrar a un idioma. Devuelve true si la tecla es del menu (el reproductor no la usa). */
  function menuKey(e: KeyboardEvent, panel: HTMLElement): boolean {
    const k = e.key;
    if (k === " " || k === "Enter") return true; // el boton enfocado se activa solo
    if (k === "ArrowDown" || k === "ArrowUp" || k === "Home" || k === "End") {
      const items = [...panel.querySelectorAll<HTMLElement>("button:not(:disabled)")];
      const i = items.indexOf(document.activeElement as HTMLElement);
      const n = k === "Home" ? 0 : k === "End" ? items.length - 1 : (i + (k === "ArrowDown" ? 1 : -1) + items.length) % items.length;
      items[n]?.focus();
      items[n]?.scrollIntoView({ block: "nearest" });
      e.preventDefault();
      return true;
    }
    if (k === "ArrowLeft" || k === "ArrowRight") {
      if (menu === "subs") {
        const name = (document.activeElement as HTMLElement | null)?.dataset.group;
        const g = name ? subGroups.find((x) => x.name === name) : undefined;
        if (k === "ArrowLeft" && subView) backToLangs();
        else if (k === "ArrowRight" && g) openGroup(g);
      }
      e.preventDefault();
      return true;
    }
    return false;
  }

  let lastSub = "";
  function toggleSubs() {
    if (subSel) {
      lastSub = subSel;
      pickSub("", false);
      bump("subs", "Subtitles off");
    } else {
      const s = subs.find((x) => x.id === lastSub) ?? subs.find((x) => x.lang === prefs.subLang) ?? subs[0];
      if (s) {
        pickSub(s.id, false);
        bump("subs", subs.filter((x) => x.group === s.group).length > 1 ? `${s.group} · ${subName(s)}` : s.group);
      }
    }
  }

  // ---- siguiente episodio

  let prefetchedNext = false;
  function prefetchNext() {
    prefetchedNext = true;
    if (!nextEp) return;
    api
      .streams("series", nextEp.id)
      .then((l) => {
        const first = sortByLang(l, prefs.audioLang)[0];
        if (first) api.prefetch(first);
      })
      .catch(() => {});
  }

  let cdTimer = 0;
  function startCountdown() {
    countdown = 10;
    clearInterval(cdTimer);
    cdTimer = window.setInterval(() => {
      if (countdown === null) return clearInterval(cdTimer);
      countdown--;
      if (countdown <= 0) {
        clearInterval(cdTimer);
        playNext();
      }
    }, 1000);
  }
  function cancelCountdown() {
    clearInterval(cdTimer);
    countdown = null;
  }
  function playNext() {
    cancelCountdown();
    if (!nextEp) return;
    saveProgress();
    go(`/watch/series/${nextEp.id}`, { replace: true });
  }

  // ---- progreso

  function saveProgress(keepalive = false) {
    if (!info || !meta || !duration || t < 5) return;
    api
      .saveProgress(
        {
          meta_id: id,
          type,
          series_id: type === "series" ? seriesId : undefined,
          name: meta.name,
          poster: meta.poster,
          background: meta.background,
          episode_label: epLabel || undefined,
          position_s: Math.round(t),
          duration_s: Math.round(duration),
          stream_key: stream?.key,
        },
        keepalive,
      )
      .catch(() => {});
  }
  $effect(() => {
    if (paused || !started) return;
    const iv = setInterval(() => saveProgress(), 10_000);
    return () => clearInterval(iv);
  });

  let left = false;
  function leave(keepalive: boolean) {
    if (left) return;
    left = true;
    clearTimeout(pollTimer);
    clearInterval(cdTimer);
    clearTimeout(hideTimer);
    clearTimeout(waitTimer);
    clearTimeout(peekTimer);
    for (const tm of [holdTimer, longTimer, tapTimer, comboTimer, flashTimer, seekTimer]) clearTimeout(tm);
    cancelAnimationFrame(raf);
    pollCtl?.abort();
    saveProgress(keepalive);
    if (info) api.closeSession(info.session_id);
    detach();
  }

  function resume(at: number) {
    askResume = null;
    resolveResume?.(at);
    resolveResume = null;
    loadSince = Date.now();
  }

  function exit() {
    back(type === "series" ? `/title/series/${seriesId}` : `/title/movie/${id}`);
  }

  // ---- controles que se esconden solos

  let hideTimer = 0;
  function activity() {
    ui = true;
    clearTimeout(hideTimer);
    hideTimer = window.setTimeout(() => {
      if (!paused && !menu && !dragging && started && !stuck) ui = false;
    }, 2800);
  }
  $effect(() => {
    if (paused || menu || stuck) ui = true;
    else activity();
  });

  // ---- control remoto (modo TV, lib/tv.svelte.ts)
  //
  // Con los controles escondidos (o sin ninguno enfocado): centro = play/pausa, izquierda/derecha
  // = -10/+10 s (manteniendo, el salto crece y se aplica al soltar: un solo pedido al server),
  // arriba/abajo = mostrar los controles y enfocar play. Con un control enfocado las flechas se
  // mueven entre ellos (lib/spatial.svelte.ts) y el centro lo aprieta; abajo de todo los esconde.
  // Atras sale (el Esc de siempre, lib/shell.svelte.ts). Los menus (.pmenu) siguen con menuKey.

  const ARROWS: Record<string, Dir> = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" };

  /** Salto que se esta armando mientras se mantiene la flecha (null: ninguno). */
  let seekTarget = $state<number | null>(null);
  let seekSince = 0;
  let seekLast = 0;
  let seekTimer = 0;
  function tvSeek(dir: 1 | -1, repeat: boolean) {
    if (!started || !duration) return;
    if (!repeat) {
      commitSeek();
      skip(10 * dir);
      return;
    }
    const now = performance.now();
    if (seekTarget === null) {
      seekTarget = t;
      seekSince = now;
    } else if (now - seekLast < 120) return; // el control repite ~20 veces por segundo
    seekLast = now;
    const held = now - seekSince;
    const step = held < 1000 ? 10 : held < 3000 ? 30 : 60;
    seekTarget = Math.max(0, Math.min(duration - 1, seekTarget + dir * step));
    bump(dir > 0 ? "fwd" : "back", clock(seekTarget));
    clearTimeout(seekTimer);
    seekTimer = window.setTimeout(commitSeek, 600);
  }
  function commitSeek() {
    clearTimeout(seekTimer);
    if (seekTarget === null) return;
    const to = seekTarget;
    seekTarget = null;
    seek(to);
  }

  /** Lo que flota y tiene botones propios (retomar, error, "siguiente episodio"): su primario. */
  const primary = () => [...(root?.querySelectorAll<HTMLElement>(".btn.primary") ?? [])].find((b) => b.getClientRects().length > 0);

  function showControls() {
    ui = true;
    const el = primary() ?? root?.querySelector<HTMLElement>(".ctrls .pbtn.big");
    if (el) focusEl(el);
  }

  /** "done": usada; "native": la aprieta el boton enfocado; "pass": sigue el teclado de siempre. */
  function tvKey(e: KeyboardEvent): "done" | "native" | "pass" {
    const k = e.key;
    const d = ARROWS[k];
    const el = document.activeElement as HTMLElement | null;
    const focused = !!el && el !== root && !!root?.contains(el);
    if (k === "MediaPlayPause") return toggle(), "done";
    if (k === "MediaPlay") return video?.paused && toggle(), "done";
    if (k === "MediaPause") return video && !video.paused && toggle(), "done";
    if (k === "MediaFastForward" || k === "MediaRewind") return tvSeek(k === "MediaFastForward" ? 1 : -1, e.repeat), "done";
    if (k === "Escape" || (!d && k !== "Enter")) return "pass";
    // El panel de streams: las flechas adentro de el.
    const side = menu === "streams" ? root?.querySelector<HTMLElement>(".side") : null;
    if (side) {
      if (!d) return "native";
      if (!el || !side.contains(el)) {
        const b = side.querySelector<HTMLElement>("[aria-current='true'], button");
        if (b) focusEl(b);
      } else moveFocus(d, side);
      return "done";
    }
    // Retomar o empezar de nuevo.
    const card = askResume ? root?.querySelector<HTMLElement>(".resume-card") : null;
    if (card) {
      if (!d) return focused ? "native" : "pass";
      if (!el || !card.contains(el)) showControls();
      else moveFocus(d, card);
      return "done";
    }
    const slider = !!el?.matches('[role="slider"]');
    if (!ui || !focused || (slider && (d === "left" || d === "right"))) {
      if (k === "Enter") return slider ? (toggle(), "done") : focused && ui ? "native" : (toggle(), "done");
      if (d === "left" || d === "right") tvSeek(d === "right" ? 1 : -1, e.repeat);
      else if (slider && d === "down") showControls();
      else if (slider) moveFocus(d!, root);
      else showControls();
      return "done";
    }
    if (!d) return "native";
    // Abajo de todo: se esconden los controles.
    if (!moveFocus(d, root) && d === "down") {
      el?.blur();
      ui = false;
    }
    return "done";
  }

  // Al esconderse los controles se suelta el foco: las flechas vuelven a ser saltos.
  $effect(() => {
    if (!tv.on || ui) return;
    const el = document.activeElement as HTMLElement | null;
    if (el && el !== root && root?.contains(el) && !el.closest(".resume-card")) el.blur();
  });

  // ---- teclado

  function onKey(e: KeyboardEvent) {
    if ((e.target as HTMLElement).closest("input, textarea")) return;
    // AltGr (Ctrl+Alt en Windows) escribe [ ] en algunos teclados: esa si pasa.
    if ((e.ctrlKey || e.metaKey || e.altKey) && !e.getModifierState?.("AltGraph")) return;
    const panel = (e.target as HTMLElement).closest<HTMLElement>(".pmenu");
    if (panel && menuKey(e, panel)) return activity();
    if (tv.on && !panel) {
      const r = tvKey(e);
      if (r === "done") e.preventDefault();
      if (r !== "pass") return activity();
    }
    const k = e.key.toLowerCase();
    let handled = true;
    if (askResume) {
      if (k === "enter") resume(askResume.position_s);
      else if (k === "escape") exit();
      else handled = false;
    } else if (k === " ") spaceDown(e);
    else if (k === "k") toggle();
    else if (k === "f") toggleFs();
    else if (k === "m") toggleMute();
    else if (k === "arrowright") skip(5);
    else if (k === "arrowleft") skip(-5);
    else if (k === "l") skip(10);
    else if (k === "j") skip(-10);
    else if (k === "arrowup") (setVolume(prefs.volume + 0.05), (prefs.muted = false), bump("vol", `${Math.round(prefs.volume * 100)}%`));
    else if (k === "arrowdown") (setVolume(prefs.volume - 0.05), bump("vol", `${Math.round(prefs.volume * 100)}%`));
    else if (k === "c") toggleSubs();
    // G/H: el subtitulo puesto 0,5 s antes/despues (con Shift, 0,1 s); se recuerda por pista.
    else if (k === "g") shiftSubs(e.shiftKey ? -0.1 : -0.5, true);
    else if (k === "h") shiftSubs(e.shiftKey ? 0.1 : 0.5, true);
    // - / =: tamaño de los subtitulos (como Stremio).
    else if (k === "-" || k === "_") stepSize(-1, true);
    else if (k === "=" || k === "+") stepSize(1, true);
    else if (k === "n") nextEp && playNext();
    else if (k === "[") stepRate(-1);
    else if (k === "]") stepRate(1);
    else if (k === "v") cycleScale();
    else if (k === "d") showStats = !showStats;
    else if (k === "?") showKeys = !showKeys;
    else if (k === "escape") {
      if (menu) {
        // Si el foco estaba en el menu, vuelve al boton que lo abrio.
        const opener = panel ? root?.querySelector<HTMLElement>(`[data-menu="${menu}"]`) : null;
        menu = "";
        opener?.focus();
      } else if (showKeys) showKeys = false;
      else if (countdown !== null) cancelCountdown();
      else if (fs) toggleFs();
      else exit();
    } else handled = false;
    if (handled) {
      e.preventDefault();
      activity();
    }
  }

  const VolIcon = $derived(prefs.muted || prefs.volume === 0 ? VolumeX : prefs.volume < 0.5 ? Volume1 : Volume2);
  const subSize = $derived(`calc(clamp(18px, 2.4vw, 40px) * ${prefs.subSize / 100})`);
  const subPreview = $derived(stylePeek && !!subSel);
  const bgImg = $derived(sized(meta?.background, "medium"));
  const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2];
</script>

<svelte:window onkeydown={onKey} onkeyup={onKeyUp} onblur={onBlur} />

<div
  class="player"
  class:hide-ui={!ui}
  bind:this={root}
  onpointermove={activity}
  onpointerdown={activity}
  role="application"
  aria-label="Video player"
>
  <!-- svelte-ignore a11y_media_has_caption -->
  <video
    bind:this={video}
    preload="auto"
    playsinline
    style="object-fit: {SCALE_CSS[scale]}"
    onpointerdown={onVDown}
    onpointermove={onVMove}
    onpointerup={onVUp}
    onpointercancel={onVUp}
    onloadedmetadata={onLoadedMeta}
    ondurationchange={() => (vDur = video && isFinite(video.duration) ? video.duration : 0)}
    onwaiting={onWaiting}
    onplaying={onPlaying}
    oncanplay={() => started && waiting && video?.paused === false && onPlaying()}
    onplay={() => (paused = false)}
    onpause={() => ((paused = true), saveProgress())}
    ontimeupdate={onTime}
    onprogress={readBuffered}
    onseeking={() => (waiting = true)}
    onseeked={() => !video?.paused && video!.readyState >= 3 && onPlaying()}
    onended={onEnded}
    onerror={onError}
    onclick={() => started && !menu && lastPointer !== "touch" && toggle()}
    ondblclick={() => lastPointer !== "touch" && toggleFs()}
  ></video>

  <!-- Subtitulos dibujados por la app (no <track> ni ::cue): siguen al tiempo real tambien en HLS
       con ?start=, el retraso de cada pista es solo restar, y tamaño, fondo y altura se ven igual
       en Chrome, WebView2 y el WebView de Android. Al cambiar el aspecto sin nada en pantalla,
       una linea de muestra por un momento. -->
  {#if started && (subHtml || subPreview)}
    <div
      class="subs"
      class:lift={ui}
      class:nobg={prefs.subBg === 0}
      style="--sub-size: {subSize}; --sub-bg: {prefs.subBg}%; --sub-pos: {prefs.subPos}%"
    >
      {#if subHtml}
        <!-- El VTT viene escapado por lib/vtt.ts (solo quedan <i>, <b>, <u> y <br>). -->
        <span class="subs-line">{@html subHtml}</span>
      {:else}
        <span class="subs-line sample">This is how subtitles will look.</span>
      {/if}
    </div>
  {/if}

  {#if flash}
    {#key flash.k}
      <div class="flash" onanimationend={() => (flash = null)}>
        <span class="flash-icon">
          {#if flash.icon === "play"}<Play size={34} fill="currentColor" />
          {:else if flash.icon === "pause"}<Pause size={34} fill="currentColor" />
          {:else if flash.icon === "fwd"}<RotateCw size={30} />
          {:else if flash.icon === "back"}<RotateCcw size={30} />
          {:else if flash.icon === "subs"}<Captions size={30} />
          {:else if flash.icon === "speed"}<Gauge size={30} />
          {:else if flash.icon === "scale"}<Scan size={30} />
          {:else}<VolIcon size={30} />{/if}
        </span>
        {#if flash.text}<span class="flash-text">{flash.text}</span>{/if}
      </div>
    {/key}
  {/if}

  <!-- Mantener apretado: 2× mientras dura. -->
  {#if holding}
    <div class="hold-pill" transition:fadeQuick><FastForward size={15} fill="currentColor" />{rate}×</div>
  {/if}

  <!-- Doble toque a un costado: el salto acumulado (−30 s) con una onda. -->
  {#if combo}
    <div class="ripple" class:right={combo.zone > 0} transition:fadeQuick>
      {#key combo.k}<span class="ring"></span>{/key}
      <span class="rtext">
        {#if combo.zone > 0}<RotateCw size={22} />{:else}<RotateCcw size={22} />{/if}
        {combo.zone > 0 ? "+" : "−"}{combo.total} s
      </span>
    </div>
  {/if}

  <!-- Estadisticas (D): lo que el server sabe del torrent y lo que el navegador del video. -->
  {#if showStats && info}
    <div class="stats" transition:fadeQuick>
      <div class="st-head">
        <span>Playback stats</span>
        <button class="st-x" onclick={() => (showStats = false)} title="Close (D)" aria-label="Close stats"><X size={14} /></button>
      </div>
      <dl>
        <dt>Mode</dt><dd>{info.mode}{info.mode === "direct" && viaHls ? " · HLS (audio track)" : ""}</dd>
        <dt>Video</dt>
        <dd>
          {[info.video_codec?.toUpperCase(), info.width && info.height ? `${info.width}×${info.height}` : "", info.hdr ? "HDR" : ""].filter(Boolean).join(" · ") || "—"}
          {#if shownRes && info.width && shownRes !== `${info.width}×${info.height}`}<span class="dim"> → {shownRes}</span>{/if}
        </dd>
        <dt>Audio</dt><dd>{curAudio ? `${curAudio.codec.toUpperCase()} · ${curAudio.title || langName(curAudio.lang)}` : "—"}</dd>
        {#if info.container}<dt>Container</dt><dd>{info.container}</dd>{/if}
        <dt>Peers</dt><dd>{stats?.peers ?? "—"}</dd>
        <dt>Download</dt><dd>{stats ? `${speed(stats.down_bps)} · up ${speed(stats.up_bps)}` : "—"}</dd>
        <dt>File</dt><dd>{stats && stats.total_bytes ? `${Math.round(stats.progress * 100)}% of ${bytes(stats.total_bytes)}` : bytes(info.size_bytes)}</dd>
        <dt>Buffered</dt><dd>{aheadS.toFixed(0)} s ahead{stats?.buffered_ahead_s ? ` · server ${Math.round(stats.buffered_ahead_s)} s` : ""}</dd>
        {#if frames}<dt>Dropped</dt><dd>{frames.dropped} of {frames.total} frames</dd>{/if}
        <dt>Speed</dt><dd>{rate}× · {SCALE_LABEL[scale]}</dd>
      </dl>
    </div>
  {/if}

  {#if loadingFull || (waitShown && started && !stuck)}
    <PlayerLoading
      full={loadingFull}
      background={bgImg}
      logo={sized(meta?.logo, "medium")}
      title={meta?.name ?? ""}
      subtitle={epLabel}
      text={fatal || loadText}
      pct={fatal ? 0 : started || info ? bufPct() : null}
      {status}
      mode={info?.mode}
      children={askResume ? resumeCard : undefined}
    />
  {/if}

  <!-- Retomar o empezar de nuevo: debajo del logo, mientras el server ya prepara el archivo. -->
  {#snippet resumeCard()}
    {#if askResume}
      <div class="resume-card" transition:pop>
        <div class="resume-bar"><div style="width: {(askResume.position_s / askResume.duration_s) * 100}%"></div></div>
        <div class="dc-actions">
          <!-- svelte-ignore a11y_autofocus -->
          <button class="btn primary lg" autofocus onclick={() => resume(askResume!.position_s)}>
            <Play size={16} fill="currentColor" />Resume from {clock(askResume.position_s)}
          </button>
          <button class="btn glass lg" onclick={() => resume(0)}><RotateCcw size={16} />Start over</button>
        </div>
        <!-- Mientras tanto el server ya va bajando: se dice como viene, sin "Buffering". -->
        <div class="prep">
          {#if !info}Connecting to peers…{:else if status?.state === "ready"}Ready{:else}Preparing…{/if}
        </div>
      </div>
    {/if}
  {/snippet}

  <!-- Barra de arriba: volver, titulo y (desktop) los controles de la ventana. -->
  <div class="topbar" data-tauri-drag-region>
    <button class="pbtn" onclick={exit} title="Back (Esc)"><ArrowLeft size={22} /></button>
    <div class="ttl" data-tauri-drag-region>
      <span class="ellipsis name">{meta?.name ?? ""}</span>
      {#if epLabel}<span class="ellipsis ep">{epLabel}</span>{/if}
    </div>
    <div class="grow" data-tauri-drag-region></div>
    {#if stream}
      <span class="badge" title={stream.title}>{stream.quality}{info?.hdr ? " · HDR" : ""}{stream.cached ? " · Instant" : ""}</span>
    {/if}
    {#if isDesktop}<WindowControls onmedia />{/if}
  </div>

  {#if started}
    <div class="bottom" class:open={!!menu}>
      <SeekBar time={seekTarget ?? t} {duration} {buffered} onseek={seek} ondrag={(d) => (dragging = d)} />
      <div class="ctrls">
        <button class="pbtn big" onclick={toggle} title={paused ? "Play (K)" : "Pause (K)"}>
          {#if paused}<Play size={26} fill="currentColor" />{:else}<Pause size={26} fill="currentColor" />{/if}
        </button>
        <button class="pbtn narrow-hide" onclick={() => skip(-10)} title="Back 10 s (J)"><RotateCcw size={21} /></button>
        <button class="pbtn narrow-hide" onclick={() => skip(10)} title="Forward 10 s (L)"><RotateCw size={21} /></button>
        <div class="vol">
          <button class="pbtn" onclick={toggleMute} title="Mute (M)"><VolIcon size={22} /></button>
          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            value={prefs.muted ? 0 : prefs.volume}
            oninput={(e) => setVolume(Number(e.currentTarget.value))}
            aria-label="Volume"
            style="--v: {(prefs.muted ? 0 : prefs.volume) * 100}%"
          />
        </div>
        <span class="time">{clock(t)} <span class="dim">/ {clock(duration)}</span></span>
        <div class="grow"></div>
        {#if nextEp}
          <button class="pbtn" onclick={playNext} title="Next episode (N)"><SkipForward size={21} /></button>
        {/if}
        <div class="menu-anchor subs-anchor">
          <button
            class="pbtn"
            class:on={!!subSel}
            data-menu="subs"
            onclick={toggleSubsMenu}
            title="Subtitles (C)"
            aria-haspopup="true"
            aria-expanded={menu === "subs"}
            disabled={!subs.length}
          >
            <Captions size={22} />
          </button>
          {#if menu === "subs"}
            <!-- Como Stremio: idioma → opciones de ese idioma, con el retraso de la elegida y el
                 aspecto. Un paso a la vez (entra de costado), asi entra igual en un celular. -->
            <div class="pmenu subs-menu" bind:this={subPanel} transition:pop={{ start: 0.94 }} role="dialog" aria-label="Subtitles">
              {#key subView}
                <div class="sv" in:step={{ dir: subDir }}>
                  {#if !viewGroup}
                    <div class="mh">Subtitles</div>
                    <div class="sv-body">
                      <button class="mi" class:sel={!subSel} aria-current={!subSel} onclick={subsOff}>
                        <span class="ck">{#if !subSel}<Check size={14} />{/if}</span>Off
                      </button>
                      {#each subGroups as g (g.name)}
                        {@const on = g.name === activeGroup}
                        <button class="mi" class:sel={on} aria-current={on} data-group={g.name} onclick={() => openGroup(g)}>
                          <span class="ck">{#if on}<Check size={14} />{/if}</span>
                          <span class="ellipsis">{g.name}</span>
                          {#if g.list.some((x) => !x.ext)}<span class="tag">Embedded</span>{/if}
                          <span class="count" class:solo={!g.list.some((x) => !x.ext)}>{g.list.length}</span>
                          <span class="chev"><ChevronRight size={15} /></span>
                        </button>
                      {/each}
                    </div>
                  {:else}
                    <div class="sv-head">
                      <button class="back" onclick={backToLangs} title="Languages" aria-label="Back to languages"><ChevronLeft size={18} /></button>
                      <span class="ellipsis">{viewGroup.name}</span>
                      <span class="faint">{viewGroup.list.length} {viewGroup.list.length === 1 ? "option" : "options"}</span>
                    </div>
                    <div class="sv-body">
                      {#each viewGroup.list as s (s.id)}
                        {@const d = delayOf(s.id)}
                        <button class="mi" class:sel={subSel === s.id} aria-current={subSel === s.id} onclick={() => pickSub(s.id)}>
                          <span class="ck">{#if subSel === s.id}<Check size={14} />{/if}</span>
                          <span class="ellipsis">{subName(s)}</span>
                          {#if d}<span class="tag accent" title="Remembered delay">{fmtDelay(d)}</span>{/if}
                          <span class="tag">{s.ext ? "OpenSubtitles" : "Embedded"}</span>
                        </button>
                      {/each}
                    </div>
                    <div class="sv-foot">
                      {#if viewGroup.list.some((x) => x.id === subSel)}
                        <div class="row" role="group" aria-label="Delay">
                          <span class="rl">Delay</span>
                          <button class="sbtn" onclick={() => shiftSubs(-0.5)} title="0.5 s earlier (G)" aria-label="0.5 seconds earlier">−0.5</button>
                          <button class="sbtn" onclick={() => shiftSubs(-0.1)} title="0.1 s earlier (Shift G)" aria-label="0.1 seconds earlier">−0.1</button>
                          <span class="val" class:off={subDelay !== 0} aria-live="polite">{fmtDelay(subDelay)}</span>
                          <button class="sbtn" onclick={() => shiftSubs(0.1)} title="0.1 s later (Shift H)" aria-label="0.1 seconds later">+0.1</button>
                          <button class="sbtn" onclick={() => shiftSubs(0.5)} title="0.5 s later (H)" aria-label="0.5 seconds later">+0.5</button>
                          <button class="sbtn icon" onclick={() => setSubDelay(0)} disabled={subDelay === 0} title="Reset delay" aria-label="Reset delay"><RotateCcw size={14} /></button>
                        </div>
                      {/if}
                      <div class="row" role="group" aria-label="Size">
                        <span class="rl">Size</span>
                        <button class="sbtn icon" onclick={() => stepSize(-1)} disabled={prefs.subSize <= SIZE_STEPS[0]} title="Smaller (-)" aria-label="Smaller"><Minus size={14} /></button>
                        <span class="val" aria-live="polite">{prefs.subSize}%</span>
                        <button class="sbtn icon" onclick={() => stepSize(1)} disabled={prefs.subSize >= SIZE_STEPS[SIZE_STEPS.length - 1]} title="Larger (=)" aria-label="Larger"><Plus size={14} /></button>
                      </div>
                      <div class="row" role="group" aria-label="Position">
                        <span class="rl">Position</span>
                        <button class="sbtn icon" onclick={() => stepPos(-1)} disabled={prefs.subPos <= POS_MIN} title="Lower" aria-label="Lower"><ArrowDown size={14} /></button>
                        <span class="val" aria-live="polite">{prefs.subPos}%</span>
                        <button class="sbtn icon" onclick={() => stepPos(1)} disabled={prefs.subPos >= POS_MAX} title="Higher" aria-label="Higher"><ArrowUp size={14} /></button>
                      </div>
                      <div class="row" role="radiogroup" aria-label="Background">
                        <span class="rl">Background</span>
                        <div class="chips">
                          {#each BG_STEPS as b (b.value)}
                            <button class="chip" class:sel={prefs.subBg === b.value} role="radio" aria-checked={prefs.subBg === b.value} onclick={() => setBg(b.value)}>{b.label}</button>
                          {/each}
                        </div>
                      </div>
                    </div>
                  {/if}
                </div>
              {/key}
            </div>
          {/if}
        </div>
        {#if canAudio}
          <div class="menu-anchor">
            <button class="pbtn" onclick={() => (menu = menu === "audio" ? "" : "audio")} title="Audio"><AudioLines size={22} /></button>
            {#if menu === "audio"}
              <div class="pmenu" transition:pop={{ start: 0.94 }}>
                <div class="mh">Audio</div>
                {#each info?.audio_tracks ?? [] as a (a.index)}
                  <button class="mi" class:sel={audioSel === a.index} onclick={() => pickAudio(a.index)}>
                    <span class="ck">{#if audioSel === a.index}<Check size={14} />{/if}</span>
                    <span class="ellipsis">{a.title || langName(a.lang)}</span>
                    <span class="tag">{a.codec.toUpperCase()}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
        <div class="menu-anchor narrow-hide">
          <button class="pbtn" onclick={() => (menu = menu === "speed" ? "" : "speed")} title="Speed">
            {#if rate !== 1}<span class="rate">{rate}×</span>{:else}<Gauge size={21} />{/if}
          </button>
          {#if menu === "speed"}
            <div class="pmenu narrow" transition:pop={{ start: 0.94 }}>
              <div class="mh">Speed</div>
              {#each SPEEDS as r (r)}
                <button class="mi" class:sel={rate === r} onclick={() => ((rate = r), (menu = ""))}>
                  <span class="ck">{#if rate === r}<Check size={14} />{/if}</span>{r === 1 ? "Normal" : `${r}×`}
                </button>
              {/each}
            </div>
          {/if}
        </div>
        <button class="pbtn" onclick={() => (menu = menu === "streams" ? "" : "streams")} title="Streams"><ListVideo size={21} /></button>
        <button class="pbtn" onclick={toggleFs} title="Full screen (F)">
          {#if fs}<Minimize size={21} />{:else}<Maximize size={21} />{/if}
        </button>
      </div>
    </div>
  {/if}

  {#if menu === "streams"}
    <div class="side" transition:fadeQuick>
      <div class="side-head">
        <h3>Streams</h3>
        <span class="faint">{streams.length} found · switching keeps your position</span>
        <!-- La entrada a las estadisticas en pantallas tactiles (en el teclado es la D). -->
        <button class="side-link" onclick={() => ((showStats = !showStats), (menu = ""))} title="Playback stats (D)">
          <Activity size={13} />{showStats ? "Hide playback stats" : "Playback stats"}
        </button>
      </div>
      <div class="side-body">
        <StreamList {streams} current={stream?.key} onpick={switchTo} onmedia linkFor={(s) => streamLink(s, stream && info ? { key: stream.key, info } : null)} />
      </div>
    </div>
  {/if}

  {#if menu}
    <div class="menu-scrim" role="presentation" onclick={() => (menu = "")}></div>
  {/if}

  <!-- El navegador no dejo arrancar solo. -->
  {#if needsClick && !askResume}
    <button class="bigplay" onclick={() => tryPlay()} transition:pop aria-label="Play"><Play size={40} fill="currentColor" /></button>
  {/if}

  <!-- Trabado: otro stream ahi mismo, sin salir. -->
  {#if stuck || fatal}
    <div class="modal-wrap">
      <div class="dialog-card stuck" transition:pop>
        <div class="dc-icon"><TriangleAlert size={22} /></div>
        <div class="dc-title">
          {#if fatal}Can't play this{:else if stuck === "slow"}This stream is slow{:else if stuck === "codec"}This stream can't be played{:else}Playback problem{/if}
        </div>
        <div class="dc-sub">
          {#if fatal}{fatal}{:else if stuck === "slow"}
            {status ? `${status.peers} peers · ${(status.down_bps / 1e6).toFixed(1)} MB/s.` : "No data is arriving."} Another stream may be faster.
          {:else}{stuckMsg}{/if}
        </div>
        {#if alternatives.length && !fatal}
          <div class="alts">
            <div class="mh">Try another stream</div>
            <StreamList streams={alternatives} onpick={switchTo} onmedia />
          </div>
        {/if}
        <div class="dc-actions">
          {#if stuck === "slow"}<button class="btn glass" onclick={keepWaiting}><LoaderCircle size={15} />Keep waiting</button>{/if}
          {#if fatal}<button class="btn glass" onclick={retryAll}><RotateCw size={15} />Retry</button>{/if}
          <button class="btn glass" onclick={exit}><ArrowLeft size={15} />Back</button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Creditos: el siguiente episodio a un clic. -->
  {#if showNextPill && nextEp}
    <button class="next-pill" onclick={playNext} transition:rise={{ y: 12 }}>
      <FastForward size={16} />Next episode<ChevronRight size={16} />
    </button>
  {/if}

  <!-- Termino: cuenta regresiva para el siguiente (cancelable). -->
  {#if ended && nextEp}
    <div class="next-card" transition:rise={{ y: 16 }}>
      <div class="nc-thumb">
        {#if nextEp.thumbnail}<img src={nextEp.thumbnail} alt="" class:spoil={prefs.hideSpoilers} />{/if}
        {#if countdown !== null}
          <svg class="nc-ring" viewBox="0 0 40 40"><circle cx="20" cy="20" r="17" style="stroke-dashoffset: {106.8 * (1 - countdown / 10)}" /></svg>
          <span class="nc-n">{countdown}</span>
        {/if}
      </div>
      <div class="nc-text">
        <span class="faint">Next episode{countdown !== null ? ` in ${countdown}` : ""}</span>
        <span class="ellipsis nc-name">{epCode(nextEp.season, nextEp.episode)} · {nextEp.name ?? `Episode ${nextEp.episode}`}</span>
        <div class="nc-actions">
          <button class="btn primary" onclick={playNext}><Play size={14} fill="currentColor" />Play now</button>
          {#if countdown !== null}<button class="btn glass" onclick={cancelCountdown}>Cancel</button>{/if}
        </div>
      </div>
    </div>
  {:else if ended && !nextEp}
    <div class="modal-wrap">
      <div class="dialog-card" transition:pop>
        <div class="dc-title">{meta?.name ?? "Finished"}</div>
        <div class="dc-sub">{epLabel || "Thanks for watching."}</div>
        <div class="dc-actions">
          <button class="btn primary lg" onclick={toggle}><RotateCcw size={16} />Watch again</button>
          <button class="btn glass lg" onclick={exit}><ArrowLeft size={16} />Back</button>
        </div>
      </div>
    </div>
  {/if}

  <!-- "?": los atajos (la lista de lib/shortcuts.ts, la misma de Settings). -->
  {#if showKeys}
    <div class="modal-wrap" role="presentation" onclick={(e) => e.target === e.currentTarget && (showKeys = false)}>
      <div class="dialog-card keys-card" transition:pop role="dialog" aria-label="Keyboard shortcuts">
        <div class="dc-title">Shortcuts</div>
        {#each coarse ? [PLAYER_TOUCH, PLAYER_KEYS] : [PLAYER_KEYS, PLAYER_TOUCH] as list, li (li)}
          <div class="mh">{list === PLAYER_TOUCH ? "Touch" : "Keyboard"}</div>
          <div class="keys">
            {#each list as [k, what] (k)}
              <div class="key-row">
                <span class="kbds">{#each keyParts(k) as one, i (one)}{#if i}<span class="or">or</span>{/if}<kbd>{one}</kbd>{/each}</span>
                <span class="what">{what}</span>
              </div>
            {/each}
          </div>
        {/each}
        <div class="dc-actions"><button class="btn glass" onclick={() => (showKeys = false)}>Close</button></div>
      </div>
    </div>
  {/if}

  {#if info && started && stream && !ui}
    <span class="sr-only" aria-live="polite">{stream.quality} {bytes(stream.size_bytes)}</span>
  {/if}
</div>

<style>
  .player {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: var(--media-bg);
    color: var(--on-media);
    overflow: hidden;
    user-select: none;
  }
  .player.hide-ui {
    cursor: none;
  }
  video {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: var(--media-bg);
  }

  /* ---- subtitulos ---- */
  .subs {
    position: absolute;
    left: 6%;
    right: 6%;
    bottom: var(--sub-pos, 8%);
    z-index: 3;
    display: flex;
    justify-content: center;
    text-align: center;
    pointer-events: none;
    transition: bottom 0.3s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .subs.lift {
    bottom: max(var(--sub-pos, 8%), 120px);
  }
  .subs-line {
    font-family: var(--font-sub);
    font-size: var(--sub-size);
    font-weight: 600;
    line-height: 1.3;
    color: var(--on-media);
    padding: 0.1em 0.4em;
    border-radius: 6px;
    background: color-mix(in srgb, var(--media-bg) var(--sub-bg, 35%), transparent);
    text-shadow:
      0 0 3px var(--media-bg),
      0 2px 6px var(--media-bg);
    -webkit-box-decoration-break: clone;
    box-decoration-break: clone;
  }
  /* Sin fondo: un contorno mas marcado para que se lea sobre cualquier imagen. */
  .nobg .subs-line {
    text-shadow:
      0 0 2px var(--media-bg),
      0 0 2px var(--media-bg),
      0 0 4px var(--media-bg),
      0 2px 6px var(--media-bg);
  }

  /* ---- barras ---- */
  .topbar,
  .bottom {
    position: absolute;
    left: 0;
    right: 0;
    z-index: 6;
    transition:
      opacity 0.3s,
      transform 0.35s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .topbar {
    top: 0;
    height: 64px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 0 0 14px;
    background: linear-gradient(180deg, var(--media-scrim), transparent);
  }
  .bottom {
    bottom: 0;
    padding: 40px clamp(14px, 2.4vw, 32px) 14px;
    background: linear-gradient(0deg, color-mix(in srgb, var(--media-bg) 80%, transparent), transparent);
  }
  .hide-ui .topbar {
    opacity: 0;
    transform: translateY(-12px);
    pointer-events: none;
  }
  .hide-ui .bottom {
    opacity: 0;
    transform: translateY(16px);
    pointer-events: none;
  }
  .ttl {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }
  .ttl .name {
    font-weight: 650;
    font-size: 15px;
  }
  .ttl .ep {
    font-size: 12.5px;
    color: var(--on-media-2);
  }
  .badge {
    flex: none;
    padding: 3px 8px;
    border-radius: 6px;
    font-size: 11.5px;
    font-weight: 650;
    border: 1px solid var(--media-line);
    color: var(--on-media-2);
    margin-right: 8px;
  }
  .ctrls {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-top: 4px;
  }
  .pbtn {
    position: relative;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    flex: none;
    border-radius: 50%;
    color: var(--on-media);
    transition:
      background 0.15s,
      transform 0.15s cubic-bezier(0.34, 1.56, 0.64, 1),
      color 0.15s;
  }
  .pbtn:hover {
    background: var(--media-line);
    transform: scale(1.08);
  }
  .pbtn:active {
    transform: scale(0.92);
  }
  .pbtn:disabled {
    opacity: 0.35;
    pointer-events: none;
  }
  .pbtn.big {
    width: 46px;
    height: 46px;
  }
  .pbtn.on {
    color: var(--accent);
  }
  .pbtn.on::after {
    content: "";
    position: absolute;
    bottom: 6px;
    left: 30%;
    right: 30%;
    height: 2px;
    border-radius: 2px;
    background: var(--accent);
  }
  .rate {
    font: 700 13px var(--mono);
  }
  .time {
    margin-left: 8px;
    font: 600 13px var(--mono);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .dim {
    color: var(--on-media-2);
  }
  /* La barra de volumen aparece al lado del parlante, flotando sobre el tiempo: solo opacidad y
     transform (no empuja nada). */
  .vol {
    position: relative;
    display: flex;
    align-items: center;
  }
  .vol input {
    position: absolute;
    left: 40px;
    top: 50%;
    z-index: 2;
    width: 112px;
    height: 32px;
    padding: 0 12px;
    margin: 0;
    border: 1px solid var(--media-line);
    border-radius: 999px;
    background: color-mix(in srgb, var(--media-bg) 80%, transparent);
    backdrop-filter: blur(10px);
    appearance: none;
    opacity: 0;
    pointer-events: none;
    transform: translateY(-50%) scaleX(0.6);
    transform-origin: left center;
    transition:
      opacity 0.18s,
      transform 0.22s cubic-bezier(0.2, 0.8, 0.2, 1);
    cursor: pointer;
  }
  .vol:hover input,
  .vol input:focus-visible {
    opacity: 1;
    pointer-events: auto;
    transform: translateY(-50%);
  }
  .vol input::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(90deg, var(--on-media) var(--v), var(--media-line) var(--v));
  }
  .vol input::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    margin-top: -4px;
    border-radius: 50%;
    background: var(--on-media);
  }
  .vol input:focus-visible {
    box-shadow: none;
  }

  /* ---- menus ---- */
  .menu-anchor {
    position: relative;
  }
  .pmenu {
    position: absolute;
    right: -8px;
    bottom: 52px;
    z-index: 9;
    min-width: 240px;
    max-width: 340px;
    max-height: 52vh;
    overflow: auto;
    padding: 6px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--media-bg) 82%, transparent);
    border: 1px solid var(--media-line);
    backdrop-filter: blur(20px);
    box-shadow: 0 20px 50px -10px var(--media-bg);
    transform-origin: bottom right;
  }
  .pmenu.narrow {
    min-width: 150px;
  }
  .mh {
    padding: 6px 10px 6px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--on-media-2);
  }
  .mi {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 10px 8px 6px;
    border-radius: 8px;
    text-align: left;
    font-size: 13.5px;
    color: var(--on-media-2);
    transition:
      background 0.1s,
      color 0.1s;
  }
  .mi:hover {
    background: var(--media-line);
    color: var(--on-media);
  }
  .mi.sel {
    color: var(--on-media);
  }
  .ck {
    width: 18px;
    flex: none;
    display: grid;
    place-items: center;
    color: var(--accent);
  }
  .tag {
    margin-left: auto;
    flex: none;
    font-size: 10px;
    font-weight: 700;
    padding: 1px 5px;
    border-radius: 4px;
    border: 1px solid var(--media-line);
    color: var(--on-media-2);
  }
  /* Menu de subtitulos por pasos: alto fijo-ish con la lista que scrollea y los controles abajo. */
  .subs-menu {
    display: flex;
    flex-direction: column;
    width: 360px;
    min-width: 0;
    max-width: calc(100vw - 24px);
    max-height: min(540px, calc(100vh - 150px));
    overflow: hidden;
    padding: 0;
  }
  .sv {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
  }
  .sv > .mh {
    padding: 12px 16px 4px;
  }
  .sv-body {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    overscroll-behavior: contain;
    padding: 4px 6px 6px;
  }
  .sv-head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 14px 6px 4px;
    border-bottom: 1px solid var(--media-line);
    font-weight: 650;
    font-size: 14px;
  }
  .sv-head .faint {
    margin-left: auto;
    flex: none;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--on-media-2);
  }
  .back {
    display: grid;
    place-items: center;
    flex: none;
    width: 36px;
    height: 36px;
    border-radius: 8px;
    color: var(--on-media);
    transition: background 0.1s;
  }
  .back:hover {
    background: var(--media-line);
  }
  .count {
    flex: none;
    min-width: 20px;
    text-align: right;
    font: 600 11.5px var(--mono);
    color: var(--on-media-2);
  }
  .count.solo {
    margin-left: auto;
  }
  .tag.accent + .tag {
    margin-left: 0;
  }
  .tag.accent {
    margin-left: auto;
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    font-family: var(--mono);
  }
  .chev {
    display: grid;
    flex: none;
    color: var(--on-media-2);
  }
  .sv-foot {
    flex: none;
    padding: 6px 10px 10px;
    border-top: 1px solid var(--media-line);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    min-height: 40px;
    font-size: 12.5px;
    color: var(--on-media-2);
  }
  .rl {
    flex: 1;
    min-width: 0;
    padding-left: 4px;
  }
  .sbtn {
    flex: none;
    min-width: 40px;
    height: 32px;
    padding: 0 6px;
    border-radius: 7px;
    font: 600 11.5px var(--mono);
    color: var(--on-media);
    border: 1px solid var(--media-line);
    transition:
      background 0.1s,
      transform 0.12s;
  }
  .sbtn.icon {
    display: grid;
    place-items: center;
    width: 36px;
    min-width: 36px;
    padding: 0;
  }
  .sbtn:hover {
    background: var(--media-line);
  }
  .sbtn:active {
    transform: scale(0.94);
  }
  .sbtn:disabled {
    opacity: 0.35;
    pointer-events: none;
  }
  .val {
    flex: none;
    min-width: 58px;
    text-align: center;
    font: 600 12px var(--mono);
    font-variant-numeric: tabular-nums;
    color: var(--on-media);
  }
  .val.off {
    color: var(--accent);
  }
  .chips {
    display: flex;
    gap: 3px;
    padding: 2px;
    border-radius: 8px;
    border: 1px solid var(--media-line);
  }
  .chip {
    height: 28px;
    padding: 0 8px;
    border-radius: 6px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--on-media-2);
    transition:
      background 0.1s,
      color 0.1s;
  }
  .chip:hover {
    color: var(--on-media);
  }
  .chip.sel {
    background: var(--media-line);
    color: var(--on-media);
  }
  /* Ventanas bajas (celular acostado): todo el paso scrollea junto. */
  @media (max-height: 520px) {
    .subs-menu {
      overflow: auto;
    }
    .sv {
      flex: none;
    }
    .sv-body {
      overflow: visible;
    }
  }
  .menu-scrim {
    position: absolute;
    inset: 0;
    z-index: 5;
  }
  .side {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    z-index: 8;
    width: min(480px, 100vw);
    display: flex;
    flex-direction: column;
    background: color-mix(in srgb, var(--media-bg) 78%, transparent);
    backdrop-filter: blur(24px);
    border-left: 1px solid var(--media-line);
    animation: side-in 0.3s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  @keyframes side-in {
    from {
      transform: translateX(40px);
    }
  }
  .side-head {
    padding: 70px 18px 12px;
  }
  .side-head h3 {
    margin: 0 0 4px;
    font-size: 17px;
  }
  .side-head .faint {
    color: var(--on-media-2);
    font-size: 12px;
  }
  .side-body {
    flex: 1;
    overflow: auto;
    padding: 4px 14px 20px;
  }

  /* ---- avisos al centro ---- */
  .flash {
    position: absolute;
    left: 50%;
    top: 50%;
    z-index: 4;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    transform: translate(-50%, -50%);
    pointer-events: none;
    animation: flash 0.6s cubic-bezier(0.2, 0.8, 0.2, 1) forwards;
  }
  .flash-icon {
    display: grid;
    place-items: center;
    width: 76px;
    height: 76px;
    border-radius: 50%;
    background: var(--media-scrim);
    backdrop-filter: blur(6px);
  }
  .flash-text {
    font-weight: 700;
    font-size: 14px;
    text-shadow: 0 2px 10px var(--media-bg);
  }
  @keyframes flash {
    0% {
      opacity: 0;
      transform: translate(-50%, -50%) scale(0.7);
    }
    25% {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
    }
    100% {
      opacity: 0;
      transform: translate(-50%, -50%) scale(1.15);
    }
  }
  .modal-wrap {
    position: absolute;
    inset: 0;
    z-index: 10;
    display: grid;
    place-items: center;
    padding: 16px;
    background: color-mix(in srgb, var(--media-bg) 45%, transparent);
  }
  .dialog-card {
    width: min(520px, 100%);
    max-height: calc(100vh - 32px);
    overflow: auto;
    padding: 24px;
    border-radius: 16px;
    background: color-mix(in srgb, var(--media-bg) 72%, transparent);
    border: 1px solid var(--media-line);
    backdrop-filter: blur(24px);
    box-shadow: 0 30px 80px -20px var(--media-bg);
    text-align: center;
  }
  .dc-icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    margin: 0 auto 10px;
    border-radius: 50%;
    color: var(--warn);
    background: color-mix(in srgb, var(--warn) 16%, transparent);
  }
  .dc-title {
    font-family: var(--display);
    font-size: 20px;
    font-weight: 700;
  }
  .dc-sub {
    margin-top: 6px;
    color: var(--on-media-2);
    font-size: 13.5px;
    line-height: 1.5;
  }
  .dc-actions {
    display: flex;
    justify-content: center;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 20px;
  }
  .resume-card {
    margin-top: 18px;
    pointer-events: auto;
  }
  .resume-card .dc-actions {
    margin-top: 16px;
  }
  .prep {
    margin-top: 14px;
    font-size: 13px;
    color: var(--on-media-2);
  }
  .resume-bar {
    height: 4px;
    margin: 0 auto;
    max-width: 320px;
    border-radius: 4px;
    background: var(--media-line);
    overflow: hidden;
  }
  .resume-bar div {
    height: 100%;
    background: var(--primary-bg);
  }
  .alts {
    margin-top: 18px;
    text-align: left;
  }
  .alts .mh {
    padding-left: 2px;
  }
  .bigplay {
    position: absolute;
    left: 50%;
    top: 50%;
    z-index: 7;
    display: grid;
    place-items: center;
    width: 96px;
    height: 96px;
    margin: -48px 0 0 -48px;
    border-radius: 50%;
    color: var(--accent-ink);
    background: var(--primary-bg);
    box-shadow: 0 10px 50px -8px color-mix(in srgb, var(--accent) 70%, transparent);
    transition: transform 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .bigplay:hover {
    transform: scale(1.08);
  }
  .next-pill {
    position: absolute;
    right: clamp(14px, 2.4vw, 32px);
    bottom: 120px;
    z-index: 7;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    padding: 0 14px 0 16px;
    border-radius: 999px;
    font-weight: 650;
    color: var(--media-bg);
    background: var(--on-media);
    box-shadow: 0 10px 40px -10px var(--media-bg);
    transition: transform 0.18s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .next-pill:hover {
    transform: scale(1.05);
  }
  .next-card {
    position: absolute;
    right: clamp(14px, 2.4vw, 32px);
    bottom: 120px;
    z-index: 9;
    display: flex;
    gap: 14px;
    align-items: center;
    width: min(460px, calc(100vw - 28px));
    padding: 12px;
    border-radius: 14px;
    background: color-mix(in srgb, var(--media-bg) 72%, transparent);
    border: 1px solid var(--media-line);
    backdrop-filter: blur(22px);
    box-shadow: 0 20px 60px -20px var(--media-bg);
  }
  .nc-thumb {
    position: relative;
    flex: none;
    width: 150px;
    aspect-ratio: 16 / 9;
    border-radius: 8px;
    overflow: hidden;
    background: var(--media-line);
    display: grid;
    place-items: center;
  }
  .nc-thumb img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0.6;
  }
  .nc-ring {
    position: absolute;
    width: 48px;
    height: 48px;
    transform: rotate(-90deg);
  }
  .nc-ring circle {
    fill: none;
    stroke: var(--accent);
    stroke-width: 3;
    stroke-linecap: round;
    stroke-dasharray: 106.8;
    transition: stroke-dashoffset 1s linear;
  }
  .nc-n {
    position: relative;
    font: 700 16px var(--mono);
  }
  .nc-text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .nc-text .faint {
    color: var(--on-media-2);
    font-size: 12px;
  }
  .nc-name {
    font-weight: 650;
  }
  .nc-actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  /* Con el dedo: los gestos son de la app (sin zoom ni scroll del navegador). */
  video {
    touch-action: none;
    -webkit-tap-highlight-color: transparent;
  }
  .hold-pill {
    position: absolute;
    top: 76px;
    left: 50%;
    z-index: 7;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 14px;
    border-radius: 999px;
    font: 700 13px var(--mono);
    background: var(--media-scrim);
    border: 1px solid var(--media-line);
    backdrop-filter: blur(10px);
    transform: translateX(-50%);
    pointer-events: none;
  }
  .ripple {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    z-index: 4;
    width: 34%;
    display: grid;
    place-items: center;
    overflow: hidden;
    border-radius: 0 50% 50% 0 / 0 50% 50% 0;
    background: color-mix(in srgb, var(--on-media) 8%, transparent);
    pointer-events: none;
  }
  .ripple.right {
    left: auto;
    right: 0;
    border-radius: 50% 0 0 50% / 50% 0 0 50%;
  }
  /* La onda: sin "reducir animaciones" crece y se va; con, no se ve (opacidad 0 en reposo). */
  .ring {
    position: absolute;
    width: 140px;
    height: 140px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--on-media) 22%, transparent);
    opacity: 0;
    animation: ring 0.5s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  @keyframes ring {
    from {
      opacity: 1;
      transform: scale(0.4);
    }
    to {
      opacity: 0;
      transform: scale(1.6);
    }
  }
  .rtext {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    font: 700 14px var(--mono);
    text-shadow: 0 2px 10px var(--media-bg);
  }
  .stats {
    position: absolute;
    top: 70px;
    left: 14px;
    z-index: 7;
    width: min(340px, calc(100vw - 28px));
    /* Celular acostado: no tapa la barra de abajo (scrollea). */
    max-height: calc(100% - 170px);
    overflow: auto;
    padding: 8px 12px 10px;
    border-radius: 10px;
    background: color-mix(in srgb, var(--media-bg) 72%, transparent);
    border: 1px solid var(--media-line);
    backdrop-filter: blur(14px);
    font-size: 12px;
  }
  .st-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--on-media-2);
  }
  .st-x {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    color: var(--on-media-2);
  }
  .st-x:hover {
    background: var(--media-line);
    color: var(--on-media);
  }
  .stats dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 3px 12px;
    margin: 4px 0 0;
  }
  .stats dt {
    color: var(--on-media-2);
  }
  .stats dd {
    margin: 0;
    min-width: 0;
    font: 600 12px var(--mono);
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }
  .side-link {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-top: 8px;
    font-size: 12px;
    color: var(--on-media-2);
  }
  .side-link:hover {
    color: var(--on-media);
  }
  .nc-thumb img.spoil {
    filter: blur(10px);
    transform: scale(1.1);
  }
  .keys-card {
    width: min(620px, 100%);
    text-align: left;
  }
  .keys-card .dc-title {
    margin-bottom: 6px;
  }
  .keys-card .mh {
    padding: 12px 0 4px;
  }
  .keys {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 270px), 1fr));
    gap: 0 18px;
  }
  .key-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 0;
    border-bottom: 1px solid var(--media-line);
    font-size: 12.5px;
  }
  .kbds {
    flex: none;
    min-width: 96px;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .keys-card kbd {
    background: var(--media-line);
    border-color: var(--media-line);
    color: var(--on-media);
    box-shadow: none;
  }
  .or {
    font-size: 10.5px;
    color: var(--on-media-2);
  }
  .what {
    color: var(--on-media-2);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
  @media (max-width: 700px) {
    .vol input,
    .badge,
    .narrow-hide {
      display: none;
    }
    .time {
      font-size: 11.5px;
    }
    .pbtn {
      width: 36px;
      height: 36px;
    }
    .nc-thumb {
      width: 100px;
    }
    /* El menu de subtitulos pasa a ser una hoja abajo, de lado a lado (sobre .bottom). */
    .subs-anchor {
      position: static;
    }
    .subs-menu {
      left: 8px;
      right: 8px;
      bottom: calc(100% - 34px);
      width: auto;
      max-width: none;
      transform-origin: bottom center;
    }
  }
  /* Dedos: filas y botones de al menos 44 px. */
  @media (max-width: 700px), (pointer: coarse) {
    .subs-menu .mi {
      min-height: 44px;
    }
    .subs-menu .sbtn,
    .subs-menu .back {
      height: 40px;
    }
    .subs-menu .sbtn.icon {
      width: 40px;
    }
    .subs-menu .chip {
      height: 36px;
    }
    .subs-menu .row {
      min-height: 46px;
    }
  }
</style>
