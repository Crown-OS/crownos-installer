//! The round mark at the end of a choice: a radio for one-of-many, a
//! checkbox for any-of-many. Both fill white with a dark check when chosen.

use crownui::prelude::{
    Color, Memo, Runtime, Spring, Styled, TRANSPARENT, View, WHITE, bind, icon, rgb, rgba, vstack,
};

use super::glyph;
use crate::ui::palette::INK;

const RADIO: f32 = 22.0;
const RADIO_CHECK: f32 = 14.0;
const RADIO_RIM: Color = rgba(0xffffff47);
const CHECKBOX: f32 = 24.0;
const CHECKBOX_CHECK: f32 = 20.0;
const CHECKBOX_WELL: Color = rgb(0x0c0c0c);
const RIM: f32 = 1.5;
const HIDDEN_SCALE: f32 = 0.4;

/// Which mark a choice ends in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Radio,
    Checkbox,
}

impl Mark {
    fn size(self) -> f32 {
        match self {
            Self::Radio => RADIO,
            Self::Checkbox => CHECKBOX,
        }
    }

    fn check(self) -> (&'static str, f32) {
        match self {
            Self::Radio => (glyph::CHECK_SMALL, RADIO_CHECK),
            Self::Checkbox => (glyph::CHECK_LARGE, CHECKBOX_CHECK),
        }
    }

    fn resting(self) -> (Color, Color) {
        match self {
            Self::Radio => (TRANSPARENT, RADIO_RIM),
            Self::Checkbox => (CHECKBOX_WELL, WHITE),
        }
    }
}

/// `mark`, filled while `chosen` holds.
pub fn mark(mark: Mark, chosen: Memo<bool>) -> impl View + Styled {
    let size = mark.size();
    let (check, check_size) = mark.check();
    let (resting_fill, resting_rim) = mark.resting();
    let lit = move |runtime: &mut Runtime| chosen.get(runtime);
    vstack(
        icon(check, INK)
            .size(check_size)
            .opacity(bind(move |runtime| if lit(runtime) { 1.0 } else { 0.0 }))
            .scale(bind(
                move |runtime| {
                    if lit(runtime) { 1.0 } else { HIDDEN_SCALE }
                },
            ))
            .transition(Spring::bouncy()),
    )
    .size(size)
    .shrink(0.0)
    .items_center()
    .justify_center()
    .rounded(size / 2.0)
    .border(RIM)
    .bg(bind(
        move |runtime| {
            if lit(runtime) { WHITE } else { resting_fill }
        },
    ))
    .border_color(bind(
        move |runtime| {
            if lit(runtime) { WHITE } else { resting_rim }
        },
    ))
    .transition(Spring::snappy())
}
