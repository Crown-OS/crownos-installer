//! A round tile behind a glyph at the start of a row: white with a dark glyph
//! when its row is lit, a faint disc with a light glyph otherwise.

use crownui::prelude::{Color, Runtime, Spring, Styled, View, WHITE, bind, icon, rgb, vstack};

use crate::ui::palette::{INK, TILE};

const RESTING_GLYPH: Color = rgb(0xd4d4d8);

/// `glyph` (an SVG source) at `glyph_size` on a tile `size` across, lit
/// while `lit` holds.
pub fn icon_tile(
    glyph: &'static str,
    size: f32,
    glyph_size: f32,
    lit: impl Fn(&mut Runtime) -> bool + Copy + 'static,
) -> impl View {
    vstack(
        icon(
            glyph,
            bind(
                move |runtime| {
                    if lit(runtime) { INK } else { RESTING_GLYPH }
                },
            ),
        )
        .size(glyph_size),
    )
    .size(size)
    .shrink(0.0)
    .items_center()
    .justify_center()
    .rounded(size / 2.0)
    .bg(bind(move |runtime| if lit(runtime) { WHITE } else { TILE }))
    .transition(Spring::smooth())
}
