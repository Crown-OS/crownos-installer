//! A radio choice drawn as a tile: blush with a filled mark when chosen,
//! quiet grey when not. The mark's dot springs in as it is picked.

use crownui::ext::{Key, NamedKey};
use crownui::prelude::{
    Color, CursorIcon, Cx, Listeners, Memo, Role, Semantic, Spring, Styled, View, WHITE, bind,
    hstack, rgb, rgba, text, vstack,
};

use crate::ui::palette::{BLUSH, BRAND, INK};

const HEIGHT: f32 = 52.0;
const RADIUS: f32 = 14.0;
const PADDING: f32 = 16.0;
const GAP: f32 = 12.0;
const LABEL_SIZE: f32 = 15.0;
const MARK_SIZE: f32 = 20.0;
const DOT_SIZE: f32 = 8.0;
const RESTING: Color = rgba(0xf3f3f3ee);
const RESTING_EDGE: Color = rgba(0x1b1b2a12);
const MARK_EDGE: Color = rgb(0xd9d6e8);

/// A tile reading `label`, chosen while `selected` holds; activating it runs
/// `choose`.
pub fn option_tile(
    label: &'static str,
    selected: Memo<bool>,
    choose: impl Fn(&mut Cx) + Copy + 'static,
) -> impl View + Styled {
    let pick = move |picked: Color, resting: Color| {
        bind(move |runtime| {
            if selected.get(runtime) {
                picked
            } else {
                resting
            }
        })
    };
    hstack((
        vstack(
            vstack(())
                .size(DOT_SIZE)
                .rounded(DOT_SIZE / 2.0)
                .bg(WHITE)
                .scale(bind(
                    move |runtime| if selected.get(runtime) { 1.0 } else { 0.0 },
                ))
                .transition(Spring::bouncy()),
        )
        .size(MARK_SIZE)
        .shrink(0.0)
        .items_center()
        .justify_center()
        .rounded(MARK_SIZE / 2.0)
        .bg(pick(BRAND, WHITE))
        .border(1.0)
        .border_color(pick(BRAND, MARK_EDGE)),
        text(label).text_size(LABEL_SIZE).text_color(INK).truncate(),
    ))
    .gap(GAP)
    .items_center()
    .h(HEIGHT)
    .px(PADDING)
    .flex_1()
    .rounded(RADIUS)
    .bg(pick(BLUSH, RESTING))
    .border(1.0)
    .border_color(pick(BRAND, RESTING_EDGE))
    .cursor(CursorIcon::Pointer)
    .role(Role::RadioButton)
    .label(label)
    .value(bind(move |runtime| {
        if selected.get(runtime) {
            "Selected"
        } else {
            ""
        }
        .to_owned()
    }))
    .focusable(true)
    .on_click(move |cx, _| choose(cx))
    .on_key_down(move |cx, event| {
        if matches!(
            event.key,
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
        ) {
            choose(cx);
        }
    })
    .transition(Spring::smooth())
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::prelude::component;

    use super::*;

    fn pair() -> TestApp {
        let mut app = TestApp::with_size(
            component(|cx| {
                let chosen = cx.signal(0usize);
                let first = cx.memo(move |runtime| chosen.get(runtime) == 0);
                let second = cx.memo(move |runtime| chosen.get(runtime) == 1);
                hstack((
                    option_tile("ext4", first, move |cx| chosen.set(cx, 0)),
                    option_tile("btrfs", second, move |cx| chosen.set(cx, 1)),
                ))
                .gap(12.0)
                .w(900.0)
            }),
            1000.0,
            200.0,
        );
        app.settle();
        app
    }

    fn selected(app: &mut TestApp, name: &str) -> bool {
        app.find(role(Role::RadioButton).name(name))
            .value
            .as_deref()
            == Some("Selected")
    }

    #[test]
    fn clicking_a_tile_chooses_it_and_releases_the_other() {
        let mut app = pair();
        assert!(selected(&mut app, "ext4"));
        app.click(role(Role::RadioButton).name("btrfs"));
        app.advance_ms(16);
        assert!(app.is_animating(), "the dot springs across");
        app.settle();
        assert!(selected(&mut app, "btrfs"));
        assert!(!selected(&mut app, "ext4"));
    }

    #[test]
    fn the_keyboard_can_choose_a_tile() {
        let mut app = pair();
        app.focus(role(Role::RadioButton).name("btrfs"));
        app.press_key("Space").settle();
        assert!(selected(&mut app, "btrfs"));
    }

    #[test]
    fn tiles_share_the_row_at_the_design_height() {
        let mut app = pair();
        let first = app.find(role(Role::RadioButton).name("ext4")).bounds;
        let second = app.find(role(Role::RadioButton).name("btrfs")).bounds;
        assert_eq!(first.size.height, HEIGHT);
        assert!((first.size.width - second.size.width).abs() < 0.5);
    }
}
