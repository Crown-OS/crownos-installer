//! The window's one view: the backdrop, and over it either the intro or the
//! current page moving in and out of the column, with the step track along
//! its foot.

use crownui::kit::provide_theme;
use crownui::prelude::{Signal, Styled, View, bind, component, show, vstack};

use super::chrome::{StepMark, backdrop, column, sized, step_track, step_transition};
use super::components::PILL_HEIGHT;
use super::layout::TRACK_TO_ACTIONS;
use super::palette::oobe_theme;
use super::{Blocker, Context, Density, Page, Wizard, pages};
use crate::engine::exec::Mode;

/// Space kept between the content and the window's sides.
const GUTTER: f32 = 24.0;

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
            move || framed(wizard),
        )
        .w_full()
        .h_full()
        .items_center()
        .justify_center(),
    ))
    .w_full()
    .h_full()
    .px(GUTTER)
}

fn framed(wizard: Signal<Wizard>) -> impl View {
    sized(move |window| {
        component(move |cx| {
            let density = cx.memo(move |runtime| Density::for_height(window.get(runtime).height));
            let closing = cx.memo(move |runtime| wizard.with(runtime, |wizard| wizard.closing));
            let mark = cx.memo(move |runtime| wizard.with(runtime, step_mark));
            let pages = step_transition(
                move |runtime| wizard.with(runtime, |wizard| wizard.page),
                Page::ordinal,
                move |cx, page| pages::page(cx, wizard, page, density),
            );
            let track = step_track(mark)
                .absolute()
                .left(0.0)
                .right(0.0)
                .bottom(PILL_HEIGHT + TRACK_TO_ACTIONS);
            column(
                vstack((pages, track)).relative().flex_1().w_full(),
                window,
                density,
                closing,
            )
        })
    })
    .w_full()
    .h_full()
    .items_center()
}

/// What the step track reads for the wizard as it stands.
pub fn step_mark(wizard: &Wizard) -> StepMark {
    match wizard.page {
        Page::Blocked | Page::Done => StepMark::Hidden,
        _ => StepMark::At {
            current: wizard.step_indicator().1,
            labels: Page::labels(wizard.provenance()),
        },
    }
}
