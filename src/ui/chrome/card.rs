//! The frosted card every page sits on: a translucent gradient slab with a
//! white-then-blush halo and a faint brand-coloured shadow, rising into place
//! when the window opens and sinking away when it closes.

use crownui::ext::DrawList;
use crownui::kit::GradientStops;
use crownui::prelude::{
    Color, CornerRadii, Memo, Rect, Runtime, Size, Spring, Styled, Vector, View, bind, canvas,
    rgba, vstack,
};

pub const CARD_WIDTH: f32 = 1163.0;
pub const CARD_HEIGHT: f32 = 660.0;
pub const CARD_PADDING_X: f32 = 59.0;
pub const CARD_PADDING_TOP: f32 = 55.0;
pub const CARD_PADDING_BOTTOM: f32 = 52.0;
const RADIUS: f32 = 30.0;

/// How far past the card the halo and shadow reach.
const HALO_REACH: f32 = 64.0;
const INNER_HALO: f32 = 12.0;
const OUTER_HALO: f32 = 22.0;
const RIM_WIDTH: f32 = 10.0;
const SHADOW_BLUR: f32 = 32.0;
const SHADOW_DROP: f32 = 12.0;

const INNER_HALO_COLOR: Color = rgba(0xffffff8c);
const OUTER_HALO_COLOR: Color = rgba(0xfcd6dd52);
const SHADOW_COLOR: Color = rgba(0xef305412);
const RIM_COLOR: Color = rgba(0xb0b0b00a);
const SLAB: GradientStops = GradientStops::new(rgba(0xfffffff0), rgba(0xfef0f2e6));

const RESTING_SCALE: f32 = 1.0;
const AWAY_SCALE: f32 = 0.96;
const AWAY_LIFT: f32 = 18.0;

/// `content` on the card. While `closing` holds the card fades and shrinks
/// away, so a restart reads as the installer stepping aside.
pub fn card(content: impl View, closing: Memo<bool>) -> impl View + Styled {
    vstack((
        canvas(paint_halo)
            .absolute()
            .inset(-HALO_REACH)
            .pointer_events_none(),
        vstack(content).flex_1().w_full(),
    ))
    .relative()
    .w_full()
    .h_full()
    .max_w(CARD_WIDTH)
    .max_h(CARD_HEIGHT)
    .pl(CARD_PADDING_X)
    .pr(CARD_PADDING_X)
    .pt(CARD_PADDING_TOP)
    .pb(CARD_PADDING_BOTTOM)
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

fn paint_halo(_: &mut Runtime, bounds: Size, list: &mut DrawList) {
    let slab = Rect::from_xywh(
        HALO_REACH,
        HALO_REACH,
        bounds.width - 2.0 * HALO_REACH,
        bounds.height - 2.0 * HALO_REACH,
    );
    let grown = |by: f32| {
        Rect::from_xywh(
            slab.origin.x - by,
            slab.origin.y - by,
            slab.size.width + 2.0 * by,
            slab.size.height + 2.0 * by,
        )
    };
    let corners = |by: f32| CornerRadii::uniform(RADIUS + by);
    list.fill(grown(INNER_HALO), corners(INNER_HALO), INNER_HALO_COLOR);
    list.fill(grown(OUTER_HALO), corners(OUTER_HALO), OUTER_HALO_COLOR);
    list.shadow(
        slab,
        corners(0.0),
        SHADOW_COLOR,
        SHADOW_BLUR,
        Vector::new(0.0, SHADOW_DROP),
    );
    list.fill(
        slab,
        corners(0.0),
        SLAB.horizontal(slab.min_x(), slab.max_x()),
    );
    let rim = RIM_WIDTH / 2.0;
    list.stroke(grown(rim), corners(rim), RIM_WIDTH, RIM_COLOR);
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::prelude::{Semantic, component, text};

    use super::*;

    fn carded() -> TestApp {
        TestApp::with_size(
            component(|cx| {
                let closing = cx.memo(|_| false);
                card(text("inside").role(Role::Heading), closing)
            }),
            1400.0,
            900.0,
        )
    }

    #[test]
    fn the_card_never_outgrows_the_design() {
        let mut app = carded();
        app.settle();
        let inside = app.find(role(Role::Heading)).bounds;
        assert!((inside.origin.x - CARD_PADDING_X).abs() < 0.5, "{inside:?}");
        assert!(
            (inside.origin.y - CARD_PADDING_TOP).abs() < 0.5,
            "{inside:?}"
        );
        assert!(inside.size.width <= CARD_WIDTH - 2.0 * CARD_PADDING_X + 0.5);
    }

    #[test]
    fn the_card_rises_in_when_the_window_opens() {
        let mut app = carded();
        assert!(app.is_animating(), "the card rises in");
        app.settle();
        assert!(!app.is_animating());
    }
}
