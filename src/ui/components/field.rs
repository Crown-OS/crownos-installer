//! A captioned text field: the kit's input inside the design's glass pill,
//! whose rim brightens while the field has focus.

use crownui::kit::{
    GradientStops, PopoverColors, TextInputKind, TextInputOptions, Theme, provide_theme, use_theme,
};
use crownui::prelude::{
    Color, Element, Listeners, Semantic, Spring, Styled, TRANSPARENT, View, bind, component,
    hstack, icon, rgba, text, vstack,
};

use crate::ui::palette::{GLASS, GLASS_LIT, SUBDUED};

const CAPTION_SIZE: f32 = 14.0;
const CAPTION_TRACKING: f32 = -0.14;
const CAPTION_GAP: f32 = 10.0;
const FIELD_HEIGHT: f32 = 56.0;
const FIELD_INSET: f32 = 24.0;
const FIELD_INSET_BESIDE_GLYPH: f32 = 19.0;
const GLYPH_SIZE: f32 = 18.0;
const GLYPH_GAP: f32 = 10.0;
const ENTRY_SIZE: f32 = 17.0;
const FOCUSED_RIM: f32 = 1.5;
const RESTING_RIM: f32 = 1.0;
const FOCUSED_RIM_COLOR: Color = rgba(0xffffffb3);
const RESTING_RIM_COLOR: Color = rgba(0xffffff1a);
const GLYPH_COLOR: Color = rgba(0xffffffc7);

/// `input` under `caption`, which also names it for screen readers, with
/// `glyph` (an SVG source) at its end when there is one.
pub fn field(
    caption: &'static str,
    input: Element<TextInputKind>,
    glyph: Option<&'static str>,
) -> impl View {
    component(move |cx| {
        let base = use_theme(cx).with_untracked(cx, |theme| *theme);
        provide_theme(cx, chromeless(base));
        let focused = cx.signal(false);
        let entry = input
            .stretch()
            .text_size(ENTRY_SIZE)
            .label(caption)
            .on_focus(move |cx, _| focused.set(cx, true))
            .on_blur(move |cx, _| focused.set(cx, false));
        let trailing = glyph.map(|source| icon(source, GLYPH_COLOR).size(GLYPH_SIZE).shrink(0.0));
        vstack((
            text(caption)
                .text_size(CAPTION_SIZE)
                .font_medium()
                .tracking(CAPTION_TRACKING)
                .text_color(SUBDUED),
            hstack((vstack(entry).flex_1().min_w(0.0), trailing))
                .gap(GLYPH_GAP)
                .items_center()
                .h(FIELD_HEIGHT)
                .pl(FIELD_INSET)
                .pr(if glyph.is_some() {
                    FIELD_INSET_BESIDE_GLYPH
                } else {
                    FIELD_INSET
                })
                .rounded(FIELD_HEIGHT / 2.0)
                .bg(bind(move |runtime| {
                    if focused.get(runtime) {
                        GLASS_LIT
                    } else {
                        GLASS
                    }
                }))
                .border(bind(move |runtime| rim(focused.get(runtime)).0))
                .border_color(bind(move |runtime| rim(focused.get(runtime)).1))
                .transition(Spring::snappy()),
        ))
        .gap(CAPTION_GAP)
        .w_full()
    })
}

/// The rim's width and colour.
fn rim(focused: bool) -> (f32, Color) {
    if focused {
        (FOCUSED_RIM, FOCUSED_RIM_COLOR)
    } else {
        (RESTING_RIM, RESTING_RIM_COLOR)
    }
}

/// `theme` with the input's own surface, rim and focus ring cleared, so only
/// the pill around it shows.
fn chromeless(theme: Theme) -> Theme {
    Theme {
        popover: PopoverColors {
            trigger_bg: TRANSPARENT,
            border: TRANSPARENT,
            ring: GradientStops::flat(TRANSPARENT),
            ..theme.popover
        },
        ..theme
    }
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role, text as text_query};

    use crownui::kit::text_input;
    use crownui::prelude::{hstack, vstack as column};

    use super::*;
    use crate::ui::palette::oobe_theme;

    fn form() -> TestApp {
        let mut app = TestApp::with_size(
            component(|cx| {
                provide_theme(cx, oobe_theme());
                let name = cx.signal(String::new());
                column((
                    hstack(field(
                        "Username",
                        text_input(name).placeholder("crown"),
                        None,
                    ))
                    .w(520.0),
                    text(bind(move |runtime| format!("typed: {}", name.get(runtime)))),
                ))
            }),
            600.0,
            300.0,
        );
        app.settle();
        app
    }

    #[test]
    fn the_caption_names_the_field() {
        let mut app = form();
        assert!(app.exists(role(Role::TextField).name("Username")));
    }

    #[test]
    fn typing_reaches_the_bound_value() {
        let mut app = form();
        app.focus(role(Role::TextField).name("Username"));
        app.type_text("ada").settle();
        assert!(app.exists(text_query("typed: ada")));
    }

    #[test]
    fn focusing_the_field_brightens_its_rim() {
        assert!(rim(true).0 > rim(false).0);
        assert!(rim(true).1.a > rim(false).1.a);
        let mut app = form();
        app.focus(role(Role::TextField).name("Username"));
        app.advance_ms(16);
        assert!(app.is_animating(), "the rim eases brighter");
    }

    #[test]
    fn the_inputs_own_chrome_is_cleared() {
        let theme = chromeless(oobe_theme());
        assert_eq!(theme.popover.border, TRANSPARENT);
        assert_eq!(theme.popover.trigger_bg, TRANSPARENT);
        assert_eq!(theme.text, oobe_theme().text);
    }
}
