//! The installer's own widgets, composed from the kit in the OOBE design's
//! shapes and colours.

mod choice_panel;
mod field;
mod hero_badge;
mod meter;
mod network_row;
mod note;
mod option_tile;
mod pill;
mod stage_row;
mod status_glyph;
mod summary_tile;
mod surface;

pub use choice_panel::{Emphasis, choice_panel};
pub use field::field;
pub use hero_badge::{Outcome, hero_badge};
pub use meter::{meter, percent};
pub use network_row::{NetworkLine, network_row};
pub use note::{Tone, alert, note};
pub use option_tile::option_tile;
pub use pill::{PILL_HEIGHT, pill};
pub use stage_row::stage_row;
pub use status_glyph::Status;
pub use summary_tile::summary_tile;
pub use surface::surface;
