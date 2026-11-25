//! The footer's buttons. The primary one is a light pill with a dark knob at
//! its end; the others are glass. Both sink slightly under the pointer.

use crownui::ext::Kind;
use crownui::kit::ButtonVariant;
use crownui::prelude::{
    CursorIcon, Element, Role, Semantic, Spring, Styled, TRANSPARENT, View, WHITE, hstack, icon,
    text, vstack,
};

use super::glyph;
use super::light_fill::light_fill;
use crate::ui::palette::{FOREGROUND, GLASS, INK, NIGHT};

pub const PILL_HEIGHT: f32 = 52.0;
const LABEL_SIZE: f32 = 17.0;
const LABEL_TRACKING: f32 = -0.17;
const PRIMARY_PADDING_LEFT: f32 = 24.0;
const PRIMARY_PADDING_RIGHT: f32 = 8.0;
const PRIMARY_GAP: f32 = 14.0;
const KNOB: f32 = 36.0;
const KNOB_GLYPH: f32 = 18.0;
const GLASS_PADDING: f32 = 24.0;
const GLASS_PADDING_BESIDE_GLYPH: f32 = 17.0;
const GLASS_GAP: f32 = 10.0;
const GLASS_RIM: f32 = 1.0;
const GLASS_RIM_ALPHA: f32 = 0.22;
const FOCUS_RING: f32 = 2.0;
const PRESSED_SCALE: f32 = 0.96;
const DISABLED_OPACITY: f32 = 0.5;

/// What the knob at the end of a primary pill shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Knob {
    /// Onwards.
    Arrow,
    /// The machine restarts.
    Restart,
}

impl Knob {
    fn glyph(self) -> &'static str {
        match self {
            Self::Arrow => glyph::ARROW_RIGHT,
            Self::Restart => glyph::REFRESH,
        }
    }
}

/// How a pill is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillStyle {
    /// Light, with a dark knob at its end.
    Primary(Knob),
    /// Glass, led by an SVG glyph when there is one.
    Glass(Option<&'static str>),
}

/// A pill labelled `label`: primary is light with an arrow knob, anything
/// else is plain glass.
pub fn pill(label: &'static str, variant: ButtonVariant) -> Element<impl Kind> {
    styled_pill(
        label,
        match variant {
            ButtonVariant::Primary => PillStyle::Primary(Knob::Arrow),
            _ => PillStyle::Glass(None),
        },
    )
}

pub fn styled_pill(label: &'static str, style: PillStyle) -> Element<impl Kind> {
    let (fill, leading, knob) = match style {
        PillStyle::Primary(knob) => (Some(light_fill()), None, Some(knob_disc(knob))),
        PillStyle::Glass(leading) => (
            None,
            leading.map(|source| icon(source, FOREGROUND).size(KNOB_GLYPH).shrink(0.0)),
            None,
        ),
    };
    let primary = matches!(style, PillStyle::Primary(_));
    let (padding_left, padding_right, gap) = match style {
        PillStyle::Primary(_) => (PRIMARY_PADDING_LEFT, PRIMARY_PADDING_RIGHT, PRIMARY_GAP),
        PillStyle::Glass(Some(_)) => (GLASS_PADDING_BESIDE_GLYPH, GLASS_PADDING, GLASS_GAP),
        PillStyle::Glass(None) => (GLASS_PADDING, GLASS_PADDING, GLASS_GAP),
    };
    hstack((
        fill,
        leading,
        caption(label).text_color(if primary { INK } else { FOREGROUND }),
        knob,
    ))
    .relative()
    .gap(gap)
    .pl(padding_left)
    .pr(padding_right)
    .bg(if primary { TRANSPARENT } else { GLASS })
    .border(if primary { 0.0 } else { GLASS_RIM })
    .border_color(WHITE.with_alpha_factor(GLASS_RIM_ALPHA))
    .items_center()
    .h(PILL_HEIGHT)
    .shrink(0.0)
    .rounded(PILL_HEIGHT / 2.0)
    .role(Role::Button)
    .label(label)
    .focusable(true)
    .cursor(CursorIcon::Pointer)
    .focus_visible(|style| style.ring(FOCUS_RING, FOREGROUND))
    .active(|style| style.scale(PRESSED_SCALE))
    .when_disabled(|style| style.opacity(DISABLED_OPACITY))
    .transition(Spring::snappy())
}

fn knob_disc(knob: Knob) -> impl View {
    vstack(icon(knob.glyph(), WHITE).size(KNOB_GLYPH))
        .size(KNOB)
        .shrink(0.0)
        .items_center()
        .justify_center()
        .rounded(KNOB / 2.0)
        .bg(NIGHT)
}

fn caption(label: &'static str) -> impl View + Styled {
    text(label)
        .text_size(LABEL_SIZE)
        .font_medium()
        .tracking(LABEL_TRACKING)
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::{ButtonVariant, provide_theme};
    use crownui::prelude::{Listeners, Semantic, bind, component, text, vstack};

    use super::*;
    use crate::ui::palette::oobe_theme;

    fn pills() -> TestApp {
        let mut app = TestApp::with_size(
            component(|cx| {
                provide_theme(cx, oobe_theme());
                let presses = cx.signal(0u32);
                vstack((
                    pill("Next", ButtonVariant::Primary)
                        .on_click(move |cx, _| presses.update(cx, |count| *count += 1)),
                    pill("Back", ButtonVariant::Secondary).disabled(true),
                    text(bind(move |runtime| {
                        format!("{} presses", presses.get(runtime))
                    }))
                    .role(Role::Text),
                ))
                .items_start()
            }),
            400.0,
            300.0,
        );
        app.settle();
        app
    }

    #[test]
    fn a_pill_is_the_design_height() {
        let mut app = pills();
        for name in ["Next", "Back"] {
            let bounds = app.find(role(Role::Button).name(name)).bounds;
            assert_eq!(bounds.size.height, PILL_HEIGHT);
        }
    }

    #[test]
    fn clicking_a_pill_runs_its_action_and_a_disabled_one_says_so() {
        let mut app = pills();
        app.click(role(Role::Button).name("Next")).settle();
        assert!(app.exists(crowntest::text("1 presses")));
        assert!(app.find(role(Role::Button).name("Back")).states.disabled);
    }

    #[test]
    fn a_disabled_pill_ignores_clicks() {
        let mut app = TestApp::with_size(
            component(|cx| {
                let presses = cx.signal(0u32);
                vstack((
                    pill("Next", ButtonVariant::Primary)
                        .disabled(true)
                        .on_click(move |cx, _| presses.update(cx, |count| *count += 1)),
                    text(bind(move |runtime| {
                        format!("{} presses", presses.get(runtime))
                    })),
                ))
                .items_start()
            }),
            400.0,
            300.0,
        );
        app.settle();
        let center = app.find(role(Role::Button).name("Next")).bounds.center();
        app.pointer_down(center.x, center.y);
        app.pointer_up(center.x, center.y).settle();
        assert!(app.exists(crowntest::text("0 presses")));
    }

    #[test]
    fn pressing_a_pill_sinks_it() {
        let mut app = pills();
        let center = app.find(role(Role::Button).name("Next")).bounds.center();
        app.pointer_down(center.x, center.y);
        app.advance_ms(16);
        assert!(app.is_animating(), "the pill scales down under the pointer");
        app.pointer_up(center.x, center.y).settle();
    }
}
