//! Short-lived snapshots with one loader shared by concurrent readers.

use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Entry<T> {
    value: T,
    expires: Instant,
}

pub struct SnapshotCache<T> {
    state: Mutex<(u64, Option<Entry<T>>)>,
    loader: Mutex<()>,
    ttl: Duration,
}

impl<T: Clone> SnapshotCache<T> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            state: Mutex::new((0, None)),
            loader: Mutex::new(()),
            ttl,
        }
    }

    pub fn invalidate(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.0 = state.0.wrapping_add(1);
        state.1 = None;
    }

    /// Failed/partial loads are returned but not retained. Invalidation during
    /// a load prevents that old result from becoming the next reader's cache.
    pub fn get_or_load(&self, load: impl FnOnce() -> T, cacheable: impl FnOnce(&T) -> bool) -> T {
        let _loader = self.loader.lock().unwrap_or_else(|e| e.into_inner());
        let generation = {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = &state.1 {
                if Instant::now() < entry.expires {
                    return entry.value.clone();
                }
            }
            state.0
        };
        let value = load();
        if cacheable(&value) {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.0 == generation {
                state.1 = Some(Entry {
                    value: value.clone(),
                    expires: Instant::now() + self.ttl,
                });
            }
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    #[test]
    fn concurrent_readers_share_one_load() {
        let cache = Arc::new(SnapshotCache::new(Duration::from_secs(30)));
        let calls = AtomicUsize::new(0);
        let barrier = Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let cache = cache.clone();
                let calls = &calls;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    assert_eq!(
                        cache.get_or_load(
                            || {
                                calls.fetch_add(1, Ordering::SeqCst);
                                42
                            },
                            |_| true
                        ),
                        42
                    );
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn invalidation_during_load_does_not_publish_stale_data() {
        let cache = SnapshotCache::new(Duration::from_secs(30));
        assert_eq!(
            cache.get_or_load(
                || {
                    cache.invalidate();
                    1
                },
                |_| true
            ),
            1
        );
        assert_eq!(cache.get_or_load(|| 2, |_| true), 2);
        cache.invalidate();
        assert_eq!(cache.get_or_load(|| 3, |_| true), 3);
    }

    #[test]
    fn partial_results_and_expired_entries_are_retried() {
        let cache = SnapshotCache::new(Duration::from_secs(30));
        assert_eq!(cache.get_or_load(|| 1, |_| false), 1);
        assert_eq!(cache.get_or_load(|| 2, |_| true), 2);
        let expired = SnapshotCache::new(Duration::ZERO);
        expired.get_or_load(|| 1, |_| true);
        assert_eq!(expired.get_or_load(|| 2, |_| true), 2);
    }
}
