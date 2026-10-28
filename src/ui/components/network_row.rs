//! One network in the Wi-Fi list: its glyph, name and security, and a
//! trailing mark that morphs from a spinner into a check as it connects.

use crownui::kit::icon_colored;
use crownui::prelude::{
    Color, CursorIcon, Cx, Listeners, Memo, Role, Semantic, Spring, Styled, View, WHITE, bind,
    hstack, rgb, rgba, text, vstack,
};

use super::status_glyph::{Look, Status, status_glyph};
use crate::ui::palette::{ACCENT, BLUSH};

const HEIGHT: f32 = 58.0;
const RADIUS: f32 = 12.0;
const PADDING: f32 = 14.0;
const GAP: f32 = 10.0;
const ICON: f32 = 24.0;
const ICON_STROKE: f32 = 1.8;
const TITLE_SIZE: f32 = 14.0;
const DETAIL_SIZE: f32 = 12.0;
const LINE_GAP: f32 = 2.0;
const UNUSABLE_OPACITY: f32 = 0.45;
const PRIMARY: Color = rgb(0x17151f);
const MUTED: Color = rgb(0x8b8996);
const ICON_COLOR: Color = rgb(0x3d3b47);
const HAIRLINE: Color = rgba(0x1b1b2a12);

/// What a row shows about one network.
#[derive(Clone, Debug, PartialEq)]
pub struct NetworkLine {
    pub ssid: String,
    pub detail: String,
    pub glyph: &'static str,
    /// Whether the installer can join it at all.
    pub usable: bool,
}

/// A row for `line`, highlighted while `selected` holds, trailing whatever
/// `link` says about joining it.
pub fn network_row(
    line: NetworkLine,
    selected: Memo<bool>,
    link: Memo<Status>,
    choose: impl Fn(&mut Cx) + 'static,
) -> impl View + Styled {
    let usable = line.usable;
    hstack((
        icon_colored(line.glyph, ICON_STROKE, |_| ICON_COLOR)
            .size(ICON)
            .shrink(0.0),
        vstack((
            text(line.ssid.clone())
                .text_size(TITLE_SIZE)
                .text_color(PRIMARY)
                .truncate(),
            text(line.detail)
                .text_size(DETAIL_SIZE)
                .text_color(MUTED)
                .truncate(),
        ))
        .gap(LINE_GAP)
        .flex_1(),
        status_glyph(link, Look::Bare),
    ))
    .gap(GAP)
    .items_center()
    .h(HEIGHT)
    .px(PADDING)
    .w_full()
    .rounded(RADIUS)
    .bg(bind(
        move |runtime| if selected.get(runtime) { BLUSH } else { WHITE },
    ))
    .border(1.0)
    .border_color(bind(move |runtime| {
        if selected.get(runtime) {
            ACCENT.start
        } else {
            HAIRLINE
        }
    }))
    .opacity(if usable { 1.0 } else { UNUSABLE_OPACITY })
    .cursor(CursorIcon::Pointer)
    .role(Role::Button)
    .label(line.ssid)
    .value(bind(move |runtime| {
        spoken(link.get(runtime), selected.get(runtime)).to_owned()
    }))
    .disabled(!usable)
    .on_click(move |cx, _| {
        if usable {
            choose(cx);
        }
    })
    .hover(|style| style.scale(1.01))
    .transition(Spring::snappy())
}

/// What a screen reader hears for a row: how joining it is going, or
/// whether it is the one chosen.
fn spoken(link: Status, selected: bool) -> &'static str {
    match link {
        Status::Working => "Connecting",
        Status::Done => "Connected",
        Status::Failed => "Failed",
        Status::Waiting(_) if selected => "Selected",
        Status::Waiting(_) => "",
    }
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::kit::{icons, provide_theme};
    use crownui::prelude::component;

    use super::*;
    use crate::ui::palette::oobe_theme;

    fn line(ssid: &str, usable: bool) -> NetworkLine {
        NetworkLine {
            ssid: ssid.to_owned(),
            detail: "WPA2 Security".to_owned(),
            glyph: icons::WIFI,
            usable,
        }
    }

    fn list() -> TestApp {
        let mut app = TestApp::with_size(
            component(|cx| {
                provide_theme(cx, oobe_theme());
                let chosen = cx.signal(None::<usize>);
                let rows: Vec<_> = [line("Home Wi-Fi", true), line("Campus", false)]
                    .into_iter()
                    .enumerate()
                    .map(|(index, line)| {
                        let selected = cx.memo(move |runtime| chosen.get(runtime) == Some(index));
                        let link = cx.memo(move |runtime| {
                            if chosen.get(runtime) == Some(index) {
                                Status::Working
                            } else {
                                Status::Waiting(0)
                            }
                        });
                        network_row(line, selected, link, move |cx| chosen.set(cx, Some(index)))
                    })
                    .collect();
                vstack(rows).gap(12.0).w(432.0)
            }),
            600.0,
            300.0,
        );
        app.settle();
        app
    }

    #[test]
    fn choosing_a_network_selects_it_and_starts_connecting() {
        let mut app = list();
        app.click(role(Role::Button).name("Home Wi-Fi")).settle();
        let row = app.find(role(Role::Button).name("Home Wi-Fi"));
        assert_eq!(row.value.as_deref(), Some("Connecting"));
    }

    #[test]
    fn a_row_says_how_joining_it_is_going() {
        assert_eq!(spoken(Status::Waiting(0), false), "");
        assert_eq!(spoken(Status::Waiting(0), true), "Selected");
        assert_eq!(spoken(Status::Done, true), "Connected");
        assert_eq!(spoken(Status::Failed, true), "Failed");
    }

    #[test]
    fn a_network_the_installer_cannot_join_is_disabled() {
        let mut app = list();
        let row = app.find(role(Role::Button).name("Campus"));
        assert!(row.states.disabled);
        assert_eq!(row.value.as_deref(), Some(""));
    }

    #[test]
    fn rows_are_the_design_height() {
        let mut app = list();
        assert_eq!(
            app.find(role(Role::Button).name("Home Wi-Fi"))
                .bounds
                .size
                .height,
            HEIGHT
        );
    }
}
