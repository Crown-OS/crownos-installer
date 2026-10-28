//! What an ISO install is going to do, as chosen so far.

use crate::engine::disks::TYPE_EFI_SYSTEM;
use crate::engine::probe::{Disk, Partition, SystemSnapshot};
use crate::oobe::{Locale, Oobe, RootFilesystem, SwapPlan};

/// The values a handoff payload would have carried. `None` means "not chosen
/// yet", which is a state the Windows flow never has and the ISO flow starts in.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub disk: Option<String>,
    pub esp: Option<String>,
    /// First sector of the free region root goes in. The region is identified by
    /// where it starts rather than by an index into a list, because a queued
    /// partition edit changes the list and must not silently change which region
    /// the user picked.
    pub region_start_sector: Option<u64>,
    pub filesystem: RootFilesystem,
    pub swap: SwapPlan,
    pub locale: Locale,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            disk: None,
            esp: None,
            region_start_sector: None,
            filesystem: RootFilesystem::Ext4,
            swap: SwapPlan::None,
            locale: Locale::default(),
        }
    }
}

impl Target {
    /// A first guess, so the first frame has a plan to show and the disk page
    /// opens with something selected rather than with nothing.
    ///
    /// The disk with the largest unallocated region *that already has an ESP*,
    /// because that is the one an install can proceed on without editing
    /// anything. A disk with more free space and no ESP is a perfectly good
    /// target and needs a partition created first, which is a decision for the
    /// user rather than a default.
    pub fn best_for(snapshot: &SystemSnapshot) -> Self {
        snapshot
            .disks
            .iter()
            .filter(|disk| disk.table == "gpt")
            .filter_map(|disk| {
                let esp = esp_of(disk)?;
                let gap = disk.free_gaps().into_iter().max_by_key(|gap| gap.sectors)?;
                Some((disk.bytes(gap.sectors), disk, esp, gap.start_sector))
            })
            .reduce(|best, next| if next.0 > best.0 { next } else { best })
            .map_or_else(Self::default, |(_, disk, esp, start)| Self {
                disk: Some(disk.path.clone()),
                esp: Some(esp.path.clone()),
                region_start_sector: Some(start),
                ..Self::default()
            })
    }

    /// The disk, ESP and free region a payload names.
    ///
    /// So that the partition editor opens on the disk the install is about. The
    /// planner still finds its own disk by PARTUUID — this is for the editor's
    /// benefit and nothing decides anything from it, which is why an unresolvable
    /// payload falls back to the same guess a flashed image gets rather than
    /// refusing here.
    pub fn from_payload(oobe: &Oobe, snapshot: &SystemSnapshot) -> Self {
        let Some((disk, esp)) = snapshot.find_partition(&oobe.target.esp_partuuid) else {
            return Self::best_for(snapshot);
        };
        Self {
            disk: Some(disk.path.clone()),
            esp: Some(esp.path.clone()),
            region_start_sector: largest_gap_start(disk),
            filesystem: oobe.install.root_filesystem,
            swap: oobe.install.swap,
            locale: oobe.locale.clone(),
        }
    }

    /// Point the target at `disk`, taking its ESP and its largest free region.
    ///
    /// Changing disk clears the region rather than keeping a sector number that
    /// meant something on a different device — which would be a plan built
    /// against an offset on the wrong disk.
    pub fn retarget(&mut self, disk: &Disk) {
        self.disk = Some(disk.path.clone());
        self.esp = esp_of(disk).map(|esp| esp.path.clone());
        self.region_start_sector = largest_gap_start(disk);
    }
}

/// The EFI System Partition on `disk`, if it has one.
///
/// Found by type GUID and not by filesystem: a FAT32 partition is not an ESP,
/// and firmware looks at the GUID. Compared case-insensitively because `sfdisk`
/// prints GUIDs upper case and `lsblk` prints them lower case.
pub fn esp_of(disk: &Disk) -> Option<&Partition> {
    disk.partitions.iter().find(|partition| {
        partition
            .type_guid
            .as_deref()
            .is_some_and(|guid| guid.eq_ignore_ascii_case(TYPE_EFI_SYSTEM))
    })
}

fn largest_gap_start(disk: &Disk) -> Option<u64> {
    disk.free_gaps()
        .into_iter()
        .max_by_key(|gap| gap.sectors)
        .map(|gap| gap.start_sector)
}
