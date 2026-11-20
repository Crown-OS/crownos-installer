//! Whether the machine can reach the package mirrors.

use std::time::Duration;

use crate::context::System;

const PROBE_ADDRESS: &str = "archlinux.org:443";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// True when a TCP connection to `archlinux.org:443` opens in 5 seconds.
pub fn is_online(system: &dyn System) -> bool {
    system.can_connect(PROBE_ADDRESS, PROBE_TIMEOUT)
}
