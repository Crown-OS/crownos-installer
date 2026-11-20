use std::time::{SystemTime, UNIX_EPOCH};

/// The current time in Unix seconds, or 0 when the clock is before 1970.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs())
}
