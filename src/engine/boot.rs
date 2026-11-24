//! rEFInd, the kernel on the ESP, and the rollback entry.
//!
//! rEFInd cannot read btrfs, so the kernel and initramfs live on the ESP. Before
//! each update crownos-updated keeps the current pair in `EFI/CrownOS/rollback`
//! and writes `crownos-rollback.conf` with one entry that boots that pair with
//! `crownos.recover=<N>`. The initramfs then rolls the root back to snapshot N.
//! The menu stays hidden until a key is held during boot.
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

/// rEFInd's directory on the ESP.
pub const REFIND_DIR: &str = "EFI/refind";

/// The binary the `refind` package ships. The package has no pacman hook, so
/// crownos-base's hook copies it to the ESP after each upgrade.
const REFIND_PACKAGED_BINARY: &str = "/usr/share/refind/refind_x64.efi";

const REFIND_BINARY: &str = "refind_x64.efi";

pub const REFIND_CONF: &str = "refind.conf";

/// Included by `refind.conf` and owned by crownos-updated.
pub const ROLLBACK_CONF: &str = "crownos-rollback.conf";

/// Where crownos-updated keeps the kernel pair the rollback entry boots.
pub const ESP_ROLLBACK_DIR: &str = "EFI/CrownOS/rollback";

/// The removable-media path the firmware boots when no boot entry works.
/// Written only on an ESP this install created.
const FALLBACK_LOADER: &str = "EFI/BOOT/BOOTX64.EFI";

const BACKUP_SUFFIX: &str = ".crownos-backup";

const ROLLBACK_CONF_PLACEHOLDER: &str = "# Written by crownos-updated before each update.\n";

const FIRMWARE_ENTRY_LABEL: &str = "CrownOS";

const FIRMWARE_ENTRY_LOADER: &str = "\\EFI\\refind\\refind_x64.efi";

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
/// always in the pacstrap set: after a rollback boot it puts the rollback
/// kernel back as the current one, so the next ordinary boot matches the
/// rolled-back root. Enabling it is not allowed to fail, because a rollback
/// without it leaves the default entry booting the new kernel against the old
/// root.
const RECOVER_FINISH_SERVICE: &str = "crownos-recover-finish.service";

/// The update daemon. [`base::CROWNOS_PRESET`](super::base::CROWNOS_PRESET) enables it;
/// the verification script reports it because the install test asserts it.
const UPDATE_SERVICE: &str = "crownos-updated.service";

/// `timeout -1` boots the default at once and shows the menu only while a key
/// is held. The ESP's kernel directories are not scanned, so the only CrownOS
/// entries are the one below and the rollback entry crownos-updated includes.
/// The include comes after the CrownOS entry: `default_selection` selects the
/// first entry whose title contains "CrownOS", and the rollback title does too.
/// A `"` in the command line is doubled, as rEFInd reads it in a quoted token.
pub fn refind_conf(kernel_cmdline: &str) -> String {
    let quoted_cmdline = kernel_cmdline.replace('"', "\"\"");
    format!(
        "# Written by crownos-installer. crownos-updated writes {ROLLBACK_CONF}.
timeout -1
use_nvram false
scanfor manual,internal,external
scan_all_linux_kernels false
dont_scan_dirs {ESP_CROWNOS_DIR},{ESP_ROLLBACK_DIR}
showtools shutdown,reboot,firmware
default_selection \"CrownOS\"

menuentry \"CrownOS\" {{
    loader /{ESP_CROWNOS_DIR}/vmlinuz-{KERNEL}
    initrd /{ESP_CROWNOS_DIR}/initramfs-{KERNEL}.img
    options \"{quoted_cmdline}\"
}}

include {ROLLBACK_CONF}
"
    )
}

/// rEFInd and its configuration on the ESP, and a firmware boot entry for it.
///
/// On a created ESP rEFInd is also the fallback loader, so the firmware boots
/// it even when the boot entry cannot be written. On a shared ESP the fallback
/// belongs to the other system, so the boot entry is the only way to boot
/// CrownOS and must not fail. An existing `refind.conf` there is kept as a
/// backup first.
pub fn install_bootloader(
    root: &str,
    esp_rel: &str,
    kernel_cmdline: &str,
    root_partuuid: &str,
    origin: EspOrigin,
    disk: &str,
    esp_number: u32,
) -> Vec<Cmd> {
    let esp = format!("{root}{esp_rel}");
    let refind_conf_path = format!("{esp}/{REFIND_DIR}/{REFIND_CONF}");
    let mut commands = vec![
        Cmd::MakeDirs {
            path: format!("{esp}/{ESP_ROLLBACK_DIR}"),
        },
        install_refind_binary(root, &format!("{esp_rel}/{REFIND_DIR}/{REFIND_BINARY}")),
    ];
    if origin == EspOrigin::Shared {
        commands.push(Cmd::CopyIfPresent {
            from: refind_conf_path.clone(),
            to: format!("{refind_conf_path}{BACKUP_SUFFIX}"),
        });
    }
    commands.extend([
        Cmd::WriteFile {
            path: refind_conf_path,
            contents: refind_conf(kernel_cmdline),
        },
        Cmd::WriteFile {
            path: format!("{esp}/{REFIND_DIR}/{ROLLBACK_CONF}"),
            contents: ROLLBACK_CONF_PLACEHOLDER.to_owned(),
        },
    ]);
    let firmware_entry = firmware_boot_entry(disk, esp_number);
    match origin {
        EspOrigin::Created => commands.extend([
            install_refind_binary(root, &format!("{esp_rel}/{FALLBACK_LOADER}")),
            Cmd::best_effort(firmware_entry),
        ]),
        EspOrigin::Shared => commands.push(firmware_entry),
    }
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

fn install_refind_binary(root: &str, target: &str) -> Cmd {
    Cmd::chroot(root, "install", &["-Dm644", REFIND_PACKAGED_BINARY, target])
}

/// Runs in the live system, which sees the firmware's variables.
fn firmware_boot_entry(disk: &str, esp_number: u32) -> Cmd {
    Cmd::run(
        "efibootmgr",
        &[
            "--create",
            "--disk",
            disk,
            "--part",
            &esp_number.to_string(),
            "--label",
            FIRMWARE_ENTRY_LABEL,
            "--loader",
            FIRMWARE_ENTRY_LOADER,
        ],
    )
}

/// A preset pointing mkinitcpio straight at the ESP, so every rebuild lands
/// where rEFInd reads it. The kernel itself is copied by crownos-base's pacman
/// hook after each upgrade, and once here.
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

/// The last step inside the new system: arm snapper, so the user's first
/// transaction is the first one snapshotted, and enable the unit that finishes
/// a rollback. Ends with a command whose output is the on-disk evidence the
/// ISO's install test reads.
pub fn prepare_recovery(root: &str, esp_rel: &str) -> Vec<Cmd> {
    vec![
        Cmd::WriteFile {
            path: format!("{root}{SNAPPER_CONFIGS_FILE}"),
            contents: "SNAPPER_CONFIGS=\"root\"\n".to_owned(),
        },
        Cmd::chroot(root, "snapper", &["--no-dbus", "-c", "root", "get-config"]),
        Cmd::chroot(root, "systemctl", &["enable", RECOVER_FINISH_SERVICE]),
        Cmd::chroot(root, "sh", &["-c", &verification_script(esp_rel)]),
    ]
}

fn verification_script(esp_rel: &str) -> String {
    let refind = format!("{esp_rel}/{REFIND_DIR}");
    let unit_state =
        |unit: &str| format!("printf '%s %s\\n' {unit} \"$(systemctl is-enabled {unit})\"");
    format!(
        "grep -H . {refind}/{REFIND_CONF} {refind}/{ROLLBACK_CONF} {MKINITCPIO_DROP_IN}; \
         grep -H NUMBER_LIMIT /etc/snapper/configs/root; \
         grep -H '^Server' /etc/pacman.conf; \
         {}; \
         {}; \
         btrfs subvolume get-default /; \
         ls {esp_rel}/{ESP_CROWNOS_DIR} {refind}",
        unit_state(RECOVER_FINISH_SERVICE),
        unit_state(UPDATE_SERVICE),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMDLINE: &str = "root=PARTUUID=x rw";

    fn log(commands: &[Cmd]) -> Vec<String> {
        commands.iter().map(ToString::to_string).collect()
    }

    fn position(log: &[String], needle: &str) -> usize {
        log.iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("`{needle}` missing from {log:?}"))
    }

    fn bootloader(origin: EspOrigin) -> Vec<Cmd> {
        install_bootloader("/mnt", "/efi", CMDLINE, "x", origin, "/dev/sda", 1)
    }

    fn firmware_entry(commands: &[Cmd]) -> &Cmd {
        commands
            .iter()
            .find(|command| command.to_string().contains("efibootmgr"))
            .unwrap_or_else(|| panic!("no boot entry in {commands:?}"))
    }

    #[test]
    fn refind_conf_hides_the_menu_and_boots_crownos_by_default() {
        assert_eq!(
            refind_conf(CMDLINE),
            "# Written by crownos-installer. crownos-updated writes crownos-rollback.conf.\n\
             timeout -1\n\
             use_nvram false\n\
             scanfor manual,internal,external\n\
             scan_all_linux_kernels false\n\
             dont_scan_dirs EFI/CrownOS,EFI/CrownOS/rollback\n\
             showtools shutdown,reboot,firmware\n\
             default_selection \"CrownOS\"\n\
             \n\
             menuentry \"CrownOS\" {\n\
             \x20   loader /EFI/CrownOS/vmlinuz-linux-cachyos\n\
             \x20   initrd /EFI/CrownOS/initramfs-linux-cachyos.img\n\
             \x20   options \"root=PARTUUID=x rw\"\n\
             }\n\
             \n\
             include crownos-rollback.conf\n"
        );
    }

    #[test]
    fn a_quote_in_the_command_line_is_doubled_inside_the_quoted_options() {
        assert!(
            refind_conf("root=PARTUUID=x rw dyndbg=\"file x.c +p\"")
                .contains("    options \"root=PARTUUID=x rw dyndbg=\"\"file x.c +p\"\"\"\n")
        );
    }

    #[test]
    fn both_configs_are_written_after_the_binary_is_installed() {
        for origin in [EspOrigin::Created, EspOrigin::Shared] {
            let log = log(&bootloader(origin));
            let binary = position(
                &log,
                "install -Dm644 /usr/share/refind/refind_x64.efi /efi/EFI/refind/refind_x64.efi",
            );
            let conf = position(&log, "cat > /mnt/efi/EFI/refind/refind.conf <<");
            let rollback = position(&log, "cat > /mnt/efi/EFI/refind/crownos-rollback.conf <<");
            assert!(position(&log, "mkdir -p /mnt/efi/EFI/CrownOS/rollback") < binary);
            assert!(binary < conf && conf < rollback, "{log:?}");
            assert!(log[rollback].contains(ROLLBACK_CONF_PLACEHOLDER), "{log:?}");
            position(&log, "cat > /mnt/etc/kernel/cmdline");
            position(&log, "cat > /mnt/etc/crownos-root-partuuid");
        }
    }

    #[test]
    fn a_created_esp_gets_the_fallback_loader_and_a_best_effort_boot_entry() {
        let commands = bootloader(EspOrigin::Created);
        let log = log(&commands);
        position(
            &log,
            "install -Dm644 /usr/share/refind/refind_x64.efi /efi/EFI/BOOT/BOOTX64.EFI",
        );
        assert!(!log.iter().any(|l| l.contains(BACKUP_SUFFIX)), "{log:?}");
        let entry = firmware_entry(&commands);
        assert!(matches!(entry, Cmd::BestEffort(_)), "{entry}");
        assert!(
            entry.to_string().starts_with(
                "efibootmgr --create --disk /dev/sda --part 1 --label CrownOS \
                 --loader \\EFI\\refind\\refind_x64.efi"
            ),
            "{entry}"
        );
    }

    #[test]
    fn a_shared_esp_keeps_its_fallback_and_backs_up_an_existing_config() {
        let commands = bootloader(EspOrigin::Shared);
        let log = log(&commands);
        assert!(!log.iter().any(|l| l.contains("EFI/BOOT")), "{log:?}");
        let backup = commands
            .iter()
            .position(|c| {
                matches!(c, Cmd::CopyIfPresent { from, to }
                    if from == "/mnt/efi/EFI/refind/refind.conf"
                        && to == "/mnt/efi/EFI/refind/refind.conf.crownos-backup")
            })
            .unwrap_or_else(|| panic!("no backup in {log:?}"));
        assert!(backup < position(&log, "cat > /mnt/efi/EFI/refind/refind.conf <<"));
        let entry = firmware_entry(&commands);
        assert!(
            matches!(entry, Cmd::Run { program, .. } if program == "efibootmgr"),
            "{entry}"
        );
    }

    #[test]
    fn the_recovery_hook_runs_between_block_and_filesystems() {
        assert!(MKINITCPIO_HOOKS.starts_with("HOOKS=(base udev "));
        assert!(MKINITCPIO_HOOKS.contains(" block crownos-recover filesystems "));
        assert!(!MKINITCPIO_HOOKS.contains("consolefont"));
        let log = log(&install_kernel("/mnt", "/efi"));
        assert!(position(&log, MKINITCPIO_DROP_IN) < position(&log, "mkinitcpio -P"));
    }

    #[test]
    fn snapper_is_armed_first_and_the_finish_unit_is_enabled_without_fail() {
        let commands = prepare_recovery("/mnt", "/efi");
        assert!(matches!(&commands[0], Cmd::WriteFile { path, contents }
            if path == "/mnt/etc/conf.d/snapper" && contents == "SNAPPER_CONFIGS=\"root\"\n"));
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, Cmd::Chroot { program, args, .. }
            if program == "systemctl" && args == &["enable", RECOVER_FINISH_SERVICE]))
        );
        assert!(
            !log(&commands)
                .iter()
                .any(|l| l.contains("crownos-esp-kernel"))
        );
        assert!(matches!(commands.last(), Some(Cmd::Chroot { program, .. }) if program == "sh"));
    }

    #[test]
    fn the_verification_reports_what_the_install_test_asserts() {
        assert_eq!(
            verification_script("/efi"),
            "grep -H . /efi/EFI/refind/refind.conf /efi/EFI/refind/crownos-rollback.conf \
             /etc/mkinitcpio.conf.d/crownos.conf; \
             grep -H NUMBER_LIMIT /etc/snapper/configs/root; \
             grep -H '^Server' /etc/pacman.conf; \
             printf '%s %s\\n' crownos-recover-finish.service \
             \"$(systemctl is-enabled crownos-recover-finish.service)\"; \
             printf '%s %s\\n' crownos-updated.service \
             \"$(systemctl is-enabled crownos-updated.service)\"; \
             btrfs subvolume get-default /; \
             ls /efi/EFI/CrownOS /efi/EFI/refind"
        );
    }
}
