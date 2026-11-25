//! The glass panel that groups content on a page.

use crownui::prelude::{Styled, View, ViewSeq, vstack};

use crate::ui::palette::{GLASS, HAIRLINE};

const PADDING: f32 = 24.0;
const RADIUS: f32 = 32.0;
const GAP: f32 = 14.0;

/// `content` stacked on a glass panel the width of the column.
pub fn surface(content: impl ViewSeq) -> impl View + Styled {
    vstack(content)
        .gap(GAP)
        .p(PADDING)
        .w_full()
        .rounded(RADIUS)
        .bg(GLASS)
        .border(1.0)
        .border_color(HAIRLINE)
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::prelude::{Semantic, component, text};

    use super::*;

    #[test]
    fn a_surface_pads_its_content_and_spans_the_column() {
        let mut app = TestApp::with_size(
            component(|_| vstack(surface(text("inside").role(Role::Heading))).w(500.0)),
            600.0,
            200.0,
        );
        app.settle();
        let inside = app.find(role(Role::Heading)).bounds;
        assert_eq!(inside.origin.x, PADDING);
        assert_eq!(inside.origin.y, PADDING);
    }
}
