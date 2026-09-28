//! Debian and Ubuntu: the same tools, pointed at different archives.

use crate::engine::exec::Cmd;
use crate::oobe::RootFilesystem;

use super::{
    BaseSystem, DESKTOP_PACKAGE, REPO_URL, copy_repo_key, kernel_install, with_filesystem_tools,
};

/// One Debian-family release.
pub struct Debian {
    codename: &'static str,
    mirror: &'static str,
    /// Where the live image keeps the archive's signing keys.
    keyring: &'static str,
    /// `debootstrap --components`.
    components: &'static str,
    /// The `debootstrap` script for this release, for a release newer than the
    /// live image's `debootstrap` knows by name.
    script: &'static str,
    /// The target's `/etc/apt/sources.list.d/<codename>.sources`.
    sources: &'static str,
    packages: &'static [&'static str],
}

pub const DEBIAN: Debian = Debian {
    codename: "trixie",
    mirror: "https://deb.debian.org/debian",
    keyring: "/usr/share/keyrings/debian-archive-keyring.gpg",
    components: "main,contrib,non-free,non-free-firmware",
    script: "/usr/share/debootstrap/scripts/sid",
    sources: "\
Types: deb
URIs: https://deb.debian.org/debian
Suites: trixie trixie-updates
Components: main contrib non-free non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg

Types: deb
URIs: https://security.debian.org/debian-security
Suites: trixie-security
Components: main contrib non-free non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
",
    packages: &[
        "linux-image-amd64",
        "firmware-linux",
        "initramfs-tools",
        "systemd-boot",
        "systemd-timesyncd",
        "network-manager",
        "sudo",
        "locales",
        "util-linux-extra",
        "efibootmgr",
        "dosfstools",
        "e2fsprogs",
        "nano",
    ],
};

pub const UBUNTU: Debian = Debian {
    codename: "resolute",
    mirror: "http://archive.ubuntu.com/ubuntu",
    keyring: "/usr/share/keyrings/ubuntu-archive-keyring.gpg",
    components: "main,restricted,universe,multiverse",
    script: "/usr/share/debootstrap/scripts/gutsy",
    sources: "\
Types: deb
URIs: http://archive.ubuntu.com/ubuntu
Suites: resolute resolute-updates resolute-security
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
",
    packages: &[
        "linux-generic",
        "initramfs-tools",
        "systemd-boot",
        "systemd-timesyncd",
        "network-manager",
        "sudo",
        "locales",
        "util-linux-extra",
        "efibootmgr",
        "dosfstools",
        "e2fsprogs",
        "nano",
    ],
};

const REPO_KEY_PATH: &str = "/etc/apt/keyrings/crownos-archive-keyring.asc";

/// Without it, `apt-get` stops to ask about every changed conffile, and nobody
/// is at the terminal to answer.
fn apt_get(root: &str, args: &[&str]) -> Cmd {
    let mut all = vec!["DEBIAN_FRONTEND=noninteractive", "apt-get"];
    all.extend_from_slice(args);
    Cmd::chroot(root, "env", &all)
}

impl BaseSystem for Debian {
    fn packages(&self, filesystem: RootFilesystem, _locale: &str) -> Vec<String> {
        with_filesystem_tools(self.packages, filesystem)
    }

    fn bootstrap(&self, root: &str, packages: &[String]) -> Vec<Cmd> {
        let mut install = vec!["install", "--yes"];
        install.extend(packages.iter().map(String::as_str));
        vec![
            Cmd::run(
                "debootstrap",
                &[
                    "--arch=amd64",
                    "--include=ca-certificates",
                    &format!("--components={}", self.components),
                    &format!("--keyring={}", self.keyring),
                    self.codename,
                    root,
                    self.mirror,
                    self.script,
                ],
            ),
            // `debootstrap` leaves a one-line `sources.list` without updates or
            // security; the deb822 file below replaces it.
            Cmd::WriteFile {
                path: format!("{root}/etc/apt/sources.list"),
                contents: String::new(),
            },
            Cmd::WriteFile {
                path: format!("{root}/etc/apt/sources.list.d/{}.sources", self.codename),
                contents: self.sources.to_owned(),
            },
            apt_get(root, &["update"]),
            apt_get(root, &install),
        ]
    }

    fn install_desktop(&self, root: &str) -> Vec<Cmd> {
        vec![
            copy_repo_key(root, REPO_KEY_PATH),
            Cmd::WriteFile {
                path: format!("{root}/etc/apt/sources.list.d/crownos.sources"),
                contents: format!(
                    "Types: deb\n\
                     URIs: {REPO_URL}/deb\n\
                     Suites: {}\n\
                     Components: main\n\
                     Signed-By: {REPO_KEY_PATH}\n",
                    self.codename
                ),
            },
            apt_get(root, &["update"]),
            apt_get(root, &["install", "--yes", DESKTOP_PACKAGE]),
        ]
    }

    /// Debian's `locale-gen` reads `/etc/locale.gen` and Ubuntu's takes the
    /// locale as an argument; each ignores what the other one uses.
    fn configure_locale(&self, root: &str, locale: &str) -> Vec<Cmd> {
        let lang = format!("LANG={locale}\n");
        vec![
            Cmd::WriteFile {
                path: format!("{root}/etc/locale.gen"),
                contents: format!("{locale} UTF-8\n"),
            },
            Cmd::chroot(root, "locale-gen", &[locale]),
            Cmd::WriteFile {
                path: format!("{root}/etc/default/locale"),
                contents: lang.clone(),
            },
            Cmd::WriteFile {
                path: format!("{root}/etc/locale.conf"),
                contents: lang,
            },
        ]
    }

    fn install_kernel(&self, root: &str, esp_rel: &str) -> Vec<Cmd> {
        kernel_install(
            root,
            esp_rel,
            "kernel-install add \"$version\" \"/boot/vmlinuz-$version\" \"/boot/initrd.img-$version\"",
        )
    }

    fn admin_group(&self) -> &'static str {
        "sudo"
    }

    fn services(&self) -> &'static [&'static str] {
        &["NetworkManager.service", "systemd-timesyncd.service"]
    }
}
