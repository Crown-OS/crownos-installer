//! ISO flow only: the first thing on screen, before the card. Installing is
//! the way in. Trying CrownOS without installing hands the screen to the
//! image's live desktop, so on an image without one that way is shown but
//! cannot be taken.

use crownui::ext::{Key, NamedKey};
use crownui::prelude::{
    Color, CursorIcon, Listeners, Role, Semantic, Shadow, Signal, Spring, Styled, Vector, View,
    WHITE, hstack, rgba, svg, text, vstack,
};

use crate::live;
use crate::ui::Wizard;
use crate::ui::components::{Emphasis, choice_panel};
use crate::ui::layout::nothing;
use crate::ui::palette::{BRAND, INK, SUBDUED};

const CONTENT_WIDTH: f32 = 1100.0;
const TITLE_SIZE: f32 = 48.0;
const SUBTITLE_SIZE: f32 = 24.0;
const COPY_GAP: f32 = 14.0;
const COPY_TO_TRAY: f32 = 40.0;
const TRAY_RADIUS: f32 = 28.0;
const TRAY_INSET: f32 = 10.0;
const TRAY_FILL: Color = rgba(0xffffffb3);
const TRAY_BLUR: f32 = 12.0;
const TRAY_SHADOW: Color = rgba(0x25171a14);
const TRAY_SHADOW_BLUR: f32 = 20.0;
const TRAY_SHADOW_OFFSET: f32 = 4.0;
const INSTALL_MARK_GAP: f32 = 8.0;
const RADIO_SIZE: f32 = 22.0;
const RADIO_RING: f32 = 2.0;
const RADIO_GAP: f32 = 16.0;
const UNAVAILABLE_OPACITY: f32 = 0.5;
const HOVER_SCALE: f32 = 1.01;
const PRESSED_SCALE: f32 = 0.99;
const ENTER_LIFT: f32 = 12.0;

const INSTALL_ART: &[u8] = include_bytes!("../../../assets/intro/install.png");
const TRY_ART: &[u8] = include_bytes!("../../../assets/intro/try.png");
const INSTALL_MARK: &str = include_str!("../../../assets/intro/install.svg");
const GO_MARK: &str = include_str!("../../../assets/intro/go.svg");

pub fn build(wizard: Signal<Wizard>) -> impl View + Styled {
    vstack((heading(), tray(wizard)))
        .gap(COPY_TO_TRAY)
        .items_center()
        .w_full()
        .max_w(CONTENT_WIDTH)
        .enter(|style| style.opacity(0.0).translate_y(ENTER_LIFT))
        .transition(Spring::smooth())
}

fn heading() -> impl View {
    vstack((
        text("Welcome to CrownOS")
            .text_size(TITLE_SIZE)
            .font_semibold()
            .text_color(INK)
            .role(Role::Heading),
        text("Choose how you want to experience it today.")
            .text_size(SUBTITLE_SIZE)
            .text_color(SUBDUED),
    ))
    .gap(COPY_GAP)
    .items_center()
}

fn tray(wizard: Signal<Wizard>) -> impl View {
    hstack((install(wizard), try_it_out(wizard)))
        .gap(TRAY_INSET)
        .p(TRAY_INSET)
        .w_full()
        .rounded(TRAY_RADIUS)
        .bg(TRAY_FILL)
        .backdrop_blur(TRAY_BLUR)
        .shadow(Shadow::new(
            TRAY_SHADOW,
            TRAY_SHADOW_BLUR,
            Vector::new(TRAY_SHADOW_OFFSET, TRAY_SHADOW_OFFSET),
        ))
}

fn install(wizard: Signal<Wizard>) -> impl View {
    choice_panel(
        INSTALL_ART,
        svg(INSTALL_MARK).mr(INSTALL_MARK_GAP),
        "Install",
        "Set up CrownOS on this device and get started with the full experience.",
        svg(GO_MARK).shrink(0.0),
        Emphasis::Chosen,
    )
    .role(Role::Button)
    .label("Install")
    .focusable(true)
    .cursor(CursorIcon::Pointer)
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
    let available = live::available();
    let radio = vstack(())
        .size(RADIO_SIZE)
        .shrink(0.0)
        .rounded(RADIO_SIZE / 2.0)
        .bg(WHITE)
        .border(RADIO_RING)
        .border_color(BRAND)
        .mr(RADIO_GAP);
    choice_panel(
        TRY_ART,
        radio,
        "Try it out",
        "Explore CrownOS without installing it. It’s perfect for a quick look around.",
        nothing(),
        Emphasis::Resting,
    )
    .role(Role::Button)
    .label("Try it out")
    .description(if available {
        ""
    } else {
        "Not available on this installer image"
    })
    .disabled(!available)
    .opacity(if available { 1.0 } else { UNAVAILABLE_OPACITY })
    .focusable(available)
    .cursor(if available {
        CursorIcon::Pointer
    } else {
        CursorIcon::Default
    })
    .on_click(move |cx, _| {
        if available {
            wizard.update(cx, Wizard::try_live);
        }
    })
    .on_key_down(move |cx, event| {
        if available
            && matches!(
                event.key,
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
            )
        {
            wizard.update(cx, Wizard::try_live);
        }
    })
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
