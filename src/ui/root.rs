//! The window's one view: the backdrop, and over it either the intro or the
//! card with the current page moving in and out of it and the step track along
//! its foot.

use crownui::kit::provide_theme;
use crownui::prelude::{Signal, Styled, View, bind, component, show, vstack};

use super::chrome::{StepMark, backdrop, card, step_track, step_transition};
use super::palette::oobe_theme;
use super::{Blocker, Context, Page, Wizard, pages};
use crate::engine::exec::Mode;

/// Space kept between the card and the window's edge.
const GUTTER: f32 = 24.0;
/// The track's segments sit level with the middle of the action pills.
const TRACK_LIFT: f32 = 17.0;
const TRACK_WIDTH: f32 = 740.0;
/// Kept clear at the track's right for the page's buttons.
const ACTIONS_ROOM: f32 = 330.0;

pub fn root(startup: Result<Context, Blocker>, mode: Mode) -> impl View {
    component(move |cx| {
        provide_theme(cx, oobe_theme());
        shell(cx.signal(Wizard::new(startup, mode)))
    })
}

/// Everything the window draws, over whatever wizard it is given.
pub fn shell(wizard: Signal<Wizard>) -> impl View {
    let on_intro = bind(move |runtime| wizard.with(runtime, |wizard| wizard.page == Page::Intro));
    vstack((
        backdrop(),
        show(
            on_intro,
            move || pages::intro(wizard),
            move || carded(wizard),
        )
        .w_full()
        .h_full()
        .items_center()
        .justify_center(),
    ))
    .w_full()
    .h_full()
    .p(GUTTER)
}

fn carded(wizard: Signal<Wizard>) -> impl View {
    component(move |cx| {
        let closing = cx.memo(move |runtime| wizard.with(runtime, |wizard| wizard.closing));
        let mark = cx.memo(move |runtime| wizard.with(runtime, step_mark));
        let pages = step_transition(
            move |runtime| wizard.with(runtime, |wizard| wizard.page),
            Page::ordinal,
            move |cx, page| pages::page(cx, wizard, page),
        );
        let track = step_track(mark)
            .absolute()
            .left(0.0)
            .right(ACTIONS_ROOM)
            .bottom(TRACK_LIFT)
            .max_w(TRACK_WIDTH);
        card(vstack((pages, track)).relative().flex_1().w_full(), closing)
    })
}

/// What the step track reads for the wizard as it stands.
pub fn step_mark(wizard: &Wizard) -> StepMark {
    let (total, current) = wizard.step_indicator();
    match wizard.page {
        Page::Blocked => StepMark::Hidden,
        Page::Done => StepMark::Complete { total },
        _ => StepMark::At { current, total },
    }
}
