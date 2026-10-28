//! A glyph that morphs with the state of a piece of work: a step number while
//! it waits, a spinner while it runs, a check that springs in when it is done,
//! a cross if it failed. Each layer scales and fades rather than swapping, so
//! the change is seen happening.

use crownui::ext::Kind;
use crownui::kit::{icon_colored, icons, spinner};
use crownui::prelude::{
    Color, Element, Memo, Role, Semantic, Spring, Styled, TRANSPARENT, View, WHITE, bind, rgb,
    show, text, vstack,
};

use crate::ui::palette::BRAND;

const DISC_SIZE: f32 = 28.0;
const BARE_SIZE: f32 = 18.0;
const NUMBER_SIZE: f32 = 12.0;
const SPINNER_RATIO: f32 = 0.55;
const MARK_RATIO: f32 = 0.5;
const MARK_STROKE: f32 = 3.0;
const HIDDEN_SCALE: f32 = 0.3;
const TWIST: f32 = -45.0;
const WAITING: Color = rgb(0xf2f2f6);
const WAITING_TEXT: Color = rgb(0x8b8996);
const FAILED: Color = rgb(0xc22727);
const WORKING_TINT: f32 = 0.12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not started; shows its position in the queue.
    Waiting(usize),
    Working,
    Done,
    Failed,
}

impl Status {
    pub fn spoken(self) -> &'static str {
        match self {
            Self::Waiting(_) => "Waiting",
            Self::Working => "In progress",
            Self::Done => "Done",
            Self::Failed => "Failed",
        }
    }

    fn disc(self) -> Color {
        match self {
            Self::Waiting(_) => WAITING,
            Self::Working => BRAND.with_alpha_factor(WORKING_TINT),
            Self::Done => BRAND,
            Self::Failed => FAILED,
        }
    }
}

/// How the glyph is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// On a coloured disc, for lists of steps.
    Disc,
    /// Just the mark, in the brand colour, for trailing a row.
    Bare,
}

/// The glyph for whatever `status` reads.
pub fn status_glyph(status: Memo<Status>, look: Look) -> impl View + Styled {
    let size = match look {
        Look::Disc => DISC_SIZE,
        Look::Bare => BARE_SIZE,
    };
    let mark_color = match look {
        Look::Disc => WHITE,
        Look::Bare => BRAND,
    };
    let shown = move |wanted: fn(Status) -> bool| {
        (
            bind(move |runtime| {
                if wanted(status.get(runtime)) {
                    1.0
                } else {
                    0.0
                }
            }),
            bind(move |runtime| {
                if wanted(status.get(runtime)) {
                    1.0
                } else {
                    HIDDEN_SCALE
                }
            }),
        )
    };

    let (number_opacity, number_scale) = shown(|status| matches!(status, Status::Waiting(_)));
    let number = layer(
        text(bind(move |runtime| match status.get(runtime) {
            Status::Waiting(position) => position.to_string(),
            _ => String::new(),
        }))
        .text_size(NUMBER_SIZE)
        .font_semibold()
        .text_color(WAITING_TEXT),
    )
    .opacity(number_opacity)
    .scale(number_scale)
    .hidden(look == Look::Bare);

    let working = show(
        bind(move |runtime| status.get(runtime) == Status::Working),
        move || {
            layer(spinner().size(size * SPINNER_RATIO).shrink(0.0))
                .enter(|style| style.opacity(0.0).scale(HIDDEN_SCALE))
                .exit(|style| style.opacity(0.0).scale(HIDDEN_SCALE))
        },
        || vstack(()).display_none(),
    )
    .absolute()
    .inset(0.0);

    let (done_opacity, done_scale) = shown(|status| status == Status::Done);
    let done =
        layer(icon_colored(icons::CHECK, MARK_STROKE, move |_| mark_color).size(size * MARK_RATIO))
            .opacity(done_opacity)
            .scale(done_scale)
            .rotate(bind(move |runtime| {
                if status.get(runtime) == Status::Done {
                    0.0
                } else {
                    TWIST
                }
            }))
            .transition(Spring::bouncy());

    let (failed_opacity, failed_scale) = shown(|status| status == Status::Failed);
    let failed =
        layer(icon_colored(icons::X, MARK_STROKE, move |_| mark_color).size(size * MARK_RATIO))
            .opacity(failed_opacity)
            .scale(failed_scale)
            .transition(Spring::bouncy());

    vstack((number, working, done, failed))
        .relative()
        .size(size)
        .shrink(0.0)
        .rounded(size / 2.0)
        .bg(bind(move |runtime| match look {
            Look::Disc => status.get(runtime).disc(),
            Look::Bare => TRANSPARENT,
        }))
        .role(Role::Image)
        .label(bind(move |runtime| status.get(runtime).spoken().to_owned()))
        .transition(Spring::smooth())
}

fn layer(content: impl View) -> Element<impl Kind> {
    vstack(content)
        .absolute()
        .inset(0.0)
        .items_center()
        .justify_center()
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::{button, provide_theme};
    use crownui::prelude::{Listeners, component, hstack};

    use super::*;
    use crate::ui::palette::oobe_theme;

    fn glyph(start: Status, then: Status) -> TestApp {
        let mut app = TestApp::new(component(move |cx| {
            provide_theme(cx, oobe_theme());
            let current = cx.signal(start);
            let status = cx.memo(move |runtime| current.get(runtime));
            hstack((
                button("Change").on_click(move |cx, _| current.set(cx, then)),
                status_glyph(status, Look::Disc),
            ))
        }));
        app.settle();
        app
    }

    fn spoken(app: &mut TestApp, status: Status) -> bool {
        app.exists(role(Role::Image).name(status.spoken()))
    }

    #[test]
    fn each_status_is_spoken_and_coloured_differently() {
        let all = [
            Status::Waiting(3),
            Status::Working,
            Status::Done,
            Status::Failed,
        ];
        let discs: Vec<_> = all.iter().map(|status| status.disc()).collect();
        for (index, disc) in discs.iter().enumerate() {
            assert!(!discs[index + 1..].contains(disc));
        }
        assert_eq!(Status::Working.spoken(), "In progress");
    }

    #[test]
    fn a_waiting_step_shows_its_number() {
        let mut app = glyph(Status::Waiting(4), Status::Working);
        assert!(spoken(&mut app, Status::Waiting(4)));
        assert!(app.exists(crowntest::text("4")));
    }

    #[test]
    fn starting_work_morphs_the_number_into_a_spinner() {
        let mut app = glyph(Status::Waiting(4), Status::Working);
        app.click(role(Role::Button).name("Change"));
        app.advance_ms(16);
        assert!(app.is_animating());
        assert!(spoken(&mut app, Status::Working));
        let disc = app.find(role(Role::Image).name("In progress")).bounds;
        let spinner = app.find(role(Role::ProgressBar)).bounds;
        assert!(
            (disc.center().x - spinner.center().x).abs() < 0.5
                && (disc.center().y - spinner.center().y).abs() < 0.5,
            "the spinner sits in the middle of the disc: {disc:?} {spinner:?}"
        );
    }

    #[test]
    fn finishing_work_retires_the_spinner_and_springs_the_check_in() {
        let mut app = glyph(Status::Working, Status::Done);
        app.click(role(Role::Button).name("Change"));
        app.advance_ms(16);
        assert!(app.is_animating(), "the check springs in");
        app.settle();
        assert!(spoken(&mut app, Status::Done));
        assert!(
            !app.exists(role(Role::ProgressBar)),
            "no spinner runs once done"
        );
        assert!(!app.is_animating(), "nothing keeps animating once settled");
    }

    #[test]
    fn a_failure_is_spoken() {
        let mut app = glyph(Status::Working, Status::Failed);
        app.click(role(Role::Button).name("Change")).settle();
        assert!(spoken(&mut app, Status::Failed));
    }
}
