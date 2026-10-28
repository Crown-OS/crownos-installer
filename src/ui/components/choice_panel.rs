//! One way into CrownOS, as the intro page offers it: artwork, a marked title,
//! and a line on what choosing it means. The chosen way glows blush from its
//! rim to a white middle.

use crownui::ext::{DrawList, Kind, Paint};
use crownui::prelude::{
    Color, CornerRadii, Element, Rect, Runtime, Size, Styled, View, WHITE, canvas, hstack, image,
    rgb, text, vstack,
};

use crate::ui::palette::{INK, SUBDUED};
use crate::util::png::decode_rgba;

const RADIUS: f32 = 20.0;
const PADDING: f32 = 28.0;
const GAP: f32 = 20.0;
const ART_ASPECT: f32 = 458.0 / 329.0;
const TITLE_HEIGHT: f32 = 26.0;
const TITLE_SIZE: f32 = 22.0;
const TITLE_TRACKING: f32 = -0.66;
const BLURB_SIZE: f32 = 16.0;
const BLURB_TRACKING: f32 = -0.8;
const BLURB_WIDTH: f32 = 344.0;
const GLOW_RIM: Color = rgb(0xfde4e8);
/// How far the white middle is softened into the rim, as a share of the
/// shorter side.
const GLOW_SPREAD: f32 = 0.22;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    Chosen,
    Resting,
}

/// A panel showing `art` (an RGBA PNG) over `mark` and `title`, with
/// `trailing` at the end of the blurb's line. The mark brings its own spacing.
pub fn choice_panel(
    art: &'static [u8],
    mark: impl View,
    title: &'static str,
    blurb: &'static str,
    trailing: impl View,
    emphasis: Emphasis,
) -> Element<impl Kind> {
    let artwork = decode_rgba(art).map(|pixels| {
        image(pixels.width, pixels.height, pixels.rgba)
            .w_full()
            .aspect(ART_ASPECT)
            .shrink(0.0)
    });
    let title_row = hstack((
        mark,
        text(title)
            .text_size(TITLE_SIZE)
            .font_semibold()
            .tracking(TITLE_TRACKING)
            .text_color(INK),
    ))
    .items_center()
    .h(TITLE_HEIGHT)
    .w_full();
    let blurb_row = hstack((
        text(blurb)
            .text_size(BLURB_SIZE)
            .tracking(BLURB_TRACKING)
            .text_color(SUBDUED)
            .max_w(BLURB_WIDTH),
        trailing,
    ))
    .justify_between()
    .items_center()
    .w_full();
    let glow = (emphasis == Emphasis::Chosen).then(|| {
        canvas(paint_glow)
            .absolute()
            .inset(0.0)
            .pointer_events_none()
    });
    vstack((glow, artwork, title_row, blurb_row))
        .relative()
        .gap(GAP)
        .p(PADDING)
        .flex_1()
        .min_w(0.0)
        .rounded(RADIUS)
        .bg(WHITE)
}

fn paint_glow(_: &mut Runtime, size: Size, list: &mut DrawList) {
    let panel = Rect::from_xywh(0.0, 0.0, size.width, size.height);
    let corners = CornerRadii::uniform(RADIUS);
    let spread = size.width.min(size.height) * GLOW_SPREAD;
    list.save();
    list.clip(panel, corners);
    list.fill(panel, corners, GLOW_RIM);
    list.fill(
        Rect::from_xywh(
            spread,
            spread,
            size.width - 2.0 * spread,
            size.height - 2.0 * spread,
        ),
        CornerRadii::uniform(spread),
        Paint {
            blur: Some(spread),
            ..Paint::solid(WHITE)
        },
    );
    list.restore();
}
