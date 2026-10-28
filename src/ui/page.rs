//! Which step is showing, and where each flow puts it.

use crate::provenance::Provenance;

/// The step rail's labels for an install driven by a handoff payload. One fewer
/// than the ISO flow's: Welcome stands in for the two questions the payload
/// already answered.
pub const WINDOWS_STEPS: &[&str] = &["Welcome", "Network", "Account", "Confirm", "Install"];
/// And for an install from a flashed image.
pub const ISO_STEPS: &[&str] = &["Network", "Disk", "Space", "Account", "Confirm", "Install"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// ISO flow only: install or try, before the card. Not a step.
    Intro,
    /// Windows flow only: what the payload decided, before doing any of it.
    Welcome,
    /// Either flow: offer to get online, so the new system can update right away.
    Network,
    /// ISO flow only: which disk.
    Disk,
    /// ISO flow only: where on it, and with what filesystem.
    Space,
    /// Either flow: the partition editor. Not a step — see the module docs.
    Partitions,
    Account,
    Confirm,
    Progress,
    Done,
    /// The machine cannot be installed to, and here is why. Not a step — it is
    /// where the wizard stops.
    Blocked,
}

impl Page {
    /// Position in the step bar, and how many stops it has.
    ///
    /// The blocked page borrows position 0 rather than getting one of its own: it
    /// is not progress, and a step bar reading "step 3 of 5" above an
    /// explanation of why there will be no step 4 says the wrong thing.
    ///
    /// [`Partitions`](Self::Partitions) has no position of its own, because it is
    /// reached from a different step in each flow and borrowing the *wrong* one
    /// moves the bar. [`Wizard::step_indicator`](super::Wizard::step_indicator)
    /// knows which, and it is what the pages use.
    pub fn step(self, provenance: &Provenance) -> (usize, usize) {
        let total = Self::labels(provenance).len();
        let index = if provenance.is_iso() {
            match self {
                Self::Intro | Self::Network | Self::Blocked | Self::Welcome | Self::Partitions => 0,
                Self::Disk => 1,
                Self::Space => 2,
                Self::Account => 3,
                Self::Confirm => 4,
                Self::Progress | Self::Done => 5,
            }
        } else {
            match self {
                Self::Intro | Self::Welcome | Self::Blocked | Self::Disk | Self::Partitions => 0,
                Self::Network => 1,
                Self::Account | Self::Space => 2,
                Self::Confirm => 3,
                Self::Progress | Self::Done => 4,
            }
        };
        (total, index.min(total.saturating_sub(1)))
    }

    /// Where the page sits in reading order across both flows, so moving to a
    /// larger ordinal is moving forward.
    pub fn ordinal(&self) -> usize {
        match self {
            Self::Intro => 0,
            Self::Welcome => 1,
            Self::Network => 2,
            Self::Disk => 3,
            Self::Space => 4,
            Self::Partitions => 5,
            Self::Account => 6,
            Self::Confirm => 7,
            Self::Progress => 8,
            Self::Done => 9,
            Self::Blocked => 10,
        }
    }

    pub fn labels(provenance: &Provenance) -> &'static [&'static str] {
        if provenance.is_iso() {
            ISO_STEPS
        } else {
            WINDOWS_STEPS
        }
    }

    /// Where a flow starts.
    ///
    /// `auto_install` asks to skip straight to the confirmation, and this is
    /// where that request is honoured as far as it can be: Welcome is skipped,
    /// and the account page is not — because a password is always needed and the
    /// payload never carries one.
    ///
    /// A flashed image always starts at the intro: `auto_install` means "the
    /// questions were already answered", and on that path nobody has answered
    /// anything.
    ///
    /// The Windows flow passes over the network step when the machine is
    /// already online, and stops at it when it is not.
    pub fn first_for(provenance: &Provenance, auto_install: bool, online: bool) -> Self {
        match provenance {
            Provenance::Iso => Self::Intro,
            Provenance::WindowsInstaller { .. } if !auto_install => Self::Welcome,
            Provenance::WindowsInstaller { .. } if online => Self::Account,
            Provenance::WindowsInstaller { .. } => Self::Network,
        }
    }
}
