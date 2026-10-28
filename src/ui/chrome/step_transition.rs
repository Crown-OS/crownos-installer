//! Moving between steps: the outgoing page drifts away and fades while the
//! incoming one drifts in from the side the user is heading towards, so going
//! forward and going back feel different.
//!
//! The technique is the kit's `slide_transition`: when a new scene mounts, the
//! one it replaces is restyled to where it should end up, and its `exit` keeps
//! it on screen until the spring gets it there.

use crownui::ext::{Kind, NodeId, StyleProp};
use crownui::prelude::{
    Cx, Element, Role, Runtime, Semantic, Signal, Spring, Store, Styled, Vector, View, for_each,
};

/// How far a page drifts while it fades.
pub const DRIFT: f32 = 40.0;

#[derive(Clone, PartialEq)]
struct Scene<T> {
    serial: u64,
    key: T,
}

struct SceneKind<V> {
    content: V,
    latest: Store<Option<NodeId>>,
}

impl<V: View> Kind for SceneKind<V> {
    const NAME: &'static str = "step_scene";

    fn mount(self, cx: &mut Cx, node: NodeId) {
        cx.set_children(node, self.content);
        self.latest.update(cx, |latest| latest.replace(node));
    }
}

/// The kind behind [`step_transition`].
pub struct StepTransitionKind<T, C, B> {
    current: C,
    ordinal: fn(&T) -> usize,
    build: B,
}

impl<T, C, B, V> Kind for StepTransitionKind<T, C, B>
where
    T: Clone + PartialEq + 'static,
    C: FnMut(&mut Runtime) -> T + 'static,
    B: FnMut(&mut Cx, T) -> V + 'static,
    V: View,
{
    const NAME: &'static str = "step_transition";

    fn mount(self, cx: &mut Cx, node: NodeId) {
        let Self {
            current,
            ordinal,
            mut build,
        } = self;
        let current = cx.memo(current);
        let first = cx.untrack(|runtime| current.get(runtime));
        let mut previous_ordinal = ordinal(&first);
        let scenes = cx.signal(vec![Scene {
            serial: 0,
            key: first,
        }]);
        cx.effect(move |runtime| {
            let key = current.get(runtime);
            scenes.update(runtime, |scenes| {
                if let Some(scene) = scenes.first_mut().filter(|scene| scene.key != key) {
                    *scene = Scene {
                        serial: scene.serial + 1,
                        key,
                    };
                }
            });
        });
        let latest = cx.store(None);
        let stage = for_each(
            scenes,
            |scene: &Scene<T>| scene.serial,
            move |cx, scene: Signal<Scene<T>>| {
                let Scene { serial, key } = scene.with_untracked(cx, Clone::clone);
                let heading = ordinal(&key);
                let direction = if heading >= previous_ordinal {
                    1.0
                } else {
                    -1.0
                };
                previous_ordinal = heading;
                if let Some(leaving) = latest.update(cx, |latest| latest.take()) {
                    cx.set_default_style(
                        leaving,
                        StyleProp::Translate(Vector::new(-direction * DRIFT, 0.0)),
                    );
                    cx.set_default_style(leaving, StyleProp::Opacity(0.0));
                }
                let incoming = Element::new(SceneKind {
                    content: build(cx, key),
                    latest,
                })
                .flex_1()
                .w_full()
                .exit(|style| style);
                if serial == 0 {
                    incoming
                } else {
                    incoming.enter(|style| style.translate_x(direction * DRIFT).opacity(0.0))
                }
            },
        )
        .role(Role::Group)
        .flex_1()
        .w_full();
        cx.set_children(node, stage);
    }
}

/// Shows `build(cx, key)` for the key `current` reads, rebuilding only when the
/// key changes. A key with a larger `ordinal` than the last is a step forward.
pub fn step_transition<T, C, B, V>(
    current: C,
    ordinal: fn(&T) -> usize,
    build: B,
) -> Element<StepTransitionKind<T, C, B>>
where
    T: Clone + PartialEq + 'static,
    C: FnMut(&mut Runtime) -> T + 'static,
    B: FnMut(&mut Cx, T) -> V + 'static,
    V: View,
{
    Element::new(StepTransitionKind {
        current,
        ordinal,
        build,
    })
    .flex_1()
    .w_full()
    .transition(Spring::smooth())
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role, text};
    use crownui::kit::button;
    use crownui::prelude::{Listeners, component, text as label, vstack};

    use super::*;

    fn steps() -> TestApp {
        TestApp::new(component(|cx| {
            let index = cx.signal(5usize);
            vstack((
                button("Next").on_click(move |cx, _| index.update(cx, |index| *index += 1)),
                button("Back").on_click(move |cx, _| index.update(cx, |index| *index -= 1)),
                step_transition(
                    move |runtime| index.get(runtime),
                    |index| *index,
                    |_, index| label(format!("Page {index}")),
                ),
            ))
        }))
    }

    fn indent(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    /// Where `label` is painted horizontally, summing the layers above it.
    fn offset(app: &TestApp, label: &str) -> Option<f32> {
        let frame = app.frame_snapshot();
        let lines: Vec<&str> = frame.lines().collect();
        let target = format!("text \"{label}\"");
        let position = lines
            .iter()
            .position(|line| line.trim_start().starts_with(&target))?;
        let mut limit = indent(lines[position]);
        let mut offset = 0.0;
        for line in lines[..position].iter().rev() {
            if indent(line) >= limit {
                continue;
            }
            limit = indent(line);
            if let Some(origin) = line.trim_start().strip_prefix("layer at ") {
                offset += origin
                    .split(',')
                    .next()
                    .and_then(|x| x.parse::<f32>().ok())?;
            }
        }
        Some(offset)
    }

    fn press(app: &mut TestApp, name: &str) {
        app.settle();
        app.click(role(Role::Button).name(name));
    }

    #[test]
    fn the_first_page_is_simply_there() {
        let app = steps();
        assert!(!app.is_animating());
        assert_eq!(offset(&app, "Page 5"), Some(0.0));
    }

    #[test]
    fn going_forward_the_new_page_drifts_in_from_the_right() {
        let mut app = steps();
        press(&mut app, "Next");
        app.advance_ms(16);
        let incoming = offset(&app, "Page 6").expect("the new page paints as it fades in");
        assert!(
            (0.0..=DRIFT).contains(&incoming) && incoming > 0.0,
            "{incoming}"
        );
    }

    #[test]
    fn going_back_the_new_page_drifts_in_from_the_left() {
        let mut app = steps();
        press(&mut app, "Back");
        app.advance_ms(16);
        let incoming = offset(&app, "Page 4").expect("the new page paints as it fades in");
        assert!((-DRIFT..0.0).contains(&incoming), "{incoming}");
    }

    #[test]
    fn the_old_page_drifts_the_other_way_while_both_are_painted() {
        let mut app = steps();
        press(&mut app, "Next");
        app.advance_ms(80);
        let (Some(incoming), Some(outgoing)) = (offset(&app, "Page 6"), offset(&app, "Page 5"))
        else {
            panic!("both pages paint mid-transition:\n{}", app.frame_snapshot());
        };
        assert!(incoming > 0.0 && incoming < DRIFT, "{incoming}");
        assert!(outgoing < 0.0 && outgoing > -DRIFT, "{outgoing}");
    }

    #[test]
    fn the_transition_settles_with_the_old_page_retired() {
        let mut app = steps();
        press(&mut app, "Next");
        app.settle();
        app.advance_ms(16);
        assert!(!app.is_animating());
        assert_eq!(offset(&app, "Page 6"), Some(0.0));
        assert_eq!(offset(&app, "Page 5"), None);
    }

    #[test]
    fn only_the_current_page_is_in_the_accessibility_tree() {
        let mut app = steps();
        press(&mut app, "Next");
        assert!(app.exists(text("Page 6")));
        assert!(!app.exists(text("Page 5")));
    }
}
