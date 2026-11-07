//! The graphics drivers a machine gets, decided from its GPUs.
//!
//! Mesa and the 64-bit Vulkan drivers come with the desktop. This adds the
//! 32-bit drivers Steam and Wine need, the NVIDIA driver the GPU supports and
//! the hybrid-graphics tools. Everything but the NVIDIA 580 driver is bundled
//! on the live image, so crownos-iso's `profile/install.packages` must name
//! every package in the offline lists below.

use crate::util::pci::{Accelerator, AcceleratorVendor, NvidiaGeneration};

use super::base;
use super::exec::Cmd;
use super::plan::ESP_MOUNT_REL;

const MESA_32BIT: &str = "lib32-mesa";
const RADEON_VULKAN_32BIT: &str = "lib32-vulkan-radeon";
const INTEL_VULKAN_32BIT: &str = "lib32-vulkan-intel";
const NOUVEAU_VULKAN_32BIT: &str = "lib32-vulkan-nouveau";

/// The open kernel modules prebuilt for [`base::KERNEL`].
const NVIDIA_OPEN_PACKAGES: &[&str] = &[
    "linux-cachyos-nvidia-open",
    "nvidia-utils",
    "lib32-nvidia-utils",
    PRIME_RUN,
];

/// CachyOS's build of the last branch that drives Maxwell to Volta. Only a
/// DKMS module exists for it, so the kernel headers come along.
const NVIDIA_580_PACKAGES: &[&str] = &[
    "linux-cachyos-headers",
    "nvidia-580xx-dkms",
    "nvidia-580xx-utils",
    "lib32-nvidia-580xx-utils",
    PRIME_RUN,
];

/// `prime-run`, which starts a program on the NVIDIA GPU.
const PRIME_RUN: &str = "nvidia-prime";

const SWITCHEROO: &str = "switcheroo-control";
/// Tells launchers which GPU a program that prefers the other one should use.
pub const SWITCHEROO_SERVICE: &str = "switcheroo-control.service";

const NVIDIA_MODPROBE_CONF: &str = "/etc/modprobe.d/crownos-nvidia.conf";
const NVIDIA_MODPROBE_OPTIONS: &str = "options nvidia_drm modeset=1 fbdev=1\n";

/// Which kernel driver runs an NVIDIA GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvidiaDriver {
    Open,
    Proprietary580,
    /// No NVIDIA driver supports Kepler and older with GBM on current kernels.
    Nouveau,
}

impl NvidiaDriver {
    fn for_generation(generation: NvidiaGeneration) -> Self {
        match generation {
            NvidiaGeneration::TuringOrLater => Self::Open,
            NvidiaGeneration::MaxwellToVolta => Self::Proprietary580,
            NvidiaGeneration::PreMaxwell => Self::Nouveau,
        }
    }
}

/// The graphics packages and services a machine gets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphicsPlan {
    /// The driver for the machine's NVIDIA GPUs, if it has any.
    pub nvidia_driver: Option<NvidiaDriver>,
    /// Installed with the base system, from the live image's offline repository.
    pub offline_packages: Vec<&'static str>,
    /// Fetched once the repositories are configured. The system runs on
    /// Nouveau if the fetch fails.
    pub online_packages: Vec<&'static str>,
    pub multi_gpu: bool,
}

impl GraphicsPlan {
    fn uses_nvidia_driver(&self) -> bool {
        matches!(
            self.nvidia_driver,
            Some(NvidiaDriver::Open | NvidiaDriver::Proprietary580)
        )
    }

    /// Whether [`Self::commands`] has anything to do.
    pub fn needs_setup(&self) -> bool {
        self.uses_nvidia_driver() || !self.online_packages.is_empty()
    }

    /// The unit to enable in the installed system.
    pub fn service(&self) -> Option<&'static str> {
        self.multi_gpu.then_some(SWITCHEROO_SERVICE)
    }

    /// Kernel mode setting for the NVIDIA driver, then whatever could not be
    /// bundled. A `pacman -Syu` may replace the kernel, so it is copied onto
    /// the ESP again afterwards.
    pub fn commands(&self, root: &str) -> Vec<Cmd> {
        let mut commands = Vec::new();
        if self.uses_nvidia_driver() {
            commands.push(Cmd::WriteFile {
                path: format!("{root}{NVIDIA_MODPROBE_CONF}"),
                contents: NVIDIA_MODPROBE_OPTIONS.to_owned(),
            });
        }
        if !self.online_packages.is_empty() {
            let mut args = vec!["-Syu", "--needed", "--noconfirm"];
            args.extend(&self.online_packages);
            commands.push(Cmd::best_effort(Cmd::chroot(root, "pacman", &args)));
            commands.push(base::copy_kernel_to_esp(root, ESP_MOUNT_REL));
        }
        commands
    }
}

/// The graphics stack for a machine with these accelerators.
pub fn graphics_plan(accelerators: &[Accelerator]) -> GraphicsPlan {
    let gpus: Vec<&Accelerator> = accelerators.iter().filter(|a| a.is_gpu()).collect();
    let has_vendor = |vendor| gpus.iter().any(|gpu| gpu.vendor == vendor);
    let nvidia_driver = gpus
        .iter()
        .filter_map(|gpu| gpu.nvidia_generation())
        .min()
        .map(NvidiaDriver::for_generation);
    let mesa_vulkan = [
        (has_vendor(AcceleratorVendor::Amd), RADEON_VULKAN_32BIT),
        (has_vendor(AcceleratorVendor::Intel), INTEL_VULKAN_32BIT),
        (
            nvidia_driver.is_some_and(|driver| driver != NvidiaDriver::Open),
            NOUVEAU_VULKAN_32BIT,
        ),
    ]
    .into_iter()
    .filter_map(|(present, package)| present.then_some(package));
    let mut offline_packages: Vec<&'static str> = mesa_vulkan.collect();
    if !offline_packages.is_empty() {
        offline_packages.push(MESA_32BIT);
    }
    let multi_gpu = gpus.len() > 1;
    if multi_gpu {
        offline_packages.push(SWITCHEROO);
    }
    let online_packages = match nvidia_driver {
        Some(NvidiaDriver::Open) => {
            offline_packages.extend(NVIDIA_OPEN_PACKAGES);
            Vec::new()
        }
        Some(NvidiaDriver::Proprietary580) => NVIDIA_580_PACKAGES.to_vec(),
        Some(NvidiaDriver::Nouveau) | None => Vec::new(),
    };
    offline_packages.sort_unstable();
    GraphicsPlan {
        nvidia_driver,
        offline_packages,
        online_packages,
        multi_gpu,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::pci::AcceleratorKind;

    const fn gpu(vendor: AcceleratorVendor, device_id: u16) -> Accelerator {
        Accelerator {
            vendor,
            kind: AcceleratorKind::Gpu,
            device_id,
        }
    }

    const RTX_4060_LAPTOP: Accelerator = gpu(AcceleratorVendor::Nvidia, 0x28e0);
    const GTX_1080: Accelerator = gpu(AcceleratorVendor::Nvidia, 0x1b80);
    const GTX_780: Accelerator = gpu(AcceleratorVendor::Nvidia, 0x1004);
    const RADEON_7900_XTX: Accelerator = gpu(AcceleratorVendor::Amd, 0x744c);
    const RAPTOR_LAKE_IGPU: Accelerator = gpu(AcceleratorVendor::Intel, 0xa7a0);
    const METEOR_LAKE_NPU: Accelerator = Accelerator {
        vendor: AcceleratorVendor::Intel,
        kind: AcceleratorKind::Npu,
        device_id: 0x7d1d,
    };

    fn writes_modeset(plan: &GraphicsPlan) -> bool {
        plan.commands("/mnt").iter().any(|command| {
            matches!(command, Cmd::WriteFile { path, contents }
                if path == "/mnt/etc/modprobe.d/crownos-nvidia.conf"
                    && contents.contains("nvidia_drm modeset=1 fbdev=1"))
        })
    }

    #[test]
    fn an_nvidia_laptop_with_an_intel_igpu_gets_the_open_driver_and_prime() {
        let plan = graphics_plan(&[RAPTOR_LAKE_IGPU, RTX_4060_LAPTOP]);
        assert_eq!(plan.nvidia_driver, Some(NvidiaDriver::Open));
        assert_eq!(
            plan.offline_packages,
            [
                "lib32-mesa",
                "lib32-nvidia-utils",
                "lib32-vulkan-intel",
                "linux-cachyos-nvidia-open",
                "nvidia-prime",
                "nvidia-utils",
                "switcheroo-control",
            ]
        );
        assert!(plan.online_packages.is_empty());
        assert_eq!(plan.service(), Some(SWITCHEROO_SERVICE));
        assert!(writes_modeset(&plan));
    }

    #[test]
    fn an_amd_desktop_gets_the_32_bit_mesa_drivers_only() {
        let plan = graphics_plan(&[RADEON_7900_XTX]);
        assert_eq!(plan.nvidia_driver, None);
        assert_eq!(plan.offline_packages, ["lib32-mesa", "lib32-vulkan-radeon"]);
        assert!(plan.online_packages.is_empty());
        assert_eq!(plan.service(), None);
        assert!(!plan.needs_setup());
        assert!(plan.commands("/mnt").is_empty());
    }

    #[test]
    fn an_nvidia_only_desktop_needs_no_mesa_or_switcheroo() {
        let plan = graphics_plan(&[RTX_4060_LAPTOP]);
        assert!(!plan.offline_packages.contains(&MESA_32BIT));
        assert!(!plan.offline_packages.contains(&SWITCHEROO));
        assert_eq!(plan.service(), None);
    }

    #[test]
    fn pascal_fetches_the_580_driver_and_keeps_nouveau_as_the_fallback() {
        let plan = graphics_plan(&[GTX_1080]);
        assert_eq!(plan.nvidia_driver, Some(NvidiaDriver::Proprietary580));
        assert_eq!(
            plan.offline_packages,
            ["lib32-mesa", "lib32-vulkan-nouveau"]
        );
        assert!(plan.online_packages.contains(&"nvidia-580xx-dkms"));
        assert!(plan.online_packages.contains(&"lib32-nvidia-580xx-utils"));
        assert!(plan.online_packages.contains(&PRIME_RUN));
        let commands = plan.commands("/mnt");
        assert!(writes_modeset(&plan));
        assert!(matches!(commands[1], Cmd::BestEffort(_)));
        assert!(
            commands[2]
                .to_string()
                .contains("/boot/vmlinuz-linux-cachyos /efi/EFI/CrownOS/vmlinuz-linux-cachyos")
        );
    }

    #[test]
    fn kepler_stays_on_nouveau() {
        let plan = graphics_plan(&[GTX_780]);
        assert_eq!(plan.nvidia_driver, Some(NvidiaDriver::Nouveau));
        assert_eq!(
            plan.offline_packages,
            ["lib32-mesa", "lib32-vulkan-nouveau"]
        );
        assert!(plan.online_packages.is_empty());
        assert!(!writes_modeset(&plan));
    }

    #[test]
    fn mixed_nvidia_generations_get_the_driver_that_runs_both() {
        let plan = graphics_plan(&[GTX_1080, RTX_4060_LAPTOP]);
        assert_eq!(plan.nvidia_driver, Some(NvidiaDriver::Proprietary580));
        assert!(!plan.offline_packages.contains(&"nvidia-utils"));
    }

    #[test]
    fn an_npu_is_not_a_second_gpu() {
        let plan = graphics_plan(&[RAPTOR_LAKE_IGPU, METEOR_LAKE_NPU]);
        assert_eq!(plan.service(), None);
        assert_eq!(plan.offline_packages, ["lib32-mesa", "lib32-vulkan-intel"]);
    }

    #[test]
    fn two_mesa_gpus_get_switcheroo() {
        let plan = graphics_plan(&[RADEON_7900_XTX, RAPTOR_LAKE_IGPU]);
        assert_eq!(plan.service(), Some(SWITCHEROO_SERVICE));
        assert!(plan.offline_packages.contains(&SWITCHEROO));
    }

    #[test]
    fn no_gpu_means_nothing_to_add() {
        assert_eq!(graphics_plan(&[]), GraphicsPlan::default());
    }

    #[test]
    fn kernel_packages_match_the_installed_kernel() {
        for package in [NVIDIA_OPEN_PACKAGES[0], NVIDIA_580_PACKAGES[0]] {
            assert!(package.starts_with(&format!("{}-", base::KERNEL)));
        }
    }
}
