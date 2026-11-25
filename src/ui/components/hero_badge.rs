//! The badge over a page's heading when the install cannot go on: a cross on
//! red. It springs in once, inside a soft halo.

use crownui::kit::{icon_colored, icons};
use crownui::prelude::{Role, Semantic, Shadow, Spring, Styled, Vector, View, WHITE, vstack};

use crate::ui::palette::DANGER;

const DISC: f32 = 72.0;
const HALO: f32 = 96.0;
const MARK: f32 = 32.0;
const MARK_STROKE: f32 = 2.5;
const HALO_ALPHA: f32 = 0.12;
const SHADOW_ALPHA: f32 = 0.3;
const SHADOW_BLUR: f32 = 24.0;
const SHADOW_DROP: f32 = 8.0;

pub fn stopped_badge() -> impl View + Styled {
    vstack(
        vstack(
            icon_colored(icons::X, MARK_STROKE, |_| WHITE)
                .size(MARK)
                .enter(|style| style.rotate(-90.0).scale(0.2).opacity(0.0))
                .transition(Spring::bouncy()),
        )
        .size(DISC)
        .items_center()
        .justify_center()
        .rounded(DISC / 2.0)
        .bg(DANGER)
        .shadow(Shadow::new(
            DANGER.with_alpha_factor(SHADOW_ALPHA),
            SHADOW_BLUR,
            Vector::new(0.0, SHADOW_DROP),
        ))
        .enter(|style| style.scale(0.4).opacity(0.0))
        .transition(Spring::bouncy()),
    )
    .size(HALO)
    .shrink(0.0)
    .items_center()
    .justify_center()
    .rounded(HALO / 2.0)
    .bg(DANGER.with_alpha_factor(HALO_ALPHA))
    .enter(|style| style.scale(0.6).opacity(0.0))
    .transition(Spring::smooth())
    .role(Role::Image)
    .label("Stopped")
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::provide_theme;
    use crownui::prelude::component;

    use super::*;
    use crate::ui::palette::oobe_theme;

    #[test]
    fn the_badge_springs_in_once_and_then_rests() {
        let mut app = TestApp::new(component(|cx| {
            provide_theme(cx, oobe_theme());
            stopped_badge()
        }));
        assert!(app.is_animating());
        app.settle();
        assert!(!app.is_animating(), "no idle animation once it has landed");
        assert!(app.exists(role(Role::Image).name("Stopped")));
    }
}
