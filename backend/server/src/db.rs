//! SQLite (rusqlite) en `$MYTHOS_DATA/mythos.db`: una sola conexión protegida por un mutex y
//! usada desde `spawn_blocking` (para un uso personal sobra y es lo más liviano). Migraciones con
//! `PRAGMA user_version` al arrancar.
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use rusqlite::Connection;

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
