use crate::engine::exec::Cmd;
use crate::oobe::RootFilesystem;

use super::{
    BaseSystem, DESKTOP_PACKAGE, REPO_URL, copy_repo_key, kernel_install, with_filesystem_tools,
};

pub struct Fedora;

const RELEASE: &str = "44";

/// The Fedora repository definitions the live image ships for
/// `dnf --installroot`. The target gets its own from `fedora-repos`.
const LIVE_REPOS_DIR: &str = "/usr/share/crownos/dnf";

const BASE_PACKAGES: &[&str] = &[
    "@core",
    "kernel",
    "systemd-boot-unsigned",
    "NetworkManager",
    "chrony",
    "sudo",
    "util-linux",
    "efibootmgr",
    "dosfstools",
    "e2fsprogs",
    "nano",
];

const REPO_KEY_PATH: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-crownos";

/// The `glibc-langpack-*` that carries `locale`. Fedora ships locales
/// precompiled per language rather than generating them.
fn langpack(locale: &str) -> Option<String> {
    let language = locale.split(['_', '.', '@']).next()?;
    let is_builtin = language.is_empty() || language == "C" || language == "POSIX";
    (!is_builtin).then(|| format!("glibc-langpack-{language}"))
}

impl BaseSystem for Fedora {
    fn packages(&self, filesystem: RootFilesystem, locale: &str) -> Vec<String> {
        let mut packages = with_filesystem_tools(BASE_PACKAGES, filesystem);
        packages.extend(langpack(locale));
        packages
    }

    fn bootstrap(&self, root: &str, packages: &[String]) -> Vec<Cmd> {
        let mut args = vec![
            format!("--installroot={root}"),
            format!("--releasever={RELEASE}"),
            format!("--setopt=reposdir={LIVE_REPOS_DIR}"),
            "--setopt=install_weak_deps=False".to_owned(),
            "--assumeyes".to_owned(),
            "install".to_owned(),
        ];
        args.extend_from_slice(packages);
        vec![
            Cmd::Run {
                program: "dnf".to_owned(),
                args,
            },
            // Files written from a host without SELinux carry no labels, and an
            // enforcing first boot would refuse to read them.
            Cmd::WriteFile {
                path: format!("{root}/.autorelabel"),
                contents: String::new(),
            },
        ]
    }

    /// The repository metadata is signed and carries every package's checksum,
    /// so the packages themselves are not signed individually.
    fn install_desktop(&self, root: &str) -> Vec<Cmd> {
        vec![
            copy_repo_key(root, REPO_KEY_PATH),
            Cmd::WriteFile {
                path: format!("{root}/etc/yum.repos.d/crownos.repo"),
                contents: format!(
                    "[crownos]\n\
                     name=CrownOS\n\
                     baseurl={REPO_URL}/rpm/fedora-$releasever/$basearch/\n\
                     gpgcheck=0\n\
                     repo_gpgcheck=1\n\
                     gpgkey=file://{REPO_KEY_PATH}\n"
                ),
            },
            Cmd::chroot(root, "dnf", &["--assumeyes", "install", DESKTOP_PACKAGE]),
        ]
    }

    fn configure_locale(&self, root: &str, locale: &str) -> Vec<Cmd> {
        vec![Cmd::WriteFile {
            path: format!("{root}/etc/locale.conf"),
            contents: format!("LANG={locale}\n"),
        }]
    }

    /// `kernel-install` runs dracut itself, so only the kernel is named.
    fn install_kernel(&self, root: &str, esp_rel: &str) -> Vec<Cmd> {
        kernel_install(
            root,
            esp_rel,
            "kernel-install add \"$version\" \"$modules/vmlinuz\"",
        )
    }

    fn services(&self) -> &'static [&'static str] {
        &["NetworkManager.service", "chronyd.service"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_langpack_follows_the_language_not_the_territory() {
        assert_eq!(
            langpack("de_AT.UTF-8").as_deref(),
            Some("glibc-langpack-de")
        );
        assert_eq!(
            langpack("sr_RS@latin").as_deref(),
            Some("glibc-langpack-sr")
        );
        assert_eq!(langpack("C.UTF-8"), None);
    }
}
