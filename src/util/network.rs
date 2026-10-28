use std::fs;

/// The kernel's IPv4 routing table, one route per line after a header.
const ROUTE_TABLE: &str = "/proc/net/route";
/// How the table spells the destination of a default route.
const DEFAULT_DESTINATION: &str = "00000000";

/// Whether any interface has a default route — the cheap, synchronous "is this
/// machine probably online" that is good enough to decide which page opens first.
pub fn has_default_route() -> bool {
    fs::read_to_string(ROUTE_TABLE).is_ok_and(|table| {
        table
            .lines()
            .skip(1)
            .any(|route| route.split_whitespace().nth(1) == Some(DEFAULT_DESTINATION))
    })
}
