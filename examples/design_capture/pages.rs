//! Every page of the wizard, in every state worth designing against, grouped as the Figma page
//! lays them out.

use anyhow::{Context as _, ensure};
use capture::Carrier;
use crowntest::{Role, role};

use crate::engine::exec::{Event, Mode};
use crate::engine::plan::bytes;
use crate::machine::{
    crowded_context, iso_context, iso_context_with, windows_context, windows_context_shifted,
};
use crate::network::{AccessPoint, Snapshot};
use crate::oobe::{AiFeatures, WindowsApps};
use crate::session::{
    PAGE_END, click_nth, on, press, scroll_by, scroll_page_to, shoot, sign_in, still,
};
use crate::ui::{Blocker, Page, Target, Wizard};

const DRY: Mode = Mode::DryRun;

pub fn capture_all(carrier: &mut Carrier) -> anyhow::Result<()> {
    start(carrier)?;
    network(carrier)?;
    disk(carrier)?;
    space(carrier)?;
    partition_editor(carrier)?;
    account(carrier)?;
    apps(carrier)?;
    confirm(carrier)?;
    progress(carrier)?;
    finished(carrier)?;
    blocked(carrier)
}

fn start(carrier: &mut Carrier) -> anyhow::Result<()> {
    still(
        carrier,
        Wizard::ready(iso_context(), DRY),
        (
            "Start",
            "Intro",
            "A flashed image opens on Install or Try it out, before the card",
        ),
        "Welcome to CrownOS",
    )?;
    let welcome = Wizard::ready(windows_context(false), DRY);
    ensure!(welcome.page == Page::Welcome && welcome.plan().is_some());
    still(
        carrier,
        welcome,
        (
            "Start",
            "Welcome",
            "Rebooted out of Windows: what the handoff payload decided",
        ),
        "CrownOS is ready to install",
    )
}

// ---- Network ----

fn access_point(ssid: &str, strength: u8, secured: bool) -> AccessPoint {
    AccessPoint {
        ssid: ssid.to_owned(),
        strength,
        secured,
        enterprise: false,
        active: false,
    }
}

fn nearby() -> Vec<AccessPoint> {
    vec![
        access_point("Lindqvist Home", 92, true),
        access_point("Lindqvist Home 5G", 84, true),
        access_point("Café Aurora Guest", 61, false),
        AccessPoint {
            enterprise: true,
            ..access_point("Northgate Staff", 55, true)
        },
        access_point("Studio 4B", 38, true),
    ]
}

fn on_network(prepare: impl FnOnce(&mut crate::network::Network)) -> Wizard {
    let mut context = iso_context();
    context.online = false;
    let mut wizard = on(context, DRY, Page::Network);
    prepare(&mut wizard.network);
    wizard
}

fn offline_list(network: &mut crate::network::Network) {
    network.snapshot = Some(Snapshot {
        online: false,
        has_wifi: true,
        access_points: nearby(),
    });
}

fn network(carrier: &mut Carrier) -> anyhow::Result<()> {
    const HEADING: &str = "Connect to the Internet";
    let states: Vec<(&str, &str, Wizard)> = vec![
        (
            "Looking for networks",
            "Before the NetworkManager worker's first report",
            on_network(|_| {}),
        ),
        (
            "Networks",
            "Wi-Fi networks in range, nothing chosen yet",
            on_network(offline_list),
        ),
        (
            "Secured network",
            "A secured network chosen: its password is asked beside the list",
            on_network(|network| {
                offline_list(network);
                network.select("Lindqvist Home".to_owned());
            }),
        ),
        (
            "Password entered",
            "A WPA passphrase typed, Connect enabled",
            on_network(|network| {
                offline_list(network);
                network.select("Lindqvist Home".to_owned());
                network.passphrase = "lantern-orchard".to_owned();
            }),
        ),
        (
            "Open network",
            "An open network chosen: Connect joins without a password",
            on_network(|network| {
                offline_list(network);
                network.select("Café Aurora Guest".to_owned());
            }),
        ),
        (
            "Connecting",
            "A join in flight, one at a time",
            on_network(|network| {
                offline_list(network);
                network.select("Lindqvist Home".to_owned());
                network.passphrase = "lantern-orchard".to_owned();
                network.joining = Some("Lindqvist Home".to_owned());
            }),
        ),
        (
            "Wrong password",
            "The join failed and says why",
            on_network(|network| {
                offline_list(network);
                network.select("Lindqvist Home".to_owned());
                network.passphrase = "lantern-orchid".to_owned();
                network.failure = Some("The password for Lindqvist Home is not right.".to_owned());
            }),
        ),
        (
            "Connected",
            "Joined and online",
            on_network(|network| {
                let mut points = nearby();
                points[0].active = true;
                network.snapshot = Some(Snapshot {
                    online: true,
                    has_wifi: true,
                    access_points: points,
                });
                network.select("Lindqvist Home".to_owned());
            }),
        ),
        (
            "Enterprise network",
            "A WPA-Enterprise network the installer cannot log in to",
            on_network(|network| {
                offline_list(network);
                network.select("Northgate Staff".to_owned());
            }),
        ),
        (
            "Wired",
            "Online by cable, no Wi-Fi card",
            on_network(|network| {
                network.snapshot = Some(Snapshot {
                    online: true,
                    has_wifi: false,
                    access_points: Vec::new(),
                });
            }),
        ),
        (
            "No Wi-Fi card",
            "Offline with no wireless hardware",
            on_network(|network| {
                network.snapshot = Some(Snapshot {
                    online: false,
                    has_wifi: false,
                    access_points: Vec::new(),
                });
            }),
        ),
        (
            "Network service missing",
            "NetworkManager could not be reached",
            on_network(|network| {
                network.unavailable = Some(
                    "NetworkManager is not running: org.freedesktop.NetworkManager was not \
                     provided by any .service files"
                        .to_owned(),
                );
            }),
        ),
    ];
    for (name, description, wizard) in states {
        still(carrier, wizard, ("Network", name, description), HEADING)?;
    }
    Ok(())
}

// ---- Disk and space ----

fn disk(carrier: &mut Carrier) -> anyhow::Result<()> {
    const HEADING: &str = "Choose a disk";
    still(
        carrier,
        on(iso_context(), DRY, Page::Disk),
        (
            "Disk",
            "Choose a disk",
            "The Windows SSD chosen; a blank drive and an MBR stick beside it",
        ),
        HEADING,
    )?;
    let mut unchosen = on(iso_context(), DRY, Page::Disk);
    unchosen.target = Target::default();
    unchosen.restage_plan();
    still(
        carrier,
        unchosen,
        (
            "Disk",
            "Nothing chosen",
            "No disk selected yet: Next is disabled",
        ),
        HEADING,
    )?;
    let mut blank = on(iso_context(), DRY, Page::Disk);
    blank.choose_disk("/dev/sda");
    still(
        carrier,
        blank,
        (
            "Disk",
            "Empty disk chosen",
            "A brand-new drive with no partition table",
        ),
        HEADING,
    )?;
    still(
        carrier,
        on(crowded_context(), DRY, Page::Disk),
        (
            "Disk",
            "Many disks",
            "More than four disks: a search field appears above the list",
        ),
        HEADING,
    )?;
    shoot(
        carrier,
        on(crowded_context(), DRY, Page::Disk),
        ("Disk", "Search", "Filtered to the NVMe drives"),
        HEADING,
        |app| {
            app.focus(role(Role::SearchField));
            app.type_text("nvme").settle();
            Ok(())
        },
    )?;
    shoot(
        carrier,
        on(crowded_context(), DRY, Page::Disk),
        ("Disk", "No match", "A search nothing matches"),
        HEADING,
        |app| {
            app.focus(role(Role::SearchField));
            app.type_text("thunderbolt").settle();
            Ok(())
        },
    )?;
    still(
        carrier,
        on(iso_context_with(Vec::new()), DRY, Page::Disk),
        (
            "Disk",
            "No disks",
            "No storage found, as when a driver is missing",
        ),
        HEADING,
    )
}

fn space(carrier: &mut Carrier) -> anyhow::Result<()> {
    const HEADING: &str = "Where should CrownOS go?";
    still(
        carrier,
        on(iso_context(), DRY, Page::Space),
        (
            "Space",
            "Free space",
            "The largest free region chosen, with the layout it gets",
        ),
        HEADING,
    )?;
    let mut blank = on(iso_context(), DRY, Page::Disk);
    blank.choose_disk("/dev/sda");
    blank.go(Page::Space);
    still(
        carrier,
        blank,
        (
            "Space",
            "Empty disk",
            "A blank drive chosen: refused until it has a partition table",
        ),
        HEADING,
    )?;
    let mut full = on(crowded_context(), DRY, Page::Disk);
    full.choose_disk("/dev/nvme1n1");
    full.go(Page::Space);
    ensure!(full.plan().is_none(), "a full disk has no plan");
    still(
        carrier,
        full,
        (
            "Space",
            "No free space",
            "A full disk: the editor is the way forward",
        ),
        HEADING,
    )
}

// ---- Partition editor ----

fn editing() -> Wizard {
    let mut wizard = on(iso_context(), DRY, Page::Space);
    wizard.open_editor();
    wizard
}

fn free_row(wizard: &Wizard) -> anyhow::Result<String> {
    let disk = wizard.target_disk().context("a target disk")?;
    let gap = disk
        .free_gaps()
        .into_iter()
        .max_by_key(|gap| gap.sectors)
        .context("free space")?;
    Ok(format!("{} unallocated", bytes(disk.bytes(gap.sectors))))
}

/// Scrolls the editor's row list to its end, where free space and queued partitions sit.
fn show_free_rows(app: &mut capture::CaptureApp) {
    scroll_by(
        app,
        role(Role::RadioButton).name("/dev/nvme0n1p1"),
        LIST_END,
    );
}

/// Further than the editor's row list reaches.
const LIST_END: f32 = 600.0;

fn choose(app: &mut capture::CaptureApp, row: &str) {
    app.click(role(Role::RadioButton).name(row)).settle();
}

fn partition_editor(carrier: &mut Carrier) -> anyhow::Result<()> {
    const HEADING: &str = "Partition editor";
    const GROUP: &str = "Partition editor";
    let free = free_row(&editing())?;
    still(
        carrier,
        editing(),
        (GROUP, "Editor", "The target disk's table, nothing selected"),
        HEADING,
    )?;
    shoot(
        carrier,
        editing(),
        (
            GROUP,
            "Partition selected",
            "Resize, Format and Delete apply to it",
        ),
        HEADING,
        |app| {
            choose(app, "/dev/nvme0n1p3");
            Ok(())
        },
    )?;
    shoot(
        carrier,
        editing(),
        (GROUP, "Free space selected", "New applies to it"),
        HEADING,
        |app| {
            choose(app, &free);
            show_free_rows(app);
            Ok(())
        },
    )?;
    shoot(
        carrier,
        editing(),
        (
            GROUP,
            "Change queued",
            "A new 120 GiB partition, queued and not written",
        ),
        HEADING,
        |app| {
            choose(app, &free);
            app.focus(role(Role::TextField).name("Size"));
            app.type_text("120").settle();
            press(app, "New");
            show_free_rows(app);
            scroll_page_to(app, "1 change queued — nothing written yet");
            Ok(())
        },
    )?;
    shoot(
        carrier,
        editing(),
        (
            GROUP,
            "Changes queued",
            "A new partition and a deleted one, both queued",
        ),
        HEADING,
        |app| {
            choose(app, &free);
            app.focus(role(Role::TextField).name("Size"));
            app.type_text("120").settle();
            press(app, "New");
            choose(app, "/dev/nvme0n1p4");
            press(app, "Delete");
            show_free_rows(app);
            scroll_page_to(app, "2 changes queued — nothing written yet");
            Ok(())
        },
    )?;
    shoot(
        carrier,
        editing(),
        (
            GROUP,
            "Change refused",
            "Resize pressed without a size: the change is not queued",
        ),
        HEADING,
        |app| {
            choose(app, "/dev/nvme0n1p3");
            press(app, "Resize");
            scroll_page_to(app, "That change was not queued");
            Ok(())
        },
    )?;
    shoot(
        carrier,
        editing(),
        (
            GROUP,
            "Partition type menu",
            "The As dropdown open over the strip",
        ),
        HEADING,
        |app| {
            choose(app, &free);
            show_free_rows(app);
            click_nth(app, role(Role::ComboBox), 0)
        },
    )?;
    shoot(
        carrier,
        editing(),
        (
            GROUP,
            "Format menu",
            "The filesystem dropdown open over the strip",
        ),
        HEADING,
        |app| {
            choose(app, &free);
            show_free_rows(app);
            click_nth(app, role(Role::ComboBox), 1)
        },
    )?;
    let mut from_welcome = Wizard::ready(windows_context(false), DRY);
    from_welcome.open_editor();
    still(
        carrier,
        from_welcome,
        (
            GROUP,
            "Editor from Welcome",
            "The Windows flow's editor, over the laptop the payload names",
        ),
        HEADING,
    )
}

// ---- Account and apps ----

fn account(carrier: &mut Carrier) -> anyhow::Result<()> {
    const HEADING: &str = "Let's get you started";
    still(
        carrier,
        on(iso_context(), DRY, Page::Account),
        (
            "Account",
            "Create your account",
            "Defaults filled in, no password yet",
        ),
        HEADING,
    )?;
    let mut filled = on(iso_context(), DRY, Page::Account);
    sign_in(&mut filled);
    still(
        carrier,
        filled,
        ("Account", "Filled in", "Every field valid, Next enabled"),
        HEADING,
    )?;
    let mut incomplete = on(iso_context(), DRY, Page::Account);
    incomplete.form.full_name = "Maya Lindqvist".to_owned();
    incomplete.submit_account();
    shoot(
        carrier,
        incomplete,
        (
            "Account",
            "Not yet",
            "Submitted without a password: what is missing",
        ),
        HEADING,
        |app| {
            scroll_page_to(app, "Not yet");
            Ok(())
        },
    )?;
    let mut mismatch = on(iso_context(), DRY, Page::Account);
    sign_in(&mut mismatch);
    mismatch.form.confirm = "lantern-orchid-42".to_owned();
    mismatch.submit_account();
    shoot(
        carrier,
        mismatch,
        (
            "Account",
            "Passwords differ",
            "The repeated password does not match",
        ),
        HEADING,
        |app| {
            scroll_page_to(app, "Not yet");
            Ok(())
        },
    )?;
    let auto = Wizard::ready(windows_context(true), DRY);
    ensure!(auto.page == Page::Account);
    still(
        carrier,
        auto,
        (
            "Account",
            "Password only",
            "An auto-install payload: only the password is asked",
        ),
        HEADING,
    )
}

fn apps(carrier: &mut Carrier) -> anyhow::Result<()> {
    const HEADING: &str = "Apps from other systems";
    let mut off = on(iso_context(), DRY, Page::Apps);
    off.set_windows_apps(WindowsApps::Disabled);
    still(
        carrier,
        off,
        ("Apps", "Windows apps off", "Wine is not set up"),
        HEADING,
    )?;
    let mut enabled = on(iso_context(), DRY, Page::Apps);
    enabled.set_windows_apps(WindowsApps::Enabled);
    still(
        carrier,
        enabled,
        (
            "Apps",
            "Windows apps on",
            "Wine sets itself up on the first online boot",
        ),
        HEADING,
    )
}

// ---- Confirm, install, done ----

fn confirming(context: crate::ui::Context, mode: Mode) -> anyhow::Result<Wizard> {
    let mut wizard = on(context, mode, Page::Account);
    sign_in(&mut wizard);
    wizard.go(Page::Confirm);
    ensure!(wizard.plan().is_some(), "{:?}", wizard.refusal());
    Ok(wizard)
}

const NO_RETURN: &str = "This is the point of no return";
/// Far enough to bring the confirmation's step list to the top of the card.
const STEPS_SCROLL: f32 = 250.0;

fn confirm(carrier: &mut Carrier) -> anyhow::Result<()> {
    const HEADING: &str = "Ready to install";
    still(
        carrier,
        confirming(windows_context(false), Mode::Commit)?,
        (
            "Confirm",
            "Ready to install",
            "Beside Windows: the point of no return and every step",
        ),
        HEADING,
    )?;
    let mut with_ai = confirming(windows_context(false), Mode::Commit)?;
    with_ai.set_ai_features(AiFeatures::Enabled);
    still(
        carrier,
        with_ai,
        (
            "Confirm",
            "AI features on",
            "AI acceleration adds its own step",
        ),
        HEADING,
    )?;
    still(
        carrier,
        confirming(iso_context(), Mode::Commit)?,
        (
            "Confirm",
            "Ready to install (image)",
            "A flashed image into free space on the desktop SSD",
        ),
        HEADING,
    )?;
    for (name, description, dy) in [
        (
            "Ready to install (steps)",
            "Scrolled to the install steps, in order",
            STEPS_SCROLL,
        ),
        (
            "Ready to install (end)",
            "Scrolled to the end: what happens to the staging partition",
            PAGE_END,
        ),
    ] {
        shoot(
            carrier,
            confirming(windows_context(false), Mode::Commit)?,
            ("Confirm", name, description),
            HEADING,
            |app| {
                scroll_by(app, role(Role::Group).name(NO_RETURN), dy);
                Ok(())
            },
        )?;
    }
    still(
        carrier,
        confirming(windows_context(false), DRY)?,
        (
            "Confirm",
            "Dry run",
            "Started with --dry-run: nothing will be written",
        ),
        HEADING,
    )
}

fn committed(context: crate::ui::Context) -> anyhow::Result<Wizard> {
    let mut wizard = confirming(context, DRY)?;
    wizard.begin_install();
    ensure!(wizard.page == Page::Progress);
    Ok(wizard)
}

fn step_labels(wizard: &Wizard) -> Vec<String> {
    wizard.committed.as_ref().map_or_else(Vec::new, |plan| {
        plan.steps.iter().map(|step| step.label()).collect()
    })
}

/// Runs the steps before `label` to completion and starts it, logging `log` under it.
fn run_until(wizard: &mut Wizard, label: &str, log: &[&str]) -> anyhow::Result<usize> {
    let labels = step_labels(wizard);
    let total = labels.len();
    let current = labels
        .iter()
        .position(|step| step == label)
        .with_context(|| format!("no `{label}` step in {labels:?}"))?;
    for (index, step) in labels.iter().enumerate().take(current + 1) {
        wizard.on_install_event(Event::StepStarted {
            index,
            total,
            label: step.clone(),
        });
        if index < current {
            wizard.on_install_event(Event::StepFinished { index });
        }
    }
    for line in log {
        wizard.on_install_event(Event::Log((*line).to_owned()));
    }
    Ok(current)
}

fn progress(carrier: &mut Carrier) -> anyhow::Result<()> {
    const HEADING: &str = "Installing CrownOS";
    still(
        carrier,
        committed(windows_context(false))?,
        (
            "Install",
            "Starting",
            "Committed: the worker has not reported yet",
        ),
        HEADING,
    )?;
    let mut packages = committed(windows_context(false))?;
    run_until(
        &mut packages,
        "Installing packages",
        &[
            ":: Synchronizing package databases...",
            "(214/873) installing linux-firmware",
        ],
    )?;
    still(
        carrier,
        packages,
        (
            "Install",
            "Installing packages",
            "Partitioned and mounted, packages going in",
        ),
        HEADING,
    )?;
    let mut nearly = committed(windows_context(false))?;
    let last = step_labels(&nearly)
        .last()
        .cloned()
        .context("a plan with steps")?;
    run_until(&mut nearly, &last, &["umount -R /mnt"])?;
    still(
        carrier,
        nearly,
        ("Install", "Last step", "The final step running"),
        HEADING,
    )?;
    still(
        carrier,
        failed_install()?,
        (
            "Install",
            "Install failed",
            "A step failed: why, and Restart",
        ),
        HEADING,
    )?;
    shoot(
        carrier,
        failed_install()?,
        (
            "Install",
            "Install failed (log)",
            "Scrolled to the log the failed step left",
        ),
        HEADING,
        |app| {
            scroll_page_to(app, STOPPED);
            Ok(())
        },
    )
}

const STOPPED: &str = "The install stopped";

/// The boot loader step failing on firmware with no room for another entry.
fn failed_install() -> anyhow::Result<Wizard> {
    let mut wizard = committed(windows_context(false))?;
    let index = run_until(
        &mut wizard,
        "Installing the boot loader",
        &[
            "arch-chroot /mnt install -Dm644 /usr/share/refind/refind_x64.efi /efi/EFI/refind/refind_x64.efi",
            "efibootmgr --create --disk /dev/nvme0n1 --part 1 --label CrownOS --loader \\EFI\\refind\\refind_x64.efi",
            "Could not prepare Boot variable: No space left on device",
        ],
    )?;
    wizard.on_install_event(Event::Failed {
        index,
        message:
            "efibootmgr exited with status 5: the firmware has no room for another boot entry."
                .to_owned(),
    });
    Ok(wizard)
}

fn done(context: crate::ui::Context, dry_run: bool) -> anyhow::Result<Wizard> {
    let mut wizard = committed(context)?;
    wizard.on_install_event(Event::Finished { dry_run });
    ensure!(wizard.page == Page::Done);
    Ok(wizard)
}

fn finished(carrier: &mut Carrier) -> anyhow::Result<()> {
    still(
        carrier,
        done(windows_context(false), false)?,
        (
            "Done",
            "You're all set",
            "Installed beside Windows; the boot menu offers both",
        ),
        "You're all set",
    )?;
    still(
        carrier,
        done(iso_context(), false)?,
        (
            "Done",
            "You're all set (image)",
            "Installed from a flashed image: remove the media",
        ),
        "You're all set",
    )?;
    still(
        carrier,
        done(windows_context(false), true)?,
        (
            "Done",
            "Dry run finished",
            "A rehearsal finished; nothing was written",
        ),
        "Dry run finished",
    )?;
    let mut closing = done(windows_context(false), false)?;
    closing.close();
    still(
        carrier,
        closing,
        (
            "Done",
            "Restarting",
            "Restart pressed: the card has sunk away over the backdrop",
        ),
        "You're all set",
    )
}

// ---- Blocked ----

fn blocked(carrier: &mut Carrier) -> anyhow::Result<()> {
    const HEADING: &str = "Nothing has been changed";
    still(
        carrier,
        Wizard::blocked(
            Blocker {
                title: "The disks could not be read".to_owned(),
                detail: "could not run `lsblk`: No such file or directory (os error 2)\n\n\
                         Nothing has been changed. This usually means a tool the installer \
                         needs is missing from the live environment."
                    .to_owned(),
                notes: vec![
                    "No handoff file on the kernel command line, so this is a flashed image."
                        .to_owned(),
                ],
            },
            DRY,
        ),
        (
            "Blocked",
            "Disks unreadable",
            "Startup failed before the first page",
        ),
        HEADING,
    )?;
    let refused = Wizard::ready(windows_context_shifted(false, 64 * 1024 * 1024 * 1024), DRY);
    ensure!(refused.page == Page::Blocked);
    still(
        carrier,
        refused,
        (
            "Blocked",
            "Payload refused",
            "The disk is not as Windows left it, so the handoff is refused",
        ),
        HEADING,
    )?;
    let mut stick = on(iso_context(), DRY, Page::Disk);
    stick.choose_disk("/dev/sdb");
    stick.go(Page::Blocked);
    still(
        carrier,
        stick,
        (
            "Blocked",
            "Cannot use this disk",
            "A flashed image refused on one disk can choose another",
        ),
        HEADING,
    )
}
