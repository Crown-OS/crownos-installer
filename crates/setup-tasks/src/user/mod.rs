//! The per-user half of the setup: `crownos-setup user` runs it as the user at
//! each login. It writes only files under the home directory and never uses
//! the network.

pub mod accessibility_flags;
pub mod dock;
pub mod marker;
pub mod notification;
pub mod snapshot;
pub mod wait;
pub mod wire;

pub use marker::UserMarker;
pub use notification::notify_once;
pub use snapshot::SetupSnapshot;
pub use wait::wait_for_wanted_tasks;
pub use wire::{SkipReason, UserSetupReport, wire};
