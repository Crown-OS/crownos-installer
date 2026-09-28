//! What changes when CrownOS is layered on a different distribution.
//!
//! Most of an install does not care: partitioning, `genfstab`, `arch-chroot`,
//! `bootctl` and the account steps work on any systemd root. What does care is
//! small and lives here — how the base system gets onto the disk, how the
//! CrownOS desktop is added on top of it, how locales and kernels are handled.
//!
//! [`Distro::NixOs`] is the exception to all of that: it is declarative, so its
//! install is a generated configuration plus `nixos-install` rather than a
//! sequence of package-manager calls. It therefore has no [`BaseSystem`] and is
//! planned separately — see [`nixos`].

mod arch;
mod debian;
mod fedora;
pub mod nixos;

use crate::oobe::RootFilesystem;

use super::exec::Cmd;

pub use crate::oobe::Distro;

/// Where CrownOS packages are published, one tree per package format.
pub const REPO_URL: &str = "https://repo.crownos.org";

/// The repository signing key, as the live image ships it.
pub const REPO_KEY_LIVE_PATH: &str = "/usr/share/crownos/crownos-archive-keyring.asc";

/// The signing key's user ID, which `pacman-key --lsign-key` accepts in place
/// of a fingerprint.
pub const REPO_KEY_UID: &str = "packages@crownos.org";

/// The meta-package that pulls in the compositor and every system app.
pub const DESKTOP_PACKAGE: &str = "crownos-desktop";

/// Files `kernel-install` writes to the ESP go under `<ESP>/<this>/`, and boot
/// entries are named `<this>-<kernel version>.conf`.
pub const BLS_ENTRY_TOKEN: &str = "crownos";

impl Distro {
    pub const ALL: [Self; 5] = [
        Self::Arch,
        Self::Debian,
        Self::Ubuntu,
        Self::Fedora,
        Self::NixOs,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Arch => "Arch Linux",
            Self::Debian => "Debian 13",
            Self::Ubuntu => "Ubuntu 26.04 LTS",
            Self::Fedora => "Fedora 44",
            Self::NixOs => "NixOS 26.05",
        }
    }

    /// The distribution installed by running a package manager into the target,
    /// or `None` for NixOS.
    pub const fn imperative(self) -> Option<ImperativeDistro> {
        match self {
            Self::Arch => Some(ImperativeDistro::Arch),
            Self::Debian => Some(ImperativeDistro::Debian),
            Self::Ubuntu => Some(ImperativeDistro::Ubuntu),
            Self::Fedora => Some(ImperativeDistro::Fedora),
            Self::NixOs => None,
        }
    }
}

/// A [`Distro`] that is installed package by package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImperativeDistro {
    Arch,
    Debian,
    Ubuntu,
    Fedora,
}

impl ImperativeDistro {
    pub fn backend(self) -> &'static dyn BaseSystem {
        match self {
            Self::Arch => &arch::Arch,
            Self::Debian => &debian::DEBIAN,
            Self::Ubuntu => &debian::UBUNTU,
            Self::Fedora => &fedora::Fedora,
        }
    }
}

/// Everything a plan needs to know about one package-managed distribution.
///
/// Every method returns commands or names and runs nothing, so the plan stays a
/// value and a dry run stays a faithful rehearsal.
pub trait BaseSystem: Sync {
    /// The packages the base system is bootstrapped with.
    fn packages(&self, filesystem: RootFilesystem, locale: &str) -> Vec<String>;

    /// Put a bootable base system into `root`.
    fn bootstrap(&self, root: &str, packages: &[String]) -> Vec<Cmd>;

    /// Trust the CrownOS repository and install [`DESKTOP_PACKAGE`] from it.
    fn install_desktop(&self, root: &str) -> Vec<Cmd>;

    /// Generate and select `locale`.
    fn configure_locale(&self, root: &str, locale: &str) -> Vec<Cmd>;

    /// Our own boot loader entry, for distributions whose kernels
    /// `kernel-install` does not manage.
    fn boot_entry(&self, _root: &str, _esp_rel: &str, _kernel_cmdline: &str) -> Vec<Cmd> {
        Vec::new()
    }

    /// Put the kernel and initramfs on the ESP, and keep them there across
    /// kernel updates.
    fn install_kernel(&self, root: &str, esp_rel: &str) -> Vec<Cmd>;

    /// The group whose members may use `sudo`.
    fn admin_group(&self) -> &'static str {
        "wheel"
    }

    /// Units enabled in the new system.
    fn services(&self) -> &'static [&'static str];
}

/// The package that provides `btrfs` userspace tools, on every base.
const BTRFS_PACKAGE: &str = "btrfs-progs";

fn with_filesystem_tools(base: &[&str], filesystem: RootFilesystem) -> Vec<String> {
    let mut packages: Vec<String> = base.iter().map(|p| (*p).to_owned()).collect();
    if filesystem == RootFilesystem::Btrfs {
        packages.push(BTRFS_PACKAGE.to_owned());
    }
    packages
}

/// Copy the repository key off the live image into the target.
fn copy_repo_key(root: &str, destination: &str) -> Cmd {
    Cmd::run(
        "install",
        &[
            "-Dm644",
            REPO_KEY_LIVE_PATH,
            &format!("{root}{destination}"),
        ],
    )
}

/// Register every installed kernel with `kernel-install`, in the Boot Loader
/// Specification layout, on the ESP mounted at `esp_rel`.
///
/// `add_one` is the shell command that adds `$version`, whose modules are in
/// `$modules`. The distributions' own kernel hooks call `kernel-install` with
/// the same `entry-token` and `install.conf` on every later update.
fn kernel_install(root: &str, esp_rel: &str, add_one: &str) -> Vec<Cmd> {
    vec![
        Cmd::WriteFile {
            path: format!("{root}/etc/kernel/entry-token"),
            contents: format!("{BLS_ENTRY_TOKEN}\n"),
        },
        Cmd::WriteFile {
            path: format!("{root}/etc/kernel/install.conf"),
            contents: "layout=bls\n".to_owned(),
        },
        Cmd::MakeDirs {
            path: format!("{root}{esp_rel}/{BLS_ENTRY_TOKEN}"),
        },
        Cmd::chroot(
            root,
            "sh",
            &[
                "-c",
                &format!(
                    "export BOOT_ROOT={esp_rel}; \
                     for modules in /usr/lib/modules/*; do \
                     version=\"${{modules##*/}}\"; {add_one} || exit 1; done"
                ),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_distro_but_nixos_is_installed_by_package_manager() {
        for distro in Distro::ALL {
            assert_eq!(distro.imperative().is_none(), distro == Distro::NixOs);
        }
    }

    #[test]
    fn btrfs_pulls_in_its_tools_on_every_base() {
        for distro in Distro::ALL.iter().filter_map(|d| d.imperative()) {
            let packages = distro
                .backend()
                .packages(RootFilesystem::Btrfs, "en_US.UTF-8");
            assert!(packages.iter().any(|p| p == BTRFS_PACKAGE), "{distro:?}");
            let packages = distro
                .backend()
                .packages(RootFilesystem::Ext4, "en_US.UTF-8");
            assert!(!packages.iter().any(|p| p == BTRFS_PACKAGE), "{distro:?}");
        }
    }

    #[test]
    fn the_desktop_is_installed_from_the_crownos_repository_on_every_base() {
        for distro in Distro::ALL.iter().filter_map(|d| d.imperative()) {
            let commands: Vec<String> = distro
                .backend()
                .install_desktop("/mnt")
                .iter()
                .map(ToString::to_string)
                .collect();
            let log = commands.join("\n");
            assert!(log.contains(DESKTOP_PACKAGE), "{distro:?}: {log}");
            assert!(log.contains(REPO_URL), "{distro:?}: {log}");
            assert!(log.contains(REPO_KEY_LIVE_PATH), "{distro:?}: {log}");
        }
    }

    #[test]
    fn nothing_but_arch_writes_a_fixed_boot_entry() {
        for distro in Distro::ALL.iter().filter_map(|d| d.imperative()) {
            let entry = distro
                .backend()
                .boot_entry("/mnt", "/efi", "root=PARTUUID=x rw");
            assert_eq!(
                entry.is_empty(),
                distro != ImperativeDistro::Arch,
                "{distro:?}"
            );
        }
    }
}
