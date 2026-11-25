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
use crate::engine::ai_level::AiLevel;
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
        ("Welcome", Wizard::ready(iso_context(), Mode::Commit)),
        ("CrownOS is ready to install", payload_wizard()),
        (
            "Connect to the internet",
            networks(&[("Home Wi-Fi", true), ("Uniworld Wifi", false)]),
        ),
        ("Select the disk", iso_wizard()),
        ("Where should CrownOS go?", on(iso_wizard(), Page::Space)),
        ("Partition editor", editing()),
        ("Let's get you started", on(payload_wizard(), Page::Account)),
        (
            "Your browser and coding agents",
            on(iso_wizard(), Page::Apps),
        ),
        ("Choose your AI level", {
            let mut wizard = on(iso_wizard(), Page::Ai);
            wizard.set_ai_level(AiLevel::Advanced);
            wizard
        }),
        ("Ready to install", confirming),
        ("Installing CrownOS", committed()),
        ("You’re all set", finished),
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
/// `$CROWNOS_INSTALLER_RENDER_DIR/<index>.png`, for looking at, at the
/// window size in `$CROWNOS_INSTALLER_RENDER_SIZE` (`1728x1117`) or the
/// default one.
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
    let (width, height) = std::env::var("CROWNOS_INSTALLER_RENDER_SIZE")
        .ok()
        .and_then(|size| {
            let (width, height) = size.split_once('x')?;
            Some((width.parse().ok()?, height.parse().ok()?))
        })
        .unwrap_or(WINDOW_SIZE);
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

/// The column does not reflow its footer, so content that outgrows it must
/// scroll rather than push the action row out of the window. At the smallest
/// window the installer accepts, every page's buttons stay in the window,
/// under the step track.
#[test]
fn every_action_row_stays_in_the_window_at_the_smallest_size() {
    let (_, height) = MIN_WINDOW_SIZE;
    let gutter = 24.0;
    let window_bottom = height as f32 - gutter;
    for (wizard, button) in [
        (payload_wizard(), "Continue"),
        (on(iso_wizard(), Page::Network), "Continue"),
        (iso_wizard(), "Continue"),
        (on(iso_wizard(), Page::Space), "Continue"),
        (editing(), "Done"),
        (on(payload_wizard(), Page::Account), "Continue"),
        (on(iso_wizard(), Page::Apps), "Continue"),
        (on(iso_wizard(), Page::Ai), "Continue"),
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
    ] {
        let page = wizard.page;
        let mut app = window_sized(wizard, MIN_WINDOW_SIZE);
        let bounds = app.find(role(Role::Button).name(button)).bounds;
        let bottom = bounds.origin.y + bounds.size.height;
        assert!(
            bottom <= window_bottom,
            "`{button}` on {page:?} ends at {bottom}, the window at {window_bottom}"
        );
        let track = app
            .find(role(Role::ProgressBar).name("Installation steps"))
            .bounds;
        assert!(
            track.origin.y + track.size.height <= bounds.origin.y,
            "the step track runs into the buttons on {page:?}"
        );
    }
    let mut app = window_sized(on(payload_wizard(), Page::Done), MIN_WINDOW_SIZE);
    let restart = app.find(role(Role::Button).name("Restart now")).bounds;
    assert!(restart.origin.y + restart.size.height <= window_bottom);
}

#[test]
fn the_step_track_follows_the_flow() {
    let mut app = window(iso_wizard());
    assert_eq!(step(&mut app), "Step 2 of 8, Disk");
    press(&mut app, "Continue");
    assert_eq!(step(&mut app), "Step 3 of 8, Space");
    press(&mut app, "Continue");
    assert_eq!(step(&mut app), "Step 4 of 8, Account");
    press(&mut app, "Go back");
    assert_eq!(step(&mut app), "Step 3 of 8, Space");

    let mut app = window(on(payload_wizard(), Page::Done));
    assert_eq!(step(&mut app), "", "the finished page has no track");
}

#[test]
fn moving_between_steps_is_animated_and_settles() {
    let mut app = window(iso_wizard());
    app.click(role(Role::Button).name("Continue"));
    app.advance_ms(16);
    assert!(app.is_animating(), "the pages cross over");
    app.settle();
    assert!(
        !app.is_animating(),
        "nothing animates once a page has landed"
    );
    assert!(showing(&mut app, "Where should CrownOS go?"));
    assert!(!showing(&mut app, "Select the disk"));
}

#[test]
fn the_window_opens_with_the_column_rising_in() {
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
fn a_flashed_image_opens_on_the_intro_without_the_step_track() {
    let mut app = window(intro());
    assert!(showing(&mut app, "Welcome"));
    assert!(!app.exists(role(Role::ProgressBar).name("Installation steps")));
    assert!(!disabled(&mut app, "Try it out"));
}

#[test]
fn installing_from_the_intro_opens_the_first_step_and_back_returns_to_it() {
    let mut app = window(intro());
    press(&mut app, "Install CrownOS");
    assert!(showing(&mut app, "Select the disk"));
    assert_eq!(step(&mut app), "Step 2 of 8, Disk");
    press(&mut app, "Go back");
    press(&mut app, "Go back");
    assert!(showing(&mut app, "Welcome"));
}

#[test]
fn the_intro_fits_the_smallest_window() {
    let (width, height) = MIN_WINDOW_SIZE;
    let mut app = window_sized(intro(), MIN_WINDOW_SIZE);
    for choice in ["Install CrownOS", "Try it out"] {
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

#[test]
fn the_intro_installs_from_the_keyboard() {
    let mut app = window(intro());
    app.focus(role(Role::Button).name("Install CrownOS"));
    app.press_key("Enter").settle();
    assert!(showing(&mut app, "Select the disk"));
    assert_eq!(step(&mut app), "Step 2 of 8, Disk");
}

// ---- Keyboard ----

#[test]
fn choices_and_buttons_answer_enter_and_space() {
    let mut wizard = iso_wizard();
    wizard.target = super::Target::default();
    let mut app = window(wizard);
    let disk = || role(Role::RadioButton).name("/dev/sda");
    app.focus(disk());
    app.press_key("Space").settle();
    assert_eq!(app.find(disk()).states.checked, Some(true));
    app.focus(role(Role::Button).name("Continue"));
    app.press_key("Enter").settle();
    assert!(showing(&mut app, "Where should CrownOS go?"));
}

#[test]
fn a_network_is_chosen_from_the_keyboard() {
    let mut app = window(networks(&[("Home Wi-Fi", true), ("Uniworld Wifi", false)]));
    app.focus(role(Role::Button).name("Uniworld Wifi"));
    app.press_key("Enter").settle();
    let row = app.find(role(Role::Button).name("Uniworld Wifi"));
    assert_eq!(row.value.as_deref(), Some("Selected"));
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
        "Step 1 of 7, Welcome",
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
    assert!(!disabled(&mut app, "Continue"));
}

// ---- Disk ----

#[test]
fn the_disk_page_needs_a_disk_before_it_continues() {
    let mut wizard = iso_wizard();
    wizard.target = super::Target::default();
    let mut app = window(wizard);
    assert!(disabled(&mut app, "Continue"));
    app.click(role(Role::RadioButton).name("/dev/sda")).settle();
    assert!(!disabled(&mut app, "Continue"));
    press(&mut app, "Continue");
    assert!(showing(&mut app, "Where should CrownOS go?"));
}

// ---- Space ----

#[test]
fn the_space_page_shows_the_automatic_layout_and_it_reaches_the_plan() {
    let mut wizard = on(iso_wizard(), Page::Space);
    sign_in(&mut wizard);
    let mut app = window(wizard);
    assert!(showing(
        &mut app,
        "CrownOS will create a 1.0 GiB boot partition"
    ));
    press(&mut app, "Continue");
    press(&mut app, "Continue");
    assert!(showing(&mut app, "Your browser and coding agents"));
    press(&mut app, "Continue");
    assert!(showing(&mut app, "Choose your AI level"));
    press(&mut app, "Continue");
    assert!(showing(&mut app, "Ready to install"));
    assert!(app.exists(role(Role::ListItem).name("Creating the boot partition")));
    assert!(app.exists(role(Role::ListItem).name("Formatting as btrfs")));
}

#[test]
fn the_iso_flow_walks_from_disk_to_account_by_its_buttons() {
    let mut app = window(iso_wizard());
    press(&mut app, "Continue");
    assert!(showing(&mut app, "Where should CrownOS go?"));
    press(&mut app, "Continue");
    assert!(showing(&mut app, "Let's get you started"));
    press(&mut app, "Go back");
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
fn typing_the_missing_username_moves_on_to_the_apps_page() {
    let mut wizard = on(iso_wizard(), Page::Account);
    sign_in(&mut wizard);
    wizard.form.username.clear();
    let mut app = window(wizard);
    assert!(disabled(&mut app, "Continue"));

    app.focus(role(Role::TextField).name("Username"));
    app.type_text("ada");
    app.settle();
    assert!(!disabled(&mut app, "Continue"));

    press(&mut app, "Continue");
    assert!(showing(&mut app, "Your browser and coding agents"));
}

// ---- Apps ----

#[test]
fn the_browser_choice_and_the_tool_switches_are_reachable() {
    let mut wizard = on(iso_wizard(), Page::Apps);
    sign_in(&mut wizard);
    let mut app = window(wizard);
    let firefox = role(Role::RadioButton).name("Firefox");
    let codex = role(Role::Checkbox).name("Codex");
    assert_eq!(app.find(firefox.clone()).states.checked, Some(false));
    assert_eq!(app.find(codex.clone()).states.checked, Some(false));

    app.click(firefox.clone()).settle();
    app.click(codex.clone()).settle();
    press(&mut app, "Continue");
    assert!(showing(&mut app, "Choose your AI level"));
    press(&mut app, "Continue");
    assert!(app.exists(role(Role::ListItem).name("Saving the setup choices")));
    press(&mut app, "Go back");
    press(&mut app, "Go back");

    assert_eq!(app.find(firefox).states.checked, Some(true));
    assert_eq!(app.find(codex).states.checked, Some(true));
    for name in ["Windows apps", "Claude Code", "Cursor"] {
        assert_eq!(
            app.find(role(Role::Checkbox).name(name)).states.checked,
            Some(false),
            "{name}"
        );
    }
}

#[test]
fn an_image_without_the_setup_runner_offers_only_windows_apps() {
    let mut context = iso_context();
    context.snapshot.published_packages = Some(vec!["crownbar".to_owned()]);
    let mut app = window(on(Wizard::ready(context, Mode::Commit), Page::Apps));
    assert!(app.exists(role(Role::Checkbox).name("Windows apps")));
    assert!(!app.exists(role(Role::RadioButton).name("Firefox")));
    assert!(!app.exists(role(Role::Checkbox).name("Codex")));
    assert!(showing(&mut app, "Apps from other systems"));
}

/// At the smallest window the choices outgrow the column and scroll; the
/// scrolling area ends above the step track, so no choice slides under it.
#[test]
fn the_apps_choices_scroll_above_the_track_at_the_smallest_window() {
    let mut app = window_sized(on(iso_wizard(), Page::Apps), MIN_WINDOW_SIZE);
    assert!(app.exists(role(Role::Checkbox).name("Cursor")));
    let scroller = app.find(role(Role::ScrollView)).bounds;
    let track = app
        .find(role(Role::ProgressBar).name("Installation steps"))
        .bounds;
    assert!(
        scroller.origin.y + scroller.size.height <= track.origin.y,
        "the choices end at {}, the step track starts at {}",
        scroller.origin.y + scroller.size.height,
        track.origin.y
    );
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
fn choosing_an_ai_level_adds_the_acceleration_step_to_the_plan() {
    let mut wizard = on(payload_wizard(), Page::Account);
    sign_in(&mut wizard);
    wizard.go(Page::Ai);
    let mut app = window(wizard);
    let ai_step = role(Role::ListItem).name("Setting up AI acceleration");
    let off = role(Role::RadioButton).name("Off");
    let minimal = role(Role::RadioButton).name("Minimal");
    assert_eq!(app.find(off.clone()).states.checked, Some(true));
    press(&mut app, "Continue");
    assert!(showing(&mut app, "Ready to install"));
    assert!(!app.exists(ai_step.clone()));

    press(&mut app, "Go back");
    app.click(minimal.clone()).settle();
    assert_eq!(app.find(minimal).states.checked, Some(true));
    assert_eq!(app.find(off).states.checked, Some(false));
    press(&mut app, "Continue");
    assert!(app.exists(ai_step));
}

#[test]
fn the_confirmation_names_what_runs_ai_only_when_ai_is_on() {
    let mut wizard = on(payload_wizard(), Page::Account);
    sign_in(&mut wizard);
    wizard.go(Page::Confirm);
    let mut app = window(wizard);
    assert!(!showing(&mut app, "AI features run on"));

    let mut wizard = on(payload_wizard(), Page::Account);
    sign_in(&mut wizard);
    wizard.set_ai_level(AiLevel::Advanced);
    wizard.go(Page::Confirm);
    let mut app = window(wizard);
    let summary = app.semantics_snapshot();
    assert!(summary.contains("AI features run on"), "{summary}");
}

#[test]
fn where_ai_runs_is_one_of_two_and_starts_on_this_device() {
    let mut app = window(on(iso_wizard(), Page::Ai));
    let on_device = role(Role::RadioButton).name("On this device only");
    let cloud = role(Role::RadioButton).name("Allow cloud models");
    assert_eq!(app.find(on_device.clone()).states.checked, Some(true));
    assert_eq!(app.find(cloud.clone()).states.checked, Some(false));
    app.click(cloud.clone()).settle();
    assert_eq!(app.find(cloud).states.checked, Some(true));
    assert_eq!(app.find(on_device).states.checked, Some(false));
}

#[test]
fn an_ai_level_is_chosen_from_the_keyboard() {
    let mut app = window(on(iso_wizard(), Page::Ai));
    let advanced = role(Role::RadioButton).name("Advanced");
    app.focus(advanced.clone());
    app.press_key("Enter").settle();
    assert_eq!(app.find(advanced).states.checked, Some(true));
}

/// The design's window: the cards are the design's size and place, and the
/// switch is the design's height.
#[test]
fn the_ai_cards_match_the_design_at_its_size() {
    let mut app = window_sized(on(iso_wizard(), Page::Ai), (1728, 1117));
    for (name, x) in [("Off", 304.0), ("Minimal", 684.0), ("Advanced", 1064.0)] {
        let card = app.find(role(Role::RadioButton).name(name)).bounds;
        assert!((card.origin.x - x).abs() < 0.5, "{name}: {card:?}");
        assert!((card.size.width - 360.0).abs() < 0.5, "{name}: {card:?}");
    }
    let switch = app.find(role(Role::Group).name("Where AI runs")).bounds;
    assert!((switch.size.height - 54.0).abs() < 0.5, "{switch:?}");
}

/// In every window the installer accepts, the cards stay side by side inside
/// it. In laptop and desktop windows the cards and the switch fit above the
/// step track without scrolling. The note under the switch is checked in the
/// GPU renders: the headless text system sets type taller than Impeller does.
#[test]
fn the_ai_page_fits_short_and_narrow_windows() {
    let sizes = [
        (1366, 768),
        (1280, 800),
        (1440, 900),
        (1600, 900),
        (1680, 1050),
        (1920, 1080),
        (1728, 1117),
        (2560, 1440),
        MIN_WINDOW_SIZE,
    ];
    for size in sizes {
        let mut wizard = on(iso_wizard(), Page::Ai);
        wizard.set_ai_level(AiLevel::Advanced);
        let mut app = window_sized(wizard, size);
        let width = size.0 as f32;
        let cards: Vec<_> = ["Off", "Minimal", "Advanced"]
            .into_iter()
            .map(|name| app.find(role(Role::RadioButton).name(name)).bounds)
            .collect();
        for card in &cards {
            assert!(
                card.origin.x >= 0.0 && card.max_x() <= width,
                "{size:?}: {card:?}"
            );
            assert!(
                (card.origin.y - cards[0].origin.y).abs() < 0.5,
                "{size:?}: not one row"
            );
            assert!(
                (card.size.height - cards[0].size.height).abs() < 0.5,
                "{size:?}: uneven"
            );
        }
        if size == MIN_WINDOW_SIZE {
            continue;
        }
        let track = app
            .find(role(Role::ProgressBar).name("Installation steps"))
            .bounds;
        let switch = app.find(role(Role::Group).name("Where AI runs")).bounds;
        assert!(cards[0].max_y() <= switch.origin.y, "{size:?}");
        assert!(
            switch.max_y() <= track.origin.y,
            "{size:?}: the switch ends at {}, the track starts at {}",
            switch.max_y(),
            track.origin.y
        );
    }
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
        label: "Creating the btrfs subvolumes".to_owned(),
    });
    let mut app = window(wizard);
    let value = |app: &mut TestApp, name: &str| {
        app.find(role(Role::ListItem).name(name))
            .value
            .unwrap_or_default()
    };
    assert_eq!(value(&mut app, "Re-reading the partition table"), "Done");
    assert_eq!(
        value(&mut app, "Creating the btrfs subvolumes"),
        "In progress"
    );
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
fn the_done_page_says_how_to_start_the_new_system() {
    let mut wizard = committed();
    wizard.on_install_event(Event::Finished { dry_run: false });
    let mut app = window(wizard);
    assert!(showing(
        &mut app,
        "CrownOS is installed. Restart to start using it."
    ));
    assert!(showing(
        &mut app,
        "CrownOS is the default in the boot menu. Hold Space while the computer starts \
         to choose Windows or recovery instead."
    ));
    assert!(!disabled(&mut app, "Restart now"));
}

#[test]
fn restarting_closes_the_column_and_locks_the_button() {
    let mut app = window(on(payload_wizard(), Page::Done));
    app.click(role(Role::Button).name("Restart now"));
    app.advance_ms(16);
    assert!(app.is_animating(), "the column sinks away");
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
    assert!(showing(&mut app, "Select the disk"));
}
