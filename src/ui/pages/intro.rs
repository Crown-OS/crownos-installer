//! ISO flow only: the first thing on screen, before the steps. Installing is
//! the way in. Trying CrownOS without installing hands the screen to the
//! image's live desktop.

use crownui::ext::{Key, NamedKey};
use crownui::prelude::{
    Color, CursorIcon, Listeners, Role, Semantic, Signal, Spring, Styled, View, hstack, rgba, text,
    vstack,
};

use crate::ui::Wizard;
use crate::ui::components::{Emphasis, choice_panel, glyph};
use crate::ui::palette::{FOREGROUND, SUBDUED};

const CONTENT_WIDTH: f32 = 1120.0;
const TITLE_SIZE: f32 = 64.0;
const TITLE_TRACKING: f32 = -1.28;
const TITLE_COLOR: Color = rgba(0xfffffff2);
const SUBTITLE_SIZE: f32 = 30.0;
const SUBTITLE_TRACKING: f32 = -0.45;
const COPY_GAP: f32 = 16.0;
const COPY_TO_CHOICES: f32 = 40.0;
const CHOICE_GAP: f32 = 24.0;
const HOVER_SCALE: f32 = 1.01;
const PRESSED_SCALE: f32 = 0.99;
const ENTER_LIFT: f32 = 12.0;
const FOCUS_RING: f32 = 2.0;

const INSTALL_ART: &[u8] = include_bytes!("../../../assets/intro/install.png");
const TRY_ART: &[u8] = include_bytes!("../../../assets/intro/try.png");

pub fn build(wizard: Signal<Wizard>) -> impl View + Styled {
    vstack((heading(), choices(wizard)))
        .gap(COPY_TO_CHOICES)
        .items_center()
        .w_full()
        .max_w(CONTENT_WIDTH)
        .enter(|style| style.opacity(0.0).translate_y(ENTER_LIFT))
        .transition(Spring::smooth())
}

fn heading() -> impl View {
    vstack((
        text("Welcome")
            .text_size(TITLE_SIZE)
            .font_medium()
            .tracking(TITLE_TRACKING)
            .text_color(TITLE_COLOR)
            .role(Role::Heading),
        text("How would you like to start?")
            .text_size(SUBTITLE_SIZE)
            .font_medium()
            .tracking(SUBTITLE_TRACKING)
            .text_color(SUBDUED),
    ))
    .gap(COPY_GAP)
    .items_center()
}

fn choices(wizard: Signal<Wizard>) -> impl View {
    hstack((install(wizard), try_it_out(wizard)))
        .gap(CHOICE_GAP)
        .w_full()
}

fn install(wizard: Signal<Wizard>) -> impl View {
    choice_panel(
        INSTALL_ART,
        glyph::ARROW_SQUARE_DOWN,
        "Install CrownOS",
        "Set up CrownOS on this device and get started with the full experience.",
        Emphasis::Chosen,
    )
    .role(Role::Button)
    .label("Install CrownOS")
    .focusable(true)
    .cursor(CursorIcon::Pointer)
    .focus_visible(|style| style.ring(FOCUS_RING, FOREGROUND))
    .on_click(move |cx, _| wizard.update(cx, Wizard::advance))
    .on_key_down(move |cx, event| {
        if matches!(
            event.key,
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
        ) {
            wizard.update(cx, Wizard::advance);
        }
    })
    .hover(|style| style.scale(HOVER_SCALE))
    .active(|style| style.scale(PRESSED_SCALE))
    .transition(Spring::snappy())
}

fn try_it_out(wizard: Signal<Wizard>) -> impl View {
    choice_panel(
        TRY_ART,
        glyph::MONITOR,
        "Try it out",
        "Explore CrownOS without installing it. It’s perfect for a quick look around.",
        Emphasis::Resting,
    )
    .role(Role::Button)
    .label("Try it out")
    .focusable(true)
    .cursor(CursorIcon::Pointer)
    .focus_visible(|style| style.ring(FOCUS_RING, FOREGROUND))
    .on_click(move |cx, _| wizard.update(cx, Wizard::try_live))
    .on_key_down(move |cx, event| {
        if matches!(
            event.key,
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
        ) {
            wizard.update(cx, Wizard::try_live);
        }
    })
    .hover(|style| style.scale(HOVER_SCALE))
    .active(|style| style.scale(PRESSED_SCALE))
    .transition(Spring::snappy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::png::decode_rgba;

    #[test]
    fn the_artwork_decodes() {
        for art in [INSTALL_ART, TRY_ART] {
            assert!(decode_rgba(art).is_some());
        }
    }
}
