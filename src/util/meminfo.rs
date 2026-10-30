//! `/proc/meminfo` parsing.

const KIB: u64 = 1024;

/// `MemTotal`, in bytes.
pub fn total_bytes(meminfo: &str) -> Option<u64> {
    meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|value| value.trim().strip_suffix("kB"))
        .and_then(|kib| kib.trim().parse::<u64>().ok())
        .map(|kib| kib * KIB)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_total() {
        let meminfo = "MemTotal:       16303372 kB\nMemFree:         1234 kB\n";
        assert_eq!(total_bytes(meminfo), Some(16_303_372 * KIB));
    }

    #[test]
    fn garbage_is_none() {
        assert_eq!(total_bytes("MemFree: 12 kB\n"), None);
    }
}
