use anyhow::{Context as _, Result, bail};

const PROCESS_STATUS_PATH: &str = "/proc/self/status";

/// Fails unless the real user ID of the process is 0.
pub fn require_root() -> Result<()> {
    let process_status = std::fs::read_to_string(PROCESS_STATUS_PATH)
        .with_context(|| format!("could not read {PROCESS_STATUS_PATH}"))?;
    if real_user_id(&process_status) != Some(0) {
        bail!("this command needs root");
    }
    Ok(())
}

/// The first value of the `Uid:` line of `/proc/<pid>/status`.
fn real_user_id(process_status: &str) -> Option<u32> {
    process_status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_user_id_is_the_first_uid_value() {
        let status = "Name:\tcrownos-setup\nUid:\t1000\t0\t0\t0\nGid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(real_user_id(status), Some(1000));
        assert_eq!(real_user_id("Name:\tx\n"), None);
    }
}
