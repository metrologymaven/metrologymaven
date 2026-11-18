//! The verified-page cache, bounded in bytes (REQ-IO.CLOUD.E57.PHYS-004).

use std::collections::BTreeMap;

/// Default byte budget of the verified-page cache: 16 MiB.
pub const DEFAULT_CACHE_BUDGET: u64 = 16 * 1024 * 1024;

#[derive(Debug)]
struct Entry {
    tick: u64,
    data: Vec<u8>,
}

/// Verified page payloads keyed by page index, evicting the least recently used page.
/// The bytes held (sum of payload lengths) never exceed `budget_bytes`; a page larger than
/// the whole budget is simply not cached.
#[derive(Debug)]
pub struct PageCache {
    budget_bytes: u64,
    used: u64,
    tick: u64,
    pages: BTreeMap<u64, Entry>,
    /// Access tick → page index, oldest first.
    order: BTreeMap<u64, u64>,
}

impl PageCache {
    /// An empty cache with the given byte budget.
    pub fn new(budget_bytes: u64) -> PageCache {
        PageCache { budget_bytes, used: 0, tick: 0, pages: BTreeMap::new(), order: BTreeMap::new() }
    }

    /// The byte budget.
    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// Bytes currently held.
    pub fn used_bytes(&self) -> u64 {
        self.used
    }

    /// Number of cached pages.
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// True if no page is cached.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// True if page `index` is cached.
    pub fn contains(&self, index: u64) -> bool {
        self.pages.contains_key(&index)
    }

    fn next_tick(&mut self) -> u64 {
        // 2^64 accesses do not happen; if the counter ever wrapped, a colliding tick only
        // makes a page unevictable, and `insert` still refuses to exceed the budget.
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    /// The cached payload of page `index`, marking it most recently used.
    pub fn get(&mut self, index: u64) -> Option<&[u8]> {
        let tick = self.next_tick();
        let entry = self.pages.get_mut(&index)?;
        let old = std::mem::replace(&mut entry.tick, tick);
        self.order.remove(&old);
        self.order.insert(tick, index);
        Some(&entry.data)
    }

    /// The cached payload of page `index`, without touching its recency.
    pub fn peek(&self, index: u64) -> Option<&[u8]> {
        self.pages.get(&index).map(|e| e.data.as_slice())
    }

    fn remove(&mut self, index: u64) {
        if let Some(e) = self.pages.remove(&index) {
            self.order.remove(&e.tick);
            self.used = self.used.saturating_sub(e.data.len() as u64);
        }
    }

    /// Caches `data` as page `index`, evicting least recently used pages to stay within the
    /// budget. Returns false (and caches nothing) if `data` alone exceeds the budget.
    pub fn insert(&mut self, index: u64, data: Vec<u8>) -> bool {
        let len = data.len() as u64;
        if len > self.budget_bytes {
            return false;
        }
        self.remove(index);
        while self.used.saturating_add(len) > self.budget_bytes {
            match self.order.pop_first() {
                Some((_, victim)) => self.remove(victim),
                None => return false,
            }
        }
        let tick = self.next_tick();
        self.order.insert(tick, index);
        self.pages.insert(index, Entry { tick, data });
        self.used = self.used.saturating_add(len);
        true
    }

    /// Drops every cached page.
    pub fn clear(&mut self) {
        self.pages.clear();
        self.order.clear();
        self.used = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::PageCache;

    #[test]
    fn evicts_least_recently_used_and_stays_in_budget() {
        let mut c = PageCache::new(30);
        assert!(c.insert(1, vec![1; 10]));
        assert!(c.insert(2, vec![2; 10]));
        assert!(c.insert(3, vec![3; 10]));
        assert_eq!(c.used_bytes(), 30);
        assert!(c.get(1).is_some()); // 2 is now the oldest
        assert!(c.insert(4, vec![4; 10]));
        assert!(!c.contains(2));
        assert!(c.contains(1) && c.contains(3) && c.contains(4));
        assert!(c.used_bytes() <= 30);
        assert!(!c.insert(5, vec![5; 31]));
        assert_eq!(c.len(), 3);
    }
}
