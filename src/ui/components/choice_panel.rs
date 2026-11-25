//! One way into CrownOS, as the intro page offers it: artwork, a marked title
//! with a knob at its end, and a line on what choosing it means. The way the
//! design leads with sits on brighter glass and has a light knob.

use crownui::ext::Kind;
use crownui::prelude::{
    Color, Element, Styled, TRANSPARENT, WHITE, hstack, icon, image, rgba, text, vstack,
};

use super::glyph;
use super::light_fill::light_fill;
use crate::ui::palette::{FOREGROUND, GLASS, GLASS_LIT, HAIRLINE, HAIRLINE_LIT, INK, SECONDARY};
use crate::util::png::decode_rgba;

const RADIUS: f32 = 32.0;
const INSET: f32 = 12.0;
const FOOT: f32 = 24.0;
const ART_RADIUS: f32 = 20.0;
const ART_ASPECT: f32 = 524.0 / 330.0;
const BODY_TOP: f32 = 24.0;
const BODY_INSET: f32 = 16.0;
const BODY_GAP: f32 = 10.0;
const TITLE_GAP: f32 = 12.0;
const MARK_SIZE: f32 = 22.0;
const TITLE_SIZE: f32 = 24.0;
const TITLE_TRACKING: f32 = -0.24;
const KNOB: f32 = 40.0;
const KNOB_GLYPH: f32 = 18.0;
const RESTING_KNOB: Color = rgba(0xffffff1a);
const BLURB_SIZE: f32 = 16.0;
const BLURB_WIDTH: f32 = 320.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    Chosen,
    Resting,
}

/// A panel showing `art` (an RGBA PNG) over `mark` (an SVG source) and
/// `title`, and `blurb` under them.
pub fn choice_panel(
    art: &'static [u8],
    mark: &'static str,
    title: &'static str,
    blurb: &'static str,
    emphasis: Emphasis,
) -> Element<impl Kind> {
    let chosen = emphasis == Emphasis::Chosen;
    let artwork = decode_rgba(art).map(|pixels| {
        vstack(
            image(pixels.width, pixels.height, pixels.rgba)
                .w_full()
                .aspect(ART_ASPECT),
        )
        .w_full()
        .shrink(0.0)
        .rounded(ART_RADIUS)
        .overflow_hidden()
    });
    let knob = vstack((
        chosen.then(light_fill),
        icon(glyph::ARROW_RIGHT, if chosen { INK } else { WHITE }).size(KNOB_GLYPH),
    ))
    .relative()
    .size(KNOB)
    .shrink(0.0)
    .items_center()
    .justify_center()
    .rounded(KNOB / 2.0)
    .bg(if chosen { TRANSPARENT } else { RESTING_KNOB });
    let title_row = hstack((
        icon(mark, FOREGROUND).size(MARK_SIZE).shrink(0.0),
        text(title)
            .text_size(TITLE_SIZE)
            .font_medium()
            .tracking(TITLE_TRACKING)
            .text_color(FOREGROUND)
            .flex_1(),
        knob,
    ))
    .gap(TITLE_GAP)
    .items_center()
    .w_full();
    let body = vstack((
        title_row,
        text(blurb)
            .text_size(BLURB_SIZE)
            .text_color(SECONDARY)
            .max_w(BLURB_WIDTH),
    ))
    .gap(BODY_GAP)
    .pt(BODY_TOP)
    .px(BODY_INSET)
    .w_full();
    vstack((artwork, body))
        .pt(INSET)
        .px(INSET)
        .pb(FOOT)
        .flex_1()
        .min_w(0.0)
        .rounded(RADIUS)
        .bg(if chosen { GLASS_LIT } else { GLASS })
        .border(1.0)
        .border_color(if chosen { HAIRLINE_LIT } else { HAIRLINE })
}
