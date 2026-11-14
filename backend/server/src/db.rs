//! SQLite (rusqlite) en `$MYTHOS_DATA/mythos.db`: una sola conexión protegida por un mutex y
//! usada desde `spawn_blocking` (para un uso personal sobra y es lo más liviano). Migraciones con
//! `PRAGMA user_version` al arrancar.
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use rusqlite::{Connection, OptionalExtension};

use crate::error::{ApiError, ApiResult};

/// Cada entrada es una migración; se aplican en orden y nunca se editan las ya publicadas.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    is_admin      INTEGER NOT NULL DEFAULT 0,
    disabled      INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,              -- sha256 hex del token, nunca el token
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,              -- unix s
    last_used  INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);
CREATE TABLE settings (
    user_id           INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    ui                TEXT NOT NULL DEFAULT '{}',
    preferred_quality TEXT,
    max_size_gb       REAL,
    rd_key            TEXT
);
CREATE TABLE progress (
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    meta_id       TEXT NOT NULL,
    type          TEXT NOT NULL,
    series_id     TEXT,
    name          TEXT NOT NULL,
    poster        TEXT,
    background    TEXT,
    episode_label TEXT,
    position_s    REAL NOT NULL,
    duration_s    REAL NOT NULL,
    stream_key    TEXT,
    updated_at    TEXT NOT NULL,
    PRIMARY KEY (user_id, meta_id)
);
CREATE INDEX progress_recent ON progress(user_id, updated_at DESC);
CREATE TABLE library (
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    type     TEXT NOT NULL,
    id       TEXT NOT NULL,
    data     TEXT NOT NULL,                   -- MetaPreview en JSON
    added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (user_id, type, id)
);
"#, r#"
CREATE INDEX progress_series ON progress(user_id, series_id) WHERE series_id IS NOT NULL;
"#, r#"
-- Sesiones largas: de qué dispositivo es cada una (para GET /api/me/sessions).
ALTER TABLE sessions ADD COLUMN device TEXT;
ALTER TABLE sessions ADD COLUMN user_agent TEXT;
ALTER TABLE sessions ADD COLUMN last_ip TEXT;

-- Sincronización: cada escritura de progreso/biblioteca/settings toma un número de secuencia
-- creciente (`seq`, el cursor de GET /api/sync?since=) y guarda `updated_ms` (reloj del
-- dispositivo que hizo el cambio, para last-write-wins). Los borrados quedan como lápidas
-- (`deleted = 1`). Lo que ya existía arranca con seq = 1 y la fecha que tenía.
CREATE TABLE sync_state (id INTEGER PRIMARY KEY CHECK (id = 1), seq INTEGER NOT NULL);
INSERT INTO sync_state (id, seq) VALUES (1, 1);
ALTER TABLE progress ADD COLUMN updated_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE progress ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;
ALTER TABLE progress ADD COLUMN seq INTEGER NOT NULL DEFAULT 1;
UPDATE progress SET updated_ms = COALESCE(CAST(ROUND((julianday(updated_at) - 2440587.5) * 86400000.0) AS INTEGER), 0);
CREATE INDEX progress_seq ON progress(user_id, seq);
ALTER TABLE library ADD COLUMN updated_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE library ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;
ALTER TABLE library ADD COLUMN seq INTEGER NOT NULL DEFAULT 1;
UPDATE library SET updated_ms = COALESCE(CAST(ROUND((julianday(added_at) - 2440587.5) * 86400000.0) AS INTEGER), 0);
CREATE INDEX library_seq ON library(user_id, seq);
ALTER TABLE settings ADD COLUMN updated_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE settings ADD COLUMN seq INTEGER NOT NULL DEFAULT 1;

-- Métricas (admin): requests crudos (~14 días) y agregados por hora (~90 días). `route` es la
-- plantilla (/api/media/{session}/file), nunca la URL real (que puede traer `?t=`).
CREATE TABLE request_log (
    id       INTEGER PRIMARY KEY,
    ts_ms    INTEGER NOT NULL,
    ip       TEXT,
    ts_login TEXT,                            -- Tailscale-User-Login, si vino por tailscale serve
    user_id  INTEGER,
    method   TEXT NOT NULL,
    route    TEXT NOT NULL,
    status   INTEGER NOT NULL,
    dur_ms   INTEGER NOT NULL,                -- hasta los headers de la respuesta
    bytes    INTEGER NOT NULL                 -- cuerpo enviado (aprox. en streams cortados)
);
CREATE INDEX request_log_ts ON request_log(ts_ms);
CREATE TABLE request_hourly (
    hour     INTEGER NOT NULL,                -- unix ms del comienzo de la hora
    method   TEXT NOT NULL,
    route    TEXT NOT NULL,
    status   INTEGER NOT NULL,
    user_id  INTEGER NOT NULL DEFAULT 0,      -- 0 = sin login
    ip       TEXT NOT NULL DEFAULT '',
    ts_login TEXT NOT NULL DEFAULT '',
    count    INTEGER NOT NULL,
    bytes    INTEGER NOT NULL,
    dur_sum  INTEGER NOT NULL,
    -- histograma de latencia (ver metrics::BUCKETS_MS)
    h0 INTEGER NOT NULL DEFAULT 0, h1 INTEGER NOT NULL DEFAULT 0, h2 INTEGER NOT NULL DEFAULT 0,
    h3 INTEGER NOT NULL DEFAULT 0, h4 INTEGER NOT NULL DEFAULT 0, h5 INTEGER NOT NULL DEFAULT 0,
    h6 INTEGER NOT NULL DEFAULT 0, h7 INTEGER NOT NULL DEFAULT 0, h8 INTEGER NOT NULL DEFAULT 0,
    h9 INTEGER NOT NULL DEFAULT 0, h10 INTEGER NOT NULL DEFAULT 0, h11 INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (hour, method, route, status, user_id, ip, ts_login)
);
CREATE TABLE audit (
    id       INTEGER PRIMARY KEY,
    ts_ms    INTEGER NOT NULL,
    event    TEXT NOT NULL,                   -- login_ok, login_fail, logout, user_created...
    user_id  INTEGER,
    username TEXT,
    ip       TEXT,
    detail   TEXT
);
CREATE INDEX audit_ts ON audit(ts_ms);

-- Modo local: a qué cuenta del server remoto corresponde cada cuenta local y cómo va la
-- sincronización. `remote_token` es una sesión del remoto (solo en este dispositivo).
CREATE TABLE local_link (
    user_id        INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    remote_url     TEXT NOT NULL,
    remote_token   TEXT,
    remote_user_id INTEGER,
    remote_cursor  INTEGER NOT NULL DEFAULT 0,   -- último `cursor` bajado del remoto
    pushed_seq     INTEGER NOT NULL DEFAULT 0,   -- último seq local ya subido
    last_sync_ms   INTEGER,
    last_error     TEXT
);
"#, r#"
-- Sesiones que no vencen (la app de TV: docs/API.md, "Sessions"). `expires_at` queda en
-- auth::NEVER_EXPIRES, asi todo lo que filtra por vencimiento sigue igual.
ALTER TABLE sessions ADD COLUMN persistent INTEGER NOT NULL DEFAULT 0;
"#, r#"
-- Avatares (avatars.rs): la imagen esta en <data_dir>/avatars/<id>.<ext>. AUTOINCREMENT: un id
-- nunca se reusa (la imagen se cachea para siempre por id).
CREATE TABLE avatars (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    mime       TEXT NOT NULL,
    ext        TEXT NOT NULL,
    created_at INTEGER NOT NULL                 -- unix s
);
ALTER TABLE users ADD COLUMN avatar_id INTEGER REFERENCES avatars(id) ON DELETE SET NULL;
"#, r#"
-- Datos sueltos de este server. `server_id`: identidad estable (aleatoria, se crea la primera vez)
-- que devuelve GET /api/health; las apps la comparan para saber que varias direcciones (Tailscale,
-- la red de casa) llevan al mismo server.
CREATE TABLE server_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- Modo local: el server_id del remoto vinculado. Cambiar a otra direccion del mismo server no
-- reinicia la sincronizacion (local.rs, save_link).
ALTER TABLE local_link ADD COLUMN remote_server_id TEXT;
"#, r#"
-- Streams que aprenden (taste.rs): cada reproducción, con el grupo del release, los idiomas reales
-- del archivo (ffprobe), si se eligió a mano y cómo salió. Se guardan las últimas 600 por usuario.
CREATE TABLE plays (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    meta_id     TEXT NOT NULL,                -- el episodio (tt:1:2) o la película
    series_id   TEXT NOT NULL,                -- la serie (o la película)
    ts_ms       INTEGER NOT NULL,
    stream_key  TEXT NOT NULL,
    grp         TEXT,                         -- grupo del release, en minúsculas
    binge       TEXT,                         -- bingeGroup de Torrentio
    quality     TEXT,
    size_bytes  INTEGER,
    dual_audio  INTEGER NOT NULL DEFAULT 0,
    audio_langs TEXT NOT NULL DEFAULT '',     -- idiomas reales, separados por coma
    sub_langs   TEXT NOT NULL DEFAULT '',
    manual      INTEGER NOT NULL DEFAULT 0,   -- elegido a mano (Choose stream, panel de streams)
    top_key     TEXT,                         -- el que la app ponía primero
    outcome     TEXT,                         -- NULL (en curso), finished, switched, stalled
    watched_s   REAL NOT NULL DEFAULT 0
);
CREATE INDEX plays_user ON plays(user_id, ts_ms DESC);
CREATE INDEX plays_meta ON plays(user_id, meta_id, ts_ms DESC);
"#, r#"
-- Desde dónde se empezó a ver en esa reproducción (el primer progreso; baja si se vuelve atrás):
-- `watched_s` pasa a ser lo visto en ESA reproducción y no la posición. Probado en la PC: al seguir
-- un episodio por la mitad y cambiar de stream al minuto, con la posición no contaba como
-- "switched" (ya pasaba los 5 min).
ALTER TABLE plays ADD COLUMN start_s REAL;
"#];

#[derive(Clone)]
pub struct Db {
    pub(crate) conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("no se pudo abrir {}", path.display()))?;
        Self::init(conn)
    }

    /// Base en memoria (tests).
    #[cfg(test)]
    pub fn open_in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql).with_context(|| format!("migración {} falló", i + 1))?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
            tracing::info!("migración de base de datos {} aplicada", i + 1);
        }
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Identidad de este server (`GET /api/health` → `server_id`): 16 bytes aleatorios en hex, creada
    /// la primera vez y guardada en la base (sobrevive a reinicios y actualizaciones; una base
    /// nueva es otro server).
    pub fn server_id(&self) -> anyhow::Result<String> {
        let c = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(id) = c.query_row("SELECT value FROM server_meta WHERE key = 'server_id'", [], |r| r.get::<_, String>(0)).optional()? {
            return Ok(id);
        }
        let mut buf = [0u8; 16];
        getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
        let id = crate::auth::hex(&buf);
        c.execute("INSERT OR IGNORE INTO server_meta (key, value) VALUES ('server_id', ?1)", [&id])?;
        Ok(c.query_row("SELECT value FROM server_meta WHERE key = 'server_id'", [], |r| r.get(0))?)
    }

    /// Corre `f` con la conexión en un hilo bloqueante.
    pub async fn call<T, F>(&self, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> ApiResult<T> + Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().unwrap_or_else(|p| p.into_inner());
            f(&mut guard)
        })
        .await
        .map_err(ApiError::internal)?
    }
}

pub fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Milisegundos unix (reloj de este dispositivo).
pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Siguiente número de secuencia de cambios (cursor de la sincronización). Llamarlo dentro de la
/// misma transacción que la escritura.
pub fn next_seq(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("UPDATE sync_state SET seq = seq + 1 WHERE id = 1 RETURNING seq", [], |r| r.get(0))
}

/// Último número de secuencia usado.
pub fn current_seq(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row("SELECT seq FROM sync_state WHERE id = 1", [], |r| r.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una base como la que hay hoy en el server (migraciones 1 y 2, con datos) migra en el lugar
    /// sin perder nada, y lo existente queda listo para sincronizar.
    #[test]
    fn migrates_v2_in_place() {
        let path = std::env::temp_dir().join(format!("mythos-mig-{}.db", now_ms()));
        {
            let c = Connection::open(&path).unwrap();
            for (i, sql) in MIGRATIONS.iter().take(2).enumerate() {
                c.execute_batch(sql).unwrap();
                c.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
            }
            c.execute_batch(
                "INSERT INTO users (id, username, password_hash) VALUES (1, 'ana', 'h');
                 INSERT INTO sessions (token_hash, user_id, created_at, last_used, expires_at) VALUES ('t', 1, 1, 1, 99999999999);
                 INSERT INTO settings (user_id, ui, rd_key) VALUES (1, '{\"theme\":\"dark\"}', 'KEY');
                 INSERT INTO progress (user_id, meta_id, type, name, position_s, duration_s, updated_at)
                   VALUES (1, 'tt1', 'movie', 'X', 10, 100, '2026-01-02T03:04:05.678Z');
                 INSERT INTO library (user_id, type, id, data, added_at) VALUES (1, 'movie', 'tt2', '{}', '2026-01-01T00:00:00.000Z');",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let c = db.conn.lock().unwrap();
        let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
        let (ms, del, seq): (i64, bool, i64) = c.query_row("SELECT updated_ms, deleted, seq FROM progress", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(ms, 1767323045678);
        assert!(!del);
        assert_eq!(seq, 1);
        let lib_ms: i64 = c.query_row("SELECT updated_ms FROM library", [], |r| r.get(0)).unwrap();
        assert_eq!(lib_ms, 1767225600000);
        let key: String = c.query_row("SELECT rd_key FROM settings", [], |r| r.get(0)).unwrap();
        assert_eq!(key, "KEY");
        let sessions: i64 = c.query_row("SELECT COUNT(*) FROM sessions WHERE device IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(sessions, 1, "las sesiones viejas siguen valiendo");
        assert_eq!(current_seq(&c).unwrap(), 1);
        assert_eq!(next_seq(&c).unwrap(), 2);
        drop(c);
        // Reabrir no vuelve a migrar, y la identidad del server queda igual.
        let id = db.server_id().unwrap();
        assert_eq!(id.len(), 32);
        assert_eq!(db.server_id().unwrap(), id);
        drop(db);
        let db = Db::open(&path).unwrap();
        assert_eq!(db.server_id().unwrap(), id, "sobrevive al reinicio");
        assert_ne!(Db::open_in_memory().unwrap().server_id().unwrap(), id, "otra base, otro server");
        drop(db);
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
    }
}
