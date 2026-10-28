//! A captioned text field: the kit's input inside the design's pill, whose
//! border brightens while the field has focus.

use crownui::kit::{
    GradientStops, PopoverColors, TextInputKind, TextInputOptions, Theme, provide_theme, use_theme,
};
use crownui::prelude::{
    Color, Element, Listeners, Semantic, Spring, Styled, TRANSPARENT, View, WHITE, bind, component,
    text, vstack,
};

use crate::ui::palette::{BRAND, INK};

const CAPTION_SIZE: f32 = 14.0;
const CAPTION_GAP: f32 = 8.0;
const FIELD_HEIGHT: f32 = 47.0;
const FIELD_INSET: f32 = 10.0;
const BORDER: f32 = 2.0;
const ENTRY_SIZE: f32 = 16.0;
const RESTING_BORDER_ALPHA: f32 = 0.4;

/// `input` under `caption`, which also names it for screen readers.
pub fn field(caption: &'static str, input: Element<TextInputKind>) -> impl View {
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
        vstack((
            text(caption)
                .text_size(CAPTION_SIZE)
                .font_semibold()
                .text_color(INK),
            vstack(entry)
                .h(FIELD_HEIGHT)
                .px(FIELD_INSET)
                .justify_center()
                .rounded(FIELD_HEIGHT / 2.0)
                .bg(WHITE)
                .border(BORDER)
                .border_color(bind(move |runtime| border_color(focused.get(runtime))))
                .transition(Spring::snappy()),
        ))
        .gap(CAPTION_GAP)
        .w_full()
    })
}

fn border_color(focused: bool) -> Color {
    if focused {
        BRAND
    } else {
        BRAND.with_alpha_factor(RESTING_BORDER_ALPHA)
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
                    hstack(field("Username", text_input(name).placeholder("crown"))).w(410.0),
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
    fn focusing_the_field_brightens_its_border() {
        assert_eq!(border_color(true), BRAND);
        assert!(border_color(false).a < BRAND.a);
        let mut app = form();
        app.focus(role(Role::TextField).name("Username"));
        app.advance_ms(16);
        assert!(app.is_animating(), "the border eases to the brand colour");
    }

    #[test]
    fn the_inputs_own_chrome_is_cleared() {
        let theme = chromeless(oobe_theme());
        assert_eq!(theme.popover.border, TRANSPARENT);
        assert_eq!(theme.popover.trigger_bg, TRANSPARENT);
        assert_eq!(theme.text, oobe_theme().text);
    }
}
