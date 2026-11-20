//! The root filesystem: btrfs, laid out in subvolumes so snapper can snapshot
//! the system without the user's files, logs or package cache, and so a
//! rollback is one `btrfs subvolume set-default`.
//!
//! Every function returns commands and runs nothing.

use super::exec::Cmd;

pub const FILESYSTEM_NAME: &str = "btrfs";

/// For every mount of the filesystem. btrfs applies these per filesystem, so
/// the first mount decides them for all of its subvolumes.
pub const MOUNT_OPTIONS: &str = "compress=zstd,noatime";

/// The system itself, and the filesystem's default subvolume. Nothing names it
/// at boot: the kernel command line and the fstab root line mount whatever is
/// the default, which is what a rollback changes.
pub const ROOT_SUBVOLUME: &str = "@";

/// Inside `@swap`, which is excluded from snapshots and copy-on-write.
pub const SWAPFILE: &str = "/swap/swapfile";

pub struct Subvolume {
    pub name: &'static str,
    pub mount_point: &'static str,
}

/// Everything kept out of the root's snapshots, so rolling the system back
/// never rolls back the user's files, the logs that explain the failure, or
/// the packages needed to update again.
pub const SUBVOLUMES: &[Subvolume] = &[
    Subvolume {
        name: "@home",
        mount_point: "/home",
    },
    Subvolume {
        name: "@snapshots",
        mount_point: "/.snapshots",
    },
    Subvolume {
        name: "@log",
        mount_point: "/var/log",
    },
    Subvolume {
        name: "@pkg",
        mount_point: "/var/cache/pacman/pkg",
    },
    Subvolume {
        name: SWAP_SUBVOLUME,
        mount_point: "/swap",
    },
];

const SWAP_SUBVOLUME: &str = "@swap";

const TOP_LEVEL_MOUNT: &str = "/run/crownos-installer/btrfs-top";

const GROW_MOUNT: &str = "/run/crownos-installer/grow";

/// Create every subvolume on a freshly made filesystem and make `@` its
/// default. `@swap` is marked no-copy-on-write while it is still empty, which
/// is the only time the flag takes effect on what is created inside it.
pub fn create_subvolumes(device: &str) -> Vec<Cmd> {
    let mut commands = vec![
        Cmd::MakeDirs {
            path: TOP_LEVEL_MOUNT.to_owned(),
        },
        Cmd::run("mount", &[device, TOP_LEVEL_MOUNT]),
    ];
    commands.extend(
        std::iter::once(ROOT_SUBVOLUME)
            .chain(SUBVOLUMES.iter().map(|subvolume| subvolume.name))
            .map(|name| Cmd::run("btrfs", &["subvolume", "create", &top_level_path(name)])),
    );
    commands.extend([
        Cmd::run("chattr", &["+C", &top_level_path(SWAP_SUBVOLUME)]),
        Cmd::run(
            "btrfs",
            &["subvolume", "set-default", &top_level_path(ROOT_SUBVOLUME)],
        ),
        Cmd::run("umount", &[TOP_LEVEL_MOUNT]),
    ]);
    commands
}

fn top_level_path(name: &str) -> String {
    format!("{TOP_LEVEL_MOUNT}/{name}")
}

/// Mount the default subvolume at `root` and every other one beneath it.
pub fn mount_all(device: &str, root: &str) -> Vec<Cmd> {
    let mut commands = vec![
        Cmd::MakeDirs {
            path: root.to_owned(),
        },
        Cmd::run("mount", &["-o", MOUNT_OPTIONS, device, root]),
    ];
    for subvolume in SUBVOLUMES {
        let at = format!("{root}{}", subvolume.mount_point);
        commands.extend([
            Cmd::MakeDirs { path: at.clone() },
            Cmd::run(
                "mount",
                &[
                    "-o",
                    &format!("{MOUNT_OPTIONS},subvol={}", subvolume.name),
                    device,
                    &at,
                ],
            ),
        ]);
    }
    commands.push(Cmd::run("chmod", &["750", &format!("{root}/.snapshots")]));
    commands
}

/// A swapfile in `@swap`. btrfs needs its own tool for one: a plain file on
/// btrfs is not contiguous and the kernel refuses to swap to it.
pub fn swapfile(root: &str, size_bytes: u64) -> Vec<Cmd> {
    let megabytes = (size_bytes / (1024 * 1024)).max(1);
    vec![
        Cmd::chroot(
            root,
            "btrfs",
            &[
                "filesystem",
                "mkswapfile",
                "--size",
                &format!("{megabytes}m"),
                SWAPFILE,
            ],
        ),
        Cmd::RunAppendingTo {
            program: "printf".to_owned(),
            args: vec![
                "%s\\n".to_owned(),
                format!("{SWAPFILE} none swap defaults 0 0"),
            ],
            path: format!("{root}/etc/fstab"),
            filter: None,
        },
    ]
}

/// Grow the filesystem to fill its partition. btrfs only resizes mounted, so
/// it is mounted on its own for the purpose, after the install unmounted it.
pub fn grow(device: &str) -> Vec<Cmd> {
    vec![
        Cmd::MakeDirs {
            path: GROW_MOUNT.to_owned(),
        },
        Cmd::run("mount", &[device, GROW_MOUNT]),
        Cmd::run("btrfs", &["filesystem", "resize", "max", GROW_MOUNT]),
        Cmd::run("umount", &[GROW_MOUNT]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(commands: &[Cmd]) -> Vec<String> {
        commands.iter().map(ToString::to_string).collect()
    }

    fn position(lines: &[String], needle: &str) -> usize {
        lines
            .iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("`{needle}` missing from {lines:?}"))
    }

    #[test]
    fn every_subvolume_exists_before_the_root_becomes_the_default() {
        let log = lines(&create_subvolumes("/dev/sda6"));
        let set_default = position(&log, "set-default");
        for name in std::iter::once(ROOT_SUBVOLUME).chain(SUBVOLUMES.iter().map(|s| s.name)) {
            let create = position(&log, &format!("subvolume create {TOP_LEVEL_MOUNT}/{name}"));
            assert!(create < set_default, "{log:?}");
        }
        assert!(log[set_default].ends_with(&format!("{TOP_LEVEL_MOUNT}/@")));
        assert!(set_default < position(&log, "umount"), "{log:?}");
        assert!(position(&log, "chattr +C") < set_default, "{log:?}");
    }

    #[test]
    fn the_root_mounts_the_default_and_every_mount_is_compressed() {
        let log = lines(&mount_all("/dev/sda6", "/mnt"));
        let mounts: Vec<&String> = log.iter().filter(|l| l.starts_with("mount ")).collect();
        assert_eq!(mounts.len(), SUBVOLUMES.len() + 1);
        assert_eq!(mounts[0], "mount -o compress=zstd,noatime /dev/sda6 /mnt");
        for mount in &mounts {
            assert!(mount.contains(MOUNT_OPTIONS), "{mount}");
        }
        assert!(
            log.iter().any(|l| l
                == "mount -o compress=zstd,noatime,subvol=@snapshots /dev/sda6 /mnt/.snapshots")
        );
    }

    #[test]
    fn growing_mounts_resizes_and_unmounts_in_that_order() {
        let log = lines(&grow("/dev/sda6"));
        let mount = position(&log, "mount /dev/sda6");
        let resize = position(&log, "btrfs filesystem resize max");
        let umount = position(&log, "umount");
        assert!(mount < resize && resize < umount, "{log:?}");
    }

    #[test]
    fn the_swapfile_lives_in_its_own_subvolume() {
        let log = lines(&swapfile("/mnt", 8 * 1024 * 1024 * 1024));
        assert_eq!(
            log[0],
            "arch-chroot /mnt btrfs filesystem mkswapfile --size 8192m /swap/swapfile"
        );
        assert!(log[1].contains("/swap/swapfile none swap defaults 0 0"));
        assert!(log[1].ends_with(">> /mnt/etc/fstab"));
    }
}
