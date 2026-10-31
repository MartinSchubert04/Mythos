//! Motor: sesión de librqbit, sesiones de reproducción, precarga, caché LRU y tareas de fondo.
//!
//! Decisiones para conseguir peers rápido y no trabarse:
//! - **DHT persistente** (`data_dir/dht.json`): tras un reinicio el bootstrap es inmediato.
//! - **Sesión persistente con fastresume** (`data_dir/session`): los torrents y lo bajado vuelven
//!   sin re-verificar todo el archivo.
//! - **Metadata cacheada** (`data_dir/meta/{hash}.torrent`): reabrir algo ya visto no espera a
//!   resolver el magnet, aunque el torrent se haya borrado por LRU.
//! - **Trackers extra**: `trackers_best.txt` de ngosang (caché en disco 12 h, timeout 4 s) más una
//!   lista fija mínima, más los trackers que vengan del stream.
//! - **Puerto fijo** TCP + uTP (`peer_port`) con UPnP si el router lo permite; si el puerto está
//!   ocupado o no hay IPv6, se reintenta (IPv4 solo / puerto efímero) en vez de no arrancar.
//! - **Límites generosos**: 160 peers por torrent, timeouts de conexión cortos (4 s) para descartar
//!   peers muertos rápido, y `max_request_window` 32 (512 KB por peer) para que una pieza urgente
//!   (tras un salto) no quede detrás de 2 MB de pedidos viejos.
//! - **Solo el archivo elegido** (`only_files`).
//! - **Torrents vivos un rato** después de cerrar la sesión (15 min si nadie más mira; 60 s si hay
//!   otra película en curso, para no robarle ancho de banda) y después en pausa, no borrados.
//! - **Precarga** (`prefetch`): principio y final del archivo sin crear sesión.
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
use crate::{ClientCaps, MediaConfig, MediaState, MediaStats, PlayInfo, PlayMode, Source, TorrentStat, Track, Tuning};
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

const TRACKERS_URL: &str = "https://raw.githubusercontent.com/ngosang/trackerslist/master/trackers_best.txt";
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
    started_unix: u64,
    pub hls_dir: PathBuf,
    pub subs_dir: PathBuf,
    meta_dir: PathBuf,
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
        let http = reqwest::Client::builder()
            .user_agent(concat!("mythos/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        let trackers = load_trackers(&cfg, &http).await;
        info!(trackers = trackers.len(), "starting torrent session");
        let rq = build_session(&cfg, &tuning, &torrents_dir, &trackers).await?;
        let engine = Arc::new(Self {
            api: Api::new(rq.clone(), None),
            rq,
            http,
            sessions: Mutex::new(HashMap::new()),
            internal: OnceLock::new(),
            usage: Mutex::new(Usage::load(d.join("usage.json"))),
            add_locks: Mutex::new(HashMap::new()),
            prefetching: Mutex::new(HashMap::new()),
            started_unix: now_unix(),
            tuning,
            hls_dir,
            subs_dir,
            meta_dir,
            cfg,
        });
        // Endpoint interno para ffmpeg/ffprobe (127.0.0.1, puerto efímero).
        let l = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let _ = engine.internal.set(l.local_addr()?);
        info!(addr = %l.local_addr()?, "internal media endpoint (ffmpeg/ffprobe)");
        let router = crate::http::internal_router(engine.clone());
        tokio::spawn(async move {
            if let Err(e) = axum::serve(l, router).await {
                warn!("internal media endpoint stopped: {e:#}");
            }
        });
        tokio::spawn(keeper(Arc::downgrade(&engine)));
        Ok(engine)
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
        s
    }

    fn in_use(&self, hash: &str) -> bool {
        let now = Instant::now();
        self.sessions.lock().unwrap().values().any(|x| x.torrent().is_some_and(|t| t.hash == hash))
            || self.prefetching.lock().unwrap().iter().any(|(k, until)| k.0 == hash && *until > now)
    }

    /// Agrega (o retoma) el torrent, se asegura de que el archivo esté seleccionado y vivo.
    async fn ensure_torrent(&self, hash: &str, file_idx: Option<usize>, trackers: &[String]) -> anyhow::Result<(ManagedTorrentHandle, usize)> {
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
        self.touch_usage(hash);
        Ok((handle, idx))
    }

    async fn add_torrent(&self, hash: &str, file_idx: Option<usize>, trackers: &[String]) -> anyhow::Result<ManagedTorrentHandle> {
        let meta_path = self.meta_dir.join(format!("{hash}.torrent"));
        let custom: Vec<String> = trackers.iter().filter(|t| t.contains("://")).cloned().collect();
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
                    .map_err(|_| anyhow::anyhow!("no peers found for this torrent (gave up after {} s); try another stream", METADATA_TIMEOUT.as_secs()))??;
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
                let (handle, idx) = self.ensure_torrent(&hash, file_idx, &trackers).await?;
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
                let (size, name) = crate::stream::url_meta(&self.http, &url).await.context("could not open the link")?;
                let sess = Arc::new(PlaySession::new(id.clone(), Backing::Url(url), name, size, tmp));
                self.sessions.lock().unwrap().insert(id.clone(), sess.clone());
                sess
            }
        };
        sess.set_state(MediaState::Probing);
        let t_src = t0.elapsed();
        let p = match probe::run(&self.cfg.ffprobe, &self.internal_url(&id, Role::Probe), &sess.file_name, PROBE_TIMEOUT).await {
            Ok(p) => p,
            Err(e) => {
                warn!(file = %sess.file_name, "ffprobe failed, guessing from the name: {e:#}");
                probe::guess(&sess.file_name)
            }
        };
        sess.set_duration(p.duration, p.bit_rate);
        let info = build_info(&id, &sess, &p, &caps);
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
        let res = self.ensure_torrent(hash, file_idx, trackers).await;
        self.prefetching.lock().unwrap().remove(&(hash.to_string(), key_idx));
        let (handle, idx) = res?;
        let until = Instant::now() + PREFETCH_MAX;
        self.prefetching.lock().unwrap().insert((hash.to_string(), idx), until);
        let (tf, _) = self.torrent_file(&handle, hash, idx)?;
        let g = tf.geom;
        let head_len = PREFETCH_HEAD.min(g.len);
        let tail_pos = g.len - window::tail_bytes(g.len);
        // Dos fantasmas: principio (24 MB) y final (índices). Se sueltan al completarse.
        let mut head = window::Phantom::open(&handle, idx, 0).await;
        let mut tail = window::Phantom::open(&handle, idx, tail_pos).await;
        while Instant::now() < until && (head.is_some() || tail.is_some()) {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let Some(hv) = window::haves(&self.api, handle.info_hash()) else { continue };
            let have = |p: usize| hv.get(p).copied().unwrap_or(false);
            if window::missing_in(&g, &have, 0, head_len) == 0 {
                head = None;
            }
            if window::missing_in(&g, &have, tail_pos, g.len - tail_pos) == 0 {
                tail = None;
            }
        }
        drop((head, tail));
        self.prefetching.lock().unwrap().remove(&(hash.to_string(), idx));
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
                    s.set_error(DISK_FULL_MSG.into());
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
            let grace = if someone_watching { KEEP_ALIVE_BUSY } else { KEEP_ALIVE_IDLE };
            if now.saturating_sub(last) >= grace {
                info!(%hash, "pausing idle torrent");
                let _ = self.rq.pause(&h).await;
            }
        }
    }
}

fn build_info(id: &str, sess: &PlaySession, p: &Probe, caps: &ClientCaps) -> PlayInfo {
    let mode = probe::choose_mode(p, caps);
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
        subtitle_tracks: p.subs.iter().filter(|s| text_subs.contains(&s.codec.as_str())).cloned().collect::<Vec<Track>>(),
    }
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

async fn load_trackers(cfg: &MediaConfig, http: &reqwest::Client) -> Vec<String> {
    if !cfg.extra_trackers {
        return Vec::new();
    }
    let cache = cfg.data_dir.join("trackers_best.txt");
    let fresh = std::fs::metadata(&cache)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < Duration::from_secs(12 * 3600));
    let mut text = if fresh { std::fs::read_to_string(&cache).ok() } else { None };
    if text.is_none() {
        let fetched = async {
            let r = http.get(TRACKERS_URL).timeout(Duration::from_secs(4)).send().await?.error_for_status()?;
            r.text().await
        }
        .await;
        match fetched {
            Ok(t) => {
                let _ = std::fs::write(&cache, &t);
                text = Some(t);
            }
            Err(e) => {
                warn!("could not download the tracker list, using cache: {e}");
                text = std::fs::read_to_string(&cache).ok();
            }
        }
    }
    let mut out: Vec<String> = FALLBACK_TRACKERS.iter().map(|s| s.to_string()).collect();
    out.extend(parse_trackers(text.as_deref().unwrap_or("")));
    out.sort();
    out.dedup();
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
                info!(listen = ?s.listen_addr(), "torrent session ready");
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
/// LRU y guardar el uso a disco.
async fn keeper(engine: Weak<Engine>) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    let mut n: u64 = 0;
    loop {
        tick.tick().await;
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
        if n % 12 == 1 {
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
    fn tracker_list() {
        let t = "udp://a:1/announce\n\nhttp://b/announce\nfoo\n  https://c/announce  \n";
        assert_eq!(parse_trackers(t), vec!["udp://a:1/announce", "http://b/announce", "https://c/announce"]);
    }
}
