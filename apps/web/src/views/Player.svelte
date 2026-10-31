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
    ArrowLeft,
    AudioLines,
    Captions,
    Check,
    ChevronRight,
    FastForward,
    Gauge,
    ListVideo,
    LoaderCircle,
    Maximize,
    Minimize,
    Pause,
    Play,
    RotateCcw,
    RotateCw,
    SkipForward,
    TriangleAlert,
    Volume1,
    Volume2,
    VolumeX,
  } from "@lucide/svelte";
  import type HlsType from "hls.js";
  import { onDestroy, onMount } from "svelte";
  import PlayerLoading from "../components/PlayerLoading.svelte";
  import SeekBar from "../components/SeekBar.svelte";
  import StreamList from "../components/StreamList.svelte";
  import WindowControls from "../components/WindowControls.svelte";
  import { api, errText, mediaUrl, withToken } from "../lib/api";
  import { degrade, detectCaps } from "../lib/caps";
  import { bytes, clock, epCode, sized } from "../lib/format";
  import { fadeQuick, pop, rise } from "../lib/motion";
  import { isDesktop, isFullscreen, setFullscreen } from "../lib/platform";
  import { langName, normLang, prefs, savePrefs } from "../lib/prefs.svelte";
  import { back, go, setQuery } from "../lib/router.svelte";
  import { toast } from "../lib/toast.svelte";
  import type { ClientCaps, Episode, MediaStatus, MediaType, Meta, PlayInfo, Progress, Stream } from "../lib/types";
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

  let ui = $state(true); // controles visibles
  let menu = $state<"" | "subs" | "audio" | "speed" | "streams">("");
  let dragging = $state(false);
  let flash = $state<{ k: number; icon: "play" | "pause" | "fwd" | "back" | "vol"; text?: string } | null>(null);

  let askResume = $state<Progress | null>(null);
  let resolveResume: ((at: number) => void) | null = null;
  let countdown = $state<number | null>(null);

  type Sub = { id: string; label: string; lang: string; url: string; ext: boolean };
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
  const intSubs = $derived<Sub[]>(
    (info?.subtitle_tracks ?? []).map((s) => ({
      id: `i${s.index}`,
      label: s.title || langName(s.lang),
      lang: normLang(s.lang),
      url: `/api/media/${info!.session_id}/subs/${s.index}.vtt`,
      ext: false,
    })),
  );
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
    if (fs) setFullscreen(root!, false);
  });

  async function init() {
    api.meta(type, seriesId).then((m) => (meta = m)).catch(() => {});
    api
      .subtitles(type, id)
      .then((l) => {
        // Hasta 3 por idioma: la lista de OpenSubtitles puede ser enorme.
        const per: Record<string, number> = {};
        extSubs = l
          .filter((s) => ((per[normLang(s.lang)] = (per[normLang(s.lang)] ?? 0) + 1) <= 3))
          .map((s, i) => ({ id: `e${i}`, label: `${langName(s.lang)} · OpenSubtitles`, lang: normLang(s.lang), url: s.url, ext: true }));
        autoSub();
      })
      .catch(() => {});

    const progP = api.progress(id).catch(() => null);
    let list: Stream[];
    try {
      list = await api.streams(type, id);
    } catch (e) {
      fatal = `Couldn't get streams: ${errText(e)}`;
      return;
    }
    if (destroyed) return;
    streams = list;
    if (!list.length) {
      fatal = "No streams found for this title.";
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
    info = next;
    audioSel = next.audio_tracks.find((a) => a.default)?.index ?? next.audio_tracks[0]?.index ?? null;
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
    const less = degrade(caps, info, what);
    if (!less || attempts.fallback >= 3) {
      stuck = "codec";
      stuckMsg = "This browser can't play this file.";
      return false;
    }
    attempts.fallback++;
    toast("Switching to compatible playback…");
    open(stream, t, less);
    return true;
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
    readBuffered();
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
      const h = cuesAt(cues, t);
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
  function bump(icon: "play" | "pause" | "fwd" | "back" | "vol", text?: string) {
    flash = { k: ++flashK, icon, text };
  }

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
    const s = intSubs.find((x) => x.lang === want && !/forced/i.test(x.label)) ?? extSubs.find((x) => x.lang === want);
    if (s) {
      subAuto = true;
      pickSub(s.id, false);
    }
  }

  async function pickSub(sid: string, close = true) {
    if (close) menu = "";
    subSel = sid;
    cues = [];
    subHtml = "";
    if (!sid) return;
    const s = subs.find((x) => x.id === sid);
    if (!s) return;
    try {
      const r = await fetch(mediaUrl(s.url));
      if (!r.ok) throw new Error(String(r.status));
      if (subSel !== sid) return;
      cues = parseVtt(await r.text());
      subHtml = cuesAt(cues, t);
      // El server todavia esta extrayendo la pista: se vuelve a pedir y se reemplaza.
      if (r.headers.get("X-Mythos-Partial") === "true" && !destroyed) {
        setTimeout(() => subSel === sid && !destroyed && refreshSub(sid), 5000);
      }
    } catch {
      if (subSel === sid) {
        subSel = "";
        // Elegidos a mano: se avisa. Los automaticos fallan en silencio.
        if (close) toast("Couldn't load those subtitles", true);
      }
    }
  }

  async function refreshSub(sid: string) {
    const s = subs.find((x) => x.id === sid);
    if (!s) return;
    try {
      const r = await fetch(mediaUrl(s.url));
      if (!r.ok || subSel !== sid) return;
      cues = parseVtt(await r.text());
      if (r.headers.get("X-Mythos-Partial") === "true") setTimeout(() => subSel === sid && !destroyed && refreshSub(sid), 5000);
    } catch {}
  }

  let lastSub = "";
  function toggleSubs() {
    if (subSel) {
      lastSub = subSel;
      pickSub("", false);
      bump("vol", "Subtitles off");
    } else {
      const s = subs.find((x) => x.id === lastSub) ?? subs.find((x) => x.lang === prefs.subLang) ?? subs[0];
      if (s) {
        pickSub(s.id, false);
        bump("vol", s.label);
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
      .then((l) => l[0] && api.prefetch(l[0]))
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

  // ---- teclado

  function onKey(e: KeyboardEvent) {
    if ((e.target as HTMLElement).closest("input, textarea")) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const k = e.key.toLowerCase();
    let handled = true;
    if (askResume) {
      if (k === "enter") resume(askResume.position_s);
      else if (k === "escape") exit();
      else handled = false;
    } else if (k === " " || k === "k") toggle();
    else if (k === "f") toggleFs();
    else if (k === "m") toggleMute();
    else if (k === "arrowright") skip(5);
    else if (k === "arrowleft") skip(-5);
    else if (k === "l") skip(10);
    else if (k === "j") skip(-10);
    else if (k === "arrowup") (setVolume(prefs.volume + 0.05), (prefs.muted = false), bump("vol", `${Math.round(prefs.volume * 100)}%`));
    else if (k === "arrowdown") (setVolume(prefs.volume - 0.05), bump("vol", `${Math.round(prefs.volume * 100)}%`));
    else if (k === "c") toggleSubs();
    else if (k === "n") nextEp && playNext();
    else if (k === "escape") {
      if (menu) menu = "";
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
  const bgImg = $derived(sized(meta?.background, "medium"));
  const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2];
</script>

<svelte:window onkeydown={onKey} />

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
    onclick={() => started && !menu && toggle()}
    ondblclick={toggleFs}
  ></video>

  {#if subHtml && started}
    <div class="subs" class:lift={ui} style="--sub-size: {subSize}">
      <!-- El VTT viene escapado por lib/vtt.ts (solo quedan <i>, <b>, <u> y <br>). -->
      <span class="subs-line">{@html subHtml}</span>
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
          {:else}<VolIcon size={30} />{/if}
        </span>
        {#if flash.text}<span class="flash-text">{flash.text}</span>{/if}
      </div>
    {/key}
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
      <SeekBar time={t} {duration} {buffered} onseek={seek} ondrag={(d) => (dragging = d)} />
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
        <div class="menu-anchor">
          <button class="pbtn" class:on={!!subSel} onclick={() => (menu = menu === "subs" ? "" : "subs")} title="Subtitles (C)" disabled={!subs.length}>
            <Captions size={22} />
          </button>
          {#if menu === "subs"}
            <div class="pmenu" transition:pop={{ start: 0.94 }}>
              <div class="mh">Subtitles</div>
              <button class="mi" class:sel={!subSel} onclick={() => pickSub("")}><span class="ck">{#if !subSel}<Check size={14} />{/if}</span>Off</button>
              {#each subs as s (s.id)}
                <button class="mi" class:sel={subSel === s.id} onclick={() => pickSub(s.id)}>
                  <span class="ck">{#if subSel === s.id}<Check size={14} />{/if}</span>
                  <span class="ellipsis">{s.label}</span>
                  {#if !s.ext}<span class="tag">In file</span>{/if}
                </button>
              {/each}
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
      </div>
      <div class="side-body">
        <StreamList {streams} current={stream?.key} onpick={switchTo} onmedia />
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
          {#if fatal}Can't play this{:else if stuck === "slow"}This stream is slow{:else if stuck === "codec"}This file won't play here{:else}Playback problem{/if}
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
        {#if nextEp.thumbnail}<img src={nextEp.thumbnail} alt="" />{/if}
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
    bottom: 7%;
    z-index: 3;
    display: flex;
    justify-content: center;
    text-align: center;
    pointer-events: none;
    transition: bottom 0.3s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .subs.lift {
    bottom: max(7%, 120px);
  }
  .subs-line {
    font-family: var(--font-sub);
    font-size: var(--sub-size);
    font-weight: 600;
    line-height: 1.3;
    color: var(--on-media);
    padding: 0.1em 0.4em;
    border-radius: 6px;
    background: color-mix(in srgb, var(--media-bg) 35%, transparent);
    text-shadow:
      0 0 3px var(--media-bg),
      0 2px 6px var(--media-bg);
    -webkit-box-decoration-break: clone;
    box-decoration-break: clone;
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
  .vol {
    display: flex;
    align-items: center;
  }
  .vol input {
    width: 0;
    height: 18px;
    padding: 0;
    border: 0;
    background: transparent;
    appearance: none;
    opacity: 0;
    transition:
      width 0.25s cubic-bezier(0.2, 0.8, 0.2, 1),
      opacity 0.2s;
    cursor: pointer;
  }
  .vol:hover input,
  .vol input:focus-visible {
    width: 86px;
    opacity: 1;
    margin-right: 6px;
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
    border: 0;
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
  }
</style>
