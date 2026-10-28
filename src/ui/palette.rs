//! The OOBE design's colours, and the kit theme they sit on.
//!
//! The design is drawn in one palette — light, with the pink accent — so the
//! installer provides that theme rather than the configured one: a dark card
//! over the pink backdrop is a combination nobody designed.

use crownui::kit::{GradientStops, Theme, ThemeMode};
use crownui::prelude::{Color, rgb, rgba};

/// Headings and the step label.
pub const INK: Color = rgb(0x25171a);
/// Subtitles and supporting lines on the intro page.
pub const SUBDUED: Color = rgb(0x6f6f6f);
/// Lit progress, focus rings, the selected radio, success badges.
pub const BRAND: Color = rgb(0xef3054);
/// Progress segments not reached yet.
pub const TRACK: Color = rgb(0xd2d2d2);
/// The hairline around white surfaces on the card.
pub const OUTLINE: Color = rgba(0xb0b0b066);
/// A selected option tile's fill.
pub const BLUSH: Color = rgb(0xfef0f2);
/// The pink accent, top to bottom.
pub const ACCENT: GradientStops = GradientStops::new(rgb(0xf472b6), rgb(0xdb2777));

/// The kit theme every page is drawn with.
pub const fn oobe_theme() -> Theme {
    Theme::with_stops(ThemeMode::Light, ACCENT)
}
