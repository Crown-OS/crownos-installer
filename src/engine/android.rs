//! Android apps: the packages they need and the first-boot setup they queue.
//!
//! The packages install offline with the base system. The images are too large
//! to bundle, so the installer only records the choice and arms
//! `crownos-android-setup.service`, which downloads them after the first boot
//! and removes the pending marker once Android is ready.

use crate::util::pci::{Accelerator, AcceleratorKind};

use super::exec::Cmd;

/// Where the installed system finds whether Android apps were chosen.
pub const ANDROID_CONFIG_PATH: &str = "/etc/crownos/android.ron";

/// The runtime manager, its units and the setup service. Probed for in the
/// live image's offline repository.
pub const ANDROID_PACKAGE: &str = "crownos-android";

/// The host render server for NVIDIA-only machines.
const VENUS_PACKAGE: &str = "crownos-android-venus";

/// Everything `crownos-android` runs on, plus itself.
const RUNTIME_PACKAGES: &[&str] = &[
    "waydroid",
    "lxc",
    "python-gbinder",
    "libgbinder",
    "libglibutil",
    "dnsmasq",
    ANDROID_PACKAGE,
];

const STATE_DIR: &str = "/var/lib/crownos/android";
/// Present while setup still has work to do; gates the setup service.
const PENDING_MARKER: &str = "/var/lib/crownos/android/pending";
const SETUP_SERVICE: &str = "crownos-android-setup.service";
/// The per-user socket that starts `crown-androidd` on demand.
const DAEMON_SOCKET: &str = "crown-androidd.socket";

/// The architectures Android 13 images are built for.
const ARCHITECTURES: &[&str] = &["x86_64", "aarch64"];
/// The only kernel page size those images run on.
const PAGE_SIZE_BYTES: u64 = 4096;

/// Whether a kernel on `architecture` whose page size `getconf PAGESIZE`
/// printed as `page_size` can run Android.
pub fn host_runs_android(architecture: &str, page_size: &str) -> bool {
    ARCHITECTURES.contains(&architecture) && page_size.trim().parse() == Ok(PAGE_SIZE_BYTES)
}

/// The packages `pacstrap` adds for Android apps on a machine with these
/// accelerators.
pub fn packages(accelerators: &[Accelerator]) -> impl Iterator<Item = &'static str> {
    let venus = renders_through_venus(accelerators).then_some(VENUS_PACKAGE);
    RUNTIME_PACKAGES.iter().copied().chain(venus)
}

/// Every GPU is an NVIDIA Turing or later, so no Mesa driver can render
/// Android and Venus has to.
fn renders_through_venus(accelerators: &[Accelerator]) -> bool {
    let mut gpus = accelerators
        .iter()
        .filter(|accelerator| accelerator.kind == AcceleratorKind::Gpu)
        .peekable();
    gpus.peek().is_some() && gpus.all(Accelerator::is_nvidia_turing_or_later)
}

/// `/etc/crownos/android.ron` as the installer writes it: the GPU tier and
/// bundle version stay unset until the setup service has probed the machine.
pub fn config(google_play: bool) -> String {
    format!(
        "(\n    enabled: true,\n    google_play: {google_play},\n    tier: None,\n    bundle: None,\n)\n"
    )
}

/// Records the choice and arms the first-boot setup, for every user.
pub fn commands(root: &str, google_play: bool) -> Vec<Cmd> {
    vec![
        Cmd::WriteFile {
            path: format!("{root}{ANDROID_CONFIG_PATH}"),
            contents: config(google_play),
        },
        Cmd::MakeDirs {
            path: format!("{root}{STATE_DIR}"),
        },
        Cmd::run("touch", &[&format!("{root}{PENDING_MARKER}")]),
        Cmd::chroot(root, "systemctl", &["enable", SETUP_SERVICE]),
        Cmd::chroot(root, "systemctl", &["--global", "enable", DAEMON_SOCKET]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::pci::AcceleratorVendor;

    fn gpu(vendor: AcceleratorVendor, device_id: u16) -> Accelerator {
        Accelerator {
            vendor,
            kind: AcceleratorKind::Gpu,
            device_id,
        }
    }

    const RTX_4050: u16 = 0x28e1;
    const GTX_1080: u16 = 0x1b80;

    #[test]
    fn only_4k_pages_on_x86_64_and_aarch64_run_android() {
        assert!(host_runs_android("x86_64", "4096\n"));
        assert!(host_runs_android("aarch64", "4096"));
        assert!(!host_runs_android("aarch64", "16384\n"));
        assert!(!host_runs_android("riscv64", "4096\n"));
        assert!(!host_runs_android("x86_64", ""));
    }

    #[test]
    fn an_nvidia_only_machine_gets_venus() {
        let nvidia = [gpu(AcceleratorVendor::Nvidia, RTX_4050)];
        assert_eq!(packages(&nvidia).last(), Some(VENUS_PACKAGE));
    }

    #[test]
    fn any_other_gpu_renders_natively() {
        let hybrid = [
            gpu(AcceleratorVendor::Amd, 0x15bf),
            gpu(AcceleratorVendor::Nvidia, RTX_4050),
        ];
        let pascal = [gpu(AcceleratorVendor::Nvidia, GTX_1080)];
        for accelerators in [&hybrid[..], &pascal, &[]] {
            assert!(
                packages(accelerators).eq(RUNTIME_PACKAGES.iter().copied()),
                "{accelerators:?}"
            );
        }
    }

    #[test]
    fn an_npu_is_not_a_second_gpu() {
        let machine = [
            gpu(AcceleratorVendor::Nvidia, RTX_4050),
            Accelerator {
                vendor: AcceleratorVendor::Intel,
                kind: AcceleratorKind::Npu,
                device_id: 0x7d1d,
            },
        ];
        assert!(packages(&machine).any(|package| package == VENUS_PACKAGE));
    }

    #[test]
    fn setup_is_recorded_armed_and_enabled_for_every_user() {
        let commands: Vec<String> = commands("/mnt", false)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(commands[0].starts_with("cat > /mnt/etc/crownos/android.ron"));
        assert!(commands[0].contains("    google_play: false,\n"));
        assert_eq!(
            commands[1..],
            [
                "mkdir -p /mnt/var/lib/crownos/android",
                "touch /mnt/var/lib/crownos/android/pending",
                "arch-chroot /mnt systemctl enable crownos-android-setup.service",
                "arch-chroot /mnt systemctl --global enable crown-androidd.socket",
            ]
        );
    }

    #[test]
    fn the_config_leaves_the_tier_to_the_setup_service() {
        assert_eq!(
            config(true),
            "(\n    enabled: true,\n    google_play: true,\n    tier: None,\n    bundle: None,\n)\n"
        );
    }
}
