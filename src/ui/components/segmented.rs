//! A glass track of two or more segments, one of which is chosen. The chosen
//! one sits on the light fill of the primary pill, with dark type.

use crownui::ext::Kind;
use crownui::prelude::{
    Color, CursorIcon, Cx, Element, Listeners, Memo, Role, Runtime, Semantic, Spring, Styled, View,
    bind, hstack, icon, rgba, text,
};

use super::checked::checked;
use super::light_fill::light_fill;
use crate::ui::palette::{FOREGROUND, GLASS, INK};

const TRACK_HEIGHT: f32 = 54.0;
const TRACK_RADIUS: f32 = 26.0;
const TRACK_INSET: f32 = 6.0;
const TRACK_RIM: Color = rgba(0xffffff1a);
const SEGMENT_GAP: f32 = 4.0;
const SEGMENT_HEIGHT: f32 = 42.0;
const SEGMENT_PADDING_LEFT: f32 = 18.0;
const SEGMENT_PADDING_RIGHT: f32 = 22.0;
const GLYPH: f32 = 16.0;
const GLYPH_GAP: f32 = 10.0;
const LABEL_SIZE: f32 = 15.0;
const LABEL_TRACKING: f32 = -0.15;
const RESTING_LABEL: Color = rgba(0xffffffbf);
const FOCUS_RING: f32 = 2.0;

/// One choice on the track: its value, its name and an SVG glyph before it.
#[derive(Clone, Copy, Debug)]
pub struct Segment<T> {
    pub value: T,
    pub label: &'static str,
    pub glyph: &'static str,
}

/// A track named `name` offering `segments`, lit on whichever matches
/// `current`, running `choose` with a segment's value when it is clicked.
pub fn segmented<T: Copy + PartialEq + 'static>(
    cx: &mut Cx,
    name: &'static str,
    segments: &[Segment<T>],
    current: Memo<T>,
    choose: impl Fn(&mut Cx, T) + Clone + 'static,
) -> impl View + Styled {
    let segments: Vec<_> = segments
        .iter()
        .map(|&segment| {
            let chosen = cx.memo(move |runtime| current.get(runtime) == segment.value);
            let choose = choose.clone();
            segment_view(segment, chosen, move |cx| choose(cx, segment.value))
        })
        .collect();
    hstack(segments)
        .gap(SEGMENT_GAP)
        .p(TRACK_INSET)
        .h(TRACK_HEIGHT)
        .shrink(0.0)
        .items_center()
        .rounded(TRACK_RADIUS)
        .bg(GLASS)
        .border(1.0)
        .border_color(TRACK_RIM)
        .role(Role::Group)
        .label(name)
}

fn segment_view<T>(
    segment: Segment<T>,
    chosen: Memo<bool>,
    choose: impl Fn(&mut Cx) + 'static,
) -> Element<impl Kind> {
    let ink = move |runtime: &mut Runtime| {
        if chosen.get(runtime) {
            INK
        } else {
            RESTING_LABEL
        }
    };
    checked(
        hstack((
            light_fill().opacity(bind(
                move |runtime| {
                    if chosen.get(runtime) { 1.0 } else { 0.0 }
                },
            )),
            icon(segment.glyph, bind(ink)).size(GLYPH).shrink(0.0),
            text(segment.label)
                .text_size(LABEL_SIZE)
                .font_medium()
                .tracking(LABEL_TRACKING)
                .text_color(bind(ink)),
        ))
        .relative()
        .gap(GLYPH_GAP)
        .items_center()
        .h(SEGMENT_HEIGHT)
        .pl(SEGMENT_PADDING_LEFT)
        .pr(SEGMENT_PADDING_RIGHT),
        chosen,
    )
    .shrink(0.0)
    .rounded(SEGMENT_HEIGHT / 2.0)
    .role(Role::RadioButton)
    .label(segment.label)
    .focusable(true)
    .cursor(CursorIcon::Pointer)
    .focus_visible(|style| style.ring(FOCUS_RING, FOREGROUND))
    .on_click(move |cx, _| choose(cx))
    .transition(Spring::snappy())
}
