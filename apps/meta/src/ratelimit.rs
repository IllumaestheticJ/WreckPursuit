//! In-memory token-bucket rate limiter. Per-process: behind several replicas the effective
//! limit multiplies by the replica count (see README "Known gaps").

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::clock::UnixMs;
use crate::config::Limit;

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_ms: UnixMs,
}

/// Token buckets keyed by `(group, key)`.
#[derive(Debug, Default)]
pub struct RateLimiter {
    buckets: Mutex<BTreeMap<(&'static str, String), Bucket>>,
}

impl RateLimiter {
    /// Take one token from the bucket `(group, key)`. `false` = over the limit.
    pub fn check(&self, group: &'static str, key: &str, limit: Limit, now_ms: UnixMs) -> bool {
        let mut map = self.buckets.lock().unwrap_or_else(|p| p.into_inner());
        // Keep memory bounded: drop full buckets when the map grows large.
        if map.len() > 100_000 {
            map.retain(|_, b| b.tokens < limit.burst);
        }
        let b = map.entry((group, key.to_string())).or_insert(Bucket {
            tokens: limit.burst,
            last_ms: now_ms,
        });
        let elapsed_min = (now_ms - b.last_ms).max(0) as f64 / 60_000.0;
        b.tokens = (b.tokens + elapsed_min * limit.per_minute).min(limit.burst);
        b.last_ms = now_ms;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_refills() {
        let rl = RateLimiter::default();
        let l = Limit {
            burst: 2.0,
            per_minute: 1.0,
        };
        assert!(rl.check("g", "k", l, 0));
        assert!(rl.check("g", "k", l, 0));
        assert!(!rl.check("g", "k", l, 0));
        assert!(rl.check("g", "other", l, 0), "keys are independent");
        assert!(!rl.check("g", "k", l, 30_000));
        assert!(rl.check("g", "k", l, 61_000));
    }
}
