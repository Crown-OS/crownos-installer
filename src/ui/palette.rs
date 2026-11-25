//! The OOBE design's colours, and the kit theme they sit on.
//!
//! The design is drawn in one palette: white type and glass over a black
//! window, with the colour kept for the glow along the foot. The installer
//! provides that theme rather than the configured one.

use crownui::kit::{GradientStops, Theme, ThemeMode};
use crownui::prelude::{Color, WHITE, rgb, rgba};

/// The window behind everything.
pub const NIGHT: Color = rgb(0x040404);
/// Headings, names and lit marks.
pub const FOREGROUND: Color = WHITE;
/// Type drawn on a light fill.
pub const INK: Color = rgb(0x040404);
/// Subtitles and field captions.
pub const SUBDUED: Color = rgb(0xaaaaaa);
/// Supporting lines under a name.
pub const SECONDARY: Color = rgba(0xffffffc7);
/// A glass surface at rest.
pub const GLASS: Color = rgba(0x08080980);
/// A glass surface that is chosen or has focus.
pub const GLASS_LIT: Color = rgba(0x0808099e);
/// The hairline around glass at rest.
pub const HAIRLINE: Color = rgba(0xffffff1f);
/// The hairline around chosen glass.
pub const HAIRLINE_LIT: Color = rgba(0xffffff52);
/// An icon tile that is not lit.
pub const TILE: Color = rgba(0xffffff0f);
/// Bars and steps not reached yet.
pub const RAIL: Color = rgb(0x5c5c5c);
/// The empty part of a progress bar.
pub const TROUGH: Color = rgba(0xffffff14);
/// Something that stopped the install.
pub const DANGER: Color = rgb(0xff5a76);
/// The light pill, top to bottom.
pub const LIGHT: GradientStops = GradientStops::new(rgb(0xf5f5f7), rgb(0xd2d2d8));
/// The rim of the light pill.
pub const LIGHT_RIM: Color = rgb(0xc2c2c9);

/// The kit theme every page is drawn with.
pub const fn oobe_theme() -> Theme {
    let mut theme = Theme::with_stops(ThemeMode::Dark, LIGHT);
    theme.text.on_accent = INK;
    theme
}
