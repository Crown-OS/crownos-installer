//! One choice as a glass pill: something to recognise it by, its name and a
//! line about it, and a mark at the end. The chosen one is lit.

use crownui::ext::Kind;
use crownui::prelude::{
    Color, CursorIcon, Cx, Element, Listeners, Memo, Role, Semantic, Spring, Styled, View, WHITE,
    bind, hstack, rgb, rgba, text, vstack,
};

use super::checked::checked;
use super::mark::{Mark, mark};
use crate::ui::palette::{FOREGROUND, GLASS, GLASS_LIT, HAIRLINE, HAIRLINE_LIT, SECONDARY};

const TAG_SIZE: f32 = 11.0;
const TAG_TRACKING: f32 = 0.88;
const TAG_GAP: f32 = 10.0;
const DETAIL_SIZE: f32 = 13.0;
const USAGE_WIDTH: f32 = 300.0;
const USAGE_HEIGHT: f32 = 4.0;
const USAGE_TRACK: Color = rgba(0xffffff1a);
const USAGE_RESTING: Color = rgba(0xffffff66);
const PICKED_FILL: Color = rgb(0x141414);
const UNUSABLE_OPACITY: f32 = 0.45;
const RIM: f32 = 1.0;
const FOCUS_RING: f32 = 2.0;
const HOVER_SCALE: f32 = 1.01;

/// What a row says about its choice.
#[derive(Clone, Debug, PartialEq)]
pub struct OptionLine {
    pub title: String,
    /// A small capitalised tag after the title, such as a device path.
    pub tag: Option<String>,
    pub detail: String,
    /// How full the choice is, as a bar under the title.
    pub usage: Option<f32>,
    /// Whether it can be chosen at all.
    pub usable: bool,
}

impl OptionLine {
    pub fn new(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            tag: None,
            detail: detail.into(),
            usage: None,
            usable: true,
        }
    }
}

/// The two sizes the design draws choices at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowSize {
    /// Disks and other places to install to.
    Large,
    /// Apps and tools, four to a line.
    Compact,
}

struct Metrics {
    height: f32,
    padding_y: f32,
    padding_left: f32,
    padding_right: f32,
    gap: f32,
    title_size: f32,
    line_gap: f32,
    chosen_fill: Color,
    chosen_rim: Color,
}

impl RowSize {
    const fn metrics(self) -> Metrics {
        match self {
            Self::Large => Metrics {
                height: 96.0,
                padding_y: 18.0,
                padding_left: 24.0,
                padding_right: 37.0,
                gap: 16.0,
                title_size: 17.0,
                line_gap: 6.0,
                chosen_fill: GLASS_LIT,
                chosen_rim: HAIRLINE_LIT,
            },
            Self::Compact => Metrics {
                height: 80.0,
                padding_y: 15.0,
                padding_left: 15.0,
                padding_right: 28.0,
                gap: 12.0,
                title_size: 16.0,
                line_gap: 2.0,
                chosen_fill: PICKED_FILL,
                chosen_rim: WHITE,
            },
        }
    }
}

/// A row for `line`, led by `leading`, lit while `chosen` holds, running
/// `choose` when clicked.
pub fn option_row(
    line: OptionLine,
    size: RowSize,
    leading: impl View,
    end: Mark,
    chosen: Memo<bool>,
    choose: impl Fn(&mut Cx) + 'static,
) -> Element<impl Kind> {
    let metrics = size.metrics();
    let usable = line.usable;
    let title_row = hstack((
        text(line.title.clone())
            .text_size(metrics.title_size)
            .font_medium()
            .tracking(-metrics.title_size / 100.0)
            .text_color(FOREGROUND)
            .truncate(),
        line.tag.map(|tag| {
            text(tag.to_uppercase())
                .text_size(TAG_SIZE)
                .font_medium()
                .tracking(TAG_TRACKING)
                .text_color(SECONDARY)
                .shrink(0.0)
        }),
    ))
    .gap(TAG_GAP)
    .items_center();
    let lines = vstack((
        title_row,
        line.usage.map(|fraction| usage_bar(fraction, chosen)),
        text(line.detail)
            .text_size(DETAIL_SIZE)
            .text_color(SECONDARY),
    ))
    .gap(metrics.line_gap)
    .flex_1()
    .min_w(0.0);
    let role = match end {
        Mark::Radio => Role::RadioButton,
        Mark::Checkbox => Role::Checkbox,
    };
    checked(
        hstack((leading, lines, mark(end, chosen)))
            .gap(metrics.gap)
            .items_center()
            .w_full(),
        chosen,
    )
    .justify_center()
    .min_h(metrics.height)
    .py(metrics.padding_y)
    .w_full()
    .min_w(0.0)
    .pl(metrics.padding_left)
    .pr(metrics.padding_right)
    .rounded(metrics.height / 2.0)
    .bg(bind(move |runtime| {
        if chosen.get(runtime) {
            metrics.chosen_fill
        } else {
            GLASS
        }
    }))
    .border(RIM)
    .border_color(bind(move |runtime| {
        if chosen.get(runtime) {
            metrics.chosen_rim
        } else {
            HAIRLINE
        }
    }))
    .opacity(if usable { 1.0 } else { UNUSABLE_OPACITY })
    .role(role)
    .label(line.title)
    .disabled(!usable)
    .focusable(usable)
    .cursor(CursorIcon::Pointer)
    .focus_visible(|style| style.ring(FOCUS_RING, FOREGROUND))
    .on_click(move |cx, _| choose(cx))
    .hover(|style| style.scale(HOVER_SCALE))
    .transition(Spring::snappy())
}

fn usage_bar(fraction: f32, chosen: Memo<bool>) -> impl View {
    let radius = USAGE_HEIGHT / 2.0;
    vstack(
        vstack(())
            .absolute()
            .left(0.0)
            .top(0.0)
            .h(USAGE_HEIGHT)
            .w(USAGE_WIDTH * fraction.clamp(0.0, 1.0))
            .rounded(radius)
            .bg(bind(move |runtime| {
                if chosen.get(runtime) {
                    WHITE
                } else {
                    USAGE_RESTING
                }
            })),
    )
    .relative()
    .h(USAGE_HEIGHT)
    .w_full()
    .max_w(USAGE_WIDTH)
    .rounded(radius)
    .overflow_hidden()
    .bg(USAGE_TRACK)
}

#[cfg(test)]
mod tests {
    use crowntest::{Role, TestApp, role};
    use crownui::prelude::component;

    use super::*;
    use crate::ui::layout::nothing;

    fn rows() -> TestApp {
        let mut app = TestApp::with_size(
            component(|cx| {
                let picked = cx.signal(None::<usize>);
                let lines = [
                    OptionLine::new("/dev/nvme0n1", "1 TB · 512 GB free"),
                    OptionLine {
                        usable: false,
                        ..OptionLine::new("/dev/sda", "In use by this installer")
                    },
                ];
                let rows: Vec<_> = lines
                    .into_iter()
                    .enumerate()
                    .map(|(index, line)| {
                        let chosen = cx.memo(move |runtime| picked.get(runtime) == Some(index));
                        option_row(
                            line,
                            RowSize::Large,
                            nothing(),
                            Mark::Radio,
                            chosen,
                            move |cx| {
                                picked.set(cx, Some(index));
                            },
                        )
                    })
                    .collect();
                vstack(rows).gap(16.0).w(552.0)
            }),
            700.0,
            300.0,
        );
        app.settle();
        app
    }

    #[test]
    fn choosing_a_row_checks_it() {
        let mut app = rows();
        let row = || role(Role::RadioButton).name("/dev/nvme0n1");
        assert_eq!(app.find(row()).states.checked, Some(false));
        app.click(row()).settle();
        assert_eq!(app.find(row()).states.checked, Some(true));
    }

    #[test]
    fn a_row_that_cannot_be_chosen_is_disabled() {
        let mut app = rows();
        let center = app
            .find(role(Role::RadioButton).name("/dev/sda"))
            .bounds
            .center();
        app.pointer_down(center.x, center.y);
        app.pointer_up(center.x, center.y).settle();
        let found = app.find(role(Role::RadioButton).name("/dev/sda"));
        assert!(found.states.disabled);
        assert_eq!(found.states.checked, Some(false));
    }

    #[test]
    fn rows_are_the_design_height() {
        let mut app = rows();
        let bounds = app
            .find(role(Role::RadioButton).name("/dev/nvme0n1"))
            .bounds;
        assert_eq!(bounds.size.height, RowSize::Large.metrics().height);
    }
}
