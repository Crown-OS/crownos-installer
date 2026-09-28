use crate::engine::exec::Cmd;
use crate::engine::plan::ESP_CROWNOS_DIR;
use crate::oobe::RootFilesystem;

use super::{
    BaseSystem, DESKTOP_PACKAGE, REPO_KEY_UID, REPO_URL, copy_repo_key, with_filesystem_tools,
};

pub struct Arch;

/// Deliberately short: the set that makes the machine boot, reach the network
/// and let a user log in. The desktop comes from [`DESKTOP_PACKAGE`].
const BASE_PACKAGES: &[&str] = &[
    "base",
    "linux",
    "linux-firmware",
    "mkinitcpio",
    "sudo",
    "networkmanager",
    "efibootmgr",
    "dosfstools",
    "e2fsprogs",
    "nano",
];

const REPO_KEY_PATH: &str = "/etc/pacman.d/crownos-archive-keyring.asc";

impl BaseSystem for Arch {
    fn packages(&self, filesystem: RootFilesystem, _locale: &str) -> Vec<String> {
        with_filesystem_tools(BASE_PACKAGES, filesystem)
    }

    fn bootstrap(&self, root: &str, packages: &[String]) -> Vec<Cmd> {
        let mut args = vec!["-K".to_owned(), root.to_owned()];
        args.extend_from_slice(packages);
        vec![Cmd::Run {
            program: "pacstrap".to_owned(),
            args,
        }]
    }

    fn install_desktop(&self, root: &str) -> Vec<Cmd> {
        vec![
            copy_repo_key(root, REPO_KEY_PATH),
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
            Cmd::chroot(root, "pacman", &["-Sy", "--noconfirm", DESKTOP_PACKAGE]),
        ]
    }

    fn configure_locale(&self, root: &str, locale: &str) -> Vec<Cmd> {
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

    /// systemd-boot finds Windows by itself, so there is deliberately no Windows
    /// entry here and nothing of Windows' to go stale.
    fn boot_entry(&self, root: &str, esp_rel: &str, kernel_cmdline: &str) -> Vec<Cmd> {
        vec![Cmd::WriteFile {
            path: format!("{root}{esp_rel}/loader/entries/crownos.conf"),
            contents: format!(
                "title   CrownOS\n\
                 linux   /{ESP_CROWNOS_DIR}/vmlinuz-linux\n\
                 initrd  /{ESP_CROWNOS_DIR}/initramfs-linux.img\n\
                 options {kernel_cmdline}\n"
            ),
        }]
    }

    /// systemd-boot cannot read ext4 or btrfs, so the kernel and initramfs live
    /// on the ESP. A preset pointing mkinitcpio straight at it means
    /// `pacman -Syu` keeps them current instead of this being a one-time copy.
    fn install_kernel(&self, root: &str, esp_rel: &str) -> Vec<Cmd> {
        vec![
            Cmd::MakeDirs {
                path: format!("{root}{esp_rel}/{ESP_CROWNOS_DIR}"),
            },
            Cmd::WriteFile {
                path: format!("{root}/etc/mkinitcpio.d/linux.preset"),
                contents: format!(
                    "ALL_kver=\"/boot/vmlinuz-linux\"\n\
                     PRESETS=('default')\n\
                     default_image=\"{esp_rel}/{ESP_CROWNOS_DIR}/initramfs-linux.img\"\n"
                ),
            },
            Cmd::chroot(root, "mkinitcpio", &["-P"]),
            Cmd::chroot(
                root,
                "cp",
                &[
                    "--",
                    "/boot/vmlinuz-linux",
                    &format!("{esp_rel}/{ESP_CROWNOS_DIR}/vmlinuz-linux"),
                ],
            ),
        ]
    }

    fn services(&self) -> &'static [&'static str] {
        &["NetworkManager.service", "systemd-timesyncd.service"]
    }
}
