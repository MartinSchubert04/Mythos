//! Caché en disco: cuándo se usó cada torrent (persistido en `data_dir/usage.json`), elección LRU
//! de qué borrar cuando se pasa de `cache_limit_bytes`, y espacio libre del disco.
//!
//! Regla de oro: nunca se borra lo que está en uso (sesión abierta, precarga en curso o usado en
//! los últimos minutos), aunque eso deje la caché por encima del límite un rato.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default, Serialize, Deserialize)]
pub struct Usage {
    /// info_hash → último uso (segundos Unix).
    pub last_used: HashMap<String, u64>,
    #[serde(skip)]
    dirty: bool,
    #[serde(skip)]
    path: PathBuf,
}

impl Usage {
    pub fn load(path: PathBuf) -> Self {
        let mut u: Usage = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        u.path = path;
        u
    }

    pub fn touch(&mut self, hash: &str, now: u64) {
        self.last_used.insert(hash.to_string(), now);
        self.dirty = true;
    }

    pub fn forget(&mut self, hash: &str) {
        self.last_used.remove(hash);
        self.dirty = true;
    }

    pub fn get(&self, hash: &str) -> Option<u64> {
        self.last_used.get(hash).copied()
    }

    /// Escribe a disco si cambió (archivo temporal + rename: nunca queda a medias).
    pub fn save_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        if let Ok(b) = serde_json::to_vec(&self) {
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, b).is_ok() && std::fs::rename(&tmp, &self.path).is_ok() {
                self.dirty = false;
            }
        }
    }
}

/// Un torrent candidato a borrar.
#[derive(Clone, Debug)]
pub struct Entry {
    pub hash: String,
    pub bytes: u64,
    pub last_used: u64,
    pub in_use: bool,
}

/// Qué torrents borrar (los menos usados primero, nunca los en uso) para que la caché, sumando
/// `extra_needed` bytes que se van a bajar, quede por debajo de `limit`.
pub fn pick_evictions(entries: &[Entry], limit: u64, extra_needed: u64) -> Vec<String> {
    let total: u64 = entries.iter().map(|e| e.bytes).sum::<u64>() + extra_needed;
    if total <= limit {
        return Vec::new();
    }
    let mut cands: Vec<&Entry> = entries.iter().filter(|e| !e.in_use).collect();
    cands.sort_by_key(|e| e.last_used);
    let mut over = total - limit;
    let mut out = Vec::new();
    for e in cands {
        if over == 0 {
            break;
        }
        out.push(e.hash.clone());
        over = over.saturating_sub(e.bytes);
    }
    out
}

/// Qué torrents vencieron: sin usar hace `max_age` segundos o más (nunca los en uso).
pub fn pick_expired(entries: &[Entry], now: u64, max_age: u64) -> Vec<String> {
    if max_age == 0 {
        return Vec::new();
    }
    entries.iter().filter(|e| !e.in_use && now.saturating_sub(e.last_used) >= max_age).map(|e| e.hash.clone()).collect()
}

/// Espacio libre del disco que contiene `path`.
#[cfg(unix)]
pub fn disk_free(path: &Path) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return 0 };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` es un C string válido y `st` un buffer del tamaño correcto.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return 0;
    }
    (st.f_bavail as u64).saturating_mul(st.f_frsize as u64)
}

#[cfg(windows)]
pub fn disk_free(path: &Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free: u64 = 0;
    // SAFETY: `wide` termina en 0 y `free` es un u64 válido para escribir.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut())
    };
    if ok == 0 {
        0
    } else {
        free
    }
}

#[cfg(not(any(unix, windows)))]
pub fn disk_free(_path: &Path) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(h: &str, bytes: u64, last: u64, in_use: bool) -> Entry {
        Entry { hash: h.into(), bytes, last_used: last, in_use }
    }

    #[test]
    fn lru_picks_oldest_not_in_use() {
        let es = vec![e("a", 50, 10, false), e("b", 50, 5, true), e("c", 30, 1, false), e("d", 20, 20, false)];
        // Total 150, límite 100: sobran 50 → c (más viejo, 30) y después a (siguiente libre).
        assert_eq!(pick_evictions(&es, 100, 0), vec!["c".to_string(), "a".to_string()]);
        // Debajo del límite: nada.
        assert!(pick_evictions(&es, 200, 0).is_empty());
        // Con lo que se va a bajar se pasa: sobran 10 → c.
        assert_eq!(pick_evictions(&es, 150, 10), vec!["c".to_string()]);
        // Si todo lo que sobra está en uso, no se borra nada en uso.
        let busy = vec![e("x", 500, 1, true)];
        assert!(pick_evictions(&busy, 100, 0).is_empty());
    }

    #[test]
    fn expired_after_max_age() {
        let day = 86_400;
        let now = 10 * day;
        let es = vec![e("old", 10, now - day, false), e("fresh", 10, now - day + 1, false), e("busy", 10, 0, true)];
        // Justo 24 h: vence. Un segundo menos: no. En uso: nunca.
        assert_eq!(pick_expired(&es, now, day), vec!["old".to_string()]);
        // Sin vencimiento configurado: nada.
        assert!(pick_expired(&es, now, 0).is_empty());
    }

    #[test]
    fn usage_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mythos-usage-{}", crate::util::random_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("usage.json");
        let mut u = Usage::load(p.clone());
        u.touch("abc", 42);
        u.save_if_dirty();
        let u2 = Usage::load(p);
        assert_eq!(u2.get("abc"), Some(42));
        let _ = std::fs::remove_dir_all(dir);
        assert!(disk_free(&std::env::temp_dir()) > 0);
    }
}
