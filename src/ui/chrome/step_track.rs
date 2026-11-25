//! The steps of the flow along the foot of the window: each named over its
//! own segment, with a check once passed, a ring while current, and its
//! number before. Reaching a step fills its segment from the left.

use crownui::ext::Kind;
use crownui::prelude::{
    Element, Memo, Role, Runtime, Semantic, Spring, Styled, TRANSPARENT, View, WHITE, bind, hstack,
    icon, keyed, text, vstack,
};

use crate::ui::components::glyph;
use crate::ui::palette::{FOREGROUND, INK, RAIL};

const LABEL_SIZE: f32 = 14.0;
const LABEL_TRACKING: f32 = -0.084;
const NUMBER_SIZE: f32 = 12.0;
const LEAD: f32 = 20.0;
const LEAD_CHECK: f32 = 16.0;
const RING: f32 = 3.5;
const RING_DOT: f32 = 10.0;
const LABEL_INSET: f32 = 8.0;
const LABEL_GAP: f32 = 10.0;
const LABEL_TO_SEGMENT: f32 = 14.0;
const SEGMENT_HEIGHT: f32 = 5.0;
const SEGMENT_GAP: f32 = 6.0;
const TRACK_INSET: f32 = 8.0;
const HIDDEN_SCALE: f32 = 0.4;
const REGULAR: u16 = 400;
const SEMIBOLD: u16 = 600;

/// What the track shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepMark {
    /// Nothing: the page is not progress, as when the install was refused.
    Hidden,
    /// Step `current` of `labels`, counting from zero.
    At {
        current: usize,
        labels: &'static [&'static str],
    },
}

/// Where one step stands against the current one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Standing {
    Passed,
    Current,
    Ahead,
}

impl StepMark {
    /// What a screen reader hears for the track as a whole.
    pub fn spoken(self) -> String {
        match self {
            Self::Hidden => String::new(),
            Self::At { current, labels } => {
                let name = labels.get(current).copied().unwrap_or_default();
                format!("Step {} of {}, {name}", current + 1, labels.len())
            }
        }
    }

    fn labels(self) -> &'static [&'static str] {
        match self {
            Self::Hidden => &[],
            Self::At { labels, .. } => labels,
        }
    }

    fn standing(self, step: usize) -> Standing {
        match self {
            Self::At { current, .. } if step < current => Standing::Passed,
            Self::At { current, .. } if step == current => Standing::Current,
            _ => Standing::Ahead,
        }
    }
}

/// The track for whatever `mark` currently reads.
pub fn step_track(mark: Memo<StepMark>) -> impl View + Styled {
    keyed(
        move |runtime| mark.get(runtime).labels(),
        move |_, labels| {
            hstack(
                labels
                    .iter()
                    .enumerate()
                    .map(|(index, label)| step(mark, index, label))
                    .collect::<Vec<_>>(),
            )
            .gap(SEGMENT_GAP)
            .px(TRACK_INSET)
            .w_full()
        },
    )
    .w_full()
    .role(Role::ProgressBar)
    .label("Installation steps")
    .value(bind(move |runtime| mark.get(runtime).spoken()))
    .opacity(bind(move |runtime| {
        if mark.get(runtime) == StepMark::Hidden {
            0.0
        } else {
            1.0
        }
    }))
    .transition(Spring::smooth())
}

fn step(mark: Memo<StepMark>, index: usize, label: &'static str) -> impl View {
    let standing = move |runtime: &mut Runtime| mark.get(runtime).standing(index);
    let shown = move |wanted: Standing| {
        bind(move |runtime| {
            if standing(runtime) == wanted {
                1.0
            } else {
                0.0
            }
        })
    };
    let grown = move |wanted: Standing| {
        bind(move |runtime| {
            if standing(runtime) == wanted {
                1.0
            } else {
                HIDDEN_SCALE
            }
        })
    };
    let check = layer(
        vstack(icon(glyph::CHECK, INK).size(LEAD_CHECK))
            .size(LEAD)
            .items_center()
            .justify_center()
            .rounded(LEAD / 2.0)
            .bg(WHITE),
    )
    .opacity(shown(Standing::Passed))
    .scale(grown(Standing::Passed));
    let ring = layer(
        vstack(vstack(()).size(RING_DOT).rounded(RING_DOT / 2.0).bg(WHITE))
            .size(LEAD)
            .items_center()
            .justify_center()
            .rounded(LEAD / 2.0)
            .border(RING)
            .border_color(WHITE)
            .bg(TRANSPARENT),
    )
    .opacity(shown(Standing::Current))
    .scale(grown(Standing::Current));
    let number = layer(
        text((index + 1).to_string())
            .text_size(NUMBER_SIZE)
            .font_semibold()
            .text_color(FOREGROUND),
    )
    .opacity(shown(Standing::Ahead));
    let lead = vstack((number, ring, check))
        .relative()
        .size(LEAD)
        .shrink(0.0)
        .transition(Spring::bouncy());
    let name = keyed(
        move |runtime| standing(runtime) == Standing::Current,
        move |_, current| {
            text(label)
                .text_size(LABEL_SIZE)
                .tracking(LABEL_TRACKING)
                .text_color(FOREGROUND)
                .font_weight(if current { SEMIBOLD } else { REGULAR })
                .truncate()
        },
    )
    .min_w(0.0);
    vstack((
        hstack((lead, name))
            .gap(LABEL_GAP)
            .pl(LABEL_INSET)
            .items_center()
            .h(LEAD),
        segment(standing),
    ))
    .gap(LABEL_TO_SEGMENT)
    .flex_1()
    .min_w(0.0)
}

fn layer(content: impl View) -> Element<impl Kind> {
    vstack(content)
        .absolute()
        .inset(0.0)
        .items_center()
        .justify_center()
}

fn segment(standing: impl Fn(&mut Runtime) -> Standing + Copy + 'static) -> impl View {
    let radius = SEGMENT_HEIGHT / 2.0;
    vstack(
        vstack(())
            .absolute()
            .inset(0.0)
            .rounded(radius)
            .bg(WHITE)
            .origin(0.0, 0.5)
            .scale_x(bind(move |runtime| {
                if standing(runtime) == Standing::Ahead {
                    0.0
                } else {
                    1.0
                }
            }))
            .transition(Spring::smooth()),
    )
    .relative()
    .h(SEGMENT_HEIGHT)
    .w_full()
    .rounded(radius)
    .bg(RAIL)
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::button;
    use crownui::prelude::{Listeners, component};

    use super::*;

    const LABELS: &[&str] = &[
        "Network", "Disk", "Space", "Account", "Apps", "Confirm", "Install",
    ];

    fn at(current: usize) -> StepMark {
        StepMark::At {
            current,
            labels: LABELS,
        }
    }

    fn track(start: StepMark, then: StepMark) -> TestApp {
        let mut app = TestApp::with_size(
            component(move |cx| {
                let shown = cx.signal(start);
                let mark = cx.memo(move |runtime| shown.get(runtime));
                vstack((
                    button("Advance").on_click(move |cx, _| shown.set(cx, then)),
                    step_track(mark).w(1120.0),
                ))
            }),
            1200.0,
            200.0,
        );
        app.settle();
        app
    }

    #[test]
    fn steps_before_the_current_one_are_passed_and_after_it_ahead() {
        let mark = at(2);
        let standings: Vec<_> = (0..4).map(|step| mark.standing(step)).collect();
        assert_eq!(
            standings,
            [
                Standing::Passed,
                Standing::Passed,
                Standing::Current,
                Standing::Ahead
            ]
        );
        assert_eq!(StepMark::Hidden.standing(0), Standing::Ahead);
    }

    #[test]
    fn the_track_is_spoken_as_a_progress_bar_with_the_step_named() {
        let mut app = track(at(1), at(2));
        let bar = app.find(role(Role::ProgressBar).name("Installation steps"));
        assert_eq!(bar.value.as_deref(), Some("Step 2 of 7, Disk"));
        assert!(app.exists(crowntest::text("Account")));
    }

    #[test]
    fn advancing_animates_the_new_segment_and_names_the_next_step() {
        let mut app = track(at(1), at(2));
        app.click(role(Role::Button).name("Advance"));
        app.advance_ms(16);
        assert!(app.is_animating(), "the third segment fills in");
        app.settle();
        let bar = app.find(role(Role::ProgressBar).name("Installation steps"));
        assert_eq!(bar.value.as_deref(), Some("Step 3 of 7, Space"));
    }

    #[test]
    fn the_steps_share_the_width() {
        let mut app = track(at(0), at(1));
        let bounds = app.find(role(Role::ProgressBar)).bounds;
        assert!(bounds.size.width <= 1120.0 + 0.5, "{bounds:?}");
    }
}
