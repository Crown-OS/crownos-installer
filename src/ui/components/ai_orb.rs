//! The round, softly shaded badge that leads an AI level: a diagonal gradient
//! with the level's glyph in the middle.

use crownui::ext::{Brush, DrawList, GradientStop, Paint};
use crownui::prelude::{
    Color, CornerRadii, Memo, Point, Rect, Runtime, Size, Styled, View, bind, canvas, svg, vstack,
};

/// Figma runs the orb's gradient from the top left corner to here, as a
/// share of the orb's size along each axis.
const GRADIENT_REACH: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// An orb's look: its glyph (an SVG source drawn in its own colours) and its
/// gradient, top left first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orb {
    pub glyph: &'static str,
    pub start: Color,
    pub end: Color,
}

/// How big an orb and its glyph are drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbSize {
    pub diameter: f32,
    pub glyph: f32,
}

/// `orb` at whatever size `size` currently reads.
pub fn ai_orb(orb: Orb, size: Memo<OrbSize>) -> impl View + Styled {
    let diameter = move |runtime: &mut Runtime| size.get(runtime).diameter;
    let glyph = move |runtime: &mut Runtime| size.get(runtime).glyph;
    vstack((
        canvas(move |_, area, list| paint(orb, area, list))
            .absolute()
            .inset(0.0)
            .pointer_events_none(),
        svg(orb.glyph).w(bind(glyph)).h(bind(glyph)),
    ))
    .relative()
    .w(bind(diameter))
    .h(bind(diameter))
    .shrink(0.0)
    .items_center()
    .justify_center()
}

fn paint(orb: Orb, area: Size, list: &mut DrawList) {
    let bounds = Rect::from_xywh(0.0, 0.0, area.width, area.height);
    let gradient = Paint {
        brush: Brush::LinearGradient {
            start: Point::new(0.0, 0.0),
            end: Point::new(area.width * GRADIENT_REACH, area.height * GRADIENT_REACH),
            stops: [
                GradientStop {
                    offset: 0.0,
                    color: orb.start,
                },
                GradientStop {
                    offset: 1.0,
                    color: orb.end,
                },
            ]
            .into_iter()
            .collect(),
        },
        blur: None,
    };
    list.fill(
        bounds,
        CornerRadii::uniform(area.width.min(area.height) / 2.0),
        gradient,
    );
}
