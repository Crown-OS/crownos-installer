//! Either flow: offer to get online. Optional, because the packages come from the live image.
//!
//! The list is NetworkManager's, refreshed by the worker this page starts.
//! Choosing a secured network asks for its password beside the list; an open
//! one joins straight away from "Connect".

use crownui::kit::{ButtonVariant, TextInputOptions, icons, text_input};
use crownui::prelude::{
    Color, Cx, Listeners, Role, Semantic, Signal, Styled, View, bind, hstack, keyed, rgb, show,
    text, vstack,
};

use crate::network::{self, AccessPoint, Network};
use crate::ui::Wizard;
use crate::ui::components::{NetworkLine, Status, Tone, alert, field, network_row, note, pill};
use crate::ui::layout::{BLOCK_GAP, PageParts, back_pill, next_pill, nothing};
use crate::ui::palette::{BRAND, INK};
use crate::util::mirror::mirror;
use crate::util::worker::spawn_worker;

const LIST_WIDTH: f32 = 432.0;
const ROW_GAP: f32 = 12.0;
const COLUMN_GAP: f32 = 32.0;
const DOT: f32 = 8.0;
const STATUS_SIZE: f32 = 15.0;
const LOOKING: Color = rgb(0x8b8996);
const OFFLINE: Color = rgb(0xb45409);

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let commands = spawn_worker(cx, network::run, move |cx, event| {
        wizard.update(cx, |wizard| wizard.network.apply(event));
    });
    wizard.update(cx, |wizard| wizard.network.attach(commands));

    let list = keyed(
        move |runtime| {
            wizard.with(runtime, |wizard| {
                wizard
                    .network
                    .snapshot
                    .as_ref()
                    .map_or_else(Vec::new, |snapshot| snapshot.access_points.clone())
            })
        },
        move |cx, points| {
            let rows: Vec<_> = points
                .iter()
                .map(|point| access_point(cx, wizard, point))
                .collect();
            vstack(rows).gap(ROW_GAP).w_full()
        },
    );

    let connection = keyed(
        move |runtime| wizard.with(runtime, |wizard| connection(&wizard.network)),
        |_, (line, color)| {
            hstack((
                vstack(()).size(DOT).rounded(DOT / 2.0).bg(color),
                text(line).text_size(STATUS_SIZE).text_color(INK),
            ))
            .gap(10.0)
            .items_center()
            .role(Role::Group)
            .label("Connection")
            .enter(|style| style.opacity(0.0))
        },
    );

    let explanation = keyed(
        move |runtime| wizard.with(runtime, |wizard| explanation(&wizard.network)),
        |_, explanation| explanation.map_or_else(nothing, |line| note(Tone::Info, line).boxed()),
    );

    let passphrase = keyed(
        move |runtime| {
            wizard.with(runtime, |wizard| {
                wizard
                    .network
                    .selection()
                    .is_some_and(AccessPoint::needs_passphrase)
            })
        },
        move |cx, asks| {
            if !asks {
                return nothing();
            }
            let value = mirror(
                cx,
                wizard,
                |wizard| wizard.network.passphrase.clone(),
                |wizard, value| wizard.network.passphrase = value,
            );
            vstack(field(
                "Password",
                text_input(value)
                    .secret()
                    .on_submit(move |cx, _| wizard.update(cx, |wizard| wizard.network.join())),
            ))
            .w_full()
            .enter(|style| style.opacity(0.0).translate_y(-8.0))
            .boxed()
        },
    );

    let join = show(
        bind(move |runtime| {
            wizard.with(runtime, |wizard| {
                wizard
                    .network
                    .selection()
                    .is_some_and(|point| !point.active && !point.enterprise)
            })
        }),
        move || {
            pill(
                bind(move |runtime| {
                    wizard.with(runtime, |wizard| {
                        if wizard.network.joining.is_some() {
                            "Connecting…"
                        } else {
                            "Connect"
                        }
                        .to_owned()
                    })
                }),
                ButtonVariant::Secondary,
            )
            .disabled(bind(move |runtime| {
                wizard.with(runtime, |wizard| !wizard.network.can_join())
            }))
            .on_click(move |cx, _| wizard.update(cx, |wizard| wizard.network.join()))
        },
        nothing,
    );

    let failure = keyed(
        move |runtime| wizard.with(runtime, |wizard| wizard.network.failure.clone()),
        |_, failure| {
            failure.map_or_else(nothing, |line| {
                alert(Tone::Danger, "That did not connect", line).boxed()
            })
        },
    );

    let side = vstack((explanation, passphrase, join, failure))
        .gap(BLOCK_GAP)
        .items_start()
        .flex_1();

    let can_go_back = wizard.with_untracked(cx, |wizard| wizard.previous_page().is_some());
    PageParts::new(
        "Connect to the Internet",
        "Optional. CrownOS installs without a connection; with one, it can update right away.",
    )
    .content(
        hstack((
            vstack((connection, list))
                .gap(BLOCK_GAP)
                .w(LIST_WIDTH)
                .shrink(0.0),
            side,
        ))
        .gap(COLUMN_GAP)
        .items_start()
        .w_full(),
    )
    .actions((
        can_go_back.then(|| back_pill(wizard)),
        next_pill(wizard, |_| false),
    ))
}

fn access_point(cx: &mut Cx, wizard: Signal<Wizard>, point: &AccessPoint) -> impl View + use<> {
    let ssid = point.ssid.clone();
    let selected = {
        let ssid = ssid.clone();
        cx.memo(move |runtime| {
            wizard.with(runtime, |wizard| {
                wizard.network.selected.as_deref() == Some(ssid.as_str())
            })
        })
    };
    let link = {
        let ssid = ssid.clone();
        cx.memo(move |runtime| wizard.with(runtime, |wizard| link(&wizard.network, &ssid)))
    };
    network_row(line(point), selected, link, move |cx| {
        let ssid = ssid.clone();
        wizard.update(cx, |wizard| wizard.network.select(ssid));
    })
}

/// How joining the network called `ssid` is going, as its row shows it.
pub fn link(network: &Network, ssid: &str) -> Status {
    let point = network.snapshot.as_ref().and_then(|snapshot| {
        snapshot
            .access_points
            .iter()
            .find(|point| point.ssid == ssid)
    });
    if network.joining.as_deref() == Some(ssid) {
        Status::Working
    } else if point.is_some_and(|point| point.active) && network.online() {
        Status::Done
    } else if network.failure.is_some() && network.selected.as_deref() == Some(ssid) {
        Status::Failed
    } else {
        Status::Waiting(0)
    }
}

fn line(point: &AccessPoint) -> NetworkLine {
    let kind = if point.active {
        "Connected"
    } else if point.enterprise {
        "Enterprise login"
    } else if point.secured {
        "Secured"
    } else {
        "Open"
    };
    NetworkLine {
        ssid: point.ssid.clone(),
        detail: format!("{kind} · signal {}%", point.strength),
        glyph: if point.secured {
            icons::LOCK
        } else {
            icons::WIFI
        },
        usable: !point.enterprise,
    }
}

/// The line beside the list, and the colour of its dot.
pub fn connection(network: &Network) -> (String, Color) {
    let active = network
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.access_points.iter().find(|point| point.active));
    match (&network.joining, active) {
        (Some(ssid), _) => (format!("Connecting to {ssid}…"), BRAND),
        (None, Some(point)) if network.online() => (format!("Connected to {}", point.ssid), BRAND),
        (None, None) if network.online() => ("Connected by cable".to_owned(), BRAND),
        _ if network.snapshot.is_none() && network.unavailable.is_none() => {
            ("Looking for networks…".to_owned(), LOOKING)
        }
        _ => ("Not connected".to_owned(), OFFLINE),
    }
}

fn explanation(network: &Network) -> Option<&'static str> {
    if network.unavailable.is_some() {
        return Some(
            "The network service is not running, so Wi-Fi cannot be set up here. Plug in a \
             network cable, or continue without one.",
        );
    }
    let snapshot = network.snapshot.as_ref()?;
    if !snapshot.has_wifi && !snapshot.online {
        return Some("No Wi-Fi card was found. Plug in a network cable, or continue without one.");
    }
    network
        .selection()
        .is_some_and(|point| point.enterprise)
        .then_some(
            "That network needs an enterprise login, which the installer cannot do. Choose \
             another, or plug in a cable.",
        )
}
