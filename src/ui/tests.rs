//! The wizard's state machine, driven without rendering a widget.
//!
//! These tests are about what happens to the disk — which page comes next, what
//! gates a step, which planner a provenance selects, and whether a queued
//! partition edit reaches the plan. The views are exercised by running the
//! binary; see `CLAUDE.md`.

use super::*;
use crate::engine::disks::DiskOp;
use crate::engine::probe::{self, fixture};
use crate::oobe::{
    Install, Locale, RootFilesystem, Staging, StagingKind, SwapPlan, Target as OobeTarget,
    UserAccount,
};

/// The standard fixture machine, and a payload that matches it.
fn payload_context(user: Option<UserAccount>, auto_install: bool) -> Context {
    let snapshot =
        probe::probe(&fixture::machine(fixture::FINDMNT_JSON_IN_RAM)).expect("probe");
    let disk = &snapshot.disks[0];
    let gap = *disk.free_gaps().last().expect("a gap");

    let oobe = Oobe {
        schema_version: crate::oobe::SCHEMA_VERSION,
        generated_by: "ui test".to_owned(),
        generated_at_unix: 0,
        install: Install {
            auto_install,
            allocated_bytes: disk.bytes(gap.sectors),
            swap: SwapPlan::None,
            root_filesystem: RootFilesystem::Ext4,
            keep_windows: true,
            distro: crate::oobe::Distro::Arch,
        },
        target: OobeTarget {
            esp_partuuid: "0fc63daf-8483-4772-8e79-3d69d8477de4".to_owned(),
            esp_size_bytes: 260 * 1024 * 1024,
            windows_partuuid: "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7".to_owned(),
            disk_guid: None,
            windows_disk_number: 0,
            expected_free_start_bytes: disk.bytes(gap.start_sector),
            expected_free_bytes: disk.bytes(gap.sectors),
            logical_sector_size: 512,
        },
        staging: Staging {
            kind: StagingKind::Partition,
            partuuid: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_owned(),
            base_dir: "EFI/ArchStage".to_owned(),
            reclaim_after_install: false,
        },
        user,
        locale: Locale::default(),
    };

    Context {
        provenance: Provenance::WindowsInstaller {
            oobe: Box::new(oobe),
            source: "a fixture".to_owned(),
        },
        notes: Vec::new(),
        snapshot,
        ids: test_ids(),
        auto_install,
    }
}

/// The same machine, reached by flashing the image instead.
fn iso_context() -> Context {
    let snapshot =
        probe::probe(&fixture::machine(fixture::FINDMNT_JSON_IN_RAM)).expect("probe");
    Context {
        provenance: Provenance::Iso,
        notes: vec!["a fixture".to_owned()],
        snapshot,
        ids: test_ids(),
        auto_install: false,
    }
}

fn test_ids() -> NewIds {
    NewIds {
        root_partuuid: "12345678-1234-1234-1234-123456789abc".to_owned(),
        swap_partuuid: "87654321-4321-4321-4321-cba987654321".to_owned(),
    }
}

fn prefilled() -> UserAccount {
    UserAccount {
        username: "ada".to_owned(),
        full_name: Some("Ada Lovelace".to_owned()),
        hostname: "analytical".to_owned(),
        autologin: true,
    }
}

/// Fill in a valid account, as the account page would.
fn sign_in(app: &mut App) {
    app.form.password = "correct-horse".to_owned();
    app.form.confirm = "correct-horse".to_owned();
    app.restage_plan();
}

// ---- The two flows ----

/// The whole point of the provenance split: the same binary, the same disk, two
/// different first pages and two different step counts.
#[test]
fn the_provenance_decides_which_flow_the_wizard_runs() {
    let windows = App::ready(payload_context(None, false), Mode::Commit);
    assert_eq!(windows.page, Page::Welcome);
    assert_eq!(Page::labels(windows.provenance()), WINDOWS_STEPS);

    let iso = App::ready(iso_context(), Mode::Commit);
    assert_eq!(iso.page, Page::Disk);
    assert_eq!(Page::labels(iso.provenance()), ISO_STEPS);
}

/// A payload-driven install has a plan on the first frame, because every
/// decision was already made.
#[test]
fn a_payload_driven_install_has_a_plan_before_the_user_does_anything() {
    let app = App::ready(payload_context(None, false), Mode::Commit);
    let plan = app.plan().expect("a plan");
    assert_eq!(plan.disk, "/dev/sda");
    assert!(plan.windows.is_some(), "the payload names an OS to keep");
    assert!(plan.authorized_destructive.is_empty());
    // No payload account, so the form carries the defaults — which is what lets
    // the plan exist on the first frame at all.
    assert_eq!(app.form.username, "crown");
    assert_eq!(app.form.hostname, "crownos");
}

/// And so does an ISO install, because `Target::best_for` picked a disk. That
/// matters: the first frame of the disk page has something selected, and the
/// step is immediately passable.
#[test]
fn an_iso_install_opens_with_a_usable_default_target() {
    let app = App::ready(iso_context(), Mode::Commit);
    assert_eq!(app.target.disk.as_deref(), Some("/dev/sda"));
    assert_eq!(app.target.esp.as_deref(), Some("/dev/sda1"));
    assert!(app.target.region_start_sector.is_some());

    let plan = app.plan().expect("a plan");
    assert_eq!(plan.disk, "/dev/sda");
    assert!(
        plan.windows.is_none(),
        "an ISO install designates nothing to preserve; see plan_iso"
    );
}

/// The base picked on the Space page is the base the plan installs.
#[test]
fn choosing_a_base_replans_for_it() {
    let mut app = App::ready(iso_context(), Mode::Commit);
    assert_eq!(app.plan().expect("a plan").distro, Distro::Arch);

    app.target.distro = Distro::Fedora;
    app.restage_plan();
    assert_eq!(app.plan().expect("a plan").distro, Distro::Fedora);
}

/// A payload's base reaches the plan without the Linux side asking again.
#[test]
fn a_payload_chooses_the_base() {
    let mut context = payload_context(None, false);
    if let Provenance::WindowsInstaller { oobe, .. } = &mut context.provenance {
        oobe.install.distro = Distro::NixOs;
    }
    let app = App::ready(context, Mode::Commit);
    assert_eq!(app.target.distro, Distro::NixOs);
    assert_eq!(app.plan().expect("a plan").distro, Distro::NixOs);
}

/// An ISO install onto a disk with nothing to install into is refused, and — the
/// part that matters — refused *on a page the user can act from* rather than by
/// blocking the whole wizard the way a payload refusal does.
#[test]
fn an_iso_refusal_does_not_block_the_wizard() {
    let mut context = iso_context();
    context.snapshot.disks[0].partitions.clear();
    // A disk with no partitions has no ESP, so there is nothing to boot from —
    // and `Target::best_for` therefore refuses to guess it, leaving no target.
    let mut app = App::ready(context, Mode::Commit);
    assert_eq!(app.page, Page::Disk, "the user can still pick another disk");
    assert!(app.context.is_ok(), "an ISO refusal must not block the wizard");
    assert!(app.plan().is_none());
    assert!(matches!(app.refusal(), Some(Refusal::NoTargetDisk { .. })));

    // Choosing it anyway — which the disk page allows, because a GPT disk with
    // no ESP is one partition editor away from being installable — names the
    // actual problem.
    let disk = app
        .context()
        .expect("context")
        .snapshot
        .disks[0]
        .clone();
    app.target.retarget(&disk);
    app.restage_plan();
    assert!(
        matches!(app.refusal(), Some(Refusal::NoEspOnDisk { .. })),
        "{:?}",
        app.refusal()
    );
}

/// A payload refusal is the opposite: knowable before the user does anything, so
/// reported before the user does anything.
#[test]
fn a_refused_payload_blocks_the_wizard_at_startup() {
    let mut context = payload_context(None, false);
    context.target_free_start_off_by(64 * 1024 * 1024 * 1024);
    let app = App::ready(context, Mode::Commit);
    assert_eq!(app.page, Page::Blocked);
    match &app.context {
        Err(blocker) => assert!(
            blocker.detail.contains("not in the state we left it in"),
            "{}",
            blocker.detail
        ),
        Ok(_) => panic!("the wizard should be blocked"),
    }
}

// ---- Navigation ----

#[test]
fn each_flow_walks_its_own_pages() {
    let mut iso = App::ready(iso_context(), Mode::Commit);
    assert_eq!(iso.next_page(), Some(Page::Space));
    iso.go(Page::Space);
    assert_eq!(iso.next_page(), Some(Page::Account));
    assert_eq!(iso.previous_page(), Some(Page::Disk));

    let mut windows = App::ready(payload_context(None, false), Mode::Commit);
    assert_eq!(windows.next_page(), Some(Page::Account));
    windows.go(Page::Account);
    assert_eq!(
        windows.previous_page(),
        Some(Page::Welcome),
        "the Windows flow has no Space page to go back to"
    );
}

/// The editor is not progress: leaving it returns to the step it interrupted,
/// and the step bar does not move while it is open.
#[test]
fn the_editor_returns_to_the_step_it_interrupted() {
    for mut app in [
        App::ready(iso_context(), Mode::Commit),
        App::ready(payload_context(None, false), Mode::Commit),
    ] {
        let iso = app.provenance().is_iso();
        let from = if iso { Page::Space } else { Page::Welcome };
        app.go(from);
        let before = app.step_indicator();

        app.open_editor();
        assert_eq!(app.page, Page::Partitions);
        assert_eq!(
            app.step_indicator(),
            before,
            "opening the editor must not move the step bar"
        );
        assert_eq!(app.previous_page(), Some(from));
        assert_eq!(app.next_page(), Some(from), "the editor is not progress");

        app.retreat();
        assert_eq!(app.page, from);
        assert_eq!(app.editor_return, None, "leaving the editor forgets the way back");
    }
}

/// Every page maps to a step that exists in its own flow.
#[test]
fn every_page_maps_to_a_step_in_both_flows() {
    for provenance in [
        Provenance::Iso,
        Provenance::WindowsInstaller {
            oobe: Box::new(payload_context(None, false).oobe().expect("payload").clone()),
            source: String::new(),
        },
    ] {
        for page in [
            Page::Welcome,
            Page::Disk,
            Page::Space,
            Page::Partitions,
            Page::Account,
            Page::Confirm,
            Page::Progress,
            Page::Done,
            Page::Blocked,
        ] {
            let (total, index) = page.step(&provenance);
            assert!(index < total, "{page:?} in {provenance:?}: {index} of {total}");
        }
    }
}

// ---- The account page ----

/// `auto_install` skips Welcome and stops at the account page, because the
/// password is the one thing the payload can never carry.
#[test]
fn an_unattended_install_still_stops_for_a_password() {
    let app = App::ready(payload_context(Some(prefilled()), true), Mode::Commit);
    assert_eq!(app.page, Page::Account);
    assert_eq!(app.form.username, "ada");
    assert_eq!(app.form.full_name, "Ada Lovelace");
    assert_eq!(app.form.hostname, "analytical");
    assert!(app.form.password.is_empty());
    assert_eq!(
        app.form.problem().as_deref(),
        Some("Choose a password."),
        "an unattended install must not be able to leave this page yet"
    );
}

/// And a flashed image never skips anything, because nothing has been answered.
#[test]
fn an_iso_install_never_skips_a_step_even_if_auto_install_was_asked_for() {
    let mut context = iso_context();
    context.auto_install = true;
    let app = App::ready(context, Mode::Commit);
    assert_eq!(app.page, Page::Disk);
}

#[test]
fn the_account_page_will_not_be_left_without_a_matching_password() {
    let mut app = App::ready(payload_context(None, false), Mode::Commit);
    app.go(Page::Account);

    app.form.password = "hunter2".to_owned();
    assert_eq!(
        app.form.problem().as_deref(),
        Some("The two passwords do not match.")
    );
    app.form.confirm = "hunter2".to_owned();
    assert_eq!(app.form.problem(), None);
}

#[test]
fn an_empty_username_or_hostname_blocks_the_account_page() {
    let mut app = App::ready(payload_context(None, false), Mode::Commit);
    app.form.password = "x".to_owned();
    app.form.confirm = "x".to_owned();

    app.form.username = "   ".to_owned();
    assert!(app.form.problem().is_some());
    app.form.username = "crown".to_owned();
    app.form.hostname = String::new();
    assert!(app.form.problem().is_some());
}

// ---- The partition editor's effect on the plan ----

/// The feature the editor exists for, end to end: a queued delete reaches the
/// plan as a step, and shows up in the authorization list that lets the guard
/// permit it.
#[test]
fn a_queued_edit_becomes_a_step_and_authorizes_itself() {
    let mut app = App::ready(iso_context(), Mode::Commit);
    sign_in(&mut app);
    let disk = app.target_disk().cloned().expect("a disk");
    let snapshot = app.context().expect("context").snapshot.clone();

    // Delete `/dev/sda4`, the staging partition, which is not mounted in the
    // in-RAM fixture.
    let rows = app.editor.rows(&disk, &snapshot);
    let index = rows
        .iter()
        .position(|row| matches!(row, super::editor::Row::Partition { number: 4, .. }))
        .expect("sda4");
    app.editor.selected = Some(index);
    app.editor.delete(&disk, &snapshot);
    app.restage_plan();

    let plan = app.plan().expect("a plan with an edit queued");
    assert!(
        plan.steps.iter().any(|step| matches!(
            step,
            crate::engine::plan::Step::EditPartitionTable(DiskOp::Delete { path, .. })
                if path == "/dev/sda4"
        )),
        "the delete should be a step: {:?}",
        plan.steps.iter().map(|s| s.label()).collect::<Vec<_>>()
    );
    assert!(
        plan.authorized_destructive.contains(&"/dev/sda4".to_owned()),
        "and it should be authorized: {:?}",
        plan.authorized_destructive
    );
    assert!(
        plan.steps
            .iter()
            .position(|step| matches!(
                step,
                crate::engine::plan::Step::EditPartitionTable(_)
            ))
            .is_some_and(|edit| edit == 0),
        "edits run first — everything else was laid out against the table they leave"
    );
}

/// The editor is reachable from the Windows flow too, and its edits reach that
/// flow's plan the same way. Except they do not: the payload-driven planner is
/// deliberately not given the queue, because its whole job is to check the disk
/// against a description written before the reboot, and an edit would invalidate
/// that description. The editor there is for making room and starting again.
#[test]
fn the_payload_planner_ignores_queued_edits_by_construction() {
    let mut app = App::ready(payload_context(None, false), Mode::Commit);
    let disk = app.target_disk().cloned().expect("a disk");
    let snapshot = app.context().expect("context").snapshot.clone();

    let rows = app.editor.rows(&disk, &snapshot);
    let index = rows
        .iter()
        .position(|row| matches!(row, super::editor::Row::Partition { number: 4, .. }))
        .expect("sda4");
    app.editor.selected = Some(index);
    app.editor.delete(&disk, &snapshot);
    app.restage_plan();

    let plan = app.plan().expect("a plan");
    assert!(
        !plan
            .steps
            .iter()
            .any(|step| matches!(step, crate::engine::plan::Step::EditPartitionTable(_))),
        "a payload-driven plan is built from the payload and nothing else"
    );
    assert!(plan.authorized_destructive.is_empty());
}

/// A queued delete usually merges the space it frees into the region next to
/// it, which moves that region's first sector — and the region is identified by
/// its first sector. Without re-resolving, the plan would refuse with
/// `ChosenRegionNotFree` immediately after the user did exactly what the editor
/// invited them to do.
#[test]
fn an_edit_that_moves_the_chosen_region_moves_the_selection_with_it() {
    let mut app = App::ready(iso_context(), Mode::Commit);
    sign_in(&mut app);
    let before = app.target.region_start_sector.expect("a region");
    assert_eq!(before, 802_129_920);

    let disk = app.target_disk().cloned().expect("a disk");
    let snapshot = app.context().expect("context").snapshot.clone();
    let index = app
        .editor
        .rows(&disk, &snapshot)
        .iter()
        .position(|row| matches!(row, super::editor::Row::Partition { number: 4, .. }))
        .expect("sda4");
    app.editor.selected = Some(index);
    app.editor.delete(&disk, &snapshot);
    app.restage_plan();

    assert_eq!(
        app.target.region_start_sector,
        Some(800_032_768),
        "the selection should have followed the merged region"
    );
    assert!(
        app.plan().is_some(),
        "and the plan should still build: {:?}",
        app.refusal()
    );
}

/// A region that is *still* free after an edit is left alone. Re-resolving
/// unconditionally would quietly move a user who picked a small region onto the
/// largest one.
#[test]
fn an_edit_elsewhere_leaves_the_chosen_region_alone() {
    let mut app = App::ready(iso_context(), Mode::Commit);
    sign_in(&mut app);
    let before = app.target.region_start_sector.expect("a region");

    let disk = app.target_disk().cloned().expect("a disk");
    let snapshot = app.context().expect("context").snapshot.clone();
    // Reformat the ESP: destructive, queued, and it moves no boundary at all.
    assert!(app.editor.try_push(
        &disk,
        &snapshot,
        DiskOp::Format {
            path: "/dev/sda1".to_owned(),
            filesystem: crate::engine::disks::Filesystem::Fat32,
            fs_label: None,
        },
    ));
    app.restage_plan();

    assert_eq!(app.target.region_start_sector, Some(before));
}

/// Changing disk throws the queue away. Every operation in it names sectors on
/// the disk it was queued against, and carrying them over would apply an offset
/// from one device to another.
#[test]
fn retargeting_a_different_disk_clears_the_queue() {
    let mut app = App::ready(iso_context(), Mode::Commit);
    let disk = app.target_disk().cloned().expect("a disk");
    let snapshot = app.context().expect("context").snapshot.clone();
    let index = app
        .editor
        .rows(&disk, &snapshot)
        .iter()
        .position(super::editor::Row::is_free)
        .expect("free space");
    app.editor.selected = Some(index);
    app.editor.size_text = "20".to_owned();
    app.editor.create(&disk, &snapshot);
    assert_eq!(app.editor.ops.len(), 1);

    // The UI's disk-page callback is what clears it; the condition it uses is
    // asserted here directly, because there is one disk in the fixture.
    app.editor.clear();
    assert!(app.editor.ops.is_empty());
}

// ---- Committing and running ----

/// The payload's autologin flag reaches the plan, and the password does not
/// reach anything that gets logged.
#[test]
fn committing_freezes_the_plan_and_the_password_separately() {
    let mut app = App::ready(payload_context(Some(prefilled()), true), Mode::Commit);
    sign_in(&mut app);

    let planned = app.plan().expect("a plan").clone();
    assert_eq!(planned.account.username, "ada");
    assert!(planned.account.autologin, "the payload asked for autologin");
    assert!(
        !format!("{planned:?}").contains("correct-horse"),
        "the plan must not carry the password"
    );

    app.begin_install();
    assert_eq!(app.page, Page::Progress);
    assert!(app.run.started);
    let committed = app.committed.as_ref().expect("a committed plan");
    assert_eq!(committed.steps, planned.steps);
    assert_eq!(
        app.secrets.as_ref().map(|s| s.user_password.as_str()),
        Some("correct-horse")
    );
}

#[test]
fn progress_events_drive_the_bar_and_land_on_the_done_page() {
    let mut app = App::ready(payload_context(None, false), Mode::DryRun);
    sign_in(&mut app);
    app.begin_install();

    app.on_install_event(Event::StepStarted {
        index: 0,
        total: 4,
        label: "Creating the CrownOS partition".to_owned(),
    });
    assert_eq!(app.run.fraction(), 0.0);
    assert_eq!(app.run.label, "Creating the CrownOS partition");

    app.on_install_event(Event::StepFinished { index: 0 });
    assert!((app.run.fraction() - 0.25).abs() < f64::EPSILON);

    app.on_install_event(Event::Log("$ sfdisk --append /dev/sda".to_owned()));
    assert_eq!(app.run.log.len(), 1);

    app.on_install_event(Event::Finished { dry_run: true });
    assert_eq!(app.page, Page::Done);
    assert!(app.run.finished);
    assert!(app.run.was_dry_run);
    assert_eq!(app.run.fraction(), 1.0);
}

#[test]
fn a_failure_stays_on_the_progress_page_and_keeps_the_message() {
    let mut app = App::ready(payload_context(None, false), Mode::Commit);
    sign_in(&mut app);
    app.begin_install();

    app.on_install_event(Event::Failed {
        index: 3,
        message: "mkfs.ext4 exited with 1".to_owned(),
    });
    assert_eq!(
        app.page,
        Page::Progress,
        "a failure must not look like success"
    );
    assert_eq!(app.run.failure.as_deref(), Some("mkfs.ext4 exited with 1"));
    assert!(app.run.finished);
}

/// A `pacstrap` produces thousands of lines and each one would be a live widget.
#[test]
fn the_log_keeps_only_its_tail() {
    let mut app = App::ready(payload_context(None, false), Mode::DryRun);
    for index in 0..LOG_LIMIT + 50 {
        app.on_install_event(Event::Log(format!("line {index}")));
    }
    assert_eq!(app.run.log.len(), LOG_LIMIT);
    assert_eq!(
        app.run.log.last().map(String::as_str),
        Some(format!("line {}", LOG_LIMIT + 49).as_str())
    );
    assert_eq!(app.run.log.first().map(String::as_str), Some("line 50"));
}

#[test]
fn a_startup_failure_blocks_the_wizard() {
    let app = App::blocked(
        Blocker {
            title: "nope".to_owned(),
            detail: "because".to_owned(),
            notes: vec!["a note".to_owned()],
        },
        Mode::Commit,
    );
    assert_eq!(app.page, Page::Blocked);
    assert!(app.plan().is_none());
}

impl Context {
    /// Test-only: shift the payload's idea of where the free region is, to
    /// exercise the "the disk is not in the state we left it in" refusal.
    fn target_free_start_off_by(&mut self, bytes: u64) {
        if let Provenance::WindowsInstaller { oobe, .. } = &mut self.provenance {
            oobe.target.expected_free_start_bytes += bytes;
        }
    }
}
