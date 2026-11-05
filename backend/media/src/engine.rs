//! Motor: sesión de librqbit, sesiones de reproducción, precarga, caché LRU y tareas de fondo.
//!
//! Decisiones para conseguir peers rápido y no trabarse:
//! - **DHT persistente** (`data_dir/dht.json`): tras un reinicio el bootstrap es inmediato.
//! - **Sesión persistente con fastresume** (`data_dir/session`): los torrents y lo bajado vuelven
//!   sin re-verificar todo el archivo.
//! - **Metadata cacheada** (`data_dir/meta/{hash}.torrent`): reabrir algo ya visto no espera a
//!   resolver el magnet, aunque el torrent se haya borrado por LRU.
//! - **Trackers extra**: `trackers_best.txt` de ngosang más la lista "stable" de newTrackon (≥ 95 %
//!   de uptime), cada una con caché en disco 12 h y timeout 4 s, más una lista fija mínima, más los
//!   trackers que vengan del stream; como mucho `MAX_TRACKERS`. El keeper la vuelve a bajar cada
//!   12 h y los trackers nuevos se suman a los torrents que se agregan desde ahí (los de la sesión
//!   de librqbit quedan fijos desde el arranque).
//! - **Puerto fijo** TCP + uTP (`peer_port`) con UPnP si el router lo permite; si el puerto está
//!   ocupado o no hay IPv6, se reintenta (IPv4 solo / puerto efímero) en vez de no arrancar. Se
//!   anuncia ese mismo puerto a trackers, DHT y LSD (el log de arranque dice cuál; si cayó en uno
//!   efímero, avisa: el reenvío del router no sirve).
//! - **DHT, PEX, LSD y uTP** prendidos (PEX lo hace librqbit solo con quien lo soporte).
//! - **Subida sin tope**: librqbit no hace tit-for-tat (le da unchoke a todos) y no le ponemos
//!   límite de subida: con el puerto abierto, subir rápido es lo que hace que los demás nos
//!   prioricen.
//! - **Peers y NAT**: ver abajo.
//! - Timeouts de conexión cortos (4 s) para descartar peers muertos rápido, y `max_request_window`
//!   32 (512 KB por peer) para que una pieza urgente (tras un salto) no quede detrás de 2 MB de
//!   pedidos viejos.
//! - **Solo el archivo elegido** (`only_files`).
//! - **Torrents vivos un rato** después de cerrar la sesión (15 min si nadie más mira; 60 s si hay
//!   otra película en curso, para no robarle ancho de banda; enseguida si una sesión pasa hambre)
//!   y después en pausa, no borrados.
//! - **Precarga** (`prefetch`): principio y final del archivo sin crear sesión. Ojo: el torrent
//!   queda vivo con el archivo elegido, así que en esos 5 min librqbit sigue bajando el resto en
//!   orden (medido: "24 MB" terminaban siendo el archivo entero con fibra).
//! - **Precarga del próximo episodio** (`prefetch_next`): atada a la sesión que se ve y con tope;
//!   ver `NextJob` y `docs/API.md`.
//! - **Prioridad de lo que se ve**: si una sesión pasa hambre (`PlaySession::starving`: arrancando,
//!   tras un salto o con < 20 s de colchón), las precargas sueltan sus fantasmas y sus torrents se
//!   pausan (si ninguna sesión los usa) hasta que se recupere, y los torrents sin uso se pausan ya.
//!
//! ## Peers y NAT
//!
//! Con el puerto abierto entran peers solos, y librqbit cuenta entrantes y salientes en el mismo
//! cupo por torrent (`Tuning::peer_limit`): si se llena de salientes, rechaza a los que entran (que
//! suelen ser los mejores). Por eso el cupo subió de 160 a **200**. El techo lo pone el router: un
//! router doméstico tiene una tabla NAT (conntrack) de unas 4–16 mil entradas para toda la casa, y
//! cada peer conectado o intento de conexión ocupa una (un intento fallido sigue ocupándola ~2 min
//! en SYN_SENT; las de UDP del DHT, 30–180 s). Presupuesto de Mythos: ~1000 entradas como mucho.
//! - Torrent que se ve: 200 peers + los intentos en curso.
//! - Normalmente hay uno o dos torrents vivos: los que no usa nadie se pausan (15 min / 60 s /
//!   ya), las precargas de la ficha duran 5 min y la del próximo episodio solo baja con colchón de
//!   sobra y después pausa su torrent.
//! - DHT y trackers (≤ 80, un anuncio cada ~30 min por torrent): unas pocas centenas de UDP.
//!
//! Con dos personas mirando a la vez son ~400 peers + intentos: sigue dentro del presupuesto. Si
//! el router se queda sin tabla (se corta internet en la casa mientras baja), bajarlo con
//! `MYTHOS_PEER_LIMIT` (p. ej. 120). El modo local (celular, TV box) usa 160: no tiene puerto
//! abierto y comparte el mismo router.
//!
//! Hilos: nada propio; todo son tareas de tokio. El semáforo de librqbit (permisos para trabajo
//! bloqueante) se agranda porque cada `FileStream` vivo retiene uno: con el valor por defecto (8)
//! unos pocos lectores dejarían sin permisos a la escritura a disco.

use crate::cache::{self, Usage};
use crate::probe::{self, Probe};
use crate::session::{Backing, PlaySession, TorrentFile};
use crate::stream::Role;
use crate::util::{self, now_unix};
use crate::window::{self, FileGeom};
use crate::{ClientCaps, MediaConfig, MediaState, MediaStats, MediaTools, PlayInfo, PlayMode, ProbeSource, Source, TorrentStat, Track, Tuning};
use tokio_util::sync::CancellationToken;
use anyhow::{bail, Context};
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};

type ManagedTorrentHandle = Arc<ManagedTorrent>;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, ConnectionOptions, DhtSessionConfig, ListenerMode, ListenerOptions,
    ManagedTorrent, PeerConnectionOptions, Session, SessionOptions, SessionPersistenceConfig,
};
use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime};
use tracing::{info, warn};

pub const BLOCKING_PERMITS: usize = 96;
/// Torrent sin sesión: cuánto sigue vivo si nadie más está mirando.
const KEEP_ALIVE_IDLE: u64 = 15 * 60;
/// Torrent sin sesión mientras otra película está en curso.
const KEEP_ALIVE_BUSY: u64 = 60;
/// Sesión sin ningún pedido HTTP: se cierra.
const SESSION_IDLE: u64 = 3 * 3600;
/// Usado hace menos que esto: el LRU no lo toca.
const LRU_RECENT: u64 = 30 * 60;
/// Espacio libre mínimo que intentamos dejar en el disco.
const MIN_FREE_DISK: u64 = 2 << 30;
/// Con menos que esto se pausan las descargas (antes de que librqbit falle al escribir).
const DISK_HARD_MIN: u64 = 1 << 30;
pub const DISK_FULL_MSG: &str = "the server ran out of disk space; free some space or lower the cache limit";
/// Sin metadata en este tiempo, el torrent no tiene peers alcanzables: error claro en vez de
/// colgar `open()` (la UI muestra el mensaje y ofrece otro stream).
const METADATA_TIMEOUT: Duration = Duration::from_secs(75);
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const PREFETCH_MAX: Duration = Duration::from_secs(5 * 60);
/// Cuánto principio del archivo precargar.
const PREFETCH_HEAD: u64 = 24 << 20;
/// Precarga atrasada por una sesión con hambre: nunca más que esto en total.
const PREFETCH_MAX_TOTAL: Duration = Duration::from_secs(30 * 60);

// Precarga del próximo episodio (ver `NextJob`).
/// Arranca (o vuelve) solo si el episodio actual tiene esto bajado por delante del lector (o ya
/// tiene todo lo que falta hasta el final)...
const NEXT_START_AHEAD_S: f64 = 300.0;
/// ...y se frena apenas baja de esto (histéresis para no prender y apagar a cada rato).
const NEXT_KEEP_AHEAD_S: f64 = 150.0;
/// Sesión cerrada: se espera esto a que se abra el siguiente antes de borrar lo precargado (la
/// UI cierra la sesión vieja y abre la nueva al pasar de episodio).
const NEXT_ORPHAN_GRACE: Duration = Duration::from_secs(45);
/// Sin pedidos HTTP de la sesión en este tiempo (pausa larga): se cancela.
const NEXT_LONG_PAUSE: u64 = 10 * 60;
/// Ningún trabajo vive más que esto.
const NEXT_MAX_AGE: Duration = Duration::from_secs(4 * 3600);
/// Principio a precargar: 90 s de video (bitrate supuesto por el nombre y el tamaño), entre 48 y
/// 128 MB. Con 60 s o más en disco la sesión nueva pasa a `ready` sin esperar a medir la bajada
/// (sin velocidad medida el colchón pedido es 60 s); los 30 s de más cubren el error del bitrate
/// supuesto.
const NEXT_HEAD_S: f64 = 90.0;
const NEXT_HEAD_MIN: u64 = 48 << 20;
const NEXT_HEAD_MAX: u64 = 128 << 20;

const TRACKERS_URL: &str = "https://raw.githubusercontent.com/ngosang/trackerslist/master/trackers_best.txt";
const NEWTRACKON_URL: &str = "https://newtrackon.com/api/stable?include_ipv6_only_trackers=false";
const MAX_TRACKERS: usize = 80;
/// Cada cuánto se vuelve a bajar la lista de trackers.
const TRACKERS_REFRESH: Duration = Duration::from_secs(12 * 3600);
const FALLBACK_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://explodie.org:6969/announce",
];

pub struct Engine {
    pub cfg: MediaConfig,
    pub tuning: Tuning,
    pub rq: Arc<Session>,
    pub api: Api,
    pub http: reqwest::Client,
    sessions: Mutex<HashMap<String, Arc<PlaySession>>>,
    internal: OnceLock<SocketAddr>,
    usage: Mutex<Usage>,
    add_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// (hash, archivo) → hasta cuándo se precarga.
    prefetching: Mutex<HashMap<(String, usize), Instant>>,
    /// Precarga del próximo episodio, por id de la sesión que se está viendo.
    next: Mutex<HashMap<String, Arc<NextJob>>>,
    /// Torrents agregados por una precarga del próximo episodio que todavía no se abrieron
    /// (`data_dir/next-prefetch.json`): si el motor se cierra antes de cancelarlos, se borran al
    /// arrancar.
    next_new: Mutex<HashSet<String>>,
    /// Trackers que pasó `load_trackers` al arrancar (ya están en la sesión de librqbit).
    trackers_boot: HashSet<String>,
    /// Trackers nuevos de los refrescos: se suman a cada torrent que se agrega.
    trackers_new: Mutex<Vec<String>>,
    started_unix: u64,
    pub hls_dir: PathBuf,
    pub subs_dir: PathBuf,
    meta_dir: PathBuf,
    /// ffmpeg/ffprobe encontrados al arrancar (sin ffmpeg: solo reproducción directa).
    pub tools: MediaTools,
    /// Se cancela en `shutdown`: para el endpoint interno y el keeper.
    stop: CancellationToken,
}

impl Engine {
    pub async fn start(cfg: MediaConfig, tuning: Tuning) -> anyhow::Result<Arc<Self>> {
        let d = &cfg.data_dir;
        let torrents_dir = d.join("torrents");
        let hls_dir = d.join("hls");
        let meta_dir = d.join("meta");
        let subs_dir = d.join("subs");
        // Restos de HLS de una corrida anterior: no sirven.
        let _ = std::fs::remove_dir_all(&hls_dir);
        for p in [&torrents_dir, &hls_dir, &meta_dir, &subs_dir, &d.join("session")] {
            std::fs::create_dir_all(p).with_context(|| format!("creating {}", p.display()))?;
        }
        if let Err(e) = librqbit::try_increase_nofile_limit() {
            warn!("could not raise the open files limit: {e:#}");
        }
        let http = crate::http_client_builder()
            .user_agent(concat!("mythos/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        let (trackers, tools) = tokio::join!(load_trackers(&cfg, &http, false), detect_tools(&cfg));
        if !tools.ffmpeg || !tools.ffprobe {
            warn!(ffmpeg = tools.ffmpeg, ffprobe = tools.ffprobe, "ffmpeg/ffprobe not found: direct play only without ffmpeg (no HLS, no internal subtitles)");
        }
        info!(trackers = trackers.len(), "starting torrent session");
        let rq = build_session(&cfg, &tuning, &torrents_dir, &trackers).await?;
        let leftover_next = read_next_registry(&d.join("next-prefetch.json"));
        let engine = Arc::new(Self {
            api: Api::new(rq.clone(), None),
            rq,
            http,
            sessions: Mutex::new(HashMap::new()),
            internal: OnceLock::new(),
            usage: Mutex::new(Usage::load(d.join("usage.json"))),
            add_locks: Mutex::new(HashMap::new()),
            prefetching: Mutex::new(HashMap::new()),
            next: Mutex::new(HashMap::new()),
            next_new: Mutex::new(HashSet::new()),
            trackers_boot: trackers.into_iter().collect(),
            trackers_new: Mutex::new(Vec::new()),
            started_unix: now_unix(),
            tuning,
            hls_dir,
            subs_dir,
            meta_dir,
            tools,
            stop: CancellationToken::new(),
            cfg,
        });
        // Endpoint interno para ffmpeg/ffprobe (127.0.0.1, puerto efímero).
        let l = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let _ = engine.internal.set(l.local_addr()?);
        info!(addr = %l.local_addr()?, "internal media endpoint (ffmpeg/ffprobe)");
        let router = crate::http::internal_router(engine.clone());
        let stop = engine.stop.clone();
        tokio::spawn(async move {
            if let Err(e) = axum::serve(l, router).with_graceful_shutdown(stop.cancelled_owned()).await {
                warn!("internal media endpoint stopped: {e:#}");
            }
        });
        // Precargas del próximo episodio que quedaron colgadas (el motor se cerró antes de
        // cancelarlas): se borran si nunca se abrieron.
        for hash in leftover_next {
            if engine.usage.lock().unwrap().get(&hash).is_none() && engine.remove_torrent(&hash, true).await.is_ok() {
                info!(%hash, "removed a leftover next-episode prefetch");
            }
        }
        engine.save_next_registry();
        tokio::spawn(keeper(Arc::downgrade(&engine), engine.stop.clone()));
        Ok(engine)
    }

    /// Apaga todo (modo embebido). Idempotente.
    pub async fn shutdown(&self) {
        if self.stop.is_cancelled() {
            return;
        }
        self.stop.cancel();
        let ids: Vec<String> = self.sessions.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.close_session(&id).await;
        }
        self.usage.lock().unwrap().save_if_dirty();
        self.rq.stop().await;
        info!("media engine stopped");
    }

    /// URL interna (para ffmpeg/ffprobe) del archivo de una sesión.
    pub fn internal_url(&self, id: &str, role: Role) -> String {
        let addr = self.internal.get().copied().unwrap_or_else(|| SocketAddr::from((Ipv4Addr::LOCALHOST, 0)));
        let r = match role {
            Role::Player => "player",
            Role::Hls => "hls",
            Role::Probe => "probe",
            Role::Passive => "subs",
        };
        format!("http://{addr}/s/{id}/file?role={r}")
    }

    fn disk_free(&self) -> u64 {
        self.tuning.disk_free_override.unwrap_or_else(|| cache::disk_free(&self.cfg.data_dir))
    }

    pub fn session(&self, id: &str) -> Option<Arc<PlaySession>> {
        self.sessions.lock().unwrap().get(id).cloned()
    }

    fn touch_usage(&self, hash: &str) {
        self.usage.lock().unwrap().touch(hash, now_unix());
    }

    fn add_lock(&self, hash: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.add_locks.lock().unwrap().entry(hash.to_string()).or_default().clone()
    }

    /// Archivos que alguien está usando de este torrent (sesiones + precargas).
    fn wanted_files(&self, hash: &str) -> HashSet<usize> {
        let mut s: HashSet<usize> = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter_map(|x| x.torrent().filter(|t| t.hash == hash).map(|t| t.idx))
            .collect();
        let now = Instant::now();
        s.extend(
            self.prefetching
                .lock()
                .unwrap()
                .iter()
                .filter(|(k, until)| k.0 == hash && k.1 != usize::MAX && **until > now)
                .map(|(k, _)| k.1),
        );
        // Precargas del próximo episodio, solo mientras están bajando.
        let jobs: Vec<Arc<NextJob>> = self.next.lock().unwrap().values().filter(|j| j.hash == hash).cloned().collect();
        s.extend(jobs.iter().filter_map(|j| {
            let st = j.st.lock().unwrap();
            st.idx.filter(|_| st.active)
        }));
        s
    }

    fn in_use(&self, hash: &str) -> bool {
        let now = Instant::now();
        self.used_by_session(hash)
            || self.prefetching.lock().unwrap().iter().any(|(k, until)| k.0 == hash && *until > now)
            || self.next.lock().unwrap().values().any(|j| j.hash == hash)
    }

    fn used_by_session(&self, hash: &str) -> bool {
        self.sessions.lock().unwrap().values().any(|x| x.torrent().is_some_and(|t| t.hash == hash))
    }

    /// Alguna sesión (de otro torrent que `hash`, si se pasa) necesita todo el ancho de banda.
    fn starving_except(&self, hash: Option<&str>) -> bool {
        let sessions: Vec<Arc<PlaySession>> = self.sessions.lock().unwrap().values().cloned().collect();
        sessions.iter().any(|s| s.starving() && !s.torrent().is_some_and(|t| Some(t.hash.as_str()) == hash))
    }

    /// Agrega (o retoma) el torrent, se asegura de que el archivo esté seleccionado y vivo.
    /// `touch`: anotar el uso para la caché (no lo hace la precarga del próximo episodio: hasta que
    /// se abra, no tiene que retenerlo la LRU ni el vencimiento).
    async fn ensure_torrent(&self, hash: &str, file_idx: Option<usize>, trackers: &[String], touch: bool) -> anyhow::Result<(ManagedTorrentHandle, usize)> {
        let lock = self.add_lock(hash);
        let _g = lock.lock().await;
        let id20 = Id20::from_str(hash).context("invalid info_hash")?;
        let handle = match self.rq.get(TorrentIdOrHash::Hash(id20)) {
            Some(h) => h,
            None => self.add_torrent(hash, file_idx, trackers).await?,
        };
        tokio::time::timeout(Duration::from_secs(600), handle.wait_until_initialized())
            .await
            .context("torrent took too long to initialize")??;
        let meta = handle.metadata.load_full().context("torrent metadata not resolved")?;
        let idx = match file_idx {
            Some(i) => i,
            None => choose_file(&meta.file_infos.iter().map(|f| (f.relative_filename.to_string_lossy().into_owned(), f.len, f.attrs.padding)).collect::<Vec<_>>())
                .context("torrent has no files")?,
        };
        anyhow::ensure!(idx < meta.file_infos.len(), "file_idx {idx} out of range");
        self.save_meta(hash, &meta.torrent_bytes).await;

        // Selección: este archivo + los que estén usando otras sesiones/precargas.
        let mut want = self.wanted_files(hash);
        want.insert(idx);
        let current: HashSet<usize> = handle.only_files().map(|v| v.into_iter().collect()).unwrap_or_else(|| (0..meta.file_infos.len()).collect());
        if !current.contains(&idx) || current != want {
            let finished = handle.stats().finished;
            if finished && !handle.is_paused() {
                // librqbit reabre en solo-lectura los archivos de un torrent terminado: para sumar
                // uno nuevo hay que pausar y reanudar.
                let _ = self.rq.pause(&handle).await;
            }
            self.rq.update_only_files(&handle, &want).await.context("selecting file")?;
        }
        if handle.is_paused() {
            self.rq.unpause(&handle).await.context("resuming torrent")?;
        }
        if touch {
            self.touch_usage(hash);
        }
        Ok((handle, idx))
    }

    async fn add_torrent(&self, hash: &str, file_idx: Option<usize>, trackers: &[String]) -> anyhow::Result<ManagedTorrentHandle> {
        let meta_path = self.meta_dir.join(format!("{hash}.torrent"));
        // Los del stream más los nuevos de los refrescos de la lista (los del arranque ya los
        // agrega la sesión de librqbit a todos).
        let mut custom: Vec<String> = trackers.iter().filter(|t| t.contains("://")).cloned().collect();
        custom.extend(self.trackers_new.lock().unwrap().iter().cloned());
        custom.sort();
        custom.dedup();
        let base = || AddTorrentOptions {
            overwrite: true,
            trackers: if custom.is_empty() { None } else { Some(custom.clone()) },
            ..Default::default()
        };
        let add = |add: AddTorrent<'static>, opts: AddTorrentOptions| {
            let rq = self.rq.clone();
            async move {
                let r = tokio::time::timeout(METADATA_TIMEOUT, rq.add_torrent(add, Some(opts)))
                    .await
                    .map_err(|_| {
                        let s = METADATA_TIMEOUT.as_secs();
                        anyhow::Error::new(
                            crate::Coded::new("no_peers", format!("no peers found for this torrent (gave up after {s} s); try another stream"))
                                .param("s", s),
                        )
                    })??;
                Ok::<_, anyhow::Error>(r)
            }
        };
        let handle_of = |r: AddTorrentResponse| r.into_handle().context("torrent was not added");

        if let Ok(bytes) = tokio::fs::read(&meta_path).await {
            // Ya vimos este torrent: arranca sin resolver el magnet.
            let bytes = bytes::Bytes::from(bytes);
            let idx = match file_idx {
                Some(i) => i,
                None => {
                    let r = add(AddTorrent::from_bytes(bytes.clone()), AddTorrentOptions { list_only: true, ..Default::default() }).await?;
                    match r {
                        AddTorrentResponse::ListOnly(l) => choose_from_info(&l.info).context("torrent has no files")?,
                        other => return handle_of(other),
                    }
                }
            };
            let opts = AddTorrentOptions { only_files: Some(vec![idx]), ..base() };
            return handle_of(add(AddTorrent::from_bytes(bytes), opts).await?);
        }

        let mut magnet = format!("magnet:?xt=urn:btih:{hash}");
        for t in &custom {
            magnet.push_str("&tr=");
            magnet.push_str(&util::pct_encode(t));
        }
        match file_idx {
            Some(i) => {
                let opts = AddTorrentOptions { only_files: Some(vec![i]), ..base() };
                handle_of(add(AddTorrent::from_url(magnet), opts).await?)
            }
            None => {
                // Sin índice: primero solo la metadata para elegir el video más grande.
                let r = add(AddTorrent::from_url(magnet), AddTorrentOptions { list_only: true, ..base() }).await?;
                let l = match r {
                    AddTorrentResponse::ListOnly(l) => l,
                    other => return handle_of(other),
                };
                let idx = choose_from_info(&l.info).context("torrent has no files")?;
                self.save_meta(hash, &l.torrent_bytes).await;
                let opts = AddTorrentOptions {
                    only_files: Some(vec![idx]),
                    initial_peers: Some(l.seen_peers.clone()),
                    ..base()
                };
                handle_of(add(AddTorrent::from_bytes(l.torrent_bytes), opts).await?)
            }
        }
    }

    async fn save_meta(&self, hash: &str, bytes: &[u8]) {
        let p = self.meta_dir.join(format!("{hash}.torrent"));
        if bytes.is_empty() || tokio::fs::try_exists(&p).await.unwrap_or(false) {
            return;
        }
        let tmp = p.with_extension("tmp");
        if tokio::fs::write(&tmp, bytes).await.is_ok() {
            let _ = tokio::fs::rename(&tmp, &p).await;
        }
    }

    fn torrent_file(&self, handle: &ManagedTorrentHandle, hash: &str, idx: usize) -> anyhow::Result<(TorrentFile, String)> {
        let meta = handle.metadata.load_full().context("torrent metadata not resolved")?;
        let fi = meta.file_infos.get(idx).context("invalid file")?;
        let geom = FileGeom { offset: fi.offset_in_torrent, len: fi.len, piece_len: meta.lengths().default_piece_length() as u64 };
        let name = fi.relative_filename.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "video".into());
        Ok((
            TorrentFile { handle: handle.clone(), hash: hash.to_string(), idx, geom, path: handle.output_folder().join(&fi.relative_filename) },
            name,
        ))
    }

    pub async fn open(self: &Arc<Self>, src: Source, caps: ClientCaps) -> anyhow::Result<PlayInfo> {
        let id = util::random_id();
        let tmp = self.hls_dir.join(&id);
        let t0 = Instant::now();
        let sess = match src {
            Source::Torrent { info_hash, file_idx, trackers } => {
                let hash = util::normalize_hash(&info_hash).context("invalid info_hash")?;
                // Si es el próximo episodio precargado, se reusa (y las precargas de sesiones ya
                // cerradas que no son esto se cancelan: se pasó a otra cosa).
                self.adopt_next(Some(&hash), file_idx);
                let (handle, idx) = self.ensure_torrent(&hash, file_idx, &trackers, true).await?;
                let (tf, name) = self.torrent_file(&handle, &hash, idx)?;
                let need = tf.geom.len.saturating_sub(handle.stats().file_progress.get(idx).copied().unwrap_or(0));
                self.evict_lru(need).await;
                let geom = tf.geom;
                let sess = Arc::new(PlaySession::new(id.clone(), Backing::Torrent(tf), name, geom.len, tmp));
                self.sessions.lock().unwrap().insert(id.clone(), sess.clone());
                tokio::spawn(window::run(Arc::downgrade(self), handle, idx, geom, sess.clone()));
                sess
            }
            Source::Url(url) => {
                self.adopt_next(None, None);
                let (size, name) = crate::stream::url_meta(&self.http, &url).await.context("could not open the link")?;
                let sess = Arc::new(PlaySession::new(id.clone(), Backing::Url(url), name, size, tmp));
                self.sessions.lock().unwrap().insert(id.clone(), sess.clone());
                sess
            }
        };
        sess.set_state(MediaState::Probing);
        let t_src = t0.elapsed();
        let (p, source) = self.probe(&id, &sess).await;
        sess.set_duration(p.duration, p.bit_rate);
        let info = build_info(&id, &sess, &p, &caps, source, self.tools.ffmpeg);
        *sess.info.lock().unwrap() = Some(info.clone());
        *sess.probe.lock().unwrap() = Some(p);
        *sess.caps.lock().unwrap() = caps;
        match sess.backing {
            // La ventana lo pasa a `ready` cuando haya colchón suficiente.
            Backing::Torrent(_) => sess.set_state(MediaState::Buffering),
            Backing::Url(_) => sess.set_state(MediaState::Ready),
        }
        info!(
            session = %id,
            file = %sess.file_name,
            mode = ?info.mode,
            probed = sess.probe.lock().unwrap().as_ref().is_some_and(|p| p.real),
            source_ms = t_src.as_millis() as u64,
            probe_ms = (t0.elapsed() - t_src).as_millis() as u64,
            "opened"
        );
        Ok(info)
    }

    /// ffprobe si está; si no (o falla), las cabeceras en Rust (`sniff`); si no, por el nombre.
    async fn probe(&self, id: &str, sess: &PlaySession) -> (Probe, ProbeSource) {
        let url = self.internal_url(id, Role::Probe);
        if self.tools.ffprobe {
            match probe::run(&self.cfg.ffprobe, &url, &sess.file_name, PROBE_TIMEOUT).await {
                Ok(p) => return (p, ProbeSource::Ffprobe),
                Err(e) => warn!(file = %sess.file_name, "ffprobe failed, trying the built-in sniffer: {e:#}"),
            }
        }
        match crate::sniff::run(&self.http, &url, sess.size, &sess.file_name, PROBE_TIMEOUT).await {
            Ok(mut p) => {
                // El sniffer no ve el bitrate: el de duración y tamaño alcanza para la ventana.
                if p.bit_rate.is_none() {
                    p.bit_rate = p.duration.filter(|d| *d > 0.0).map(|d| (sess.size as f64 * 8.0 / d) as u64);
                }
                (p, ProbeSource::Sniff)
            }
            Err(e) => {
                warn!(file = %sess.file_name, "built-in sniffer failed, guessing from the name: {e:#}");
                (probe::guess(&sess.file_name), ProbeSource::Guess)
            }
        }
    }

    pub fn prefetch(self: &Arc<Self>, src: Source) -> anyhow::Result<()> {
        let me = self.clone();
        match src {
            Source::Torrent { info_hash, file_idx, trackers } => {
                let hash = util::normalize_hash(&info_hash).context("invalid info_hash")?;
                tokio::spawn(async move {
                    if let Err(e) = me.prefetch_torrent(&hash, file_idx, &trackers).await {
                        warn!(%hash, "prefetch failed: {e:#}");
                    }
                });
            }
            Source::Url(url) => {
                // Un link directo no se cachea; solo "calentamos" el origen.
                tokio::spawn(async move {
                    let _ = crate::stream::url_meta(&me.http, &url).await;
                });
            }
        }
        Ok(())
    }

    async fn prefetch_torrent(&self, hash: &str, file_idx: Option<usize>, trackers: &[String]) -> anyhow::Result<()> {
        // Se marca antes de agregar para que el keeper no lo pause mientras resuelve.
        let key_idx = file_idx.unwrap_or(usize::MAX);
        self.prefetching.lock().unwrap().insert((hash.to_string(), key_idx), Instant::now() + PREFETCH_MAX);
        let res = self.ensure_torrent(hash, file_idx, trackers, true).await;
        self.prefetching.lock().unwrap().remove(&(hash.to_string(), key_idx));
        let (handle, idx) = res?;
        let key = (hash.to_string(), idx);
        let mut until = Instant::now() + PREFETCH_MAX;
        let hard_until = Instant::now() + PREFETCH_MAX_TOTAL;
        self.prefetching.lock().unwrap().insert(key.clone(), until);
        let (tf, _) = self.torrent_file(&handle, hash, idx)?;
        let g = tf.geom;
        let head_len = PREFETCH_HEAD.min(g.len);
        let tail_pos = g.len - window::tail_bytes(g.len);
        // Dos fantasmas: principio (24 MB) y final (índices). Se sueltan al completarse.
        let (mut head, mut tail) = (None, None);
        let (mut head_done, mut tail_done) = (false, false);
        let mut yielded = false;
        while Instant::now() < until && !(head_done && tail_done) {
            tokio::time::sleep(Duration::from_secs(1)).await;
            // Una sesión de otro torrent pasa hambre: se le cede todo (sin fantasmas y, si nadie
            // mira este torrent, en pausa) y el plazo se corre.
            if self.starving_except(Some(hash)) {
                if !yielded {
                    yielded = true;
                    (head, tail) = (None, None);
                    if !self.used_by_session(hash) {
                        let _ = self.rq.pause(&handle).await;
                    }
                }
                until = (until + Duration::from_secs(1)).min(hard_until);
                self.prefetching.lock().unwrap().insert(key.clone(), until);
                continue;
            }
            if yielded {
                yielded = false;
                if handle.is_paused() {
                    let _ = self.rq.unpause(&handle).await;
                }
            }
            if !head_done && head.is_none() {
                head = window::Phantom::open(&handle, idx, 0).await;
            }
            if !tail_done && tail.is_none() {
                tail = window::Phantom::open(&handle, idx, tail_pos).await;
            }
            let Some(hv) = window::haves(&self.api, handle.info_hash()) else { continue };
            let have = |p: usize| hv.get(p).copied().unwrap_or(false);
            if window::missing_in(&g, &have, 0, head_len) == 0 {
                head_done = true;
                head = None;
            }
            if window::missing_in(&g, &have, tail_pos, g.len - tail_pos) == 0 {
                tail_done = true;
                tail = None;
            }
        }
        drop((head, tail));
        self.prefetching.lock().unwrap().remove(&key);
        self.touch_usage(hash);
        Ok(())
    }

    pub async fn close_session(&self, id: &str) {
        let Some(sess) = self.sessions.lock().unwrap().remove(id) else { return };
        sess.close();
        crate::hls::stop(&sess).await;
        let _ = tokio::fs::remove_dir_all(&sess.tmp_dir).await;
        if let Some(t) = sess.torrent() {
            self.touch_usage(&t.hash);
        }
        info!(session = %id, "closed");
    }

    pub fn torrents(&self) -> Vec<TorrentStat> {
        let handles: Vec<ManagedTorrentHandle> = self.rq.with_torrents(|it| it.map(|(_, h)| h.clone()).collect());
        let usage = self.usage.lock().unwrap();
        handles
            .iter()
            .map(|h| {
                let st = h.stats();
                let hash = h.info_hash().as_string();
                let (down, up, peers) = h
                    .live()
                    .map(|l| (l.down_speed_estimator().bps(), l.up_speed_estimator().bps(), l.stats_snapshot().peer_stats.live))
                    .unwrap_or((0, 0, 0));
                TorrentStat {
                    name: h.name().unwrap_or_else(|| hash.clone()),
                    last_used_at: util::rfc3339(usage.get(&hash).unwrap_or(self.started_unix)),
                    info_hash: hash,
                    size_bytes: st.total_bytes,
                    downloaded_bytes: st.progress_bytes,
                    down_bps: down,
                    up_bps: up,
                    peers,
                    state: st.state.to_string(),
                }
            })
            .collect()
    }

    pub async fn remove_torrent(&self, info_hash: &str, delete_files: bool) -> anyhow::Result<()> {
        let hash = util::normalize_hash(info_hash).context("invalid info_hash")?;
        let ids: Vec<String> = self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, s)| s.torrent().is_some_and(|t| t.hash == hash))
            .map(|(k, _)| k.clone())
            .collect();
        for id in ids {
            self.close_session(&id).await;
        }
        let id20 = Id20::from_str(&hash)?;
        if self.rq.get(TorrentIdOrHash::Hash(id20)).is_none() {
            bail!("torrent not found");
        }
        self.rq.delete(TorrentIdOrHash::Hash(id20), delete_files).await?;
        self.usage.lock().unwrap().forget(&hash);
        if delete_files {
            crate::subs::forget_torrent(&self.subs_dir, &hash).await;
            crate::thumbs::forget_torrent(&self.cfg.data_dir, &hash).await;
        }
        Ok(())
    }

    pub fn stats(&self) -> MediaStats {
        let cache_bytes: u64 = self.rq.with_torrents(|it| it.map(|(_, h)| h.stats().progress_bytes).sum());
        MediaStats {
            cache_bytes,
            cache_limit_bytes: self.cfg.cache_limit_bytes,
            disk_free_bytes: self.disk_free(),
            active_sessions: self.sessions.lock().unwrap().len() as u32,
        }
    }

    pub fn is_downloaded(&self, info_hash: &str, file_idx: Option<usize>) -> bool {
        let Some(hash) = util::normalize_hash(info_hash) else { return false };
        let Ok(id20) = Id20::from_str(&hash) else { return false };
        let Some(h) = self.rq.get(TorrentIdOrHash::Hash(id20)) else { return false };
        let Some(meta) = h.metadata.load_full() else { return false };
        let idx = match file_idx {
            Some(i) => i,
            None => match choose_file(&meta.file_infos.iter().map(|f| (f.relative_filename.to_string_lossy().into_owned(), f.len, f.attrs.padding)).collect::<Vec<_>>()) {
                Some(i) => i,
                None => return false,
            },
        };
        let Some(fi) = meta.file_infos.get(idx) else { return false };
        h.stats().file_progress.get(idx).is_some_and(|&have| have >= fi.len)
    }

    /// Borra torrents viejos (nunca los en uso) para que entren `extra_needed` bytes más.
    /// `cache_limit_bytes == 0` = sin límite. Si el disco se está por llenar, también se borran
    /// los usados hace poco (pero nunca los que están en uso).
    async fn evict_lru(&self, extra_needed: u64) {
        let handles: Vec<ManagedTorrentHandle> = self.rq.with_torrents(|it| it.map(|(_, h)| h.clone()).collect());
        let now = now_unix();
        let free = self.disk_free();
        let want_free = MIN_FREE_DISK.max(extra_needed);
        let low_disk = free > 0 && free < want_free;
        let entries: Vec<cache::Entry> = {
            let usage = self.usage.lock().unwrap();
            handles
                .iter()
                .map(|h| {
                    let hash = h.info_hash().as_string();
                    let last = usage.get(&hash).unwrap_or(0);
                    cache::Entry {
                        in_use: self.in_use(&hash) || (!low_disk && now.saturating_sub(last) < LRU_RECENT),
                        bytes: h.stats().progress_bytes,
                        last_used: last,
                        hash,
                    }
                })
                .collect()
        };
        let total: u64 = entries.iter().map(|e| e.bytes).sum();
        let mut limit = if self.cfg.cache_limit_bytes == 0 { u64::MAX / 2 } else { self.cfg.cache_limit_bytes };
        if low_disk {
            // Lo que falta para tener `want_free` libres sale de la caché.
            limit = limit.min(total.saturating_sub(want_free - free) + extra_needed);
        }
        for hash in cache::pick_evictions(&entries, limit, extra_needed) {
            info!(%hash, "cache full: removing least recently used torrent");
            if let Err(e) = self.remove_torrent(&hash, true).await {
                warn!(%hash, "could not evict: {e:#}");
            }
        }
    }

    /// Borra los torrents sin usar hace `cache_max_age_secs` (nunca los en uso). Corre al arrancar,
    /// así lo que venció con la app cerrada se borra apenas se abre.
    async fn evict_expired(&self) {
        let max_age = self.cfg.cache_max_age_secs;
        if max_age == 0 {
            return;
        }
        let handles: Vec<ManagedTorrentHandle> = self.rq.with_torrents(|it| it.map(|(_, h)| h.clone()).collect());
        let entries: Vec<cache::Entry> = {
            let usage = self.usage.lock().unwrap();
            handles
                .iter()
                .map(|h| {
                    let hash = h.info_hash().as_string();
                    cache::Entry {
                        in_use: self.in_use(&hash),
                        bytes: 0,
                        // Sin registro de uso: cuenta desde que arrancó el motor.
                        last_used: usage.get(&hash).unwrap_or(self.started_unix),
                        hash,
                    }
                })
                .collect()
        };
        for hash in cache::pick_expired(&entries, now_unix(), max_age) {
            info!(%hash, "unused for too long: removing torrent");
            if let Err(e) = self.remove_torrent(&hash, true).await {
                warn!(%hash, "could not remove expired torrent: {e:#}");
            }
        }
    }

    /// Disco casi lleno: primero se libera caché (LRU, también lo usado hace poco) y, si igual no
    /// alcanza, se pausan los torrents que siguen bajando, con un error claro en sus sesiones. Así
    /// lo ya bajado sigue reproduciéndose, en vez de que librqbit falle al escribir y el torrent
    /// quede en error. La ventana lo llama en cada vuelta (es un `statvfs`, barato).
    pub async fn guard_disk(&self) {
        let free = self.disk_free();
        if free == 0 || free >= DISK_HARD_MIN {
            return;
        }
        self.evict_lru(0).await;
        if self.disk_free() >= DISK_HARD_MIN {
            return;
        }
        let handles: Vec<ManagedTorrentHandle> = self.rq.with_torrents(|it| it.map(|(_, h)| h.clone()).collect());
        for h in handles {
            if h.is_paused() || h.live().is_none() || h.stats().finished {
                continue;
            }
            let hash = h.info_hash().as_string();
            warn!(%hash, free, "disk almost full: pausing download");
            let _ = self.rq.pause(&h).await;
            for s in self.sessions.lock().unwrap().values() {
                if s.torrent().is_some_and(|t| t.hash == hash) {
                    s.set_error(crate::Coded::new("no_space", DISK_FULL_MSG));
                }
            }
        }
    }

    /// Pausa y reanuda un torrent: libera todas las piezas en vuelo (las retenidas por peers
    /// lentos vuelven a la cola) y reconecta. Lo usa la ventana cuando la pieza que espera el
    /// reproductor queda trabada.
    pub async fn rescue(&self, h: &ManagedTorrentHandle) {
        if self.rq.pause(h).await.is_ok() {
            if let Err(e) = self.rq.unpause(h).await {
                warn!("could not resume torrent after rescue: {e:#}");
            }
        }
    }

    /// Pausa torrents sin uso pasado su tiempo de gracia.
    async fn pause_idle_torrents(&self) {
        let handles: Vec<ManagedTorrentHandle> = self.rq.with_torrents(|it| it.map(|(_, h)| h.clone()).collect());
        let someone_watching = !self.sessions.lock().unwrap().is_empty();
        let starving = self.starving_except(None);
        let now = now_unix();
        for h in handles {
            if h.is_paused() || h.live().is_none() {
                continue;
            }
            let hash = h.info_hash().as_string();
            if self.in_use(&hash) {
                continue;
            }
            let last = self.usage.lock().unwrap().get(&hash).unwrap_or(0).max(self.started_unix);
            let grace = if starving {
                0
            } else if someone_watching {
                KEEP_ALIVE_BUSY
            } else {
                KEEP_ALIVE_IDLE
            };
            if now.saturating_sub(last) >= grace {
                info!(%hash, "pausing idle torrent");
                let _ = self.rq.pause(&h).await;
            }
        }
    }
}

/// Precarga del próximo episodio, atada a la sesión `parent` que se está viendo. Reglas (las
/// mismas que `docs/API.md`, "Precarga del próximo episodio"):
/// 1. **Nunca perjudica al actual**: solo baja si el actual tiene ≥ 5 min bajados por delante del
///    lector (o todo lo que falta hasta el final) y ninguna sesión pasa hambre; se frena al toque
///    si baja de 2,5 min. Prioridad mínima: un solo fantasma (primero el final, después el
///    principio) contra las ventanas del actual. Frenada = sin fantasma y sin el archivo en la
///    selección (mismo torrent) o con el torrent en pausa (otro torrent).
/// 2. **Se cancela y se borra** si la sesión se cierra y en 45 s no se abre el siguiente, si se
///    abre otra cosa con la sesión ya cerrada, si la sesión no pide nada en 10 min (pausa larga)
///    o si llega otra precarga para la misma sesión: torrent agregado por la precarga → se quita
///    con archivos; mismo pack (o uno que ya estaba en caché) → el archivo sale de la selección.
///    Nunca se anota el uso, así que ni la LRU ni el vencimiento lo retienen.
/// 3. **Se reusa** si se abre (`open`) la misma fuente: queda como cualquier torrent abierto.
/// 4. **Tope**: el final del archivo (índice/moov/cues) y `next_head_bytes` del principio; al
///    completarse se frena igual que en 1, no baja el episodio entero.
struct NextJob {
    parent: String,
    hash: String,
    want_idx: Option<usize>,
    trackers: Vec<String>,
    st: Mutex<NextJobState>,
    cancel: CancellationToken,
    adopted: std::sync::atomic::AtomicBool,
}

#[derive(Default, Clone)]
struct NextJobState {
    /// Archivo resuelto (con `want_idx = None`, el que elige `choose_file`).
    idx: Option<usize>,
    /// Bajando (archivo seleccionado, fantasma puesto, torrent sin pausa).
    active: bool,
    done: bool,
    /// El torrent lo agregó esta precarga (si se cancela, se borra con sus archivos).
    new_torrent: bool,
    same_torrent: bool,
    /// Selección que tenía un torrent que ya existía (para dejarla igual al frenar/cancelar).
    prev_only: Option<HashSet<usize>>,
    have: u64,
    target: u64,
}

/// Lo que hace falta del archivo precargado para ir midiendo.
struct NextFile {
    handle: ManagedTorrentHandle,
    idx: usize,
    geom: FileGeom,
    head_len: u64,
    tail_pos: u64,
}

/// Cuánto principio del próximo episodio bajar (ver `NEXT_HEAD_S`).
pub fn next_head_bytes(name: &str, len: u64) -> u64 {
    let br = crate::session::guess_bitrate(name, len) as f64;
    ((br * NEXT_HEAD_S) as u64).clamp(NEXT_HEAD_MIN, NEXT_HEAD_MAX).min(len)
}

/// ¿El episodio actual tiene colchón para precargar? `rest_done`: ya está todo hasta el final.
pub fn next_cushion_ok(rest_done: bool, ahead_s: f64, active: bool) -> bool {
    rest_done || ahead_s >= if active { NEXT_KEEP_AHEAD_S } else { NEXT_START_AHEAD_S }
}

fn read_next_registry(path: &Path) -> Vec<String> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

impl Engine {
    pub fn prefetch_next(self: &Arc<Self>, session_id: &str, src: Source) -> anyhow::Result<()> {
        let parent = self.session(session_id).filter(|s| !s.is_closed()).context("unknown or expired media session")?;
        let (info_hash, file_idx, trackers) = match src {
            Source::Torrent { info_hash, file_idx, trackers } => (info_hash, file_idx, trackers),
            // Un link directo no se cachea: solo se "calienta" el origen.
            url @ Source::Url(_) => return self.prefetch(url),
        };
        let hash = util::normalize_hash(&info_hash).context("invalid info_hash")?;
        if parent.torrent().is_some_and(|t| t.hash == hash && file_idx == Some(t.idx)) {
            return Ok(());
        }
        let job = {
            let mut m = self.next.lock().unwrap();
            if let Some(old) = m.get(session_id) {
                if old.hash == hash && old.want_idx == file_idx {
                    return Ok(());
                }
                // Otra fuente para la misma sesión: la vieja se cancela (y se limpia sola).
                old.cancel.cancel();
            }
            let job = Arc::new(NextJob {
                parent: session_id.to_string(),
                hash,
                want_idx: file_idx,
                trackers,
                st: Mutex::new(NextJobState::default()),
                cancel: CancellationToken::new(),
                adopted: Default::default(),
            });
            m.insert(session_id.to_string(), job.clone());
            job
        };
        info!(session = %session_id, hash = %job.hash, file_idx = ?file_idx, "next episode prefetch requested");
        let me = self.clone();
        tokio::spawn(async move {
            if let Err(e) = me.next_loop(&job).await {
                warn!(hash = %job.hash, "next episode prefetch failed: {e:#}");
            }
            me.next_finish(&job).await;
        });
        Ok(())
    }

    pub fn next_status(&self, session_id: &str) -> Option<crate::NextStatus> {
        let job = self.next.lock().unwrap().get(session_id).cloned()?;
        let st = job.st.lock().unwrap().clone();
        Some(crate::NextStatus {
            state: if st.done {
                crate::NextState::Done
            } else if st.active {
                crate::NextState::Active
            } else {
                crate::NextState::Waiting
            },
            info_hash: job.hash.clone(),
            file_idx: st.idx.or(job.want_idx),
            same_torrent: st.same_torrent,
            have_bytes: st.have,
            target_bytes: st.target,
        })
    }

    /// Al abrir una fuente: si es la de una precarga del próximo episodio, se la queda (la
    /// precarga termina sin borrar nada). Las precargas cuya sesión ya se cerró y que no son
    /// esto se cancelan: el usuario pasó a otra cosa.
    fn adopt_next(&self, hash: Option<&str>, file_idx: Option<usize>) {
        let jobs: Vec<(String, Arc<NextJob>)> = self.next.lock().unwrap().iter().map(|(k, j)| (k.clone(), j.clone())).collect();
        for (key, j) in jobs {
            let idx = j.st.lock().unwrap().idx;
            let same = hash == Some(j.hash.as_str()) && (file_idx.is_none() || file_idx == j.want_idx || file_idx == idx);
            if same {
                j.adopted.store(true, std::sync::atomic::Ordering::Relaxed);
                j.cancel.cancel();
                self.next.lock().unwrap().remove(&key);
                if self.next_new.lock().unwrap().remove(&j.hash) {
                    self.save_next_registry();
                }
                info!(hash = %j.hash, "next episode prefetch reused");
            } else if self.session(&j.parent).is_none_or(|s| s.is_closed()) {
                j.cancel.cancel();
            }
        }
    }

    fn save_next_registry(&self) {
        let list: Vec<String> = self.next_new.lock().unwrap().iter().cloned().collect();
        let path = self.cfg.data_dir.join("next-prefetch.json");
        if list.is_empty() {
            let _ = std::fs::remove_file(path);
        } else if let Ok(b) = serde_json::to_vec(&list) {
            let _ = std::fs::write(path, b);
        }
    }

    /// Vuelve cuando la precarga tiene que terminar (cancelada, adoptada, sesión cerrada o en
    /// pausa larga, vencida). `next_finish` limpia.
    async fn next_loop(&self, job: &NextJob) -> anyhow::Result<()> {
        let started = Instant::now();
        let mut orphan: Option<Instant> = None;
        let mut file: Option<NextFile> = None;
        let mut phantom: Option<window::Phantom> = None;
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = job.cancel.cancelled() => return Ok(()),
            }
            if started.elapsed() > NEXT_MAX_AGE {
                return Ok(());
            }
            let active = job.st.lock().unwrap().active;
            let ok = match self.session(&job.parent).filter(|s| !s.is_closed()) {
                None => {
                    if orphan.get_or_insert_with(Instant::now).elapsed() > NEXT_ORPHAN_GRACE {
                        info!(hash = %job.hash, "next episode prefetch: the session ended without opening it");
                        return Ok(());
                    }
                    false
                }
                Some(p) => {
                    orphan = None;
                    if p.idle_secs() > NEXT_LONG_PAUSE {
                        info!(hash = %job.hash, "next episode prefetch: long pause, cancelling");
                        return Ok(());
                    }
                    let (rest_done, ahead) = p.cushion();
                    next_cushion_ok(rest_done, ahead, active) && !self.starving_except(None)
                }
            };
            let done = job.st.lock().unwrap().done;
            if !ok || done {
                if active {
                    phantom = None;
                    self.next_set_active(job, file.as_ref(), false).await;
                }
                continue;
            }

            if file.is_none() {
                {
                    // Primera vez con colchón: recién ahora se agrega el torrent.
                    let existing = Id20::from_str(&job.hash).ok().and_then(|id| self.rq.get(TorrentIdOrHash::Hash(id)));
                    let same = self.session(&job.parent).and_then(|p| p.torrent().map(|t| t.hash == job.hash)).unwrap_or(false);
                    {
                        let mut st = job.st.lock().unwrap();
                        st.same_torrent = same;
                        st.prev_only = existing.as_ref().and_then(|h| h.only_files()).map(|v| v.into_iter().collect());
                        st.new_torrent = existing.is_none();
                        st.active = true;
                    }
                    if existing.is_none() {
                        self.next_new.lock().unwrap().insert(job.hash.clone());
                        self.save_next_registry();
                    }
                    let (handle, idx) = tokio::select! {
                        r = self.ensure_torrent(&job.hash, job.want_idx, &job.trackers, false) => r?,
                        _ = job.cancel.cancelled() => return Ok(()),
                    };
                    let (tf, name) = self.torrent_file(&handle, &job.hash, idx)?;
                    let g = tf.geom;
                    let head_len = next_head_bytes(&name, g.len);
                    let tail_pos = g.len - window::tail_bytes(g.len);
                    {
                        let mut st = job.st.lock().unwrap();
                        st.idx = Some(idx);
                        st.target = head_len.min(tail_pos) + (g.len - tail_pos);
                    }
                    info!(hash = %job.hash, idx, file = %name, head_mb = head_len >> 20, "next episode prefetch started");
                    file = Some(NextFile { handle, idx, geom: g, head_len, tail_pos });
                }
            }
            let Some(f) = file.as_ref() else { continue };
            if !job.st.lock().unwrap().active {
                self.next_set_active(job, Some(f), true).await;
            }

            let Some(hv) = window::haves(&self.api, f.handle.info_hash()) else { continue };
            let have = |p: usize| hv.get(p).copied().unwrap_or(false);
            let g = &f.geom;
            let tail_missing = window::missing_in(g, &have, f.tail_pos, g.len - f.tail_pos);
            let head_missing = window::missing_in(g, &have, 0, f.head_len.min(f.tail_pos));
            {
                let mut st = job.st.lock().unwrap();
                st.have = st.target.saturating_sub((tail_missing + head_missing) as u64 * g.piece_len);
            }
            if tail_missing == 0 && head_missing == 0 {
                phantom = None;
                job.st.lock().unwrap().done = true;
                self.next_set_active(job, Some(f), false).await;
                info!(hash = %job.hash, idx = f.idx, "next episode prefetch ready");
                continue;
            }
            // Un solo fantasma: primero el índice (sin él no se puede sondear), después el
            // principio desde la primera pieza que falta.
            let pos = if tail_missing > 0 {
                f.tail_pos
            } else {
                window::first_missing_from(g, &have, 0).map(|p| g.piece_start(p)).unwrap_or(0)
            };
            match phantom.as_mut() {
                Some(ph) => ph.move_to(pos).await,
                None => phantom = window::Phantom::open(&f.handle, f.idx, pos).await,
            }
        }
    }

    /// Prende o frena la precarga: con el archivo en la selección y el torrent andando, o fuera
    /// de la selección y (si ninguna sesión lo usa) con el torrent en pausa.
    async fn next_set_active(&self, job: &NextJob, file: Option<&NextFile>, on: bool) {
        let prev_only = {
            let mut st = job.st.lock().unwrap();
            st.active = on;
            st.prev_only.clone()
        };
        let Some(f) = file else { return };
        self.apply_selection(&f.handle, &job.hash, prev_only).await;
        if on {
            if f.handle.is_paused() {
                let _ = self.rq.unpause(&f.handle).await;
            }
        } else if !job.adopted.load(std::sync::atomic::Ordering::Relaxed) && !self.used_by_session(&job.hash) && !f.handle.is_paused() {
            let _ = self.rq.pause(&f.handle).await;
        }
    }

    /// Deja la selección de archivos en lo que alguien está usando (`wanted_files`); si no queda
    /// nada, en `fallback` (la que tenía antes de la precarga).
    async fn apply_selection(&self, h: &ManagedTorrentHandle, hash: &str, fallback: Option<HashSet<usize>>) {
        let mut want = self.wanted_files(hash);
        if want.is_empty() {
            match fallback {
                Some(f) if !f.is_empty() => want = f,
                _ => return,
            }
        }
        let current: Option<HashSet<usize>> = h.only_files().map(|v| v.into_iter().collect());
        if current.as_ref() == Some(&want) {
            return;
        }
        let adds = current.as_ref().is_none_or(|c| !want.is_subset(c));
        let reopen = adds && h.stats().finished && !h.is_paused();
        if reopen {
            // Igual que en `ensure_torrent`: un torrent terminado tiene los archivos en
            // solo-lectura; para sumar uno hay que pausar y reanudar.
            let _ = self.rq.pause(h).await;
        }
        if let Err(e) = self.rq.update_only_files(h, &want).await {
            warn!(%hash, "could not update the file selection: {e:#}");
        }
        if reopen {
            let _ = self.rq.unpause(h).await;
        }
    }

    /// Fin de una precarga del próximo episodio: si no se reusó, se borra lo que agregó.
    async fn next_finish(&self, job: &Arc<NextJob>) {
        {
            let mut m = self.next.lock().unwrap();
            if m.get(&job.parent).is_some_and(|j| Arc::ptr_eq(j, job)) {
                m.remove(&job.parent);
            }
        }
        if job.adopted.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let st = job.st.lock().unwrap().clone();
        let handle = Id20::from_str(&job.hash).ok().and_then(|id| self.rq.get(TorrentIdOrHash::Hash(id)));
        if let Some(h) = handle {
            if st.new_torrent && !self.in_use(&job.hash) {
                match self.remove_torrent(&job.hash, true).await {
                    Ok(()) => info!(hash = %job.hash, "next episode prefetch cancelled: torrent removed"),
                    Err(e) => warn!(hash = %job.hash, "could not remove the next episode prefetch: {e:#}"),
                }
            } else if st.idx.is_some() {
                self.apply_selection(&h, &job.hash, st.prev_only).await;
                if !self.in_use(&job.hash) && !h.is_paused() {
                    let _ = self.rq.pause(&h).await;
                }
                info!(hash = %job.hash, "next episode prefetch cancelled: file deselected");
            }
        }
        if st.new_torrent && self.next_new.lock().unwrap().remove(&job.hash) {
            self.save_next_registry();
        }
    }
}

fn build_info(id: &str, sess: &PlaySession, p: &Probe, caps: &ClientCaps, source: ProbeSource, ffmpeg: bool) -> PlayInfo {
    let wanted = probe::choose_mode(p, caps);
    // Sin ffmpeg lo único posible es directo; `direct_ok` le avisa a la UI si va a andar.
    let mode = if ffmpeg { wanted } else { PlayMode::Direct };
    let v = p.video.as_ref();
    let text_subs = ["subrip", "srt", "ass", "ssa", "mov_text", "webvtt", "text"];
    PlayInfo {
        session_id: id.to_string(),
        mode,
        direct_url: format!("/api/media/{id}/file"),
        hls_url: (mode != PlayMode::Direct).then(|| format!("/api/media/{id}/hls/index.m3u8")),
        file_name: sess.file_name.clone(),
        size_bytes: sess.size,
        duration_s: p.duration,
        container: p.container.clone(),
        video_codec: v.map(|v| v.codec.clone()),
        width: v.and_then(|v| v.width),
        height: v.and_then(|v| v.height),
        hdr: v.map(|v| v.hdr),
        audio_tracks: p.audio.iter().map(|a| a.track.clone()).collect(),
        // Los internos se extraen con ffmpeg: sin ffmpeg no se ofrecen.
        subtitle_tracks: if ffmpeg {
            p.subs.iter().filter(|s| text_subs.contains(&s.codec.as_str())).cloned().collect::<Vec<Track>>()
        } else {
            Vec::new()
        },
        transcode_available: ffmpeg,
        direct_ok: wanted == PlayMode::Direct,
        probe_source: source,
    }
}

/// `-version` de ffmpeg y ffprobe (en paralelo, 5 s como mucho). Ruta vacía = deshabilitado.
async fn detect_tools(cfg: &MediaConfig) -> MediaTools {
    async fn ok(path: &Path) -> bool {
        if path.as_os_str().is_empty() {
            return false;
        }
        let mut cmd = crate::util::tool_command(path);
        cmd.arg("-version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        matches!(tokio::time::timeout(Duration::from_secs(5), cmd.status()).await, Ok(Ok(s)) if s.success())
    }
    let (ffmpeg, ffprobe) = tokio::join!(ok(&cfg.ffmpeg), ok(&cfg.ffprobe));
    MediaTools { ffmpeg, ffprobe }
}

/// El video más grande; si no hay videos, el archivo más grande. Ignora archivos de relleno.
pub fn choose_file(files: &[(String, u64, bool)]) -> Option<usize> {
    let best = |only_video: bool| {
        files
            .iter()
            .enumerate()
            .filter(|(_, (n, _, pad))| !pad && (!only_video || util::is_video_name(n)))
            .max_by_key(|(_, (_, len, _))| *len)
            .map(|(i, _)| i)
    };
    best(true).or_else(|| best(false))
}

fn choose_from_info(info: &librqbit::ValidatedTorrentMetaV1Info<librqbit::ByteBufOwned>) -> Option<usize> {
    let files: Vec<(String, u64, bool)> = info
        .iter_file_details()
        .map(|fd| (fd.filename.to_pathbuf().to_string_lossy().into_owned(), fd.len, fd.attrs().padding))
        .collect();
    choose_file(&files)
}

/// Trackers públicos: la lista fija, `trackers_best` de ngosang y la "stable" de newTrackon (en
/// ese orden de preferencia, sin repetidos, como mucho `MAX_TRACKERS`). `refresh`: bajar aunque la
/// caché en disco sea de hace menos de 12 h.
async fn load_trackers(cfg: &MediaConfig, http: &reqwest::Client, refresh: bool) -> Vec<String> {
    if !cfg.extra_trackers {
        return Vec::new();
    }
    let (best_cache, stable_cache) = (cfg.data_dir.join("trackers_best.txt"), cfg.data_dir.join("trackers_newtrackon.txt"));
    let (best, stable) = tokio::join!(
        tracker_source(http, TRACKERS_URL, &best_cache, refresh),
        tracker_source(http, NEWTRACKON_URL, &stable_cache, refresh),
    );
    merge_trackers(&[FALLBACK_TRACKERS.iter().map(|s| s.to_string()).collect(), parse_trackers(&best), parse_trackers(&stable)])
}

/// Una lista de trackers con caché en disco de 12 h (si falla la bajada, la caché aunque sea vieja).
async fn tracker_source(http: &reqwest::Client, url: &str, cache: &Path, refresh: bool) -> String {
    let fresh = std::fs::metadata(cache)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < TRACKERS_REFRESH);
    if fresh && !refresh {
        if let Ok(t) = std::fs::read_to_string(cache) {
            return t;
        }
    }
    let fetched = async {
        let r = http.get(url).timeout(Duration::from_secs(4)).send().await?.error_for_status()?;
        r.text().await
    }
    .await;
    match fetched {
        Ok(t) if !parse_trackers(&t).is_empty() => {
            let _ = std::fs::write(cache, &t);
            t
        }
        other => {
            if let Err(e) = other {
                warn!("could not download the tracker list {url}, using cache: {e}");
            }
            std::fs::read_to_string(cache).unwrap_or_default()
        }
    }
}

/// Junta listas en orden de preferencia, sin repetidos, hasta `MAX_TRACKERS`.
pub fn merge_trackers(lists: &[Vec<String>]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in lists.iter().flatten() {
        if out.len() >= MAX_TRACKERS {
            break;
        }
        if seen.insert(t.trim_end_matches('/').to_ascii_lowercase()) {
            out.push(t.clone());
        }
    }
    out
}

pub fn parse_trackers(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("udp://") || l.starts_with("http://") || l.starts_with("https://"))
        .map(str::to_string)
        .collect()
}

async fn build_session(cfg: &MediaConfig, tuning: &Tuning, out: &Path, trackers: &[String]) -> anyhow::Result<Arc<Session>> {
    let tr: HashSet<url::Url> = trackers.iter().filter_map(|t| url::Url::parse(t).ok()).collect();
    // Dual-stack en el puerto fijo; si falla (sin IPv6 o puerto ocupado), IPv4; si no, efímero.
    let attempts = [(false, cfg.peer_port), (true, cfg.peer_port), (true, 0)];
    let mut last_err = None;
    for (ipv4_only, port) in attempts {
        let listen_addr: SocketAddr = if ipv4_only { (Ipv4Addr::UNSPECIFIED, port).into() } else { (Ipv6Addr::UNSPECIFIED, port).into() };
        let opts = SessionOptions {
            dht: Some(DhtSessionConfig {
                bootstrap_addrs: None,
                port: None,
                persistence: Some(DhtPersistenceConfig {
                    dump_interval: Some(Duration::from_secs(60)),
                    config_filename: Some(cfg.data_dir.join("dht.json")),
                }),
            }),
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json { folder: Some(cfg.data_dir.join("session")) }),
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpAndUtp,
                listen_addr,
                enable_upnp_port_forwarding: true,
                ipv4_only,
                ..Default::default()
            }),
            connect: Some(ConnectionOptions {
                peer_opts: Some(PeerConnectionOptions {
                    connect_timeout: Some(Duration::from_secs(4)),
                    read_write_timeout: Some(Duration::from_secs(20)),
                    keep_alive_interval: None,
                    max_request_window: Some(tuning.peer_request_window.max(1)),
                }),
                ..Default::default()
            }),
            runtime_worker_threads: Some(BLOCKING_PERMITS),
            concurrent_init_limit: Some(4),
            trackers: tr.clone(),
            peer_limit: Some(tuning.peer_limit.max(1)),
            ratelimits: librqbit::limits::LimitsConfig {
                download_bps: tuning.download_limit_bps.and_then(std::num::NonZeroU32::new),
                upload_bps: None,
            },
            ipv4_only,
            ..Default::default()
        };
        match Session::new_with_opts(out.to_path_buf(), opts).await {
            Ok(s) => {
                let announce = s.announce_port();
                info!(listen = ?s.listen_addr(), announce_port = ?announce, peer_limit = tuning.peer_limit, "torrent session ready");
                if cfg.peer_port != 0 && announce != Some(cfg.peer_port) {
                    warn!(wanted = cfg.peer_port, announce_port = ?announce, "not listening on the configured peer port: the router's port forward won't reach us (slower downloads)");
                }
                return Ok(s);
            }
            Err(e) => {
                warn!(ipv4_only, port, "torrent session failed to start: {e:#}");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("could not start the torrent session")))
}

/// Tareas periódicas: cerrar sesiones abandonadas, matar ffmpeg ocioso, pausar torrents sin uso,
/// borrar los vencidos, LRU y guardar el uso a disco.
async fn keeper(engine: Weak<Engine>, stop: CancellationToken) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    let mut n: u64 = 0;
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = stop.cancelled() => return,
        }

        let Some(e) = engine.upgrade() else { return };
        n += 1;
        let sessions: Vec<Arc<PlaySession>> = e.sessions.lock().unwrap().values().cloned().collect();
        for s in sessions {
            crate::hls::reap_idle(&s).await;
            if s.idle_secs() > SESSION_IDLE {
                e.close_session(&s.id).await;
            }
        }
        e.prefetching.lock().unwrap().retain(|_, until| *until > Instant::now());
        e.pause_idle_torrents().await;
        if n.is_multiple_of(TRACKERS_REFRESH.as_secs() / 5) {
            let w = engine.clone();
            tokio::spawn(async move {
                let Some(e) = w.upgrade() else { return };
                let fresh = load_trackers(&e.cfg, &e.http, true).await;
                let new: Vec<String> = fresh.into_iter().filter(|t| !e.trackers_boot.contains(t)).collect();
                info!(new = new.len(), "tracker list refreshed");
                *e.trackers_new.lock().unwrap() = new;
            });
        }
        if n % 12 == 1 {
            e.evict_expired().await;
            e.evict_lru(0).await;
        }
        e.usage.lock().unwrap().save_if_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_biggest_video() {
        let files = vec![
            ("Sample/sample.mkv".to_string(), 50, false),
            ("Movie.mkv".to_string(), 5000, false),
            ("Movie.nfo".to_string(), 9000, false),
            ("pad".to_string(), 99999, true),
        ];
        assert_eq!(choose_file(&files), Some(1));
        assert_eq!(choose_file(&[("a.txt".to_string(), 1, false), ("b.bin".to_string(), 2, false)]), Some(1));
        assert_eq!(choose_file(&[]), None);
    }

    #[test]
    fn tracker_merge() {
        let a = vec!["udp://a:1/announce".to_string(), "udp://b:1/announce".to_string()];
        let b = vec!["UDP://A:1/announce/".to_string(), "http://c/announce".to_string()];
        assert_eq!(merge_trackers(&[a, b]), vec!["udp://a:1/announce", "udp://b:1/announce", "http://c/announce"]);
        let many: Vec<String> = (0..200).map(|i| format!("udp://t{i}:1/announce")).collect();
        assert_eq!(merge_trackers(&[many]).len(), MAX_TRACKERS);
    }

    #[test]
    fn next_head_size() {
        const MB: u64 = 1 << 20;
        // 1080p de 1,5 GB: 750 KB/s supuestos × 90 s ≈ 64 MB.
        assert_eq!(next_head_bytes("Show.S01E02.1080p.WEB.mkv", 1_500_000_000), 67_500_000);
        // 4K: tope 128 MB; 720p chico: piso 48 MB; archivo más chico que el piso: el archivo.
        assert_eq!(next_head_bytes("Show.S01E02.2160p.mkv", 8_000_000_000), 128 * MB);
        assert_eq!(next_head_bytes("Show.S01E02.720p.mkv", 300_000_000), 48 * MB);
        assert_eq!(next_head_bytes("clip.mkv", 10 * MB), 10 * MB);
    }

    #[test]
    fn next_gate() {
        // Arranca con 5 min por delante o con todo bajado; ya prendida, aguanta hasta 150 s.
        assert!(!next_cushion_ok(false, 200.0, false));
        assert!(next_cushion_ok(false, 200.0, true));
        assert!(next_cushion_ok(false, 301.0, false));
        assert!(next_cushion_ok(true, 0.0, false));
        assert!(!next_cushion_ok(false, 100.0, true));
    }

    #[test]
    fn tracker_list() {
        let t = "udp://a:1/announce\n\nhttp://b/announce\nfoo\n  https://c/announce  \n";
        assert_eq!(parse_trackers(t), vec!["udp://a:1/announce", "http://b/announce", "https://c/announce"]);
    }
}
