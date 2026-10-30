//! What an ISO install is going to do, as chosen so far.

use crate::engine::probe::{Disk, SystemSnapshot};
use crate::oobe::{Locale, Oobe};

/// The values a handoff payload would have carried. `None` means "not chosen
/// yet", which is a state the Windows flow never has and the ISO flow starts in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target {
    pub disk: Option<String>,
    /// First sector of the free region CrownOS goes in. The region is
    /// identified by where it starts rather than by an index into a list,
    /// because a queued partition edit changes the list and must not silently
    /// change which region the user picked.
    pub region_start_sector: Option<u64>,
    pub locale: Locale,
}

impl Target {
    /// A first guess, so the first frame has a plan to show and the disk page
    /// opens with something selected rather than with nothing: the GPT disk
    /// with the largest unallocated region.
    pub fn best_for(snapshot: &SystemSnapshot) -> Self {
        snapshot
            .disks
            .iter()
            .filter(|disk| disk.table == "gpt")
            .filter_map(|disk| {
                let gap = disk.free_gaps().into_iter().max_by_key(|gap| gap.sectors)?;
                Some((disk.bytes(gap.sectors), disk, gap.start_sector))
            })
            .reduce(|best, next| if next.0 > best.0 { next } else { best })
            .map_or_else(Self::default, |(_, disk, start)| Self {
                disk: Some(disk.path.clone()),
                region_start_sector: Some(start),
                ..Self::default()
            })
    }

    /// The disk and free region a payload names.
    ///
    /// So that the partition editor opens on the disk the install is about. The
    /// planner still finds its own disk by PARTUUID — this is for the editor's
    /// benefit and nothing decides anything from it, which is why an unresolvable
    /// payload falls back to the same guess a flashed image gets rather than
    /// refusing here.
    pub fn from_payload(oobe: &Oobe, snapshot: &SystemSnapshot) -> Self {
        let Some((disk, _)) = snapshot.find_partition(&oobe.target.esp_partuuid) else {
            return Self::best_for(snapshot);
        };
        Self {
            disk: Some(disk.path.clone()),
            region_start_sector: largest_gap_start(disk),
            locale: oobe.locale.clone(),
        }
    }

    /// Point the target at `disk`, taking its largest free region.
    ///
    /// Changing disk clears the region rather than keeping a sector number that
    /// meant something on a different device — which would be a plan built
    /// against an offset on the wrong disk.
    pub fn retarget(&mut self, disk: &Disk) {
        self.disk = Some(disk.path.clone());
        self.region_start_sector = largest_gap_start(disk);
    }
}

fn largest_gap_start(disk: &Disk) -> Option<u64> {
    disk.free_gaps()
        .into_iter()
        .max_by_key(|gap| gap.sectors)
        .map(|gap| gap.start_sector)
}
