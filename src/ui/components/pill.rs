//! The footer's buttons: the kit button at the design's 50px pill size,
//! sinking slightly under the pointer.

use crownui::kit::{ButtonKind, ButtonOptions, ButtonVariant, button};
use crownui::prelude::{Element, IntoProp, Spring, Styled};

pub const PILL_HEIGHT: f32 = 50.0;
const PILL_PADDING: f32 = 24.0;
const PILL_MIN_WIDTH: f32 = 127.0;
const LABEL_SIZE: f32 = 14.0;
const PRESSED_SCALE: f32 = 0.96;

/// A pill-shaped button. The kit draws its fill at its own corner radius, so
/// the pill clips it to the round ends.
pub fn pill(label: impl IntoProp<String>, variant: ButtonVariant) -> Element<ButtonKind> {
    button(label)
        .variant(variant)
        .min_width(PILL_MIN_WIDTH)
        .h(PILL_HEIGHT)
        .rounded(PILL_HEIGHT / 2.0)
        .px(PILL_PADDING)
        .text_size(LABEL_SIZE)
        .overflow_hidden()
        .active(|style| style.scale(PRESSED_SCALE))
        .transition(Spring::snappy())
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::{ButtonVariant, provide_theme};
    use crownui::prelude::{Listeners, Semantic, bind, component, text, vstack};

    use super::*;
    use crate::ui::palette::oobe_theme;

    fn pills() -> TestApp {
        let mut app = TestApp::with_size(
            component(|cx| {
                provide_theme(cx, oobe_theme());
                let presses = cx.signal(0u32);
                vstack((
                    pill("Next", ButtonVariant::Primary)
                        .on_click(move |cx, _| presses.update(cx, |count| *count += 1)),
                    pill("Back", ButtonVariant::Secondary).disabled(true),
                    text(bind(move |runtime| {
                        format!("{} presses", presses.get(runtime))
                    }))
                    .role(Role::Text),
                ))
                .items_start()
            }),
            400.0,
            300.0,
        );
        app.settle();
        app
    }

    #[test]
    fn a_pill_is_the_design_height_and_never_narrower_than_the_next_button() {
        let mut app = pills();
        let bounds = app.find(role(Role::Button).name("Next")).bounds;
        assert_eq!(bounds.size.height, PILL_HEIGHT);
        assert!(bounds.size.width >= PILL_MIN_WIDTH);
    }

    #[test]
    fn clicking_a_pill_runs_its_action_and_a_disabled_one_says_so() {
        let mut app = pills();
        app.click(role(Role::Button).name("Next")).settle();
        assert!(app.exists(crowntest::text("1 presses")));
        assert!(app.find(role(Role::Button).name("Back")).states.disabled);
    }

    #[test]
    fn pressing_a_pill_sinks_it() {
        let mut app = pills();
        let center = app.find(role(Role::Button).name("Next")).bounds.center();
        app.pointer_down(center.x, center.y);
        app.advance_ms(16);
        assert!(app.is_animating(), "the pill scales down under the pointer");
        app.pointer_up(center.x, center.y).settle();
    }
}
