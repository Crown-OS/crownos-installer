//! The installed system's `/etc/pacman.conf`.
//!
//! CachyOS rebuilds come first and Arch's `core` and `extra` stay behind them
//! for everything CachyOS does not rebuild. The live image bundles generic
//! x86-64 packages so it installs on any CPU; on one that runs a more specific
//! target its rebuilds are listed first, and the first `pacman -Syu` moves
//! every package onto them, because CachyOS release numbers sort above Arch's.

use crate::util::cpuinfo::Microarch;

const OPTIONS: &str = "[options]
HoldPkg = pacman glibc
CheckSpace
ParallelDownloads = 5
DownloadUser = alpm
SigLevel = Required DatabaseOptional
LocalFileSigLevel = Optional
";

const CACHYOS_MIRRORLIST: &str = "/etc/pacman.d/cachyos-mirrorlist";
const ARCH_MIRRORLIST: &str = "/etc/pacman.d/mirrorlist";

/// The packages that put the mirrorlists and the CachyOS signing key in place.
pub const PACKAGES: &[&str] = &[
    "cachyos-keyring",
    "cachyos-mirrorlist",
    "cachyos-v3-mirrorlist",
    "cachyos-v4-mirrorlist",
];

/// The repositories rebuilt for one target, and the mirrorlist serving them.
struct Rebuilds {
    architectures: &'static str,
    repositories: &'static [&'static str],
    mirrorlist: &'static str,
}

/// `x86_64` comes first in every list: it is what `$arch` expands to.
fn rebuilds(microarch: Microarch) -> Rebuilds {
    match microarch {
        Microarch::Generic => Rebuilds {
            architectures: "x86_64",
            repositories: &[],
            mirrorlist: CACHYOS_MIRRORLIST,
        },
        Microarch::V3 => Rebuilds {
            architectures: "x86_64 x86_64_v3",
            repositories: &["cachyos-v3", "cachyos-core-v3", "cachyos-extra-v3"],
            mirrorlist: "/etc/pacman.d/cachyos-v3-mirrorlist",
        },
        Microarch::Znver4 => Rebuilds {
            architectures: "x86_64 x86_64_v3 x86_64_v4",
            repositories: &[
                "cachyos-znver4",
                "cachyos-core-znver4",
                "cachyos-extra-znver4",
            ],
            mirrorlist: "/etc/pacman.d/cachyos-v4-mirrorlist",
        },
    }
}

/// The whole file, replacing the one the `pacman` package installed.
pub fn pacman_conf(microarch: Microarch) -> String {
    let rebuilds = rebuilds(microarch);
    let repositories: String = rebuilds
        .repositories
        .iter()
        .map(|&repo| (repo, rebuilds.mirrorlist))
        .chain([
            ("cachyos", CACHYOS_MIRRORLIST),
            ("core", ARCH_MIRRORLIST),
            ("extra", ARCH_MIRRORLIST),
        ])
        .map(|(repo, mirrorlist)| format!("\n[{repo}]\nInclude = {mirrorlist}\n"))
        .collect();
    format!(
        "{OPTIONS}Architecture = {}\n{repositories}",
        rebuilds.architectures
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repositories(conf: &str) -> Vec<&str> {
        conf.lines()
            .filter_map(|line| line.strip_prefix('[')?.strip_suffix(']'))
            .filter(|&section| section != "options")
            .collect()
    }

    #[test]
    fn a_generic_cpu_gets_generic_builds_only() {
        let conf = pacman_conf(Microarch::Generic);
        assert!(conf.contains("Architecture = x86_64\n"));
        assert_eq!(repositories(&conf), ["cachyos", "core", "extra"]);
    }

    #[test]
    fn a_v3_cpu_prefers_the_v3_rebuilds() {
        let conf = pacman_conf(Microarch::V3);
        assert!(conf.contains("Architecture = x86_64 x86_64_v3\n"));
        assert_eq!(
            repositories(&conf),
            [
                "cachyos-v3",
                "cachyos-core-v3",
                "cachyos-extra-v3",
                "cachyos",
                "core",
                "extra"
            ]
        );
    }

    #[test]
    fn zen4_prefers_the_znver4_rebuilds() {
        let conf = pacman_conf(Microarch::Znver4);
        assert!(conf.contains("Architecture = x86_64 x86_64_v3 x86_64_v4\n"));
        assert!(conf.contains("[cachyos-znver4]\nInclude = /etc/pacman.d/cachyos-v4-mirrorlist\n"));
        assert_eq!(
            repositories(&conf),
            [
                "cachyos-znver4",
                "cachyos-core-znver4",
                "cachyos-extra-znver4",
                "cachyos",
                "core",
                "extra"
            ]
        );
    }
}
