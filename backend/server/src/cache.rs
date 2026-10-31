//! Caché en memoria con TTL y tamaño máximo (al llenarse se descartan primero los vencidos y
//! después los más viejos). Pensada para pocas centenas de entradas: catálogos, metas, streams.
use std::{
    collections::HashMap,
    hash::Hash,
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct TtlCache<K, V> {
    ttl: Duration,
    max: usize,
    map: Mutex<HashMap<K, (Instant, V)>>,
}

impl<K: Eq + Hash + Clone, V: Clone> TtlCache<K, V> {
    pub fn new(ttl: Duration, max: usize) -> Self {
        Self { ttl, max: max.max(1), map: Mutex::new(HashMap::new()) }
    }

    /// Valor vigente (no vencido).
    pub fn get(&self, k: &K) -> Option<V> {
        let map = self.map.lock().unwrap();
        map.get(k).filter(|(at, _)| at.elapsed() < self.ttl).map(|(_, v)| v.clone())
    }

    /// Valor aunque esté vencido: para responder algo cuando el upstream está caído. Los vencidos
    /// se descartan recién cuando hace falta lugar.
    pub fn get_stale(&self, k: &K) -> Option<V> {
        self.map.lock().unwrap().get(k).map(|(_, v)| v.clone())
    }

    pub fn insert(&self, k: K, v: V) {
        let mut map = self.map.lock().unwrap();
        if map.len() >= self.max && !map.contains_key(&k) {
            let ttl = self.ttl;
            map.retain(|_, (at, _)| at.elapsed() < ttl);
            // Si sigue lleno, se va el cuarto más viejo de una vez (evita ordenar en cada insert).
            if map.len() >= self.max {
                let mut ages: Vec<(Instant, K)> = map.iter().map(|(k, (at, _))| (*at, k.clone())).collect();
                ages.sort_by_key(|(at, _)| *at);
                for (_, k) in ages.into_iter().take((self.max / 4).max(1)) {
                    map.remove(&k);
                }
            }
        }
        map.insert(k, (Instant::now(), v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_and_bound() {
        let c = TtlCache::new(Duration::from_millis(50), 4);
        for i in 0..10 {
            c.insert(i, i * 10);
        }
        assert!(c.map.lock().unwrap().len() <= 4);
        assert_eq!(c.get(&9), Some(90));
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(c.get(&9), None);
        assert_eq!(c.get_stale(&9), Some(90));
    }
}
