//! A fact about the install: an icon tile, what it is, and its value, on a
//! glass pill.

use crownui::kit::icon_colored;
use crownui::prelude::{Color, Role, Semantic, Styled, View, hstack, rgb, text, vstack};

use crate::ui::palette::{FOREGROUND, GLASS, HAIRLINE, SECONDARY, TILE};

const HEIGHT: f32 = 72.0;
const PADDING_LEFT: f32 = 14.0;
const PADDING_RIGHT: f32 = 28.0;
const TILE_SIZE: f32 = 44.0;
const ICON: f32 = 20.0;
const ICON_STROKE: f32 = 1.8;
const TEXT_GAP: f32 = 14.0;
const LINE_GAP: f32 = 2.0;
const TITLE_SIZE: f32 = 16.0;
const TITLE_TRACKING: f32 = -0.16;
const VALUE_SIZE: f32 = 13.0;
const ICON_COLOR: Color = rgb(0xd4d4d8);

/// `title` reading `value`, beside `glyph`.
pub fn summary_tile(
    glyph: &'static str,
    title: &'static str,
    value: impl Into<String>,
) -> impl View + Styled {
    let value = value.into();
    hstack((
        vstack(icon_colored(glyph, ICON_STROKE, |_| ICON_COLOR).size(ICON))
            .size(TILE_SIZE)
            .shrink(0.0)
            .items_center()
            .justify_center()
            .rounded(TILE_SIZE / 2.0)
            .bg(TILE),
        vstack((
            text(title)
                .text_size(TITLE_SIZE)
                .font_medium()
                .tracking(TITLE_TRACKING)
                .text_color(FOREGROUND)
                .truncate(),
            text(value.clone())
                .text_size(VALUE_SIZE)
                .text_color(SECONDARY)
                .truncate(),
        ))
        .gap(LINE_GAP)
        .ml(TEXT_GAP)
        .flex_1()
        .min_w(0.0),
    ))
    .items_center()
    .h(HEIGHT)
    .pl(PADDING_LEFT)
    .pr(PADDING_RIGHT)
    .rounded(HEIGHT / 2.0)
    .bg(GLASS)
    .border(1.0)
    .border_color(HAIRLINE)
    .role(Role::Group)
    .label(title)
    .value(value)
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::{icons, provide_theme};
    use crownui::prelude::component;

    use super::*;
    use crate::ui::palette::oobe_theme;

    #[test]
    fn a_tile_is_named_by_what_it_describes_and_reads_its_value() {
        let mut app = TestApp::new(component(|cx| {
            provide_theme(cx, oobe_theme());
            vstack(summary_tile(icons::USER, "Account", "ada")).w(500.0)
        }));
        app.settle();
        let tile = app.find(role(Role::Group).name("Account"));
        assert_eq!(tile.value.as_deref(), Some("ada"));
        assert_eq!(tile.bounds.size.height, HEIGHT);
    }
}
