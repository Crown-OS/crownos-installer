//! PCI device identification from the hex strings sysfs exposes under
//! `/sys/bus/pci/devices/<slot>/{vendor,device,class}`.

const NVIDIA: u16 = 0x10de;
const ATI: u16 = 0x1002;
const AMD: u16 = 0x1022;
const INTEL: u16 = 0x8086;

const DISPLAY_CLASS: u32 = 0x03;
const PROCESSING_ACCELERATOR_CLASS: u32 = 0x12;
const SIGNAL_PROCESSING_SUBCLASS: u32 = 0x1180;

/// The first Turing device ID. NVIDIA allocates IDs by generation, so every
/// GPU from here on has the hardware the open kernel modules and Maxine need.
const NVIDIA_FIRST_TURING_DEVICE: u16 = 0x1e00;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AcceleratorVendor {
    Nvidia,
    Amd,
    Intel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AcceleratorKind {
    Gpu,
    Npu,
}

/// A GPU or NPU that can run inference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Accelerator {
    pub vendor: AcceleratorVendor,
    pub kind: AcceleratorKind,
    pub device_id: u16,
}

impl Accelerator {
    /// Whether this is an NVIDIA GPU of the Turing generation or later.
    pub fn is_nvidia_turing_or_later(&self) -> bool {
        self.vendor == AcceleratorVendor::Nvidia
            && self.kind == AcceleratorKind::Gpu
            && self.device_id >= NVIDIA_FIRST_TURING_DEVICE
    }
}

fn parse_hex(value: &str) -> Option<u32> {
    let digits = value.trim();
    u32::from_str_radix(digits.strip_prefix("0x").unwrap_or(digits), 16).ok()
}

fn vendor_of(id: u32) -> Option<AcceleratorVendor> {
    match u16::try_from(id).ok()? {
        NVIDIA => Some(AcceleratorVendor::Nvidia),
        ATI | AMD => Some(AcceleratorVendor::Amd),
        INTEL => Some(AcceleratorVendor::Intel),
        _ => None,
    }
}

fn kind_of(class: u32) -> Option<AcceleratorKind> {
    let base_class = class >> 16;
    let subclass = class >> 8;
    match (base_class, subclass) {
        (DISPLAY_CLASS, _) => Some(AcceleratorKind::Gpu),
        (PROCESSING_ACCELERATOR_CLASS, _) | (_, SIGNAL_PROCESSING_SUBCLASS) => {
            Some(AcceleratorKind::Npu)
        }
        _ => None,
    }
}

/// The accelerator a device's sysfs `vendor`, `device` and `class` files
/// describe, or `None` for anything else.
pub fn accelerator(vendor: &str, device: &str, class: &str) -> Option<Accelerator> {
    Some(Accelerator {
        vendor: vendor_of(parse_hex(vendor)?)?,
        kind: kind_of(parse_hex(class)?)?,
        device_id: u16::try_from(parse_hex(device)?).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_nvidia_vga_controller_is_a_gpu() {
        let rtx_4090 = accelerator("0x10de\n", "0x2684\n", "0x030000\n");
        assert_eq!(
            rtx_4090,
            Some(Accelerator {
                vendor: AcceleratorVendor::Nvidia,
                kind: AcceleratorKind::Gpu,
                device_id: 0x2684,
            })
        );
        assert!(rtx_4090.is_some_and(|gpu| gpu.is_nvidia_turing_or_later()));
    }

    #[test]
    fn pascal_predates_turing() {
        let gtx_1080 = accelerator("0x10de", "0x1b80", "0x030000");
        assert!(gtx_1080.is_some_and(|gpu| !gpu.is_nvidia_turing_or_later()));
    }

    #[test]
    fn an_amd_3d_controller_is_a_gpu() {
        let accelerator = accelerator("0x1002", "0x744c", "0x038000");
        assert_eq!(accelerator.map(|a| a.kind), Some(AcceleratorKind::Gpu));
        assert_eq!(accelerator.map(|a| a.vendor), Some(AcceleratorVendor::Amd));
    }

    #[test]
    fn intel_and_amd_npus_are_npus() {
        let meteor_lake = accelerator("0x8086", "0x7d1d", "0x120000");
        let ryzen_ai = accelerator("0x1022", "0x1502", "0x118000");
        assert_eq!(meteor_lake.map(|a| a.kind), Some(AcceleratorKind::Npu));
        assert_eq!(ryzen_ai.map(|a| a.kind), Some(AcceleratorKind::Npu));
    }

    #[test]
    fn other_devices_and_vendors_are_ignored() {
        assert_eq!(accelerator("0x8086", "0x7a60", "0x0c0330"), None);
        assert_eq!(accelerator("0x1af4", "0x1050", "0x030000"), None);
        assert_eq!(accelerator("garbage", "0x0", "0x030000"), None);
    }
}
