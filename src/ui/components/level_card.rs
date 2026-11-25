//! One level of a scale as a tall glass card: an orb and a radio along the
//! top, a name and a line under them, and the features the level includes.
//! The orb and the radio sit on the centres of the card's rounded corners.

use crownui::ext::Kind;
use crownui::prelude::{
    Color, CursorIcon, Cx, Element, Listeners, Memo, Role, Runtime, Semantic, Spring, Styled, View,
    WHITE, bind, hstack, rgb, rgba, svg, text, vstack,
};

use super::ai_orb::{Orb, OrbSize, ai_orb};
use super::checked::checked;
use super::glyph;
use super::mark::{Mark, mark};
use crate::ui::Density;
use crate::ui::palette::{FOREGROUND, GLASS, HAIRLINE, SECONDARY};

const NAME_SIZE: f32 = 22.0;
const NAME_TRACKING: f32 = -0.22;
const NAME_GAP: f32 = 8.0;
const LINE_GAP: f32 = 4.0;
const SUMMARY_SIZE: f32 = 15.0;
const BADGE_SIZE: f32 = 12.0;
const BADGE_PADDING_X: f32 = 8.0;
const BADGE_PADDING_Y: f32 = 3.0;
const BADGE_FILL: Color = rgba(0xffffff24);
const DIVIDER: Color = rgba(0xffffff1a);
const FEATURE_SIZE: f32 = 14.0;
const FEATURE_GLYPH: f32 = 16.0;
const FEATURE_GLYPH_GAP: f32 = 10.0;
const INCLUDED: Color = rgba(0xffffffd9);
const EXCLUDED: Color = rgba(0xffffff66);
const PICKED_FILL: Color = rgb(0x141414);
const RIM: f32 = 1.0;
const FOCUS_RING: f32 = 2.0;
const HOVER_SCALE: f32 = 1.01;

/// What a card says about its level.
pub struct LevelCard {
    pub name: &'static str,
    pub summary: &'static str,
    pub orb: Orb,
    /// A short tag after the name, such as "Recommended".
    pub badge: Option<&'static str>,
    /// Each feature of the scale, and whether this level includes it.
    pub features: Vec<(&'static str, bool)>,
}

/// The sizes that change with the window's density. The orb and the radio
/// stay centred on the corners: `radius` is `padding` plus half the orb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardMetrics {
    pub radius: f32,
    pub padding: f32,
    pub gap: f32,
    pub orb: OrbSize,
    pub feature_gap: f32,
}

impl CardMetrics {
    pub const fn for_density(density: Density) -> Self {
        match density {
            Density::Roomy => Self {
                radius: 48.0,
                padding: 24.0,
                gap: 16.0,
                orb: OrbSize {
                    diameter: 48.0,
                    glyph: 22.0,
                },
                feature_gap: 8.0,
            },
            Density::Compact => Self {
                radius: 38.0,
                padding: 18.0,
                gap: 12.0,
                orb: OrbSize {
                    diameter: 40.0,
                    glyph: 18.0,
                },
                feature_gap: 4.0,
            },
        }
    }
}

/// A card for `card`, sized by `metrics`, lit while `chosen` holds, running
/// `choose` when clicked.
pub fn level_card<Choose: Fn(&mut Cx) + 'static>(
    cx: &mut Cx,
    card: LevelCard,
    metrics: Memo<CardMetrics>,
    chosen: Memo<bool>,
    choose: Choose,
) -> Element<impl Kind + use<Choose>> {
    let measure = move |pick: fn(&CardMetrics) -> f32| {
        bind(move |runtime: &mut Runtime| pick(&metrics.get(runtime)))
    };
    let orb_size = cx.memo(move |runtime| metrics.get(runtime).orb);
    let top_row = hstack((
        ai_orb(card.orb, orb_size),
        vstack(()).flex_1(),
        vstack(mark(Mark::Radio, chosen))
            .w(measure(|metrics| metrics.orb.diameter))
            .h(measure(|metrics| metrics.orb.diameter))
            .shrink(0.0)
            .items_center()
            .justify_center(),
    ))
    .items_center()
    .w_full();
    let name_row = hstack((
        text(card.name)
            .text_size(NAME_SIZE)
            .font_medium()
            .tracking(NAME_TRACKING)
            .text_color(FOREGROUND),
        card.badge.map(badge),
    ))
    .gap(NAME_GAP)
    .items_center();
    let lines = vstack((
        name_row,
        text(card.summary)
            .text_size(SUMMARY_SIZE)
            .text_color(SECONDARY),
    ))
    .gap(LINE_GAP)
    .w_full();
    let features: Vec<_> = card
        .features
        .into_iter()
        .map(|(name, included)| feature(name, included))
        .collect();
    checked(
        vstack((
            top_row,
            lines,
            vstack(()).h(1.0).w_full().shrink(0.0).bg(DIVIDER),
            vstack(features).gap(measure(|metrics| metrics.feature_gap)),
        ))
        .gap(measure(|metrics| metrics.gap))
        .w_full(),
        chosen,
    )
    .p(measure(|metrics| metrics.padding))
    .rounded(measure(|metrics| metrics.radius))
    .bg(bind(move |runtime| {
        if chosen.get(runtime) {
            PICKED_FILL
        } else {
            GLASS
        }
    }))
    .border(RIM)
    .border_color(bind(
        move |runtime| {
            if chosen.get(runtime) { WHITE } else { HAIRLINE }
        },
    ))
    .role(Role::RadioButton)
    .label(card.name)
    .focusable(true)
    .cursor(CursorIcon::Pointer)
    .focus_visible(|style| style.ring(FOCUS_RING, FOREGROUND))
    .on_click(move |cx, _| choose(cx))
    .hover(|style| style.scale(HOVER_SCALE))
    .transition(Spring::snappy())
}

fn badge(label: &'static str) -> impl View {
    vstack(
        text(label)
            .text_size(BADGE_SIZE)
            .font_medium()
            .text_color(FOREGROUND),
    )
    .px(BADGE_PADDING_X)
    .py(BADGE_PADDING_Y)
    .rounded(BADGE_SIZE)
    .bg(BADGE_FILL)
    .shrink(0.0)
}

fn feature(name: &'static str, included: bool) -> impl View {
    let (mark, colour) = if included {
        (glyph::FEATURE_INCLUDED, INCLUDED)
    } else {
        (glyph::FEATURE_EXCLUDED, EXCLUDED)
    };
    hstack((
        svg(mark).size(FEATURE_GLYPH).shrink(0.0),
        text(name).text_size(FEATURE_SIZE).text_color(colour),
    ))
    .gap(FEATURE_GLYPH_GAP)
    .items_center()
    .role(Role::ListItem)
    .label(name)
    .value(if included { "Included" } else { "Not included" })
}
