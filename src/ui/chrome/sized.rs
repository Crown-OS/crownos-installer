//! A view built with a signal of its own laid-out size.

use crownui::ext::{Kind, NodeId};
use crownui::prelude::{Cx, Element, Signal, Size, View};

pub struct SizedKind<F> {
    build: F,
}

impl<F, V> Kind for SizedKind<F>
where
    F: FnOnce(Signal<Size>) -> V + 'static,
    V: View,
{
    const NAME: &'static str = "installer_sized";

    fn mount(self, cx: &mut Cx, node: NodeId) {
        let size = cx.observe_size(node);
        cx.set_children(node, (self.build)(size));
    }
}

/// What `build` makes from the size this element is laid out at.
pub fn sized<F, V>(build: F) -> Element<SizedKind<F>>
where
    F: FnOnce(Signal<Size>) -> V + 'static,
    V: View,
{
    Element::new(SizedKind { build })
}
