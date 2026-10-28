//! How a page is laid out on the card, and the buttons every page shares.

use std::time::Duration;

use crownui::kit::{ButtonKind, ButtonVariant, INTER_FAMILY};
use crownui::prelude::{
    AnyView, Color, Element, IntoProp, Listeners, Role, Semantic, Signal, Styled, View, ViewSeq,
    bind, hstack, rgb, scroll_view, text, vstack,
};

use super::Wizard;
use super::components::{PILL_HEIGHT, pill};
use super::palette::INK;

/// Gap between the blocks of a page's content.
pub const BLOCK_GAP: f32 = 16.0;
/// Gap between tiles in a grid.
pub const TILE_GAP: f32 = 12.0;
const TITLE_SIZE: f32 = 48.0;
const SUBTITLE_SIZE: f32 = 24.0;
const HEADER_TO_CONTENT: f32 = 37.0;
/// Room left above the action row for the step caption beside it.
const CONTENT_TO_ACTIONS: f32 = 48.0;
const ACTION_GAP: f32 = 12.0;
const BODY: Color = rgb(0x403e4a);
const MUTED: Color = rgb(0x8b8996);
const NOTE_SIZE: f32 = 13.0;
/// How long the card takes to sink away before the machine restarts.
const CLOSE_DELAY: Duration = Duration::from_millis(450);

/// What a page puts on the card.
pub struct PageParts {
    pub title: String,
    pub subtitle: String,
    /// Shown beside the heading, as the outcome badge is.
    pub badge: Option<AnyView>,
    pub content: AnyView,
    pub actions: AnyView,
}

impl PageParts {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            badge: None,
            content: nothing(),
            actions: nothing(),
        }
    }

    pub fn badge(mut self, badge: impl View) -> Self {
        self.badge = Some(badge.boxed());
        self
    }

    pub fn content(mut self, content: impl View) -> Self {
        self.content = content.boxed();
        self
    }

    pub fn actions(mut self, actions: impl ViewSeq) -> Self {
        self.actions = hstack(actions).gap(ACTION_GAP).items_center().boxed();
        self
    }

    /// The page: heading, content that scrolls if it must, actions bottom right.
    pub fn into_view(self) -> impl View {
        let heading = vstack((
            text(self.title)
                .text_size(TITLE_SIZE)
                .font_family(INTER_FAMILY)
                .text_color(INK)
                .role(Role::Heading),
            text(self.subtitle)
                .text_size(SUBTITLE_SIZE)
                .font_family(INTER_FAMILY)
                .text_color(BODY),
        ))
        .flex_1();
        vstack((
            hstack((heading, self.badge)).items_start().w_full(),
            scroll_view(vstack(self.content).w_full())
                .flex_1()
                .w_full()
                .mt(HEADER_TO_CONTENT),
            hstack(self.actions)
                .justify_end()
                .items_center()
                .w_full()
                .h(PILL_HEIGHT)
                .mt(CONTENT_TO_ACTIONS),
        ))
        .w_full()
        .flex_1()
    }
}

/// Nothing, taking no space and no gap among its siblings.
pub fn nothing() -> AnyView {
    vstack(()).display_none().boxed()
}

/// Two columns of equal tiles.
pub fn tiles(children: impl ViewSeq) -> impl View + Styled {
    vstack(children).grid().grid_cols(2).gap(TILE_GAP).w_full()
}

/// A small heading over a block.
pub fn caption(content: impl IntoProp<String>) -> impl View + Styled {
    text(content)
        .text_size(14.0)
        .font_semibold()
        .text_color(INK)
}

pub fn muted(content: impl IntoProp<String>) -> impl View + Styled {
    text(content).text_size(NOTE_SIZE).text_color(MUTED)
}

/// A pill that runs `change` on the wizard.
pub fn wizard_pill(
    wizard: Signal<Wizard>,
    label: &'static str,
    variant: ButtonVariant,
    change: impl Fn(&mut Wizard) + 'static,
) -> Element<ButtonKind> {
    pill(label, variant).on_click(move |cx, _| wizard.update(cx, &change))
}

/// "Back", when the flow has a step behind this one.
pub fn back_pill(wizard: Signal<Wizard>) -> Element<ButtonKind> {
    wizard_pill(wizard, "Back", ButtonVariant::Secondary, Wizard::retreat)
}

/// "Next", disabled while `blocked` holds.
pub fn next_pill(
    wizard: Signal<Wizard>,
    blocked: impl Fn(&Wizard) -> bool + 'static,
) -> Element<ButtonKind> {
    wizard_pill(wizard, "Next", ButtonVariant::Primary, Wizard::advance)
        .disabled(bind(move |runtime| wizard.with(runtime, &blocked)))
}

/// Ask for a reboot once the card has sunk away. There is nothing left for
/// the window to do afterwards, so it closes.
pub fn restart_pill(
    wizard: Signal<Wizard>,
    label: &'static str,
    variant: ButtonVariant,
) -> Element<ButtonKind> {
    pill(label, variant)
        .disabled(bind(move |runtime| {
            wizard.with(runtime, |wizard| wizard.closing)
        }))
        .on_click(move |cx, _| {
            wizard.update(cx, Wizard::close);
            cx.spawn(
                async { tokio::time::sleep(CLOSE_DELAY).await },
                move |cx, ()| {
                    wizard.with(cx, Wizard::restart);
                    std::process::exit(0);
                },
            );
        })
}
