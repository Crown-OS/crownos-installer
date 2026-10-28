//! A thin bar that fills smoothly towards a fraction.

use crownui::prelude::{Color, Memo, Role, Semantic, Spring, Styled, View, bind, rgb, vstack};

use crate::ui::palette::BRAND;

const HEIGHT: f32 = 8.0;
const TRACK: Color = rgb(0xf2f2f6);

/// A bar `fraction` full, spoken as a percentage and named `name`.
pub fn meter(name: &'static str, fraction: Memo<f32>) -> impl View + Styled {
    vstack(
        vstack(())
            .absolute()
            .inset(0.0)
            .rounded(HEIGHT / 2.0)
            .bg(BRAND)
            .origin(0.0, 0.5)
            .scale_x(bind(move |runtime| fraction.get(runtime).clamp(0.0, 1.0)))
            .transition(Spring::smooth()),
    )
    .relative()
    .h(HEIGHT)
    .w_full()
    .rounded(HEIGHT / 2.0)
    .overflow_hidden()
    .bg(TRACK)
    .role(Role::ProgressBar)
    .label(name)
    .value(bind(move |runtime| percent(fraction.get(runtime))))
}

/// `fraction` as the whole percentage a person would say.
pub fn percent(fraction: f32) -> String {
    format!("{}%", (fraction.clamp(0.0, 1.0) * 100.0).round() as u32)
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::button;
    use crownui::prelude::{Listeners, component};

    use super::*;

    #[test]
    fn a_fraction_reads_as_a_rounded_percentage() {
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(0.414), "41%");
        assert_eq!(percent(1.7), "100%");
        assert_eq!(percent(-1.0), "0%");
    }

    #[test]
    fn the_bar_follows_its_fraction_smoothly() {
        let mut app = TestApp::new(component(|cx| {
            let done = cx.signal(0.25f32);
            let fraction = cx.memo(move |runtime| done.get(runtime));
            vstack((
                button("More").on_click(move |cx, _| done.set(cx, 0.75)),
                meter("Overall progress", fraction).w(400.0),
            ))
        }));
        app.settle();
        let bar = app.find(role(Role::ProgressBar).name("Overall progress"));
        assert_eq!(bar.value.as_deref(), Some("25%"));
        app.click(role(Role::Button).name("More"));
        app.advance_ms(16);
        assert!(app.is_animating(), "the fill eases forward");
        app.settle();
        let bar = app.find(role(Role::ProgressBar).name("Overall progress"));
        assert_eq!(bar.value.as_deref(), Some("75%"));
    }
}
