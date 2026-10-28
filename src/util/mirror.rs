//! Two-way binding between one value inside a model signal and a signal of its
//! own, for widgets that own the signal they edit.

use crownui::prelude::{Cx, Signal};

/// A signal holding `read(model)`, kept equal to it in both directions: a
/// change the widget makes is written back through `write`, and a change made
/// to the model elsewhere is copied in. When both move in one flush the widget
/// wins, since it is what the user just touched.
pub fn mirror<M, T>(
    cx: &mut Cx,
    model: Signal<M>,
    read: impl Fn(&M) -> T + 'static,
    write: impl Fn(&mut M, T) + 'static,
) -> Signal<T>
where
    M: 'static,
    T: Clone + PartialEq + 'static,
{
    let initial = model.with_untracked(cx, &read);
    let local = cx.signal(initial.clone());
    let mut last = initial;
    cx.effect(move |runtime| {
        let from_model = model.with(runtime, &read);
        let from_widget = local.get(runtime);
        if from_widget != last {
            last = from_widget.clone();
            model.update(runtime, |model| write(model, from_widget));
        } else if from_model != last {
            last = from_model.clone();
            local.set(runtime, from_model);
        }
    });
    local
}
