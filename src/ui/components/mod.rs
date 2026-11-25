//! The installer's own widgets, composed from the kit in the OOBE design's
//! shapes and colours.

mod ai_orb;
mod app_icon;
pub mod app_icons;
mod checked;
mod choice_panel;
mod field;
pub mod glyph;
mod hero_badge;
mod icon_tile;
mod level_card;
mod light_fill;
mod mark;
mod meter;
mod network_row;
mod note;
mod option_row;
mod pill;
mod segmented;
mod stage_row;
mod status_glyph;
mod summary_tile;
mod surface;

pub use ai_orb::Orb;
pub use app_icon::app_icon;
pub use choice_panel::{Emphasis, choice_panel};
pub use field::field;
pub use hero_badge::stopped_badge;
pub use icon_tile::icon_tile;
pub use level_card::{CardMetrics, LevelCard, level_card};
pub use mark::Mark;
pub use meter::{meter, percent};
pub use network_row::{NetworkLine, network_row};
pub use note::{Tone, alert, note};
pub use option_row::{OptionLine, RowSize, option_row};
pub use pill::{Knob, PILL_HEIGHT, PillStyle, pill, styled_pill};
pub use segmented::{Segment, segmented};
pub use stage_row::stage_row;
pub use status_glyph::Status;
pub use summary_tile::summary_tile;
pub use surface::surface;
