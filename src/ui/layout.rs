//! How a page is laid out in the column, and the buttons every page shares.

use std::time::Duration;

use crownui::ext::Kind;
use crownui::kit::ButtonVariant;
use crownui::prelude::{
    AnyView, Element, IntoProp, Listeners, Memo, Role, Semantic, Signal, Styled, View, ViewSeq,
    bind, hstack, scroll_view, text, vstack,
};

use super::components::{Knob, PILL_HEIGHT, PillStyle, glyph, pill, styled_pill};
use super::palette::{FOREGROUND, SECONDARY, SUBDUED};
use super::{Density, Measures, Wizard};

/// Gap between the blocks of a page's content.
pub const BLOCK_GAP: f32 = 16.0;
/// Gap between tiles in a grid.
pub const TILE_GAP: f32 = 16.0;
/// From the foot of the step track's segments to the top of the buttons.
pub const TRACK_TO_ACTIONS: f32 = 15.0;
/// The step track's own height: a label row, then its segment.
const TRACK_HEIGHT: f32 = 39.0;
/// Kept clear between the content and the step track.
const CONTENT_TO_TRACK: f32 = 24.0;
const TITLE_TRACKING: f32 = -1.5;
/// The design sets a step's title 66 px high at 60 px.
const TITLE_LEADING: f32 = 1.1;
const SUBTITLE_TRACKING: f32 = -0.45;
const BADGE_GAP: f32 = 24.0;
const HERO_TITLE_TRACKING: f32 = -5.28;
const HERO_HEADING_GAP: f32 = 20.0;
const HERO_TO_ACTIONS: f32 = 40.0;
const HERO_NOTE_WIDTH: f32 = 640.0;
const REGULAR: u16 = 400;
const ACTION_GAP: f32 = 12.0;
/// One column of a two-column grid at the design's width.
const LONE_CHOICE_WIDTH: f32 = 552.0;
const CAPTION_SIZE: f32 = 16.0;
const CAPTION_TRACKING: f32 = -0.48;
const NOTE_SIZE: f32 = 13.0;
/// How long the column takes to sink away before the machine restarts.
const CLOSE_DELAY: Duration = Duration::from_millis(450);

/// How a page arranges its parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrangement {
    /// Heading at the top, content under it, buttons along the foot.
    Steps,
    /// A large heading in the middle, its buttons and content under it.
    Hero,
}

/// What a page puts in the column.
pub struct PageParts {
    pub title: String,
    pub subtitle: String,
    /// Shown over the heading, as the outcome badge is.
    pub badge: Option<AnyView>,
    pub content: AnyView,
    /// Bottom left: the way back.
    pub leading: AnyView,
    /// Bottom right: the way on.
    pub actions: AnyView,
    pub arrangement: Arrangement,
}

impl PageParts {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            badge: None,
            content: nothing(),
            leading: nothing(),
            actions: nothing(),
            arrangement: Arrangement::Steps,
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

    pub fn leading(mut self, leading: impl View) -> Self {
        self.leading = leading.boxed();
        self
    }

    pub fn actions(mut self, actions: impl ViewSeq) -> Self {
        self.actions = hstack(actions).gap(ACTION_GAP).items_center().boxed();
        self
    }

    pub fn hero(mut self) -> Self {
        self.arrangement = Arrangement::Hero;
        self
    }

    /// The page, set at `density`.
    pub fn into_view(self, density: Memo<Density>) -> AnyView {
        match self.arrangement {
            Arrangement::Steps => self.into_steps(density).boxed(),
            Arrangement::Hero => self.into_hero(density).boxed(),
        }
    }

    /// Heading, content that scrolls if it must, buttons along the foot.
    fn into_steps(self, density: Memo<Density>) -> impl View {
        let badge = self
            .badge
            .map(|badge| vstack(badge).items_center().mb(BADGE_GAP));
        vstack((
            badge,
            heading(
                self.title,
                measure(density, |measures| measures.title),
                TITLE_TRACKING,
                TitleStyle::Step,
                self.subtitle,
                density,
            )
            .gap(measure(density, |measures| measures.heading_gap)),
            scroll_view(vstack(self.content).w_full())
                .flex_1()
                .w_full()
                .mt(measure(density, |measures| measures.header_to_content))
                .mb(CONTENT_TO_TRACK + TRACK_HEIGHT + TRACK_TO_ACTIONS),
            hstack((self.leading, vstack(()).flex_1(), self.actions))
                .items_center()
                .w_full()
                .h(PILL_HEIGHT)
                .shrink(0.0),
        ))
        .w_full()
        .flex_1()
    }

    /// A large heading in the middle of the column, everything else under it.
    fn into_hero(self, density: Memo<Density>) -> impl View {
        vstack((
            vstack(()).flex_1(),
            self.badge.map(|badge| vstack(badge).mb(BADGE_GAP)),
            heading(
                self.title,
                measure(density, |measures| measures.hero_title),
                HERO_TITLE_TRACKING,
                TitleStyle::Hero,
                self.subtitle,
                density,
            )
            .gap(HERO_HEADING_GAP),
            hstack((self.leading, self.actions))
                .gap(ACTION_GAP)
                .items_center()
                .mt(HERO_TO_ACTIONS),
            vstack(self.content)
                .w_full()
                .max_w(HERO_NOTE_WIDTH)
                .mt(HERO_TO_ACTIONS),
            vstack(()).flex_1(),
        ))
        .items_center()
        .w_full()
        .flex_1()
    }
}

/// One of the sizes that follow the density.
fn measure(density: Memo<Density>, pick: fn(&Measures) -> f32) -> impl IntoProp<f32> + 'static {
    bind(move |runtime| pick(&density.get(runtime).measures()))
}

/// How a heading's title is set.
#[derive(Clone, Copy)]
enum TitleStyle {
    /// Medium weight, on the design's tight line.
    Step,
    /// Regular weight, on the font's own line.
    Hero,
}

fn heading(
    title: String,
    size: impl IntoProp<f32>,
    tracking: f32,
    style: TitleStyle,
    subtitle: String,
    density: Memo<Density>,
) -> impl View + Styled {
    let title = text(title)
        .text_size(size)
        .tracking(tracking)
        .text_color(FOREGROUND)
        .text_center()
        .role(Role::Heading);
    let title = match style {
        TitleStyle::Step => title.font_medium().leading(TITLE_LEADING),
        TitleStyle::Hero => title.font_weight(REGULAR),
    };
    vstack((
        title,
        text(subtitle)
            .text_size(measure(density, |measures| measures.subtitle))
            .font_medium()
            .tracking(SUBTITLE_TRACKING)
            .text_color(SUBDUED)
            .text_center(),
    ))
    .items_center()
    .w_full()
    .shrink(0.0)
}

/// Nothing, taking no space and no gap among its siblings.
pub fn nothing() -> AnyView {
    vstack(()).display_none().boxed()
}

/// Two columns of equal tiles.
pub fn tiles(children: impl ViewSeq) -> impl View + Styled {
    vstack(children).grid().grid_cols(2).gap(TILE_GAP).w_full()
}

/// Choices two to a line. A lone choice sits in the middle, one column wide.
pub fn choice_grid<V: View>(mut choices: Vec<V>) -> AnyView {
    match (choices.len(), choices.pop()) {
        (1, Some(choice)) => vstack(choice)
            .w_full()
            .max_w(LONE_CHOICE_WIDTH)
            .self_center()
            .boxed(),
        (_, last) => {
            choices.extend(last);
            tiles(choices).boxed()
        }
    }
}

/// A small heading over a group of choices.
pub fn caption(content: impl IntoProp<String>) -> impl View + Styled {
    text(content)
        .text_size(CAPTION_SIZE)
        .font_medium()
        .tracking(CAPTION_TRACKING)
        .text_color(SECONDARY)
}

pub fn muted(content: impl IntoProp<String>) -> impl View + Styled {
    text(content).text_size(NOTE_SIZE).text_color(SECONDARY)
}

/// A pill that runs `change` on the wizard.
pub fn wizard_pill(
    wizard: Signal<Wizard>,
    label: &'static str,
    variant: ButtonVariant,
    change: impl Fn(&mut Wizard) + 'static,
) -> Element<impl Kind> {
    pill(label, variant).on_click(move |cx, _| wizard.update(cx, &change))
}

/// "Go back", when the flow has a step behind this one.
pub fn back_pill(wizard: Signal<Wizard>) -> Element<impl Kind> {
    styled_pill("Go back", PillStyle::Glass(Some(glyph::ARROW_LEFT)))
        .on_click(move |cx, _| wizard.update(cx, Wizard::retreat))
}

/// "Continue", disabled while `blocked` holds.
pub fn next_pill(
    wizard: Signal<Wizard>,
    blocked: impl Fn(&Wizard) -> bool + 'static,
) -> Element<impl Kind> {
    wizard_pill(wizard, "Continue", ButtonVariant::Primary, Wizard::advance)
        .disabled(bind(move |runtime| wizard.with(runtime, &blocked)))
}

/// Ask for a reboot once the column has sunk away. There is nothing left for
/// the window to do afterwards, so it closes.
pub fn restart_pill(
    wizard: Signal<Wizard>,
    label: &'static str,
    variant: ButtonVariant,
) -> Element<impl Kind> {
    let style = match variant {
        ButtonVariant::Primary => PillStyle::Primary(Knob::Restart),
        _ => PillStyle::Glass(None),
    };
    styled_pill(label, style)
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
