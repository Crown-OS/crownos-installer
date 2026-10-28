//! A fact about the install: an icon tile, what it is, and its value.

use crownui::kit::icon_colored;
use crownui::prelude::{
    Color, Role, Semantic, Styled, View, WHITE, hstack, rgb, rgba, text, vstack,
};

const HEIGHT: f32 = 72.0;
const RADIUS: f32 = 14.0;
const PADDING: f32 = 14.0;
const TILE: f32 = 44.0;
const TILE_RADIUS: f32 = 11.0;
const ICON: f32 = 22.0;
const ICON_STROKE: f32 = 1.8;
const TEXT_GAP: f32 = 14.0;
const LINE_GAP: f32 = 3.0;
const TITLE_SIZE: f32 = 15.0;
const VALUE_SIZE: f32 = 13.0;
const TILE_FILL: Color = rgb(0xf2f2f6);
const ICON_COLOR: Color = rgb(0x78768a);
const PRIMARY: Color = rgb(0x17151f);
const MUTED: Color = rgb(0x8b8996);
const HAIRLINE: Color = rgba(0x1b1b2a12);

/// `title` reading `value`, beside `glyph`.
pub fn summary_tile(
    glyph: &'static str,
    title: &'static str,
    value: impl Into<String>,
) -> impl View + Styled {
    let value = value.into();
    hstack((
        vstack(icon_colored(glyph, ICON_STROKE, |_| ICON_COLOR).size(ICON))
            .size(TILE)
            .shrink(0.0)
            .items_center()
            .justify_center()
            .rounded(TILE_RADIUS)
            .bg(TILE_FILL),
        vstack((
            text(title)
                .text_size(TITLE_SIZE)
                .text_color(PRIMARY)
                .truncate(),
            text(value.clone())
                .text_size(VALUE_SIZE)
                .text_color(MUTED)
                .truncate(),
        ))
        .gap(LINE_GAP)
        .ml(TEXT_GAP)
        .flex_1(),
    ))
    .items_center()
    .h(HEIGHT)
    .px(PADDING)
    .rounded(RADIUS)
    .bg(WHITE)
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
