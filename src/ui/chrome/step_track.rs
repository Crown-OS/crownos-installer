//! "Step 3" over a row of segments along the card's foot. Reaching a step
//! fills its segment from the left; the caption fades up when it changes.

use crownui::kit::INTER_FAMILY;
use crownui::prelude::{
    Memo, Role, Semantic, Spring, Styled, View, bind, hstack, keyed, text, vstack,
};

use crate::ui::palette::{BRAND, INK, TRACK};

const CAPTION_SIZE: f32 = 24.0;
const CAPTION_RISE: f32 = 8.0;
const SEGMENT_WIDTH: f32 = 115.0;
const SEGMENT_HEIGHT: f32 = 6.0;
const SEGMENT_GAP: f32 = 10.0;
const CAPTION_GAP: f32 = 12.0;

/// What the track shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepMark {
    /// Nothing: the page is not progress, as when the install was refused.
    Hidden,
    /// `current` of `total`, counting from zero.
    At { current: usize, total: usize },
    /// Every step done.
    Complete { total: usize },
}

impl StepMark {
    pub fn caption(self) -> String {
        match self {
            Self::Hidden => String::new(),
            Self::At { current, .. } => format!("Step {}", current + 1),
            Self::Complete { .. } => "Complete".to_owned(),
        }
    }

    /// What a screen reader hears for the track as a whole.
    pub fn spoken(self) -> String {
        match self {
            Self::Hidden => String::new(),
            Self::At { current, total } => format!("Step {} of {total}", current + 1),
            Self::Complete { total } => format!("All {total} steps complete"),
        }
    }

    fn total(self) -> usize {
        match self {
            Self::Hidden => 0,
            Self::At { total, .. } | Self::Complete { total } => total,
        }
    }

    fn reached(self, segment: usize) -> bool {
        match self {
            Self::Hidden => false,
            Self::At { current, .. } => segment <= current,
            Self::Complete { .. } => true,
        }
    }
}

/// The track for whatever `mark` currently reads.
pub fn step_track(mark: Memo<StepMark>) -> impl View + Styled {
    let caption = keyed(
        move |runtime| mark.get(runtime).caption(),
        |_, caption| {
            text(caption)
                .text_size(CAPTION_SIZE)
                .font_semibold()
                .font_family(INTER_FAMILY)
                .text_color(INK)
                .enter(|style| style.opacity(0.0).translate_y(CAPTION_RISE))
        },
    );
    let segments = keyed(
        move |runtime| mark.get(runtime).total(),
        move |_, total| {
            hstack(
                (0..total)
                    .map(|index| segment(mark, index))
                    .collect::<Vec<_>>(),
            )
            .gap(SEGMENT_GAP)
            .w_full()
        },
    )
    .w_full();
    vstack((caption, segments))
        .gap(CAPTION_GAP)
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

fn segment(mark: Memo<StepMark>, index: usize) -> impl View {
    let radius = SEGMENT_HEIGHT / 2.0;
    vstack(
        vstack(())
            .absolute()
            .inset(0.0)
            .rounded(radius)
            .bg(BRAND)
            .origin(0.0, 0.5)
            .scale_x(bind(move |runtime| {
                if mark.get(runtime).reached(index) {
                    1.0
                } else {
                    0.0
                }
            }))
            .transition(Spring::smooth()),
    )
    .relative()
    .h(SEGMENT_HEIGHT)
    .flex_1()
    .max_w(SEGMENT_WIDTH)
    .rounded(radius)
    .bg(TRACK)
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::button;
    use crownui::prelude::{Listeners, component};

    use super::*;

    fn track(start: StepMark, then: StepMark) -> TestApp {
        let mut app = TestApp::with_size(
            component(move |cx| {
                let shown = cx.signal(start);
                let mark = cx.memo(move |runtime| shown.get(runtime));
                vstack((
                    button("Advance").on_click(move |cx, _| shown.set(cx, then)),
                    step_track(mark).w(740.0),
                ))
            }),
            800.0,
            200.0,
        );
        app.settle();
        app
    }

    #[test]
    fn the_caption_counts_from_one() {
        assert_eq!(
            StepMark::At {
                current: 0,
                total: 6
            }
            .caption(),
            "Step 1"
        );
        assert_eq!(StepMark::Complete { total: 6 }.caption(), "Complete");
        assert_eq!(StepMark::Hidden.caption(), "");
    }

    #[test]
    fn segments_up_to_and_including_the_current_one_are_reached() {
        let mark = StepMark::At {
            current: 2,
            total: 6,
        };
        let reached: Vec<bool> = (0..6).map(|index| mark.reached(index)).collect();
        assert_eq!(reached, [true, true, true, false, false, false]);
        assert!((0..6).all(|index| StepMark::Complete { total: 6 }.reached(index)));
        assert!(!StepMark::Hidden.reached(0));
    }

    #[test]
    fn the_track_is_spoken_as_a_progress_bar() {
        let mut app = track(
            StepMark::At {
                current: 1,
                total: 6,
            },
            StepMark::At {
                current: 2,
                total: 6,
            },
        );
        let bar = app.find(role(Role::ProgressBar).name("Installation steps"));
        assert_eq!(bar.value.as_deref(), Some("Step 2 of 6"));
    }

    #[test]
    fn advancing_changes_the_caption_and_animates_the_new_segment() {
        let mut app = track(
            StepMark::At {
                current: 1,
                total: 6,
            },
            StepMark::At {
                current: 2,
                total: 6,
            },
        );
        app.click(role(Role::Button).name("Advance"));
        app.advance_ms(16);
        assert!(app.is_animating(), "the third segment fills in");
        app.settle();
        let bar = app.find(role(Role::ProgressBar).name("Installation steps"));
        assert_eq!(bar.value.as_deref(), Some("Step 3 of 6"));
    }

    #[test]
    fn six_segments_share_the_width_without_outgrowing_the_design() {
        let mut app = track(
            StepMark::At {
                current: 0,
                total: 6,
            },
            StepMark::Complete { total: 6 },
        );
        let bounds = app.find(role(Role::ProgressBar)).bounds;
        let widest = 6.0 * SEGMENT_WIDTH + 5.0 * SEGMENT_GAP;
        assert!(bounds.size.width <= 740.0 + 0.5, "{bounds:?}");
        assert!(widest <= 740.0);
    }
}
