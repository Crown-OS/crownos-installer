//! The on-device AI stack a machine gets, decided from its accelerators.
//!
//! Each backend is one `crownos-ai-*` meta-package. The CPU and NVIDIA metas
//! are bundled and installed offline with the base system; the vendor runtimes
//! too large to bundle are fetched over the network on a best effort basis.
//! Without one, the installed system runs the same effects on the next backend
//! down [`AiPlan::backends`].

use std::fmt::Write as _;

use crate::util::pci::{Accelerator, AcceleratorKind, AcceleratorVendor};

use super::exec::Cmd;

/// Where the installed system finds what was detected and chosen.
pub const AI_CONFIG_PATH: &str = "/etc/crownos/ai.ron";

/// Downloads the NVIDIA Maxine SDK from NGC. Shipped by [`NVIDIA_META`].
const MAXINE_FETCH: &str = "crownos-maxine-fetch";

/// The CPU runtime and every app's CPU backend. Every other meta is a faster
/// way to run the same effects.
pub const CPU_META: &str = "crownos-ai-cpu";
/// The open kernel modules, the userspace driver and the Maxine plugins.
const NVIDIA_META: &str = "crownos-ai-nvidia";
const NVIDIA_CUDA_META: &str = "crownos-ai-nvidia-cuda";
const AMD_META: &str = "crownos-ai-amd";
const INTEL_META: &str = "crownos-ai-intel";

/// An inference stack, in order of preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Backend {
    Maxine,
    Cuda,
    MiGraphX,
    OpenVino,
    VitisAi,
    Cpu,
}

impl Backend {
    fn for_accelerator(accelerator: &Accelerator) -> &'static [Self] {
        match (accelerator.vendor, accelerator.kind) {
            (AcceleratorVendor::Nvidia, _) if accelerator.is_nvidia_turing_or_later() => {
                &[Self::Maxine, Self::Cuda]
            }
            (AcceleratorVendor::Nvidia, _) => &[],
            (AcceleratorVendor::Amd, AcceleratorKind::Gpu) => &[Self::MiGraphX],
            (AcceleratorVendor::Amd, AcceleratorKind::Npu) => &[Self::VitisAi],
            (AcceleratorVendor::Intel, _) => &[Self::OpenVino],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Maxine => "NVIDIA Maxine",
            Self::Cuda => "CUDA",
            Self::MiGraphX => "AMD MIGraphX",
            Self::OpenVino => "Intel OpenVINO",
            Self::VitisAi => "AMD Ryzen AI",
            Self::Cpu => "the CPU",
        }
    }

    /// The meta-package that installs this backend, and whether the live image
    /// bundles it.
    fn package(self) -> (&'static str, Bundled) {
        match self {
            Self::Maxine => (NVIDIA_META, Bundled::Offline),
            Self::Cpu => (CPU_META, Bundled::Offline),
            Self::Cuda => (NVIDIA_CUDA_META, Bundled::Online),
            Self::MiGraphX | Self::VitisAi => (AMD_META, Bundled::Online),
            Self::OpenVino => (INTEL_META, Bundled::Online),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bundled {
    Offline,
    Online,
}

/// What AI acceleration is installed, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiPlan {
    pub accelerators: Vec<Accelerator>,
    /// Usable backends, best first. Always ends in [`Backend::Cpu`].
    pub backends: Vec<Backend>,
}

impl AiPlan {
    fn packages(&self, bundled: Bundled) -> Vec<&'static str> {
        let mut packages: Vec<&'static str> = self
            .backends
            .iter()
            .map(|backend| backend.package())
            .filter(|(_, how)| *how == bundled)
            .map(|(package, _)| package)
            .collect();
        packages.sort_unstable();
        packages.dedup();
        packages
    }

    /// Installed with the base system, from the live image's offline repository.
    pub fn offline_packages(&self) -> Vec<&'static str> {
        self.packages(Bundled::Offline)
    }

    /// Fetched from the network once the repositories are configured.
    pub fn online_packages(&self) -> Vec<&'static str> {
        self.packages(Bundled::Online)
    }

    fn uses_maxine(&self) -> bool {
        self.backends.contains(&Backend::Maxine)
    }

    /// `/etc/crownos/ai.ron`.
    pub fn config(&self) -> String {
        let mut config = String::from("(\n    accelerators: [\n");
        for accelerator in &self.accelerators {
            let _ = writeln!(
                config,
                "        (vendor: {:?}, kind: {:?}, device_id: {:#06x}),",
                accelerator.vendor, accelerator.kind, accelerator.device_id
            );
        }
        let backends: Vec<String> = self
            .backends
            .iter()
            .map(|backend| format!("{backend:?}"))
            .collect();
        let _ = writeln!(
            config,
            "    ],\n    backends: [{}],\n)",
            backends.join(", ")
        );
        config
    }

    /// Writes the config, then fetches what could not be bundled. A fetch that
    /// fails leaves the system on a slower backend, never without one.
    pub fn commands(&self, root: &str) -> Vec<Cmd> {
        let mut commands = vec![Cmd::WriteFile {
            path: format!("{root}{AI_CONFIG_PATH}"),
            contents: self.config(),
        }];
        let online = self.online_packages();
        if !online.is_empty() {
            let mut args = vec!["-Syu", "--needed", "--noconfirm"];
            args.extend(online);
            commands.push(Cmd::best_effort(Cmd::chroot(root, "pacman", &args)));
        }
        if self.uses_maxine() {
            commands.push(Cmd::best_effort(Cmd::chroot(root, MAXINE_FETCH, &[])));
        }
        commands
    }
}

/// The AI stack for a machine with these accelerators.
pub fn ai_plan(accelerators: &[Accelerator]) -> AiPlan {
    let mut backends: Vec<Backend> = accelerators
        .iter()
        .flat_map(Backend::for_accelerator)
        .copied()
        .chain([Backend::Cpu])
        .collect();
    backends.sort_unstable();
    backends.dedup();
    AiPlan {
        accelerators: accelerators.to_vec(),
        backends,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(vendor: AcceleratorVendor, kind: AcceleratorKind, device_id: u16) -> Accelerator {
        Accelerator {
            vendor,
            kind,
            device_id,
        }
    }

    const RTX_4090: Accelerator = Accelerator {
        vendor: AcceleratorVendor::Nvidia,
        kind: AcceleratorKind::Gpu,
        device_id: 0x2684,
    };

    #[test]
    fn no_accelerator_means_cpu_only_and_nothing_to_fetch() {
        let plan = ai_plan(&[]);
        assert_eq!(plan.backends, [Backend::Cpu]);
        assert_eq!(plan.offline_packages(), [CPU_META]);
        assert!(plan.online_packages().is_empty());
        assert_eq!(plan.commands("/mnt").len(), 1);
    }

    #[test]
    fn a_turing_or_later_nvidia_gpu_gets_the_driver_and_maxine_offline() {
        let plan = ai_plan(&[RTX_4090]);
        assert_eq!(
            plan.backends,
            [Backend::Maxine, Backend::Cuda, Backend::Cpu]
        );
        assert_eq!(plan.offline_packages(), [CPU_META, NVIDIA_META]);
        assert_eq!(plan.online_packages(), [NVIDIA_CUDA_META]);
        assert!(
            plan.commands("/mnt")
                .iter()
                .any(|command| command.to_string().contains(MAXINE_FETCH))
        );
    }

    #[test]
    fn a_pascal_gpu_stays_on_the_cpu() {
        let gtx_1080 = device(AcceleratorVendor::Nvidia, AcceleratorKind::Gpu, 0x1b80);
        assert_eq!(ai_plan(&[gtx_1080]).backends, [Backend::Cpu]);
    }

    #[test]
    fn intel_gpu_and_npu_share_one_openvino_fetch() {
        let plan = ai_plan(&[
            device(AcceleratorVendor::Intel, AcceleratorKind::Gpu, 0x7d55),
            device(AcceleratorVendor::Intel, AcceleratorKind::Npu, 0x7d1d),
        ]);
        assert_eq!(plan.backends, [Backend::OpenVino, Backend::Cpu]);
        assert_eq!(plan.online_packages(), [INTEL_META]);
    }

    #[test]
    fn backends_are_ordered_by_preference_not_by_bus_order() {
        let plan = ai_plan(&[
            device(AcceleratorVendor::Amd, AcceleratorKind::Npu, 0x1502),
            device(AcceleratorVendor::Amd, AcceleratorKind::Gpu, 0x744c),
            RTX_4090,
        ]);
        assert_eq!(
            plan.backends,
            [
                Backend::Maxine,
                Backend::Cuda,
                Backend::MiGraphX,
                Backend::VitisAi,
                Backend::Cpu
            ]
        );
        assert_eq!(plan.online_packages(), [AMD_META, NVIDIA_CUDA_META]);
    }

    #[test]
    fn fetches_are_best_effort() {
        let amd = device(AcceleratorVendor::Amd, AcceleratorKind::Gpu, 0x744c);
        let commands = ai_plan(&[amd, RTX_4090]).commands("/mnt");
        assert!(matches!(commands[0], Cmd::WriteFile { .. }));
        assert!(
            commands[1..]
                .iter()
                .all(|command| matches!(command, Cmd::BestEffort(_)))
        );
    }

    #[test]
    fn the_config_names_every_accelerator_and_backend() {
        let config = ai_plan(&[RTX_4090]).config();
        assert!(config.contains("(vendor: Nvidia, kind: Gpu, device_id: 0x2684)"));
        assert!(config.contains("backends: [Maxine, Cuda, Cpu]"));
    }
}
