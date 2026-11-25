//! One step of the install: its glyph, its name, and how it is going. The
//! row lights up while its step runs.

use crownui::ext::Kind;
use crownui::prelude::{
    Element, IntoProp, Memo, Role, Runtime, Semantic, Spring, Styled, bind, hstack, text, vstack,
};

use super::status_glyph::{Look, Status, status_glyph};
use crate::ui::palette::{FOREGROUND, GLASS, GLASS_LIT, HAIRLINE, HAIRLINE_LIT, SECONDARY};

const HEIGHT: f32 = 72.0;
const PADDING_LEFT: f32 = 20.0;
const PADDING_RIGHT: f32 = 36.0;
const GAP: f32 = 14.0;
const TITLE_SIZE: f32 = 16.0;
const TITLE_TRACKING: f32 = -0.16;
const CAPTION_SIZE: f32 = 13.0;
const LINE_GAP: f32 = 2.0;

/// `title`, with `caption` under it, for the step whose state `status` reads.
pub fn stage_row(
    title: impl Into<String>,
    caption: impl IntoProp<String>,
    status: Memo<Status>,
) -> Element<impl Kind> {
    let working = move |runtime: &mut Runtime| status.get(runtime) == Status::Working;
    let title = title.into();
    hstack((
        status_glyph(status, Look::Disc),
        vstack((
            text(title.clone())
                .text_size(TITLE_SIZE)
                .font_medium()
                .tracking(TITLE_TRACKING)
                .truncate()
                .text_color(bind(move |runtime| {
                    if matches!(status.get(runtime), Status::Waiting(_)) {
                        SECONDARY
                    } else {
                        FOREGROUND
                    }
                })),
            text(caption)
                .text_size(CAPTION_SIZE)
                .truncate()
                .text_color(SECONDARY),
        ))
        .gap(LINE_GAP)
        .flex_1()
        .min_w(0.0),
    ))
    .gap(GAP)
    .items_center()
    .h(HEIGHT)
    .pl(PADDING_LEFT)
    .pr(PADDING_RIGHT)
    .rounded(HEIGHT / 2.0)
    .bg(bind(
        move |runtime| if working(runtime) { GLASS_LIT } else { GLASS },
    ))
    .border(1.0)
    .border_color(bind(move |runtime| {
        if working(runtime) {
            HAIRLINE_LIT
        } else {
            HAIRLINE
        }
    }))
    .role(Role::ListItem)
    .label(title)
    .value(bind(move |runtime| status.get(runtime).spoken().to_owned()))
    .transition(Spring::smooth())
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::{button, provide_theme};
    use crownui::prelude::{Listeners, component};

    use super::*;
    use crate::ui::palette::oobe_theme;

    #[test]
    fn a_row_says_which_step_it_is_and_how_it_is_going() {
        let mut app = TestApp::new(component(|cx| {
            provide_theme(cx, oobe_theme());
            let current = cx.signal(Status::Waiting(9));
            let status = cx.memo(move |runtime| current.get(runtime));
            vstack((
                button("Start").on_click(move |cx, _| current.set(cx, Status::Working)),
                stage_row("Installing packages", "Waiting", status).w(500.0),
            ))
        }));
        app.settle();
        let row = app.find(role(Role::ListItem).name("Installing packages"));
        assert_eq!(row.value.as_deref(), Some("Waiting"));
        assert_eq!(row.bounds.size.height, HEIGHT);

        app.click(role(Role::Button).name("Start"));
        app.advance_ms(16);
        assert!(app.is_animating(), "the row lights up");
        let row = app.find(role(Role::ListItem).name("Installing packages"));
        assert_eq!(row.value.as_deref(), Some("In progress"));
    }
}
