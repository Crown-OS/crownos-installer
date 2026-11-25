//! A wrapper that tells assistive technology whether the choice inside it is
//! checked.

use crownui::ext::{Kind, NodeId};
use crownui::prelude::{Cx, Element, Memo, View};

pub struct CheckedKind<V> {
    content: V,
    chosen: Memo<bool>,
}

impl<V: View> Kind for CheckedKind<V> {
    const NAME: &'static str = "installer_checked";

    fn mount(self, cx: &mut Cx, node: NodeId) {
        let chosen = self.chosen;
        cx.set_children(node, self.content);
        cx.bind_checked(node, move |runtime| chosen.get(runtime));
    }
}

/// `content`, reported as checked while `chosen` holds.
pub fn checked<V: View>(content: V, chosen: Memo<bool>) -> Element<CheckedKind<V>> {
    Element::new(CheckedKind { content, chosen })
}
