//! Who a visitor is, without knowing who a visitor is.
//!
//! Counting people twice is the whole problem with visit statistics, and the
//! usual fixes - a cookie, a stored identifier - turn a counter into a personal
//! data store. This takes the address and the browser string, salts them with a
//! secret that is generated at start-up and thrown away every night, and keeps
//! only the hash. Two requests from the same person on the same day collide;
//! tomorrow's salt makes yesterday's hashes meaningless, and nothing anywhere
//! can be turned back into an address.

use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// A day, as days since the epoch. The unit every count is bucketed by.
pub type Day = u64;

/// The current UTC day.
pub fn today() -> Day {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() / 86_400)
        .unwrap_or(0)
}

/// A day as `YYYY-MM-DD`, for Redis keys and for reading in a dashboard.
pub fn day_label(day: Day) -> String {
    // Civil-from-days, Howard Hinnant's algorithm. No date crate for one line.
    let z = day as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day_of_month = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day_of_month:02}")
}

/// The secret a day's hashes are salted with.
///
/// Random at start-up and replaced whenever the day rolls over, so the only
/// thing that survives midnight is a number.
pub struct Salt {
    day: Day,
    bytes: [u8; 32],
}

impl Salt {
    /// A salt for the current day.
    pub fn new() -> Self {
        Self {
            day: today(),
            bytes: random_bytes(),
        }
    }

    /// The salt for `day`, rotating it first if the day has moved on.
    pub fn for_day(&mut self, day: Day) -> &[u8; 32] {
        if day != self.day {
            self.day = day;
            self.bytes = random_bytes();
        }
        &self.bytes
    }
}

impl Default for Salt {
    fn default() -> Self {
        Self::new()
    }
}

/// The identifier a visitor is counted under, as hex.
pub fn fingerprint(salt: &[u8; 32], address: &str, agent: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(address.as_bytes());
    hasher.update([0]);
    hasher.update(agent.as_bytes());
    let digest = hasher.finalize();
    // Half a SHA-256 is far more than enough to keep a day's visitors apart, and
    // half as much to hold on to.
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Entropy for a salt, from the operating system.
fn random_bytes() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        if file.read_exact(&mut bytes).is_ok() {
            return bytes;
        }
    }
    // Without a random device the salt is worth less, but a predictable salt is
    // still better than none: it never leaves the process either way.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or(0);
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = ((now >> (index % 16)) as u8) ^ (index as u8).wrapping_mul(31);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_reads_as_a_date() {
        assert_eq!(day_label(0), "1970-01-01");
        assert_eq!(day_label(19_000), "2022-01-08");
        assert_eq!(day_label(20_714), "2026-09-18");
    }

    #[test]
    fn the_same_visitor_hashes_the_same_and_a_different_one_does_not() {
        let salt = [7u8; 32];
        let one = fingerprint(&salt, "203.0.113.7", "Firefox");
        assert_eq!(one, fingerprint(&salt, "203.0.113.7", "Firefox"));
        assert_ne!(one, fingerprint(&salt, "203.0.113.8", "Firefox"));
        assert_ne!(one, fingerprint(&salt, "203.0.113.7", "Chrome"));
        assert_eq!(one.len(), 32);
    }

    #[test]
    fn a_new_day_makes_yesterdays_hashes_meaningless() {
        let mut salt = Salt::new();
        let today = today();
        let before = fingerprint(salt.for_day(today), "203.0.113.7", "Firefox");
        let after = fingerprint(salt.for_day(today + 1), "203.0.113.7", "Firefox");
        assert_ne!(before, after, "the salt did not rotate");
    }
}
