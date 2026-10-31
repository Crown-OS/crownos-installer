//! The Arch Linux base CrownOS is layered on: how it and the CrownOS desktop get
//! onto the disk from the packages the live image bundles, how the installed
//! system is pointed at the CachyOS, Arch and CrownOS repositories for updates,
//! how locales and the kernel are handled.
//!
//! Every function returns commands and runs nothing, so the plan stays a value
//! and a dry run stays a faithful rehearsal.

use crate::oobe::RootFilesystem;
use crate::util::cpuinfo::Microarch;

use super::exec::Cmd;
use super::plan::ESP_CROWNOS_DIR;
use super::repos;

/// Where CrownOS packages are published.
pub const REPO_URL: &str = "https://repo.crownos.org";

/// The repository signing key, as the live image ships it.
pub const REPO_KEY_LIVE_PATH: &str = "/usr/share/crownos/crownos-archive-keyring.asc";

/// The signing key's user ID, which `pacman-key --lsign-key` accepts in place
/// of a fingerprint.
pub const REPO_KEY_UID: &str = "packages@crownos.org";

/// The meta-package that pulls in the compositor and every system app.
pub const DESKTOP_PACKAGE: &str = "crownos-desktop";

/// The live image's repository of every package [`packages`] can name, with its
/// dependencies. Its `CacheDir` is the repository itself, so `pacstrap -c`
/// installs straight from it with no network and no copy.
pub const OFFLINE_PACMAN_CONF: &str = "/etc/pacman.d/crownos-offline.conf";

/// The boot loader entry's name: `loader/entries/<this>.conf`.
pub const BOOT_ENTRY: &str = "crownos";

/// The group whose members may use `sudo`.
pub const ADMIN_GROUP: &str = "wheel";

/// Units enabled in the new system. CrownOS ships no display manager: signing
/// in on tty1 opens the desktop.
pub const SERVICES: &[&str] = &["NetworkManager.service", "systemd-timesyncd.service"];

const AUTOLOGIN_DROP_IN: &str = "/etc/systemd/system/getty@tty1.service.d/autologin.conf";

/// The kernel package, and the name of its image and initramfs.
pub const KERNEL: &str = "linux-cachyos";

/// Deliberately short: the set that makes the machine boot, reach the network
/// and let a user log in. The desktop comes from [`DESKTOP_PACKAGE`].
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
];

const BTRFS_PACKAGE: &str = "btrfs-progs";

const REPO_KEY_PATH: &str = "/etc/pacman.d/crownos-archive-keyring.asc";

const PACMAN_GNUPG_DIR: &str = "/etc/pacman.d/gnupg";

/// The packages `pacstrap` installs. crownos-iso's `profile/install.packages`
/// must name every one of them, or the live image cannot install offline.
pub fn packages(filesystem: RootFilesystem) -> Vec<String> {
    let btrfs = (filesystem == RootFilesystem::Btrfs).then_some(BTRFS_PACKAGE);
    BASE_PACKAGES
        .iter()
        .copied()
        .chain([KERNEL])
        .chain(repos::PACKAGES.iter().copied())
        .chain(btrfs)
        .chain([DESKTOP_PACKAGE])
        .map(str::to_owned)
        .collect()
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

/// Point the installed system at the CachyOS builds `microarch` runs, with
/// Arch behind them.
pub fn configure_repositories(root: &str, microarch: Microarch) -> Cmd {
    Cmd::WriteFile {
        path: format!("{root}/etc/pacman.conf"),
        contents: repos::pacman_conf(microarch),
    }
}

/// Trust the CrownOS repository, so the installed system updates from it.
/// Listed last, after the CachyOS and Arch repositories.
pub fn add_crownos_repository(root: &str) -> Vec<Cmd> {
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
                "\\n%s\\n%s\\n".to_owned(),
                "[crownos]".to_owned(),
                format!("Server = {REPO_URL}/arch/$arch"),
            ],
            path: format!("{root}/etc/pacman.conf"),
        },
    ]
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

/// The systemd-boot entry. systemd-boot finds Windows by itself, so there is
/// deliberately no Windows entry and nothing of Windows' to go stale.
pub fn boot_entry(root: &str, esp_rel: &str, kernel_cmdline: &str) -> Cmd {
    Cmd::WriteFile {
        path: format!("{root}{esp_rel}/loader/entries/{BOOT_ENTRY}.conf"),
        contents: format!(
            "title   CrownOS\n\
             linux   /{ESP_CROWNOS_DIR}/vmlinuz-{KERNEL}\n\
             initrd  /{ESP_CROWNOS_DIR}/initramfs-{KERNEL}.img\n\
             options {kernel_cmdline}\n"
        ),
    }
}

/// systemd-boot cannot read ext4 or btrfs, so the kernel and initramfs live on
/// the ESP. A preset pointing mkinitcpio straight at it means `pacman -Syu`
/// keeps them current instead of this being a one-time copy.
pub fn install_kernel(root: &str, esp_rel: &str) -> Vec<Cmd> {
    vec![
        Cmd::MakeDirs {
            path: format!("{root}{esp_rel}/{ESP_CROWNOS_DIR}"),
        },
        Cmd::WriteFile {
            path: format!("{root}/etc/mkinitcpio.d/{KERNEL}.preset"),
            contents: format!(
                "ALL_kver=\"/boot/vmlinuz-{KERNEL}\"\n\
                 PRESETS=('default')\n\
                 default_image=\"{esp_rel}/{ESP_CROWNOS_DIR}/initramfs-{KERNEL}.img\"\n"
            ),
        },
        Cmd::chroot(root, "mkinitcpio", &["-P"]),
        Cmd::chroot(
            root,
            "cp",
            &[
                "--",
                &format!("/boot/vmlinuz-{KERNEL}"),
                &format!("{esp_rel}/{ESP_CROWNOS_DIR}/vmlinuz-{KERNEL}"),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btrfs_pulls_in_its_tools_and_ext4_does_not() {
        assert!(
            packages(RootFilesystem::Btrfs)
                .iter()
                .any(|p| p == BTRFS_PACKAGE)
        );
        assert!(
            !packages(RootFilesystem::Ext4)
                .iter()
                .any(|p| p == BTRFS_PACKAGE)
        );
    }

    #[test]
    fn the_desktop_is_installed_from_the_bundled_packages() {
        assert!(
            packages(RootFilesystem::Ext4)
                .iter()
                .any(|p| p == DESKTOP_PACKAGE)
        );
        let Cmd::Run { program, args } = bootstrap("/mnt", &[]) else {
            panic!("pacstrap runs in the live system");
        };
        assert_eq!(program, "pacstrap");
        assert_eq!(args, ["-C", OFFLINE_PACMAN_CONF, "-c", "-K", "/mnt"]);
    }

    #[test]
    fn the_installed_system_updates_from_the_crownos_repository() {
        let log: String = add_crownos_repository("/mnt")
            .iter()
            .map(|command| format!("{command}\n"))
            .collect();
        for expected in [REPO_URL, REPO_KEY_LIVE_PATH, "/mnt/etc/pacman.conf"] {
            assert!(log.contains(expected), "{expected} missing from {log}");
        }
        assert!(!log.contains("pacman -S"), "nothing is downloaded: {log}");
    }

    #[test]
    fn the_boot_entry_lives_where_the_loader_default_points() {
        let Cmd::WriteFile { path, contents } = boot_entry("/mnt", "/efi", "root=PARTUUID=x rw")
        else {
            panic!("the boot entry is a file");
        };
        assert_eq!(path, format!("/mnt/efi/loader/entries/{BOOT_ENTRY}.conf"));
        assert!(contents.ends_with("options root=PARTUUID=x rw\n"));
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
