//! Counting distinct visitors.
//!
//! Prometheus cannot answer this one. It stores aggregates, and "how many
//! different people" is a question about the size of a set - so the counting has
//! to happen here, where individual requests are still visible, and only the
//! number goes out.
//!
//! Two ways to hold the set. In this process, exactly, which is right for one
//! replica and costs a few megabytes; or in Redis as a HyperLogLog, which is a
//! few kilobytes a day whatever the traffic, is shared across replicas, and
//! survives a restart. Redis is used when it is configured and reachable, and
//! the local set takes over the moment it is not, so a broken cache costs
//! accuracy rather than the whole feature.

use crate::ident::{day_label, Day};
use std::collections::{BTreeMap, HashSet};

/// How many distinct visitors a day is counted exactly before giving up.
///
/// Five million hashes would be about 160MB; this keeps the ceiling in sight.
const LOCAL_CAP: usize = 250_000;

/// How many days of counts are kept and exported.
const DAYS_KEPT: usize = 3;

/// The distinct visitors of the last few days.
pub struct Visitors {
    local: BTreeMap<Day, HashSet<String>>,
    redis: Option<redis::Client>,
    /// Whether Redis answered the last time it was asked.
    store_up: bool,
    saturated: bool,
}

impl Visitors {
    /// A counter, using Redis when a URL is given and it can be reached.
    pub fn new(redis_url: Option<&str>) -> Self {
        let redis = redis_url.and_then(|url| match redis::Client::open(url) {
            Ok(client) => Some(client),
            Err(error) => {
                eprintln!("beacon: redis url rejected, counting locally instead: {error}");
                None
            }
        });
        Self {
            local: BTreeMap::new(),
            store_up: redis.is_some(),
            redis,
            saturated: false,
        }
    }

    /// Whether a shared store is configured at all.
    pub fn shared(&self) -> bool {
        self.redis.is_some()
    }

    /// Whether the shared store answered last time, or none is configured.
    pub fn store_up(&self) -> bool {
        self.redis.is_none() || self.store_up
    }

    /// Whether any day has stopped being exact.
    pub fn saturated(&self) -> bool {
        self.saturated
    }

    /// Records one visitor for one day.
    pub fn see(&mut self, day: Day, fingerprint: &str) {
        if let Some(client) = &self.redis {
            match add_to_redis(client, day, fingerprint) {
                Ok(()) => {
                    self.store_up = true;
                    return;
                }
                Err(error) => {
                    if self.store_up {
                        eprintln!("beacon: redis unreachable, counting locally instead: {error}");
                    }
                    self.store_up = false;
                }
            }
        }
        let set = self.local.entry(day).or_default();
        if set.len() >= LOCAL_CAP {
            self.saturated = true;
            return;
        }
        set.insert(fingerprint.to_owned());
        self.forget_old(day);
    }

    /// The counts to export, by date.
    pub fn counts(&mut self, today: Day) -> BTreeMap<String, u64> {
        let mut counts = BTreeMap::new();
        for offset in 0..DAYS_KEPT as u64 {
            let day = today.saturating_sub(offset);
            let label = day_label(day);
            let count = self
                .redis
                .as_ref()
                .and_then(|client| count_in_redis(client, day).ok())
                .or_else(|| self.local.get(&day).map(|set| set.len() as u64))
                .unwrap_or(0);
            counts.insert(label, count);
        }
        counts
    }

    /// Drops days that are no longer exported, so memory does not creep.
    fn forget_old(&mut self, today: Day) {
        let oldest = today.saturating_sub(DAYS_KEPT as u64);
        self.local.retain(|day, _| *day >= oldest);
    }
}

/// The Redis key a day's visitors live under.
fn key(day: Day) -> String {
    format!("kongzilla:visitors:{}", day_label(day))
}

fn add_to_redis(client: &redis::Client, day: Day, fingerprint: &str) -> redis::RedisResult<()> {
    let mut connection = client.get_connection()?;
    let key = key(day);
    redis::pipe()
        .cmd("PFADD")
        .arg(&key)
        .arg(fingerprint)
        .ignore()
        // A week is long enough to look back at and short enough to forget.
        .cmd("EXPIRE")
        .arg(&key)
        .arg(8 * 86_400)
        .ignore()
        .query::<()>(&mut connection)
}

fn count_in_redis(client: &redis::Client, day: Day) -> redis::RedisResult<u64> {
    let mut connection = client.get_connection()?;
    redis::cmd("PFCOUNT").arg(key(day)).query(&mut connection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ident::today;

    #[test]
    fn the_same_visitor_counts_once() {
        let mut visitors = Visitors::new(None);
        let day = today();
        visitors.see(day, "aaaa");
        visitors.see(day, "aaaa");
        visitors.see(day, "bbbb");
        assert_eq!(visitors.counts(day)[&day_label(day)], 2);
    }

    #[test]
    fn days_are_counted_apart_and_old_ones_are_forgotten() {
        let mut visitors = Visitors::new(None);
        let day = today();
        visitors.see(day - 1, "aaaa");
        visitors.see(day, "bbbb");
        let counts = visitors.counts(day);
        assert_eq!(counts[&day_label(day)], 1);
        assert_eq!(counts[&day_label(day - 1)], 1);
        assert_eq!(counts.len(), DAYS_KEPT);

        // Rolling far enough forward drops what is no longer exported.
        visitors.see(day + 10, "cccc");
        assert!(visitors.local.len() <= DAYS_KEPT + 1);
    }

    #[test]
    fn without_redis_the_store_reads_as_up_rather_than_broken() {
        let visitors = Visitors::new(None);
        assert!(!visitors.shared());
        assert!(visitors.store_up(), "no store is not a broken store");
        assert!(!visitors.saturated());
    }
}
