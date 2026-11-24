//! The Arch Linux base CrownOS is layered on: how it and the CrownOS desktop get
//! onto the disk from the packages the live image bundles, how the installed
//! system is pointed at the CachyOS, Arch and CrownOS repositories for updates,
//! and how locales are handled. The kernel and boot loader are [`boot`](super::boot)'s.
//!
//! Every function returns commands and runs nothing, so the plan stays a value
//! and a dry run stays a faithful rehearsal.

use crate::util::cpuinfo::Microarch;

use super::exec::Cmd;
use super::repos;

/// Where CrownOS packages are published.
pub const REPO_URL: &str = "https://repo.crownos.org";

/// The repository signing key, as the live image ships it.
pub const REPO_KEY_LIVE_PATH: &str = "/usr/share/crownos/crownos-archive-keyring.asc";

/// The signing key's user ID, which `pacman-key --lsign-key` accepts in place
/// of a fingerprint.
pub const REPO_KEY_UID: &str = "packages@crownos.org";

/// The live image's copy of the published manifest. Its `packages` array is
/// the complete set of CrownOS components an installed system must have.
pub const MANIFEST_LIVE_PATH: &str = "/usr/share/crownos/manifest.toml";

/// Installed whatever the manifest says: the snapshot stack, the boot loader,
/// the system defaults with the boot recovery hooks, the release identity and
/// the key the repository is signed with.
pub const REQUIRED_CROWNOS: &[&str] = &[
    "snapper",
    "snap-pac",
    "refind",
    "crownos-base",
    "crownos-release",
    "crownos-keyring",
];

/// The live image's repository of every package [`packages`] can name, with its
/// dependencies. Its `CacheDir` is the repository itself, so `pacstrap -c`
/// installs straight from it with no network and no copy.
pub const OFFLINE_PACMAN_CONF: &str = "/etc/pacman.d/crownos-offline.conf";

/// The group whose members may use `sudo`.
pub const ADMIN_GROUP: &str = "wheel";

/// The distribution default of which units are on, shipped by crownos-base.
/// crownos-updated enables the units of lines that a later release adds.
pub const CROWNOS_PRESET: &str = "/usr/lib/systemd/system-preset/80-crownos.preset";

/// The units of [`CROWNOS_PRESET`] that are already applied, read by
/// crownos-updated, so the first update does not enable them again.
pub const APPLIED_PRESETS_RECORD: &str = "/var/lib/crownos/update/applied-presets";

/// The units enabled when the installed crownos-base has no
/// [`CROWNOS_PRESET`], as in 0.3.0 and earlier. CrownOS ships no display
/// manager: signing in on tty1 opens the desktop.
pub const SERVICES: &[&str] = &[
    "NetworkManager.service",
    "systemd-timesyncd.service",
    "power-profiles-daemon.service",
    "snapper-cleanup.timer",
    "crownos-updated.service",
];

/// Installs Wine on the first boot that is online. Shipped by crownos-base.
pub const WINDOWS_APPS_SERVICE: &str = "crownos-windows-apps.service";

/// Compressed swap in RAM, as a share of physical memory. Settings resizes it
/// through `crownos-memory`, which edits only the `zram-size` line.
pub const ZRAM_PERCENT: u8 = 50;

const ZRAM_CONFIG: &str = "/etc/systemd/zram-generator.conf";

/// Swapping to zram costs a compression rather than a disk write, so the
/// kernel is told to prefer it to dropping page cache, and to read single
/// pages back instead of the clusters a disk wants.
const ZRAM_SYSCTL: &str = "/etc/sysctl.d/90-crownos-zram.conf";

const AUTOLOGIN_DROP_IN: &str = "/etc/systemd/system/getty@tty1.service.d/autologin.conf";

/// The kernel package, and the name of its image and initramfs.
pub const KERNEL: &str = "linux-cachyos";

/// Deliberately short: the set that makes the machine boot, reach the network
/// and let a user log in. The desktop comes from the published manifest.
const BASE_PACKAGES: &[&str] = &[
    "base",
    "linux-firmware",
    "mkinitcpio",
    "sudo",
    "networkmanager",
    "efibootmgr",
    "dosfstools",
    "e2fsprogs",
    "nano",
    "zram-generator",
];

const BTRFS_PACKAGE: &str = "btrfs-progs";

const REPO_KEY_PATH: &str = "/etc/pacman.d/crownos-archive-keyring.asc";

const PACMAN_GNUPG_DIR: &str = "/etc/pacman.d/gnupg";

/// The packages `pacstrap` installs: the base, and every CrownOS package the
/// manifest `published`. crownos-iso's `profile/install.packages` must name
/// every one of them, or the live image cannot install offline.
pub fn packages(published: &[String]) -> Vec<String> {
    let mut packages: Vec<String> = BASE_PACKAGES
        .iter()
        .chain(&[KERNEL, BTRFS_PACKAGE])
        .chain(repos::PACKAGES)
        .chain(REQUIRED_CROWNOS)
        .map(|name| (*name).to_owned())
        .chain(published.iter().cloned())
        .collect();
    packages.sort_unstable();
    packages.dedup();
    packages
}

/// Put the base system and the desktop into `root`, from the live image's
/// bundled packages.
pub fn bootstrap(root: &str, packages: &[String]) -> Cmd {
    let mut args = Vec::with_capacity(packages.len() + 5);
    args.extend(["-C", OFFLINE_PACMAN_CONF, "-c", "-K", root].map(str::to_owned));
    args.extend_from_slice(packages);
    Cmd::Run {
        program: "pacstrap".to_owned(),
        args,
    }
}

/// Stop the GnuPG daemons that populating the keyrings in `root` left running
/// out of it, which would otherwise keep it from being unmounted.
pub fn stop_keyring_daemons(root: &str) -> Cmd {
    Cmd::run(
        "gpgconf",
        &[
            "--homedir",
            &format!("{root}{PACMAN_GNUPG_DIR}"),
            "--kill",
            "all",
        ],
    )
}

/// zram swap at `percent` of RAM, ahead of the swap partition.
pub fn compressed_memory(root: &str, percent: u8) -> Vec<Cmd> {
    vec![
        Cmd::WriteFile {
            path: format!("{root}{ZRAM_CONFIG}"),
            contents: format!(
                "[zram0]\nzram-size = ram * {percent} / 100\ncompression-algorithm = zstd\nswap-priority = 100\n"
            ),
        },
        Cmd::WriteFile {
            path: format!("{root}{ZRAM_SYSCTL}"),
            contents: "vm.swappiness = 100\nvm.page-cluster = 0\n".to_owned(),
        },
    ]
}

/// Point the installed system at the CachyOS builds `microarch` runs, with
/// Arch behind them.
pub fn configure_repositories(root: &str, microarch: Microarch) -> Cmd {
    Cmd::WriteFile {
        path: format!("{root}/etc/pacman.conf"),
        contents: repos::pacman_conf(microarch),
    }
}

/// Trust the CrownOS repository, so the installed system updates from it.
/// Listed last, after the CachyOS and Arch repositories, and pinned to
/// `channel`, which serves only the release this image installs. A plain
/// `pacman -Syu` therefore never skips a release; crownos-updated moves the
/// pin one release at a time.
pub fn add_crownos_repository(root: &str, channel: &str) -> Vec<Cmd> {
    vec![
        Cmd::run(
            "install",
            &[
                "-Dm644",
                REPO_KEY_LIVE_PATH,
                &format!("{root}{REPO_KEY_PATH}"),
            ],
        ),
        Cmd::chroot(root, "pacman-key", &["--add", REPO_KEY_PATH]),
        Cmd::chroot(root, "pacman-key", &["--lsign-key", REPO_KEY_UID]),
        Cmd::RunAppendingTo {
            program: "printf".to_owned(),
            args: vec![
                "\\n%s\\n%s\\n%s\\n".to_owned(),
                "[crownos]".to_owned(),
                "SigLevel = Required DatabaseOptional".to_owned(),
                format!("Server = {REPO_URL}/{channel}/$arch"),
            ],
            path: format!("{root}/etc/pacman.conf"),
            filter: None,
        },
    ]
}

/// `systemctl preset` for the units of [`CROWNOS_PRESET`] in `root`, or
/// `systemctl enable` for [`SERVICES`] when the file is missing, and the
/// record of them for crownos-updated.
pub fn enable_preset_services(root: &str) -> Cmd {
    Cmd::PresetUnits {
        root: root.to_owned(),
        preset: CROWNOS_PRESET.to_owned(),
        fallback_units: SERVICES.iter().map(|unit| (*unit).to_owned()).collect(),
        record: APPLIED_PRESETS_RECORD.to_owned(),
    }
}

/// Sign `username` in on tty1 at boot without a password.
pub fn autologin(root: &str, username: &str) -> Cmd {
    Cmd::WriteFile {
        path: format!("{root}{AUTOLOGIN_DROP_IN}"),
        contents: format!(
            "[Service]\n\
             ExecStart=\n\
             ExecStart=-/usr/bin/agetty -o '-p -f -- \\\\u' --noclear --autologin {username} %I $TERM\n"
        ),
    }
}

/// Generate and select `locale`.
pub fn configure_locale(root: &str, locale: &str) -> Vec<Cmd> {
    vec![
        Cmd::WriteFile {
            path: format!("{root}/etc/locale.gen"),
            contents: format!("{locale} UTF-8\n"),
        },
        Cmd::chroot(root, "locale-gen", &[]),
        Cmd::WriteFile {
            path: format!("{root}/etc/locale.conf"),
            contents: format!("LANG={locale}\n"),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_install_gets_the_recovery_stack_and_the_published_packages() {
        let published = ["crownbar".to_owned(), "crownbar".to_owned()];
        let packages = packages(&published);
        for expected in REQUIRED_CROWNOS.iter().chain(&[BTRFS_PACKAGE, KERNEL]) {
            assert!(packages.iter().any(|p| p == expected), "{expected} missing");
        }
        assert_eq!(packages.iter().filter(|p| *p == "crownbar").count(), 1);
        assert!(!packages.iter().any(|p| p == "crownos-desktop"));
        assert!(packages.is_sorted());
    }

    #[test]
    fn pacstrap_installs_offline_from_the_bundled_packages() {
        let Cmd::Run { program, args } = bootstrap("/mnt", &[]) else {
            panic!("pacstrap runs in the live system");
        };
        assert_eq!(program, "pacstrap");
        assert_eq!(args, ["-C", OFFLINE_PACMAN_CONF, "-c", "-K", "/mnt"]);
    }

    #[test]
    fn the_installed_system_updates_from_the_crownos_repository() {
        let log: String = add_crownos_repository("/mnt", "v0.4.2")
            .iter()
            .map(|command| format!("{command}\n"))
            .collect();
        for expected in [
            REPO_URL,
            REPO_KEY_LIVE_PATH,
            "/mnt/etc/pacman.conf",
            "SigLevel = Required DatabaseOptional",
            "Server = https://repo.crownos.org/v0.4.2/$arch",
            "printf \\n%s\\n%s\\n%s\\n [crownos]",
        ] {
            assert!(log.contains(expected), "{expected} missing from {log}");
        }
        assert!(!log.contains("pacman -S"), "nothing is downloaded: {log}");
    }

    #[test]
    fn zram_size_is_the_line_crownos_memory_edits() {
        let Some(Cmd::WriteFile { path, contents }) =
            compressed_memory("/mnt", 50).into_iter().next()
        else {
            panic!("the zram config is a file");
        };
        assert_eq!(path, format!("/mnt{ZRAM_CONFIG}"));
        assert!(
            contents
                .lines()
                .any(|line| line == "zram-size = ram * 50 / 100")
        );
    }

    #[test]
    fn autologin_replaces_the_tty1_getty_command() {
        let Cmd::WriteFile { path, contents } = autologin("/mnt", "crown") else {
            panic!("autologin is a drop-in file");
        };
        assert_eq!(path, format!("/mnt{AUTOLOGIN_DROP_IN}"));
        assert!(contents.contains("ExecStart=\n"));
        assert!(contents.contains("--autologin crown %I $TERM\n"));
    }

    #[test]
    fn the_installed_system_updates_from_cachyos_and_arch() {
        let Cmd::WriteFile { path, contents } = configure_repositories("/mnt", Microarch::V3)
        else {
            panic!("pacman.conf is a file");
        };
        assert_eq!(path, "/mnt/etc/pacman.conf");
        assert_eq!(contents, repos::pacman_conf(Microarch::V3));
    }
}
