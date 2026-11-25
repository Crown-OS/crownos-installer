//! The light, softly shaded fill of the design's primary controls, with its
//! rim. Fully rounded to whatever it fills.

use crownui::ext::DrawList;
use crownui::prelude::{CornerRadii, Rect, Runtime, Size, Styled, View, canvas};

use crate::ui::palette::{LIGHT, LIGHT_RIM};

const RIM: f32 = 1.0;

/// Fills its parent, under the parent's other children.
pub fn light_fill() -> impl View + Styled {
    canvas(paint).absolute().inset(0.0).pointer_events_none()
}

fn paint(_: &mut Runtime, size: Size, list: &mut DrawList) {
    let bounds = Rect::from_xywh(0.0, 0.0, size.width, size.height);
    let corners = CornerRadii::uniform(size.width.min(size.height) / 2.0);
    list.fill(bounds, corners, LIGHT.vertical(0.0, size.height));
    let inset = RIM / 2.0;
    let rim = Rect::from_xywh(inset, inset, size.width - RIM, size.height - RIM);
    list.stroke(
        rim,
        CornerRadii::uniform(size.width.min(size.height) / 2.0 - inset),
        RIM,
        LIGHT_RIM,
    );
}
