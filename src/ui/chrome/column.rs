//! The column every page is laid out in: as wide as the design's content,
//! centred, with the design's room above and below it. It rises into place
//! when the window opens and sinks away when it closes.

use crownui::prelude::{Memo, Signal, Size, Spring, Styled, View, bind, vstack};

use crate::ui::{Density, Measures};

/// The design's content width.
const COLUMN_WIDTH: f32 = 1120.0;
const RESTING_SCALE: f32 = 1.0;
const AWAY_SCALE: f32 = 0.96;
const AWAY_LIFT: f32 = 18.0;

/// `content` in the column, in a window `window` big, set at `density`.
/// While `closing` holds the column fades and shrinks away, so a restart
/// reads as the installer stepping aside.
pub fn column(
    content: impl View,
    window: Signal<Size>,
    density: Memo<Density>,
    closing: Memo<bool>,
) -> impl View + Styled {
    let room = move |share: fn(&Measures) -> f32| {
        bind(move |runtime| window.get(runtime).height * share(&density.get(runtime).measures()))
    };
    vstack((
        vstack(())
            .h(room(|measures| measures.top_share))
            .shrink(0.0),
        vstack(content).flex_1().min_h(0.0).w_full(),
        vstack(())
            .h(room(|measures| measures.bottom_share))
            .shrink(0.0),
    ))
    .w_full()
    .h_full()
    .max_w(COLUMN_WIDTH)
    .opacity(bind(
        move |runtime| if closing.get(runtime) { 0.0 } else { 1.0 },
    ))
    .scale(bind(move |runtime| {
        if closing.get(runtime) {
            AWAY_SCALE
        } else {
            RESTING_SCALE
        }
    }))
    .enter(|style| style.opacity(0.0).scale(AWAY_SCALE).translate_y(AWAY_LIFT))
    .transition(Spring::smooth())
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::prelude::{Semantic, component, text};

    use super::*;

    fn framed(width: f32, height: f32) -> TestApp {
        let mut app = TestApp::with_size(
            component(move |cx| {
                let closing = cx.memo(|_| false);
                let window = cx.signal(Size::new(width, height));
                let density =
                    cx.memo(move |runtime| Density::for_height(window.get(runtime).height));
                vstack(column(
                    text("inside").role(Role::Heading),
                    window,
                    density,
                    closing,
                ))
                .w_full()
                .h_full()
                .items_center()
            }),
            width,
            height,
        );
        app.settle();
        app
    }

    #[test]
    fn the_column_keeps_the_designs_room_above_and_width() {
        let mut app = framed(1728.0, 1117.0);
        let inside = app.find(role(Role::Heading)).bounds;
        assert!((inside.origin.y - 198.0).abs() < 0.5, "{inside:?}");
        assert!((inside.origin.x - 304.0).abs() < 0.5, "{inside:?}");
    }

    #[test]
    fn a_short_window_keeps_less_room_above() {
        let mut app = framed(1366.0, 768.0);
        let inside = app.find(role(Role::Heading)).bounds;
        let compact = 768.0 * Density::Compact.measures().top_share;
        assert!((inside.origin.y - compact).abs() < 0.5, "{inside:?}");
    }

    #[test]
    fn the_column_rises_in_when_the_window_opens() {
        let mut app = TestApp::new(component(|cx| {
            let closing = cx.memo(|_| false);
            let window = cx.signal(Size::new(1728.0, 1117.0));
            let density = cx.memo(|_| Density::Roomy);
            column(text("inside"), window, density, closing)
        }));
        assert!(app.is_animating(), "the column rises in");
        app.settle();
        assert!(!app.is_animating());
    }
}
