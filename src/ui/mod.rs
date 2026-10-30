//! The kiosk wizard: the only thing on the screen, under `cage`, on a machine
//! that has either just rebooted out of Windows or just booted a CrownOS image.
//!
//! # Shape
//!
//! The OOBE design: one frosted card over a pink backdrop, the page's heading
//! across its top, a step track and the page's buttons along its foot. Pages
//! drift and fade between each other ([`chrome`]); the widgets they are made
//! of are the kit's, in the design's shapes ([`components`]). This module owns
//! navigation, because which step comes next depends on how the installer was
//! started and on what the disk turns out to look like.
//!
//! [`Wizard`] is every rule about moving through the steps, with no views in
//! it; [`pages`] read it through one signal and call into it. The tests drive
//! the wizard directly, and the pages headlessly.
//!
//! # Two flows through one card
//!
//! [`Provenance`](crate::provenance::Provenance) decides which:
//!
//! | | from Windows | from a flashed image |
//! |---|---|---|
//! | 1 | Welcome — what the payload decided | Network — optional |
//! | 2 | Network — optional | Disk — which disk to install on |
//! | 3 | | Space — where on it, and how |
//! | 4 | Account | Account |
//! | 5 | Confirm | Confirm |
//! | 6 | Install → Restart | Install → Restart |
//!
//! The network step is walked past when the machine is already online at
//! startup. [`Page::Partitions`], the partition editor, is reachable from
//! either flow and is not a step; [`Page::Blocked`] is where the wizard stops.
//! The queued partition edits live in [`Editor::ops`] and become the first
//! steps of whatever plan is built afterwards — see
//! [`engine::disks`](crate::engine::disks).

mod chrome;
mod components;
mod context;
mod editor;
mod form;
mod layout;
mod page;
mod pages;
mod palette;
mod progress;
mod root;
mod target;
mod wizard;

use crownui::kit::{INTER_FAMILY, inter_font};
use crownui::{AppConfig, RunError};

use crate::engine::exec::Mode;

pub use context::{Blocker, Context};
pub use editor::Editor;
pub use form::Form;
pub use page::Page;
pub use progress::InstallProgress;
pub use target::Target;
pub use wizard::Wizard;

/// The window a developer gets when rehearsing outside `cage`. Under `cage` it
/// is full-screen and this is never consulted.
const WINDOW_SIZE: (u32, u32) = (1280, 800);
/// Below this the card's two-column grids no longer fit their rows.
const MIN_WINDOW_SIZE: (u32, u32) = (1024, 640);

/// Run the wizard until the user restarts or the window closes.
pub fn run(startup: Result<Context, Blocker>, mode: Mode) -> Result<(), RunError> {
    let config = AppConfig::new("CrownOS Installer")
        .app_id("crownos-installer")
        .size(WINDOW_SIZE.0, WINDOW_SIZE.1)
        .min_size(MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1)
        .font(inter_font().to_vec(), Some(INTER_FAMILY));
    crownui::run(root::root(startup, mode), config)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod window_tests;
