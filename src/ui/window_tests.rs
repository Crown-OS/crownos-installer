//! The whole window, driven headlessly: every page built in the state that
//! reaches it, and every interaction that moves the wizard clicked, typed and
//! keyed. No executor runs here, so neither the network worker nor the install
//! ever starts, and a restart never reaches `systemctl`.

use crowntest::{Role, TestApp, role, text};
use crownui::kit::provide_theme;
use crownui::prelude::component;

use super::editor::Row;
use super::palette::oobe_theme;
use super::root::shell;
use super::tests::{iso_context, payload_context, sign_in};
use super::{Blocker, MIN_WINDOW_SIZE, Page, WINDOW_SIZE, Wizard};
use crate::engine::exec::{Event, Mode};
use crate::engine::plan::bytes;
use crate::network::{AccessPoint, Snapshot};

fn window_sized(wizard: Wizard, (width, height): (u32, u32)) -> TestApp {
    let mut app = TestApp::with_size(
        component(move |cx| {
            provide_theme(cx, oobe_theme());
            shell(cx.signal(wizard))
        }),
        width as f32,
        height as f32,
    );
    app.settle();
    app
}

fn window(wizard: Wizard) -> TestApp {
    window_sized(wizard, WINDOW_SIZE)
}

/// Whether `words` are on screen: as a heading, a titled block, or text.
fn showing(app: &mut TestApp, words: &str) -> bool {
    app.exists(role(Role::Heading).name(words))
        || app.exists(role(Role::Group).name(words))
        || app.exists(text(words))
}

fn press(app: &mut TestApp, button: &str) {
    app.click(role(Role::Button).name(button)).settle();
}

fn disabled(app: &mut TestApp, button: &str) -> bool {
    app.find(role(Role::Button).name(button)).states.disabled
}

fn step(app: &mut TestApp) -> String {
    app.find(role(Role::ProgressBar).name("Installation steps"))
        .value
        .unwrap_or_default()
}

fn payload_wizard() -> Wizard {
    Wizard::ready(payload_context(None, false), Mode::Commit)
}

/// The ISO flow at its first step, past the intro.
fn iso_wizard() -> Wizard {
    on(Wizard::ready(iso_context(), Mode::Commit), Page::Disk)
}

fn on(mut wizard: Wizard, page: Page) -> Wizard {
    wizard.go(page);
    wizard
}

fn committed() -> Wizard {
    let mut wizard = on(payload_wizard(), Page::Confirm);
    sign_in(&mut wizard);
    wizard.begin_install();
    wizard
}

fn editing() -> Wizard {
    let mut wizard = on(iso_wizard(), Page::Space);
    wizard.open_editor();
    wizard
}

fn failed() -> Wizard {
    let mut wizard = committed();
    wizard.on_install_event(Event::Log("pacstrap -K /mnt base".to_owned()));
    wizard.on_install_event(Event::Failed {
        index: 0,
        message: "pacstrap exited with status 1".to_owned(),
    });
    wizard
}

fn networks(points: &[(&str, bool)]) -> Wizard {
    let mut wizard = on(iso_wizard(), Page::Network);
    wizard.network.snapshot = Some(Snapshot {
        online: false,
        has_wifi: true,
        access_points: points
            .iter()
            .map(|&(ssid, secured)| AccessPoint {
                ssid: ssid.to_owned(),
                strength: 70,
                secured,
                enterprise: false,
                active: false,
            })
            .collect(),
    });
    wizard
}

/// Every page, each in the state that reaches it, with its heading.
fn every_page() -> Vec<(&'static str, Wizard)> {
    let blocked = Wizard::blocked(
        Blocker {
            title: "The disks could not be read".to_owned(),
            detail: "because".to_owned(),
            notes: vec!["a note".to_owned()],
        },
        Mode::Commit,
    );
    let confirming = {
        let mut wizard = on(payload_wizard(), Page::Account);
        sign_in(&mut wizard);
        wizard.go(Page::Confirm);
        wizard
    };
    let finished = {
        let mut wizard = committed();
        wizard.on_install_event(Event::Finished { dry_run: false });
        wizard
    };
    vec![
        (
            "Welcome to CrownOS",
            Wizard::ready(iso_context(), Mode::Commit),
        ),
        ("CrownOS is ready to install", payload_wizard()),
        (
            "Connect to the Internet",
            networks(&[("Home Wi-Fi", true), ("Uniworld Wifi", false)]),
        ),
        ("Choose a disk", iso_wizard()),
        ("Where should CrownOS go?", on(iso_wizard(), Page::Space)),
        ("Partition editor", editing()),
        ("Let's get you started", on(payload_wizard(), Page::Account)),
        ("Ready to install", confirming),
        ("Installing CrownOS", committed()),
        ("You're all set", finished),
        ("Nothing has been changed", blocked),
        ("Installing CrownOS", failed()),
    ]
}

#[test]
fn every_page_builds_in_the_state_that_reaches_it() {
    for (heading, wizard) in every_page() {
        let mut app = window(wizard);
        assert!(
            showing(&mut app, heading),
            "`{heading}` did not show:\n{}",
            app.semantics_snapshot()
        );
    }
}

/// Renders every page through Impeller's offscreen GLES target into
/// `$CROWNOS_INSTALLER_RENDER_DIR/<index>.png`, for looking at.
#[test]
#[ignore = "needs a GPU; writes PNGs for manual inspection"]
fn render_every_page() {
    use crownui::ext::{InlineApp, TextSystem};
    use crownui::kit::{INTER_FAMILY, inter_font};
    use crownui::prelude::Size;
    use impeller::{ImpellerBackend, Offscreen};
    use taffy_layout::TaffyEngine;

    let Some(dir) = std::env::var_os("CROWNOS_INSTALLER_RENDER_DIR") else {
        return;
    };
    let (width, height) = WINDOW_SIZE;
    for (index, (_, wizard)) in every_page().into_iter().enumerate() {
        let surface = Offscreen {
            width,
            height,
            scale: 1.0,
        };
        let root = component(move |cx| {
            provide_theme(cx, oobe_theme());
            shell(cx.signal(wizard))
        });
        let mut app = InlineApp::<ImpellerBackend, TaffyEngine>::with_surface(
            root,
            TaffyEngine::new(),
            Size::new(width as f32, height as f32),
            1.0,
            &surface,
        )
        .expect("an offscreen renderer");
        app.text_cache()
            .system_mut()
            .register_font(inter_font().to_vec(), Some(INTER_FAMILY))
            .expect("Inter registers");
        app.settle().expect("a frame renders");
        let pixels = app.renderer_mut().read_pixels().expect("pixels read back");
        let png = impeller::encode_png(&pixels).expect("PNG encodes");
        std::fs::write(std::path::Path::new(&dir).join(format!("{index}.png")), png)
            .expect("PNG written");
        // NVIDIA's EGL crashes in `eglDestroyContext` when a headless context drops.
        std::mem::forget(app);
    }
}

// ---- The frame ----

/// The card does not reflow its footer, so content that outgrows the column
/// must scroll rather than push the action row off the card. At the smallest
/// window the installer accepts, every page's buttons stay on the card.
#[test]
fn every_action_row_sits_on_the_card_at_the_smallest_window() {
    let (_, height) = MIN_WINDOW_SIZE;
    let gutter = 24.0;
    let card_bottom = height as f32 - gutter;
    for (wizard, button) in [
        (payload_wizard(), "Next"),
        (on(iso_wizard(), Page::Network), "Next"),
        (iso_wizard(), "Next"),
        (on(iso_wizard(), Page::Space), "Next"),
        (editing(), "Done"),
        (on(payload_wizard(), Page::Account), "Next"),
        (
            {
                let mut wizard = on(payload_wizard(), Page::Account);
                sign_in(&mut wizard);
                wizard.go(Page::Confirm);
                wizard
            },
            "Install CrownOS",
        ),
        (failed(), "Restart"),
        (on(payload_wizard(), Page::Done), "Restart now"),
    ] {
        let page = wizard.page;
        let mut app = window_sized(wizard, MIN_WINDOW_SIZE);
        let bounds = app.find(role(Role::Button).name(button)).bounds;
        let bottom = bounds.origin.y + bounds.size.height;
        assert!(
            bottom <= card_bottom - 30.0,
            "`{button}` on {page:?} ends at {bottom}, the card at {card_bottom}"
        );
        let track = app
            .find(role(Role::ProgressBar).name("Installation steps"))
            .bounds;
        let leftmost = ["Back", "Edit partitions", "Discard changes", button]
            .into_iter()
            .filter_map(|name| {
                let query = role(Role::Button).name(name);
                app.exists(query.clone())
                    .then(|| app.find(query).bounds.origin.x)
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            track.origin.x + track.size.width < leftmost,
            "the step track runs under the buttons on {page:?}"
        );
    }
}

#[test]
fn the_step_track_follows_the_flow() {
    let mut app = window(iso_wizard());
    assert_eq!(step(&mut app), "Step 2 of 6");
    press(&mut app, "Next");
    assert_eq!(step(&mut app), "Step 3 of 6");
    press(&mut app, "Next");
    assert_eq!(step(&mut app), "Step 4 of 6");
    press(&mut app, "Back");
    assert_eq!(step(&mut app), "Step 3 of 6");

    let mut app = window(on(payload_wizard(), Page::Done));
    assert_eq!(step(&mut app), "All 5 steps complete");
}

#[test]
fn moving_between_steps_is_animated_and_settles() {
    let mut app = window(iso_wizard());
    app.click(role(Role::Button).name("Next"));
    app.advance_ms(16);
    assert!(app.is_animating(), "the pages cross over");
    app.settle();
    assert!(
        !app.is_animating(),
        "nothing animates once a page has landed"
    );
    assert!(showing(&mut app, "Where should CrownOS go?"));
    assert!(!showing(&mut app, "Choose a disk"));
}

#[test]
fn the_window_opens_with_the_card_rising_in() {
    let app = TestApp::with_size(
        component(|cx| {
            provide_theme(cx, oobe_theme());
            shell(cx.signal(payload_wizard()))
        }),
        WINDOW_SIZE.0 as f32,
        WINDOW_SIZE.1 as f32,
    );
    assert!(app.is_animating());
}

// ---- Intro ----

fn intro() -> Wizard {
    Wizard::ready(iso_context(), Mode::Commit)
}

#[test]
fn a_flashed_image_opens_on_the_intro_without_the_card() {
    let mut app = window(intro());
    assert!(showing(&mut app, "Welcome to CrownOS"));
    assert!(!app.exists(role(Role::ProgressBar).name("Installation steps")));
    assert_eq!(disabled(&mut app, "Try it out"), !crate::live::available());
}

#[test]
fn installing_from_the_intro_opens_the_first_step_and_back_returns_to_it() {
    let mut app = window(intro());
    press(&mut app, "Install");
    assert!(showing(&mut app, "Choose a disk"));
    assert_eq!(step(&mut app), "Step 2 of 6");
    press(&mut app, "Back");
    press(&mut app, "Back");
    assert!(showing(&mut app, "Welcome to CrownOS"));
}

#[test]
fn the_intro_fits_the_smallest_window() {
    let (width, height) = MIN_WINDOW_SIZE;
    let mut app = window_sized(intro(), MIN_WINDOW_SIZE);
    for choice in ["Install", "Try it out"] {
        let bounds = app.find(role(Role::Button).name(choice)).bounds;
        assert!(
            bounds.origin.x >= 0.0
                && bounds.origin.y >= 0.0
                && bounds.max_x() <= width as f32
                && bounds.max_y() <= height as f32,
            "`{choice}` spills out of the window: {bounds:?}"
        );
    }
}

// ---- Welcome ----

#[test]
fn the_welcome_page_states_what_the_payload_decided() {
    let mut app = window(payload_wizard());
    let partition = app.find(role(Role::Group).name("CrownOS partition"));
    assert!(
        partition
            .value
            .as_deref()
            .is_some_and(|value| value.starts_with("/dev/sda5")),
        "{partition:?}"
    );
    assert!(app.exists(role(Role::Group).name("Shared EFI partition")));
}

#[test]
fn the_partition_editor_opens_from_welcome_and_returns_to_it() {
    let mut app = window(payload_wizard());
    press(&mut app, "Edit partitions");
    assert!(showing(&mut app, "Partition editor"));
    assert_eq!(
        step(&mut app),
        "Step 1 of 5",
        "the editor does not move the track"
    );
    press(&mut app, "Done");
    assert!(showing(&mut app, "CrownOS is ready to install"));
}

// ---- Network ----

#[test]
fn the_network_page_says_it_is_looking_before_the_worker_reports() {
    let mut app = window(on(iso_wizard(), Page::Network));
    assert!(showing(&mut app, "Looking for networks…"));
}

#[test]
fn choosing_a_secured_network_asks_for_its_password() {
    let mut app = window(networks(&[("Home Wi-Fi", true), ("Uniworld Wifi", false)]));
    assert!(!app.exists(role(Role::TextField).name("Password")));
    press(&mut app, "Home Wi-Fi");
    assert!(app.exists(role(Role::TextField).name("Password")));
    assert!(disabled(&mut app, "Connect"), "no passphrase typed yet");
}

#[test]
fn choosing_an_open_network_offers_to_connect_without_a_password() {
    let mut app = window(networks(&[("Home Wi-Fi", true), ("Uniworld Wifi", false)]));
    press(&mut app, "Uniworld Wifi");
    assert!(!app.exists(role(Role::TextField).name("Password")));
    assert!(!disabled(&mut app, "Connect"));
    press(&mut app, "Connect");
    let row = app.find(role(Role::Button).name("Uniworld Wifi"));
    assert_eq!(row.value.as_deref(), Some("Connecting"));
    assert!(disabled(&mut app, "Connecting…"), "one join at a time");
}

#[test]
fn an_offline_machine_may_continue_past_the_network_page() {
    let mut wizard = networks(&[("Home Wi-Fi", true)]);
    if let Ok(context) = &mut wizard.context {
        context.online = false;
    }
    let mut app = window(wizard);
    assert!(!disabled(&mut app, "Next"));
}

// ---- Disk ----

#[test]
fn the_disk_page_needs_a_disk_before_it_continues() {
    let mut wizard = iso_wizard();
    wizard.target = super::Target::default();
    let mut app = window(wizard);
    assert!(disabled(&mut app, "Next"));
    app.click(role(Role::RadioButton).name("/dev/sda")).settle();
    assert!(!disabled(&mut app, "Next"));
    press(&mut app, "Next");
    assert!(showing(&mut app, "Where should CrownOS go?"));
}

// ---- Space ----

#[test]
fn the_filesystem_tiles_reach_the_plan() {
    let mut wizard = on(iso_wizard(), Page::Space);
    sign_in(&mut wizard);
    let mut app = window(wizard);
    app.click(role(Role::RadioButton).name("btrfs — snapshots"))
        .settle();
    let tile = app.find(role(Role::RadioButton).name("btrfs — snapshots"));
    assert_eq!(tile.value.as_deref(), Some("Selected"));
    press(&mut app, "Next");
    press(&mut app, "Next");
    assert!(showing(&mut app, "Ready to install"));
    assert!(app.exists(role(Role::ListItem).name("Formatting as btrfs")));
}

#[test]
fn the_iso_flow_walks_from_disk_to_account_by_its_buttons() {
    let mut app = window(iso_wizard());
    press(&mut app, "Next");
    assert!(showing(&mut app, "Where should CrownOS go?"));
    press(&mut app, "Next");
    assert!(showing(&mut app, "Let's get you started"));
    press(&mut app, "Back");
    assert!(showing(&mut app, "Where should CrownOS go?"));
}

// ---- Partition editor ----

#[test]
fn a_queued_partition_says_nothing_is_written_yet() {
    let wizard = editing();
    let free = {
        let context = wizard.context().expect("context");
        let disk = wizard.target_disk().expect("a disk");
        wizard
            .editor
            .rows(disk, &context.snapshot)
            .into_iter()
            .find_map(|row| match row {
                Row::Free { size_bytes, .. } => Some(format!("{} unallocated", bytes(size_bytes))),
                Row::Partition { .. } => None,
            })
            .expect("free space")
    };
    let mut app = window(wizard);

    assert!(disabled(&mut app, "New"));
    assert!(disabled(&mut app, "Discard changes"));
    app.click(role(Role::RadioButton).name(&free)).settle();
    press(&mut app, "New");
    assert!(
        showing(&mut app, "1 change queued — nothing written yet"),
        "{}",
        app.semantics_snapshot()
    );
    assert!(!disabled(&mut app, "Discard changes"));

    press(&mut app, "Undo");
    assert!(!showing(&mut app, "1 change queued — nothing written yet"));
}

// ---- Account ----

/// Typing reaches the wizard and the gate follows it. The password is filled in
/// on the model: test actions may not type into a secure field.
#[test]
fn typing_the_missing_username_opens_the_confirmation() {
    let mut wizard = on(iso_wizard(), Page::Account);
    sign_in(&mut wizard);
    wizard.form.username.clear();
    let mut app = window(wizard);
    assert!(disabled(&mut app, "Next"));

    app.focus(role(Role::TextField).name("Username"));
    app.type_text("ada");
    app.settle();
    assert!(!disabled(&mut app, "Next"));

    press(&mut app, "Next");
    assert!(showing(&mut app, "Ready to install"));
}

#[test]
fn submitting_an_incomplete_form_explains_what_is_missing() {
    let mut app = window(on(iso_wizard(), Page::Account));
    assert!(!app.exists(role(Role::Group).name("Not yet")));
    app.focus(role(Role::TextField).name("Full name"));
    app.type_text("Ada Lovelace");
    app.press_key("Enter").settle();
    assert!(app.exists(role(Role::Group).name("Not yet")));
    assert!(showing(&mut app, "Choose a password."));
}

// ---- Confirm and install ----

#[test]
fn installing_starts_from_the_confirmation_and_lights_the_first_step() {
    let mut wizard = on(payload_wizard(), Page::Account);
    sign_in(&mut wizard);
    wizard.go(Page::Confirm);
    let mut app = window(wizard);
    assert!(app.exists(role(Role::Group).name("This is the point of no return")));
    press(&mut app, "Install CrownOS");
    assert!(showing(&mut app, "Installing CrownOS"));
    let first = app.find(role(Role::ListItem).name("Creating the CrownOS partition"));
    assert_eq!(first.value.as_deref(), Some("In progress"));
    assert!(app.exists(role(Role::Group).name("Keep your computer on")));
}

#[test]
fn the_install_page_shows_finished_running_and_waiting_steps() {
    let mut wizard = committed();
    for index in 0..3 {
        wizard.on_install_event(Event::StepStarted {
            index,
            total: 20,
            label: format!("step {index}"),
        });
        wizard.on_install_event(Event::StepFinished { index });
    }
    wizard.on_install_event(Event::StepStarted {
        index: 3,
        total: 20,
        label: "Mounting the new system".to_owned(),
    });
    let mut app = window(wizard);
    let value = |app: &mut TestApp, name: &str| {
        app.find(role(Role::ListItem).name(name))
            .value
            .unwrap_or_default()
    };
    assert_eq!(value(&mut app, "Re-reading the partition table"), "Done");
    assert_eq!(value(&mut app, "Mounting the new system"), "In progress");
    let progress = app.find(role(Role::ProgressBar).name("Overall progress"));
    assert_eq!(progress.value.as_deref(), Some("15%"));
}

#[test]
fn a_failed_install_shows_why_and_offers_a_restart() {
    let mut app = window(failed());
    assert!(app.exists(role(Role::Group).name("The install stopped")));
    assert!(showing(&mut app, "pacstrap exited with status 1"));
    let first = app.find(role(Role::ListItem).name("Creating the CrownOS partition"));
    assert_eq!(first.value.as_deref(), Some("Failed"));
    assert!(!disabled(&mut app, "Restart"));
}

// ---- Done and leaving ----

#[test]
fn the_done_page_summarises_what_was_installed() {
    let mut wizard = committed();
    wizard.on_install_event(Event::Finished { dry_run: false });
    let mut app = window(wizard);
    assert!(app.exists(role(Role::Image).name("Installed")));
    let account = app.find(role(Role::Group).name("Account"));
    assert_eq!(account.value.as_deref(), Some("crown"));
    assert!(app.exists(role(Role::Group).name("Computer name")));
}

#[test]
fn restarting_closes_the_card_and_locks_the_button() {
    let mut app = window(on(payload_wizard(), Page::Done));
    app.click(role(Role::Button).name("Restart now"));
    app.advance_ms(16);
    assert!(app.is_animating(), "the card sinks away");
    app.settle();
    assert!(
        disabled(&mut app, "Restart now"),
        "a second press does nothing"
    );
}

#[test]
fn a_dry_run_closes_rather_than_restarting() {
    let mut wizard = committed();
    wizard.on_install_event(Event::Finished { dry_run: true });
    let mut app = window(wizard);
    assert!(showing(&mut app, "Dry run finished"));
    assert!(app.exists(role(Role::Button).name("Close")));
}

// ---- Blocked ----

#[test]
fn a_refused_iso_install_can_choose_another_disk() {
    let mut wizard = iso_wizard();
    wizard.go(Page::Blocked);
    let mut app = window(wizard);
    assert!(app.exists(role(Role::Image).name("Stopped")));
    assert_eq!(
        step(&mut app),
        "",
        "the track says nothing about progress here"
    );
    press(&mut app, "Choose another disk");
    assert!(showing(&mut app, "Choose a disk"));
}
