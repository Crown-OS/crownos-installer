//! One network in the Wi-Fi list: a Wi-Fi tile, its name and security, and a
//! trailing mark that morphs from a spinner into a check as it connects. The
//! chosen or connected network sits on its own glass.

use crownui::prelude::{
    Color, CursorIcon, Cx, Listeners, Memo, Role, Runtime, Semantic, Spring, Styled, TRANSPARENT,
    View, bind, hstack, icon, rgb, rgba, text, vstack,
};

use super::glyph;
use super::icon_tile::icon_tile;
use super::status_glyph::{Look, Status, status_glyph};
use crate::ui::palette::{FOREGROUND, GLASS};

const RADIUS: f32 = 33.0;
const PADDING_Y: f32 = 12.0;
const PADDING_LEFT: f32 = 13.0;
const PADDING_RIGHT: f32 = 24.0;
const GAP: f32 = 14.0;
const TILE: f32 = 40.0;
const TILE_GLYPH: f32 = 20.0;
const TITLE_SIZE: f32 = 17.0;
const DETAIL_SIZE: f32 = 14.0;
const LINE_GAP: f32 = 3.0;
const TRAILING: f32 = 18.0;
const LOCK: f32 = 16.0;
const UNUSABLE_OPACITY: f32 = 0.45;
const FOCUS_RING: f32 = 2.0;
const DETAIL: Color = rgb(0xc7c7cf);
const LOCK_COLOR: Color = rgba(0xffffffb3);
const LIT_RIM: Color = rgba(0xffffff1c);

/// What a row shows about one network.
#[derive(Clone, Debug, PartialEq)]
pub struct NetworkLine {
    pub ssid: String,
    pub detail: String,
    /// Whether joining needs a password.
    pub secured: bool,
    /// Whether the installer can join it at all.
    pub usable: bool,
}

/// A row for `line`, lit while `selected` holds or it is connected, trailing
/// whatever `link` says about joining it.
pub fn network_row(
    line: NetworkLine,
    selected: Memo<bool>,
    link: Memo<Status>,
    choose: impl Fn(&mut Cx) + 'static,
) -> impl View + Styled {
    let usable = line.usable;
    let secured = line.secured;
    let lit =
        move |runtime: &mut Runtime| selected.get(runtime) || link.get(runtime) == Status::Done;
    let waiting = move |runtime: &mut Runtime| matches!(link.get(runtime), Status::Waiting(_));
    let lock = icon(glyph::LOCK, LOCK_COLOR)
        .size(LOCK)
        .absolute()
        .opacity(bind(move |runtime| {
            if secured && waiting(runtime) {
                1.0
            } else {
                0.0
            }
        }));
    hstack((
        icon_tile(glyph::WIFI, TILE, TILE_GLYPH, lit),
        vstack((
            text(line.ssid.clone())
                .text_size(TITLE_SIZE)
                .font_medium()
                .tracking(-TITLE_SIZE / 100.0)
                .text_color(FOREGROUND)
                .truncate(),
            text(line.detail)
                .text_size(DETAIL_SIZE)
                .text_color(DETAIL)
                .truncate(),
        ))
        .gap(LINE_GAP)
        .flex_1()
        .min_w(0.0),
        vstack((status_glyph(link, Look::Bare), lock))
            .relative()
            .size(TRAILING)
            .shrink(0.0)
            .items_center()
            .justify_center(),
    ))
    .gap(GAP)
    .items_center()
    .py(PADDING_Y)
    .pl(PADDING_LEFT)
    .pr(PADDING_RIGHT)
    .w_full()
    .rounded(RADIUS)
    .border(1.0)
    .bg(bind(
        move |runtime| if lit(runtime) { GLASS } else { TRANSPARENT },
    ))
    .border_color(bind(
        move |runtime| {
            if lit(runtime) { LIT_RIM } else { TRANSPARENT }
        },
    ))
    .opacity(if usable { 1.0 } else { UNUSABLE_OPACITY })
    .cursor(CursorIcon::Pointer)
    .focus_visible(|style| style.ring(FOCUS_RING, FOREGROUND))
    .role(Role::Button)
    .label(line.ssid)
    .value(bind(move |runtime| {
        spoken(link.get(runtime), selected.get(runtime)).to_owned()
    }))
    .disabled(!usable)
    .focusable(usable)
    .on_click(move |cx, _| {
        if usable {
            choose(cx);
        }
    })
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
    use crownui::kit::provide_theme;
    use crownui::prelude::component;

    use super::*;
    use crate::ui::palette::oobe_theme;

    fn line(ssid: &str, usable: bool) -> NetworkLine {
        NetworkLine {
            ssid: ssid.to_owned(),
            detail: "WPA2 Security".to_owned(),
            secured: true,
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
        let height = app
            .find(role(Role::Button).name("Home Wi-Fi"))
            .bounds
            .size
            .height;
        assert!((height - (TILE + 2.0 * PADDING_Y)).abs() < 2.0, "{height}");
    }
}
