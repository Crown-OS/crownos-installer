//! One step of the install: its glyph, its name, and how it is going. The
//! row lights up while its step runs.

use crownui::ext::Kind;
use crownui::prelude::{
    Color, Element, IntoProp, Memo, Role, Runtime, Semantic, Spring, Styled, WHITE, bind, hstack,
    rgb, rgba, text, vstack,
};

use super::status_glyph::{Look, Status, status_glyph};
use crate::ui::palette::{ACCENT, BRAND, INK};

const HEIGHT: f32 = 60.0;
const RADIUS: f32 = 14.0;
const PADDING: f32 = 14.0;
const GAP: f32 = 12.0;
const TITLE_SIZE: f32 = 14.0;
const CAPTION_SIZE: f32 = 13.0;
const LINE_GAP: f32 = 3.0;
const HAIRLINE: Color = rgba(0x1b1b2a12);
const MUTED: Color = rgb(0x8b8996);
const WORKING_WASH: Color = rgb(0xfdf0f5);

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
                .font_semibold()
                .truncate()
                .text_color(bind(move |runtime| {
                    if matches!(status.get(runtime), Status::Waiting(_)) {
                        MUTED
                    } else {
                        INK
                    }
                })),
            text(caption)
                .text_size(CAPTION_SIZE)
                .truncate()
                .text_color(bind(
                    move |runtime| {
                        if working(runtime) { BRAND } else { MUTED }
                    },
                )),
        ))
        .gap(LINE_GAP)
        .flex_1(),
    ))
    .gap(GAP)
    .items_center()
    .h(HEIGHT)
    .px(PADDING)
    .rounded(RADIUS)
    .bg(bind(move |runtime| {
        if working(runtime) {
            WORKING_WASH
        } else {
            WHITE
        }
    }))
    .border(1.0)
    .border_color(bind(move |runtime| {
        if working(runtime) {
            ACCENT.start
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
