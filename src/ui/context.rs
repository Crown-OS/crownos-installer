//! Everything learned before the first frame, or why nothing can be.

use crate::engine::plan::NewIds;
use crate::engine::probe::{Disk, SystemSnapshot};
use crate::oobe::Oobe;
use crate::provenance::Provenance;

/// Everything learned before the first frame.
#[derive(Debug)]
pub struct Context {
    /// Which of the two installers this is, and the payload if there is one.
    pub provenance: Provenance,
    /// What the resolver tried, and how the provenance was settled. Diagnostics.
    pub notes: Vec<String>,
    pub snapshot: SystemSnapshot,
    /// GUIDs for the partitions the plan will create. Fixed once, at startup, so
    /// the plan shown on the confirmation page names the same GUIDs the one that
    /// runs will.
    pub ids: NewIds,
    pub auto_install: bool,
    /// Whether the machine had a route out when the wizard started. Decides
    /// whether the network step is shown or walked past.
    pub online: bool,
}

impl Context {
    pub fn oobe(&self) -> Option<&Oobe> {
        self.provenance.oobe()
    }

    /// The disk the given path names, if the snapshot still has it.
    pub fn disk(&self, path: &str) -> Option<&Disk> {
        self.snapshot.disks.iter().find(|disk| disk.path == path)
    }
}

/// Why the wizard has stopped before it started.
#[derive(Clone, Debug)]
pub struct Blocker {
    pub title: String,
    pub detail: String,
    /// Diagnostics — what was tried, what was found.
    pub notes: Vec<String>,
}
