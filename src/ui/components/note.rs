//! A line the user should read: an icon in the tone's colour, then the
//! message, on a white strip — or, for something that stopped the install,
//! a titled block on blush.

use crownui::kit::{icon_colored, icons};
use crownui::prelude::{
    Color, IntoProp, Role, Semantic, Styled, View, WHITE, hstack, rgb, text, vstack,
};

use crate::ui::palette::{BLUSH, BRAND, INK, OUTLINE};

const ICON_SIZE: f32 = 16.0;
const ICON_STROKE: f32 = 2.0;
const GAP: f32 = 10.0;
const PADDING_X: f32 = 16.0;
const PADDING_Y: f32 = 12.0;
const RADIUS: f32 = 14.0;
const MESSAGE_SIZE: f32 = 13.0;
const TITLE_SIZE: f32 = 15.0;
const DETAIL_GAP: f32 = 4.0;
const BODY: Color = rgb(0x403e4a);
const DANGER: Color = rgb(0xc22727);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Worth knowing.
    Info,
    /// Read this before continuing.
    Caution,
    /// Something stopped.
    Danger,
}

impl Tone {
    fn glyph(self) -> &'static str {
        match self {
            Self::Info => icons::INFO,
            Self::Caution => icons::TRIANGLE_ALERT,
            Self::Danger => icons::OCTAGON_ALERT,
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Info | Self::Caution => BRAND,
            Self::Danger => DANGER,
        }
    }
}

/// One line of advice.
pub fn note(tone: Tone, message: impl IntoProp<String>) -> impl View + Styled {
    hstack((
        glyph(tone),
        text(message)
            .text_size(MESSAGE_SIZE)
            .text_color(BODY)
            .flex_1(),
    ))
    .gap(GAP)
    .items_center()
    .px(PADDING_X)
    .py(PADDING_Y)
    .w_full()
    .rounded(RADIUS)
    .bg(WHITE)
    .border(1.0)
    .border_color(OUTLINE)
    .role(Role::Group)
}

/// A titled explanation: why something will not work, or why it stopped.
pub fn alert(
    tone: Tone,
    title: impl Into<String>,
    detail: impl IntoProp<String>,
) -> impl View + Styled {
    let title = title.into();
    hstack((
        glyph(tone).mt(1.0),
        vstack((
            text(title.clone())
                .text_size(TITLE_SIZE)
                .font_semibold()
                .text_color(INK),
            text(detail).text_size(MESSAGE_SIZE).text_color(BODY),
        ))
        .gap(DETAIL_GAP)
        .flex_1(),
    ))
    .gap(GAP)
    .items_start()
    .px(PADDING_X)
    .py(PADDING_Y + 2.0)
    .w_full()
    .rounded(RADIUS)
    .bg(if tone == Tone::Danger { BLUSH } else { WHITE })
    .border(1.0)
    .border_color(tone.color().with_alpha_factor(0.35))
    .role(Role::Group)
    .label(title)
    .enter(|style| style.opacity(0.0).translate_y(-6.0))
}

fn glyph(tone: Tone) -> impl View + Styled {
    let color = tone.color();
    icon_colored(tone.glyph(), ICON_STROKE, move |_| color)
        .size(ICON_SIZE)
        .shrink(0.0)
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role, text as text_query};
    use crownui::kit::provide_theme;
    use crownui::prelude::component;

    use super::*;
    use crate::ui::palette::oobe_theme;

    #[test]
    fn each_tone_has_its_own_glyph() {
        let glyphs = [Tone::Info, Tone::Caution, Tone::Danger].map(Tone::glyph);
        assert_ne!(glyphs[0], glyphs[1]);
        assert_ne!(glyphs[1], glyphs[2]);
        assert_eq!(Tone::Danger.color(), DANGER);
    }

    #[test]
    fn an_alert_is_named_by_its_title_and_shows_its_detail() {
        let mut app = TestApp::new(component(|cx| {
            provide_theme(cx, oobe_theme());
            vstack((
                alert(
                    Tone::Danger,
                    "The install stopped",
                    "pacstrap exited with status 1",
                ),
                note(Tone::Info, "Remove the installation media."),
            ))
        }));
        app.settle();
        assert!(app.exists(role(Role::Group).name("The install stopped")));
        assert!(app.exists(text_query("pacstrap exited with status 1")));
        assert!(app.exists(text_query("Remove the installation media.")));
    }
}
