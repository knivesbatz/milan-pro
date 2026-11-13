//! Byte-budgeted LRU for decoded source frames and composited frames.
use std::collections::HashMap;
use std::hash::Hash;

pub struct ByteLru<K, V> {
    map: HashMap<K, (V, usize, u64)>,
    budget: usize,
    used: usize,
    tick: u64,
    /// R19: lookups through `get` that found / missed their entry, for the playback statistics.
    pub hits: u64,
    pub misses: u64,
}

impl<K: Hash + Eq + Clone, V: Clone> ByteLru<K, V> {
    pub fn new(budget: usize) -> Self {
        Self { map: HashMap::new(), budget, used: 0, tick: 0, hits: 0, misses: 0 }
    }

    pub fn get(&mut self, k: &K) -> Option<V> {
        let v = self.touch(k);
        if v.is_some() {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        v
    }

    /// `get` without counting it as a lookup (pre-warm checks).
    pub fn touch(&mut self, k: &K) -> Option<V> {
        self.tick += 1;
        let e = self.map.get_mut(k)?;
        e.2 = self.tick;
        Some(e.0.clone())
    }

    /// Insert `v` (`bytes` large), evicting the least recently used entries beyond the budget.
    /// No pinning: a still is read every frame its event plays, so it stays recent while it is needed.
    pub fn insert(&mut self, k: K, v: V, bytes: usize) {
        self.tick += 1;
        if let Some(old) = self.map.insert(k, (v, bytes, self.tick)) {
            self.used -= old.1;
        }
        self.used += bytes;
        while self.used > self.budget {
            // O(n) scan: a few hundred entries at most.
            let Some(old) = self.map.iter().min_by_key(|e| e.1.2).map(|e| e.0.clone()) else { break };
            self.used -= self.map.remove(&old).map_or(0, |e| e.1);
        }
    }

    pub fn clear(&mut self) {
        (self.map, self.used) = (HashMap::new(), 0);
    }

    #[cfg(test)]
    pub fn used(&self) -> usize {
        self.used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_under_budget_least_recent_first() {
        let mut c = ByteLru::new(100);
        c.insert("still", 0, 30);
        for i in 1..20 {
            assert_eq!(c.get(&"still"), Some(0)); // read every frame, like a playing still: never evicted
            c.insert(if i % 2 == 0 { "even" } else { "odd" }, i, 1);
            c.insert(["a", "b", "c", "d", "e"][i % 5], i, 20);
            assert!(c.used() <= 100, "{}", c.used());
        }
        // Recently used entries survive; the least recently used goes first.
        assert!(c.get(&"c").is_some() && c.get(&"a").is_none());
        c.insert("f", 99, 20);
        assert!(c.get(&"c").is_some() && c.get(&"f").is_some() && c.get(&"d").is_none());
        assert!(c.used() <= 100);
        // An unused still is evicted like anything else, so memory stays bounded.
        for k in ["g", "h", "i", "j", "k"] {
            c.insert(k, 0, 20);
        }
        assert!(c.get(&"still").is_none() && c.used() <= 100);
        // R19: lookups are counted, pre-warm touches are not.
        let (h, m) = (c.hits, c.misses);
        c.touch(&"k");
        c.get(&"k");
        c.get(&"nope");
        assert_eq!((c.hits, c.misses), (h + 1, m + 1));
        c.clear();
        assert_eq!(c.used(), 0);
    }
}
