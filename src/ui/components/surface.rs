//! The white, hairlined panel that groups content on the card.

use crownui::prelude::{Styled, View, ViewSeq, WHITE, vstack};

use crate::ui::palette::OUTLINE;

const PADDING: f32 = 20.0;
const RADIUS: f32 = 16.0;
const GAP: f32 = 14.0;

/// `content` stacked on a white panel the width of the column.
pub fn surface(content: impl ViewSeq) -> impl View + Styled {
    vstack(content)
        .gap(GAP)
        .p(PADDING)
        .w_full()
        .rounded(RADIUS)
        .bg(WHITE)
        .border(1.0)
        .border_color(OUTLINE)
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
