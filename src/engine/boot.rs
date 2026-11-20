//! systemd-boot, the kernel on the ESP, and "Recover the previous version".
//!
//! systemd-boot cannot read btrfs, so the kernel and initramfs live on the ESP.
//! Beside them sits a copy of the pair that booted before the last pacman
//! transaction (`-previous`, kept current by crownos-base's pacman hooks), and a
//! second loader entry boots that copy with `crownos.recover`, which makes the
//! initramfs roll the root back to snapper's newest pre-update snapshot.
//!
//! Every function returns commands and runs nothing.

use super::base::KERNEL;
use super::exec::Cmd;
use super::plan::ESP_CROWNOS_DIR;

/// Whose ESP the boot loader goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EspOrigin {
    /// Formatted by this install: everything on it is CrownOS's.
    Created,
    /// Already there, typically Windows': files that exist belong to something
    /// else and are left alone.
    Shared,
}

/// The two loader entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootEntry {
    Current,
    Previous,
}

impl BootEntry {
    pub const ALL: [Self; 2] = [Self::Current, Self::Previous];

    pub fn file_name(self) -> &'static str {
        match self {
            Self::Current => "crownos.conf",
            Self::Previous => "crownos-recover.conf",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Current => "CrownOS",
            Self::Previous => "Recover the previous version",
        }
    }

    fn image_suffix(self) -> &'static str {
        match self {
            Self::Current => "",
            Self::Previous => "-previous",
        }
    }

    fn sort_key(self) -> &'static str {
        match self {
            Self::Current => "crownos",
            Self::Previous => "crownos-recover",
        }
    }

    fn extra_options(self) -> &'static str {
        match self {
            Self::Current => "",
            Self::Previous => " crownos.recover",
        }
    }
}

/// systemd-boot's settings. `menu-hidden` boots the default at once and shows
/// the menu (CrownOS, "Recover the previous version", Windows) only while a key
/// is held during boot. Space is reliable; a bare Shift depends on the
/// firmware reporting it.
pub const LOADER_CONF: &str =
    "default crownos.conf\ntimeout menu-hidden\nconsole-mode keep\neditor no\n";

/// Overrides the HOOKS of `/etc/mkinitcpio.conf`. The busybox flavour, because
/// the systemd flavour never runs a hook's `run_hook`, and `crownos-recover`
/// must run after the root device exists and before it is mounted. `keymap`
/// reads `/etc/vconsole.conf`; `consolefont` is left out because no console
/// font is configured and it would warn on every rebuild.
pub const MKINITCPIO_DROP_IN: &str = "/etc/mkinitcpio.conf.d/crownos.conf";

const MKINITCPIO_HOOKS: &str = "HOOKS=(base udev autodetect microcode modconf kms keyboard keymap block crownos-recover filesystems fsck)\n";

/// Read by snapper and snap-pac. Empty until [`prepare_recovery`], so nothing
/// the install itself does is snapshotted.
const SNAPPER_CONFIGS_FILE: &str = "/etc/conf.d/snapper";

/// Shipped by crownos-base (crownos-iso's `packaging/boot/systemd/`), which is
/// always in the pacstrap set: after a recovery boot it puts the recovered
/// kernel back as the current one, so the next ordinary boot matches the
/// rolled-back root. Enabling it is not allowed to fail, because a recovery
/// without it leaves the default entry booting the new kernel against the old
/// root.
const RECOVER_FINISH_SERVICE: &str = "crownos-recover-finish.service";

/// One loader entry. systemd-boot finds Windows by itself, so there is
/// deliberately no Windows entry and nothing of Windows' to go stale.
pub fn boot_entry(root: &str, esp_rel: &str, kernel_cmdline: &str, entry: BootEntry) -> Cmd {
    let suffix = entry.image_suffix();
    Cmd::WriteFile {
        path: format!("{root}{esp_rel}/loader/entries/{}", entry.file_name()),
        contents: format!(
            "title   {}\n\
             sort-key {}\n\
             linux   /{ESP_CROWNOS_DIR}/vmlinuz-{KERNEL}{suffix}\n\
             initrd  /{ESP_CROWNOS_DIR}/initramfs-{KERNEL}{suffix}.img\n\
             options {kernel_cmdline}{}\n",
            entry.title(),
            entry.sort_key(),
            entry.extra_options(),
        ),
    }
}

/// On a shared ESP a `loader.conf` that is already there was put there by
/// something else, and its `default` line decides what the machine boots,
/// Windows included.
pub fn loader_config(root: &str, esp_rel: &str, origin: EspOrigin) -> Cmd {
    let path = format!("{root}{esp_rel}/loader/loader.conf");
    let contents = LOADER_CONF.to_owned();
    match origin {
        EspOrigin::Created => Cmd::WriteFile { path, contents },
        EspOrigin::Shared => Cmd::WriteFileIfAbsent { path, contents },
    }
}

/// `loader.conf` goes first: `bootctl install` writes a stub of its own when
/// none exists, which would otherwise win on the installer's own ESP.
pub fn install_bootloader(
    root: &str,
    esp_rel: &str,
    kernel_cmdline: &str,
    root_partuuid: &str,
    origin: EspOrigin,
) -> Vec<Cmd> {
    let mut commands = vec![
        loader_config(root, esp_rel, origin),
        Cmd::chroot(
            root,
            "bootctl",
            &[&format!("--esp-path={esp_rel}"), "install"],
        ),
    ];
    commands.extend(
        BootEntry::ALL
            .into_iter()
            .map(|entry| boot_entry(root, esp_rel, kernel_cmdline, entry)),
    );
    commands.extend([
        Cmd::WriteFile {
            path: format!("{root}/etc/kernel/cmdline"),
            contents: format!("{kernel_cmdline}\n"),
        },
        Cmd::WriteFile {
            path: format!("{root}/etc/crownos-root-partuuid"),
            contents: format!("{root_partuuid}\n"),
        },
    ]);
    commands
}

/// A preset pointing mkinitcpio straight at the ESP, so every rebuild lands
/// where systemd-boot reads it. The kernel itself is copied by crownos-base's
/// pacman hook after each upgrade, and once here.
pub fn install_kernel(root: &str, esp_rel: &str) -> Vec<Cmd> {
    vec![
        Cmd::MakeDirs {
            path: format!("{root}{esp_rel}/{ESP_CROWNOS_DIR}"),
        },
        Cmd::WriteFile {
            path: format!("{root}{MKINITCPIO_DROP_IN}"),
            contents: MKINITCPIO_HOOKS.to_owned(),
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
        copy_kernel_to_esp(root, esp_rel),
    ]
}

/// Copy the kernel onto the ESP beside the initramfs mkinitcpio keeps there.
/// Run again after any `pacman -Syu` during the install, which may replace it.
pub fn copy_kernel_to_esp(root: &str, esp_rel: &str) -> Cmd {
    Cmd::chroot(
        root,
        "cp",
        &[
            "--",
            &format!("/boot/vmlinuz-{KERNEL}"),
            &format!("{esp_rel}/{ESP_CROWNOS_DIR}/vmlinuz-{KERNEL}"),
        ],
    )
}

/// The last thing before services are enabled: arm snapper, so the user's
/// first transaction is the first one snapshotted, and seed the `-previous`
/// kernel from the final installed one so the recovery entry boots from day
/// one. Ends with a command whose output is the on-disk evidence the ISO's
/// install test reads.
pub fn prepare_recovery(root: &str, esp_rel: &str, origin: EspOrigin) -> Vec<Cmd> {
    let seed = Cmd::chroot(
        root,
        "env",
        &[
            &format!("CROWNOS_ESP={esp_rel}"),
            "crownos-esp-kernel",
            "seed",
        ],
    );
    vec![
        Cmd::WriteFile {
            path: format!("{root}{SNAPPER_CONFIGS_FILE}"),
            contents: "SNAPPER_CONFIGS=\"root\"\n".to_owned(),
        },
        Cmd::chroot(root, "snapper", &["--no-dbus", "-c", "root", "get-config"]),
        match origin {
            EspOrigin::Created => seed,
            EspOrigin::Shared => Cmd::best_effort(seed),
        },
        Cmd::chroot(root, "systemctl", &["enable", RECOVER_FINISH_SERVICE]),
        Cmd::chroot(root, "sh", &["-c", &verification_script(esp_rel)]),
    ]
}

fn verification_script(esp_rel: &str) -> String {
    format!(
        "grep -H . {esp_rel}/loader/loader.conf {esp_rel}/loader/entries/*.conf {MKINITCPIO_DROP_IN}; \
         grep -H NUMBER_LIMIT /etc/snapper/configs/root; \
         printf '{RECOVER_FINISH_SERVICE} %s\\n' \"$(systemctl is-enabled {RECOVER_FINISH_SERVICE})\"; \
         btrfs subvolume get-default /; \
         ls {esp_rel}/{ESP_CROWNOS_DIR}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(command: &Cmd) -> (&str, &str) {
        match command {
            Cmd::WriteFile { path, contents } | Cmd::WriteFileIfAbsent { path, contents } => {
                (path, contents)
            }
            other => panic!("expected a file write, got {other}"),
        }
    }

    #[test]
    fn the_entries_are_named_for_the_loader_default_and_the_recovery_menu() {
        assert_eq!(BootEntry::Current.file_name(), "crownos.conf");
        assert_eq!(BootEntry::Previous.file_name(), "crownos-recover.conf");
        assert_eq!(BootEntry::Previous.title(), "Recover the previous version");
        assert!(LOADER_CONF.starts_with(&format!("default {}\n", BootEntry::Current.file_name())));
        assert!(LOADER_CONF.contains("timeout menu-hidden\n"));
    }

    #[test]
    fn the_boot_entry_lives_where_the_loader_default_points() {
        let current = boot_entry("/mnt", "/efi", "root=PARTUUID=x rw", BootEntry::Current);
        let (path, contents) = written(&current);
        assert_eq!(path, "/mnt/efi/loader/entries/crownos.conf");
        assert!(contents.contains("linux   /EFI/CrownOS/vmlinuz-linux-cachyos\n"));
        assert!(contents.ends_with("options root=PARTUUID=x rw\n"));

        let previous = boot_entry("/mnt", "/efi", "root=PARTUUID=x rw", BootEntry::Previous);
        let (path, contents) = written(&previous);
        assert_eq!(path, "/mnt/efi/loader/entries/crownos-recover.conf");
        assert!(contents.starts_with("title   Recover the previous version\n"));
        assert!(contents.contains("linux   /EFI/CrownOS/vmlinuz-linux-cachyos-previous\n"));
        assert!(contents.contains("initrd  /EFI/CrownOS/initramfs-linux-cachyos-previous.img\n"));
        assert!(contents.ends_with("options root=PARTUUID=x rw crownos.recover\n"));
    }

    #[test]
    fn loader_conf_is_written_before_bootctl_can_write_its_stub() {
        for origin in [EspOrigin::Created, EspOrigin::Shared] {
            let commands = install_bootloader("/mnt", "/efi", "root=PARTUUID=x rw", "x", origin);
            let log: Vec<String> = commands.iter().map(ToString::to_string).collect();
            let loader = log
                .iter()
                .position(|l| l.contains("loader/loader.conf"))
                .expect("a loader.conf");
            let bootctl = log
                .iter()
                .position(|l| l.contains("bootctl --esp-path=/efi install"))
                .expect("bootctl");
            assert!(loader < bootctl, "{log:?}");
            let expected_kind = matches!(
                (&commands[loader], origin),
                (Cmd::WriteFile { .. }, EspOrigin::Created)
                    | (Cmd::WriteFileIfAbsent { .. }, EspOrigin::Shared)
            );
            assert!(expected_kind, "{origin:?}: {}", commands[loader]);
            for entry in BootEntry::ALL {
                assert!(log.iter().any(|l| l.contains(entry.file_name())), "{log:?}");
            }
        }
    }

    #[test]
    fn the_recovery_hook_runs_between_block_and_filesystems() {
        assert!(MKINITCPIO_HOOKS.starts_with("HOOKS=(base udev "));
        assert!(MKINITCPIO_HOOKS.contains(" block crownos-recover filesystems "));
        assert!(!MKINITCPIO_HOOKS.contains("consolefont"));
        let log: Vec<String> = install_kernel("/mnt", "/efi")
            .iter()
            .map(ToString::to_string)
            .collect();
        let drop_in = log
            .iter()
            .position(|l| l.contains(MKINITCPIO_DROP_IN))
            .expect("the drop-in");
        let rebuild = log
            .iter()
            .position(|l| l.contains("mkinitcpio -P"))
            .expect("a rebuild");
        assert!(drop_in < rebuild, "{log:?}");
    }

    #[test]
    fn snapper_is_armed_first_and_seeding_is_mandatory_only_on_our_own_esp() {
        let created = prepare_recovery("/mnt", "/efi", EspOrigin::Created);
        let (path, contents) = written(&created[0]);
        assert_eq!(path, "/mnt/etc/conf.d/snapper");
        assert_eq!(contents, "SNAPPER_CONFIGS=\"root\"\n");
        let is_seed = |command: &Cmd| command.to_string().contains("crownos-esp-kernel seed");
        assert!(
            created
                .iter()
                .any(|c| is_seed(c) && !matches!(c, Cmd::BestEffort(_)))
        );

        let shared = prepare_recovery("/mnt", "/efi", EspOrigin::Shared);
        assert!(
            shared
                .iter()
                .any(|c| is_seed(c) && matches!(c, Cmd::BestEffort(_)))
        );
        assert!(
            shared
                .last()
                .is_some_and(|c| c.to_string().contains("grep -H . /efi/loader/loader.conf"))
        );
    }

    #[test]
    fn the_recovery_finish_service_is_enabled_without_fail_and_verified_on_every_esp() {
        for origin in [EspOrigin::Created, EspOrigin::Shared] {
            let commands = prepare_recovery("/mnt", "/efi", origin);
            let enable = commands
                .iter()
                .position(|c| {
                    matches!(c, Cmd::Chroot { program, args, .. }
                        if program == "systemctl" && args == &["enable", RECOVER_FINISH_SERVICE])
                })
                .expect("the unit is enabled, and not best effort");
            let seed = commands
                .iter()
                .position(|c| c.to_string().contains("crownos-esp-kernel seed"))
                .expect("a seed");
            assert!(seed < enable, "{commands:?}");
            let verification = commands.last().map(ToString::to_string).unwrap_or_default();
            assert!(
                verification.contains("systemctl is-enabled crownos-recover-finish.service"),
                "{verification}"
            );
        }
    }
}
