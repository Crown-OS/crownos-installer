//! A believable machine for every page to describe: fake disks, a fake handoff payload, and the
//! contexts the two flows start from. Nothing here reads the real system.

use crate::engine::plan::NewIds;
use crate::engine::probe::{Disk, Mount, Partition, SystemSnapshot};
use crate::oobe::{
    AiFeatures, Browser, DeveloperTools, Install, Locale, Oobe, SCHEMA_VERSION, Staging,
    StagingKind, SwapPlan, Target, WindowsApps,
};
use crate::provenance::Provenance;
use crate::ui::Context;
use crate::util::cpuinfo::Microarch;
use crate::util::pci::{Accelerator, AcceleratorKind, AcceleratorVendor};

const SECTOR_SIZE: u32 = 512;
const FIRST_USABLE_SECTOR: u64 = 34;
const GPT_TAIL_SECTORS: u64 = 34;
const GIB_SECTORS: u64 = 2 * 1024 * 1024;
const MIB_SECTORS: u64 = 2 * 1024;
const MEMORY_BYTES: u64 = 32 * 1024 * 1024 * 1024;

const ESP_TYPE: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
const MSR_TYPE: &str = "e3c9e316-0b5c-4db8-817d-f92df00215ae";
const BASIC_DATA_TYPE: &str = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7";
const RECOVERY_TYPE: &str = "de94bba4-06d1-4d40-a16a-bfd50179d6ac";
const LINUX_DATA_TYPE: &str = "0fc63daf-8483-4772-8e79-3d69d8477de4";

const ESP_PARTUUID: &str = "0fc63daf-8483-4772-8e79-3d69d8477de4";
const WINDOWS_PARTUUID: &str = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7";
const STAGING_PARTUUID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

/// 512 GB, the size of the laptop the handoff payload describes.
const LAPTOP_DISK_SECTORS: u64 = 1_000_215_216;
const ONE_TB_SECTORS: u64 = 2_000_409_264;
const TWO_TB_SECTORS: u64 = 3_907_029_168;
const STICK_SECTORS: u64 = 121_110_528;
const PORTABLE_SECTORS: u64 = 976_773_168;

struct Slice {
    start: u64,
    sectors: u64,
    type_guid: &'static str,
    partuuid: &'static str,
    partlabel: &'static str,
    fstype: Option<&'static str>,
    label: Option<&'static str>,
}

fn gpt_disk(path: &str, sectors: u64, guid: &str, slices: &[Slice]) -> Disk {
    let separator = if path.ends_with(|c: char| c.is_ascii_digit()) {
        "p"
    } else {
        ""
    };
    let partitions = slices
        .iter()
        .zip(1..)
        .map(|(slice, number)| Partition {
            path: format!("{path}{separator}{number}"),
            number,
            partuuid: Some(slice.partuuid.to_owned()),
            type_guid: Some(slice.type_guid.to_owned()),
            partlabel: Some(slice.partlabel.to_owned()),
            start_sector: slice.start,
            sectors: slice.sectors,
            fstype: slice.fstype.map(str::to_owned),
            label: slice.label.map(str::to_owned),
        })
        .collect();
    Disk {
        path: path.to_owned(),
        table: "gpt".to_owned(),
        disk_guid: Some(guid.to_owned()),
        sector_size: SECTOR_SIZE,
        first_usable_sector: FIRST_USABLE_SECTOR,
        last_usable_sector: sectors - GPT_TAIL_SECTORS,
        partitions,
    }
}

fn blank_disk(path: &str, sectors: u64) -> Disk {
    Disk {
        path: path.to_owned(),
        table: String::new(),
        disk_guid: None,
        sector_size: SECTOR_SIZE,
        first_usable_sector: FIRST_USABLE_SECTOR,
        last_usable_sector: sectors - GPT_TAIL_SECTORS,
        partitions: Vec::new(),
    }
}

fn windows_front(windows_sectors: u64) -> [Slice; 3] {
    [
        Slice {
            start: 2048,
            sectors: 532_480,
            type_guid: ESP_TYPE,
            partuuid: ESP_PARTUUID,
            partlabel: "EFI system partition",
            fstype: Some("vfat"),
            label: Some("SYSTEM"),
        },
        Slice {
            start: 534_528,
            sectors: 32_768,
            type_guid: MSR_TYPE,
            partuuid: "11111111-2222-3333-4444-555555555555",
            partlabel: "Microsoft reserved partition",
            fstype: None,
            label: None,
        },
        Slice {
            start: 567_296,
            sectors: windows_sectors,
            type_guid: BASIC_DATA_TYPE,
            partuuid: WINDOWS_PARTUUID,
            partlabel: "Basic data partition",
            fstype: Some("ntfs"),
            label: Some("Windows"),
        },
    ]
}

/// The laptop Windows shrank for CrownOS: the ESP, MSR and C: at the front, the 1 GiB staging
/// partition the Windows installer carved, and ~94 GiB free behind it.
fn laptop_disk() -> Disk {
    let [esp, msr, windows] = windows_front(799_465_472);
    let staging = Slice {
        start: 800_032_768,
        sectors: GIB_SECTORS,
        type_guid: BASIC_DATA_TYPE,
        partuuid: STAGING_PARTUUID,
        partlabel: "CROWNSTAGE",
        fstype: Some("vfat"),
        label: Some("CROWNSTAGE"),
    };
    gpt_disk(
        "/dev/nvme0n1",
        LAPTOP_DISK_SECTORS,
        "4c1e7a52-90d3-4f1b-9a35-2b8e6d0c7f11",
        &[esp, msr, windows, staging],
    )
}

/// A 1 TB desktop SSD with Windows and its recovery partition, and ~352 GiB never allocated.
fn desktop_disk() -> Disk {
    let windows_sectors = 600 * GIB_SECTORS;
    let [esp, msr, windows] = windows_front(windows_sectors);
    let recovery = Slice {
        start: 567_296 + windows_sectors,
        sectors: 750 * MIB_SECTORS,
        type_guid: RECOVERY_TYPE,
        partuuid: "6b1f3c2e-8d4a-4e7b-b0c9-3a5d1e2f4a6b",
        partlabel: "Basic data partition",
        fstype: Some("ntfs"),
        label: Some("Recovery"),
    };
    gpt_disk(
        "/dev/nvme0n1",
        ONE_TB_SECTORS,
        "9e2d4b71-3c58-4a06-8f1e-7b2c5d9a0e34",
        &[esp, msr, windows, recovery],
    )
}

/// A 2 TB drive given over entirely to a games library.
fn full_disk() -> Disk {
    gpt_disk(
        "/dev/nvme1n1",
        TWO_TB_SECTORS,
        "1f6a9c3d-5e27-4b80-a4d1-8c3e2b7f9a05",
        &[Slice {
            start: 2048,
            sectors: TWO_TB_SECTORS - GPT_TAIL_SECTORS - 2048 + 1,
            type_guid: LINUX_DATA_TYPE,
            partuuid: "c7d2e9a1-4b36-4f58-9e0d-2a7c1b5f8e63",
            partlabel: "Games",
            fstype: Some("ext4"),
            label: Some("Games"),
        }],
    )
}

/// A portable drive with photo backups on one half and the other half empty.
fn portable_disk() -> Disk {
    gpt_disk(
        "/dev/sdc",
        PORTABLE_SECTORS,
        "5a8e1d3f-7c24-4b96-8d0a-3e6f2c9b1a47",
        &[Slice {
            start: 2048,
            sectors: 240 * GIB_SECTORS,
            type_guid: BASIC_DATA_TYPE,
            partuuid: "e4b8c1d6-2a95-4f37-b0e8-6d3a9c2f5b18",
            partlabel: "Photos",
            fstype: Some("exfat"),
            label: Some("Photos"),
        }],
    )
}

/// A USB stick with an old MBR table, which CrownOS cannot install onto.
fn mbr_stick() -> Disk {
    Disk {
        path: "/dev/sdb".to_owned(),
        table: "dos".to_owned(),
        disk_guid: None,
        sector_size: SECTOR_SIZE,
        first_usable_sector: 2048,
        last_usable_sector: STICK_SECTORS - 1,
        partitions: vec![Partition {
            path: "/dev/sdb1".to_owned(),
            number: 1,
            partuuid: Some("3a1c5e7f-01".to_owned()),
            type_guid: None,
            partlabel: None,
            start_sector: 2048,
            sectors: 32 * GIB_SECTORS,
            fstype: Some("vfat".to_owned()),
            label: Some("STICK".to_owned()),
        }],
    }
}

fn live_mounts() -> Vec<Mount> {
    [
        ("airootfs", "/", "overlay"),
        ("tmpfs", "/run/archiso/copytoram", "tmpfs"),
        ("/dev/loop0", "/run/archiso/airootfs", "squashfs"),
    ]
    .into_iter()
    .map(|(source, target, fstype)| Mount {
        source: source.to_owned(),
        target: target.to_owned(),
        fstype: fstype.to_owned(),
    })
    .collect()
}

fn snapshot(disks: Vec<Disk>) -> SystemSnapshot {
    SystemSnapshot {
        disks,
        mounts: live_mounts(),
        memory_bytes: MEMORY_BYTES,
        microarch: Microarch::V3,
        crownos_repo_key: true,
        published_packages: Some(
            ["crownbar", "crowndock", "crownos-appstore"]
                .map(str::to_owned)
                .to_vec(),
        ),
        published_version: Some("0.1.12".to_owned()),
        accelerators: vec![Accelerator {
            vendor: AcceleratorVendor::Nvidia,
            kind: AcceleratorKind::Gpu,
            device_id: 0x2684,
        }],
        ai_bundled: true,
    }
}

fn ids() -> NewIds {
    NewIds {
        root_partuuid: "3f9a6c2e-1b47-4d85-9e0a-7c2b5d8f1e63".to_owned(),
        swap_partuuid: "8d1e4a7b-2c59-4f36-a0b8-5e3c9d2f7a14".to_owned(),
        boot_partuuid: "b2c7e5a9-4d18-4e63-9f0b-1a6d3c8e2f57".to_owned(),
    }
}

const ISO_NOTE: &str = "No handoff file on the kernel command line, so this is a flashed image: every choice is \
     asked here.";

/// A flashed image booted on the desktop: the Windows SSD, an empty 2 TB drive and a USB stick.
pub fn iso_context() -> Context {
    iso_context_with(vec![
        desktop_disk(),
        blank_disk("/dev/sda", TWO_TB_SECTORS),
        mbr_stick(),
    ])
}

/// The same desktop with every drive bay full, so the disk page offers a search.
pub fn crowded_context() -> Context {
    iso_context_with(vec![
        desktop_disk(),
        full_disk(),
        blank_disk("/dev/sda", TWO_TB_SECTORS),
        mbr_stick(),
        portable_disk(),
        blank_disk("/dev/sdd", PORTABLE_SECTORS),
    ])
}

pub fn iso_context_with(disks: Vec<Disk>) -> Context {
    Context {
        provenance: Provenance::Iso,
        notes: vec![ISO_NOTE.to_owned()],
        snapshot: snapshot(disks),
        ids: ids(),
        auto_install: false,
        online: true,
    }
}

/// The laptop rebooted out of Windows, with the payload the Windows installer left on it.
pub fn windows_context(auto_install: bool) -> Context {
    windows_context_shifted(auto_install, 0)
}

/// [`windows_context`] whose payload remembers the free space starting `shift` bytes later than
/// it does, as if the disk changed after Windows handed over.
pub fn windows_context_shifted(auto_install: bool, shift: u64) -> Context {
    let snapshot = snapshot(vec![laptop_disk()]);
    let disk = &snapshot.disks[0];
    let gap = disk
        .free_gaps()
        .last()
        .copied()
        .unwrap_or(crate::engine::probe::Gap {
            start_sector: 0,
            sectors: 0,
        });
    let oobe = Oobe {
        schema_version: SCHEMA_VERSION,
        generated_by: "windows-migration-assistant".to_owned(),
        generated_at_unix: 1_794_268_800,
        install: Install {
            auto_install,
            allocated_bytes: disk.bytes(gap.sectors),
            swap: SwapPlan::None,
            keep_windows: true,
            ai_features: AiFeatures::Disabled,
            windows_apps: WindowsApps::Disabled,
            browser: Browser::Chromium,
            developer_tools: DeveloperTools::default(),
        },
        target: Target {
            esp_partuuid: ESP_PARTUUID.to_owned(),
            esp_size_bytes: 260 * 1024 * 1024,
            windows_partuuid: WINDOWS_PARTUUID.to_owned(),
            disk_guid: None,
            windows_disk_number: 0,
            expected_free_start_bytes: disk.bytes(gap.start_sector) + shift,
            expected_free_bytes: disk.bytes(gap.sectors),
            logical_sector_size: SECTOR_SIZE,
        },
        staging: Staging {
            kind: StagingKind::Partition,
            partuuid: STAGING_PARTUUID.to_owned(),
            base_dir: "EFI/ArchStage".to_owned(),
            reclaim_after_install: true,
        },
        user: None,
        locale: Locale::default(),
    };
    Context {
        provenance: Provenance::WindowsInstaller {
            oobe: Box::new(oobe),
            source: "/run/archiso/bootmnt/EFI/ArchStage/oobe.json".to_owned(),
        },
        notes: Vec::new(),
        snapshot,
        ids: ids(),
        auto_install,
        online: true,
    }
}
