//! The wizard's pages.
//!
//! One function per page, each returning the three things
//! [`hero_wizard`](crownuikit::layouts::hero_wizard::hero_wizard) needs: a
//! heading, the page's own content, and the action row at the foot of the card.
//! [`page`] assembles them into the card and is the only function here that
//! knows what the frame looks like.
//!
//! # Why the shapes repeat
//!
//! Almost every page's content is a [`choice_list`] — disks, free regions,
//! partitions, filesystems. That is not laziness: the card's right column is
//! [`COLUMN_WIDTH`] wide and about four rows tall, and "pick one of these,
//! here is what each one is" is the only interaction that fits in it without
//! scrolling. Where a page genuinely has more than that to say — the partition
//! editor's list, the confirmation's step list, the install log — it wraps its
//! own content in a [`portal`], which the frame deliberately does not do for it.
//!
//! # The action row
//!
//! One [`button`] per page, stretched across the column, is the mockup's shape
//! and the right one for a linear flow: there is one thing to do next. Pages
//! that also need a way back put a secondary button beside it and let the
//! primary take the remaining width, so the primary is still visibly the
//! answer.

use crownuikit::config::theme;
use crownuikit::layouts::hero_wizard::{hero_pane, hero_wizard, HeroStep, COLUMN_WIDTH};
use crownuikit::widgets::{
    button, callout, choice_list, divider, icon, pane, progress, radio_group, search,
    segmented_bar, status, text_input, ButtonSize, ButtonVariant, Choice, Radii,
    Segment, Tone,
};
use xilem::masonry::properties::types::AsUnit;
use xilem::masonry::properties::Padding;
use xilem::style::Style;
use xilem::view::{
    flex_col, flex_row, label, portal, prose, sized_box, CrossAxisAlignment, FlexExt, FlexSpacer,
};
use xilem::WidgetView;

use crate::engine::plan::{bytes, Plan, Reclaim};
use crate::oobe::{Distro, RootFilesystem};

use super::editor::{Row, FILESYSTEMS, KINDS};
use super::{esp_of, App, Boxed, Page};

/// Gap between the blocks that make up a page's content.
const BLOCK_GAP: f64 = 14.0;
/// Gap between rows inside a block.
const ROW_GAP: f64 = 8.0;
/// Gap between the two buttons of a two-button action row.
const ACTION_GAP: f64 = 10.0;
/// Inner padding of a panel inside the column.
const PANEL_PADDING: f64 = 14.0;
/// Height of the disk allocation bar.
const BAR_HEIGHT: f64 = 14.0;
/// Width of the label column in a key/value list, so the values line up.
const KEY_WIDTH: f64 = 148.0;
/// Width of the size field in the partition editor.
const SIZE_FIELD_WIDTH: f64 = 116.0;
/// How much of the column the editor's list may take before it scrolls.
const EDITOR_LIST_HEIGHT: f64 = 210.0;
/// Height the install log gets.
const LOG_HEIGHT: f64 = 176.0;

/// The current page, as a whole card.
pub fn page(app: &mut App) -> impl WidgetView<App> + use<> {
    let (step, content, action) = match app.page {
        Page::Welcome => welcome(app),
        Page::Disk => disk(app),
        Page::Space => space(app),
        Page::Partitions => partitions(app),
        Page::Account => account(app),
        Page::Confirm => confirm(app),
        Page::Progress => running(app),
        Page::Done => done(app),
        Page::Blocked => blocked(app),
    };
    hero_wizard(hero_pane(brand(app)), step, content, action)
}

/// The card's left panel: the mark, the name, and which of the two installers
/// this is.
///
/// The last line is the one worth having. "Continuing from Windows" and
/// "Installing from a CrownOS image" are the two things a user could be wrong
/// about in a way that wastes an hour, and the difference is invisible from
/// anything else on screen.
fn brand(app: &mut App) -> impl WidgetView<App> + use<> {
    let theme = theme();
    let mode = app.provenance().name();
    flex_col((
        FlexSpacer::Fixed(14.0.px()),
        // `label` and not `prose` for the wordmark: a `prose` takes the whole
        // width it is offered and left-aligns its text inside that, so centring
        // it in the pane centres a full-width box around text that stays at the
        // left edge. A `label` is as wide as its glyphs, which is what
        // `CrossAxisAlignment::Center` can actually centre.
        label("CrownOS")
            .text_size(34.0)
            .color(theme.text.on_accent),
        FlexSpacer::Fixed(8.0.px()),
        // The mode line does wrap, so it *is* a `prose` — in a box narrow enough
        // that the centring has something to work with, and with the text
        // centred inside the box as well.
        sized_box(
            prose(mode)
                .text_size(13.0)
                .text_alignment(xilem::masonry::TextAlign::Center)
                .text_color(theme.text.on_accent),
        )
        .width(240.0.px()),
    ))
    .gap(0.0.px())
    .cross_axis_alignment(CrossAxisAlignment::Center)
}

// --- MARK: Welcome (Windows flow) ---

fn welcome(app: &mut App) -> (HeroStep, Boxed, Boxed) {
    let Some(plan) = app.plan().cloned() else {
        return blocked(app);
    };
    let (total, current) = app.step_indicator();

    let content = flex_col((
        allocation(&plan),
        summary(&plan),
        note(
            "Windows keeps everything it is using now. Nothing outside the space you set \
             aside is touched.",
        ),
    ))
    .gap(BLOCK_GAP.px())
    .cross_axis_alignment(CrossAxisAlignment::Fill);

    (
        HeroStep::new("CrownOS is ready to install")
            .subtitle(
                "The Windows installer already set aside room for it. Check the numbers \
                 below and continue.",
            )
            .steps(total, current),
        Box::new(content),
        Box::new(two_actions(
            button("Edit partitions", |app: &mut App| app.open_editor()),
            button("Next step", App::advance)
                .variant(ButtonVariant::Primary)
                .stretch(),
        )),
    )
}

/// The disk, as a bar: what the other OS keeps, what CrownOS takes, what is left.
///
/// A [`segmented_bar`] rather than three boxes in a row, because the whole point
/// is that the runs are proportions of one disk and their boundaries have to be
/// exact — see that widget's docs.
fn allocation(plan: &Plan) -> impl WidgetView<App> + use<> {
    let theme = theme();
    let crownos = plan.crownos_bytes();
    let kept = plan.windows_bytes;
    let other = plan.disk_size_bytes.saturating_sub(crownos + kept);

    let bar = segmented_bar([
        Segment::new(kept as f64, theme.text.muted),
        Segment::new(crownos as f64, theme.accent.end),
        Segment::new(other as f64, theme.control.track),
    ])
    .height(BAR_HEIGHT);

    let legend = flex_row((
        (kept > 0).then(|| status(format!("Kept — {}", bytes(kept)))),
        status(format!("CrownOS — {}", bytes(crownos))).tone(Tone::Success),
        status(format!("Free — {}", bytes(other))),
    ))
    .gap(14.0.px())
    .cross_axis_alignment(CrossAxisAlignment::Center);

    panel(
        flex_col((
            prose(format!("{} — {}", plan.disk, bytes(plan.disk_size_bytes)))
                .text_size(13.0)
                .text_color(theme.text.primary),
            bar,
            legend,
        ))
        .gap(ROW_GAP.px())
        .cross_axis_alignment(CrossAxisAlignment::Fill),
    )
}

/// The handful of facts about the install that fit in the column.
///
/// Deliberately short. The *whole* description is the step list on the
/// confirmation page, and repeating it here would be a second description that
/// can drift from the one that runs.
fn summary(plan: &Plan) -> impl WidgetView<App> + use<> {
    let filesystem = match plan.root_filesystem {
        RootFilesystem::Ext4 => "ext4",
        RootFilesystem::Btrfs => "btrfs",
    };
    let swap = match (&plan.swap_partition, plan.swap_file_bytes) {
        (Some(partition), _) => format!("{} partition", bytes(partition.size_bytes())),
        (None, Some(size)) => format!("{} file", bytes(size)),
        (None, None) => "None".to_owned(),
    };
    panel(
        flex_col((
            row("CrownOS partition", plan.root.path.clone()),
            divider(),
            row("Base system", plan.distro.label().to_owned()),
            divider(),
            row("Filesystem", filesystem.to_owned()),
            divider(),
            row("Swap", swap),
            divider(),
            row("Shared EFI partition", plan.esp.path.clone()),
            divider(),
            row(
                "Left untouched",
                match &plan.windows {
                    Some(windows) => windows.path.clone(),
                    None => "nothing else is on this disk".to_owned(),
                },
            ),
        ))
        .gap(6.0.px())
        .cross_axis_alignment(CrossAxisAlignment::Fill),
    )
}

// --- MARK: Disk (ISO flow) ---

/// The page the mockup is of: pick one of the machine's disks.
fn disk(app: &mut App) -> (HeroStep, Boxed, Boxed) {
    let (total, current) = app.step_indicator();
    let Some(context) = app.context() else {
        return blocked(app);
    };

    let filter = app.search.trim().to_ascii_lowercase();
    // The filtered list and the index mapping are built together: a
    // `choice_list` hands back an index into what it was given, and a filtered
    // list's index 0 is not the snapshot's disk 0.
    let mut choices = Vec::new();
    let mut paths = Vec::new();
    for disk in &context.snapshot.disks {
        let summary = disk_summary(disk);
        if !filter.is_empty()
            && !disk.path.to_ascii_lowercase().contains(&filter)
            && !summary.to_ascii_lowercase().contains(&filter)
        {
            continue;
        }
        let usable = disk.table == "gpt";
        choices.push(
            Choice::new(disk.path.clone(), summary)
                .icon(blinc_icons::icons::HARD_DRIVE)
                .enabled(usable),
        );
        paths.push(disk.path.clone());
    }

    let selected = app
        .target
        .disk
        .as_deref()
        .and_then(|chosen| paths.iter().position(|path| path == chosen));
    let empty = choices.is_empty();
    let chosen_paths = paths.clone();

    let content = flex_col((
        search(app.search.clone(), |app: &mut App, value| app.search = value)
            .placeholder("Search disks…")
            .width(COLUMN_WIDTH),
        (!empty).then(|| {
            choice_list(choices, selected, move |app: &mut App, index| {
                let Some(path) = chosen_paths.get(index).cloned() else {
                    return;
                };
                let Some(disk) = app.context().and_then(|c| c.disk(&path)).cloned() else {
                    return;
                };
                // A different disk means a different table, so the queued edits
                // — every one of which names sectors on the old one — go with it.
                if app.target.disk.as_deref() != Some(path.as_str()) {
                    app.editor.clear();
                }
                app.target.retarget(&disk);
                app.restage_plan();
            })
        }),
        empty.then(|| {
            note(if filter.is_empty() {
                "No disks were found. This machine may need a storage driver the live \
                 image does not have."
            } else {
                "No disk matches that."
            })
        }),
    ))
    .gap(ROW_GAP.px())
    .cross_axis_alignment(CrossAxisAlignment::Fill);

    let ready = app.target.disk.is_some();
    (
        HeroStep::new("Please select the disk to install to")
            .subtitle(
                "After selecting the disk, click “Next step” and continue with the \
                 installation",
            )
            .steps(total, current),
        Box::new(content),
        Box::new(one_action(
            button("Next step", App::advance)
                .variant(ButtonVariant::Primary)
                .stretch()
                .disabled(!ready),
        )),
    )
}

/// One line describing a disk: how big, what is on it, how much is spare.
fn disk_summary(disk: &crate::engine::probe::Disk) -> String {
    if disk.table != "gpt" {
        return format!(
            "{} · {} partition table — CrownOS can only install to GPT",
            bytes(disk.bytes(disk.last_usable_sector + 1)),
            if disk.table.is_empty() {
                "no"
            } else {
                disk.table.as_str()
            }
        );
    }
    let free: u64 = disk
        .free_gaps()
        .iter()
        .map(|gap| disk.bytes(gap.sectors))
        .sum();
    let mut parts = vec![bytes(disk.bytes(disk.last_usable_sector + 1))];
    if disk.partitions.is_empty() {
        parts.push("empty".to_owned());
    } else {
        parts.push(format!("{} partitions", disk.partitions.len()));
    }
    parts.push(match esp_of(disk) {
        Some(_) => "has an EFI partition".to_owned(),
        None => "no EFI partition".to_owned(),
    });
    parts.push(format!("{} free", bytes(free)));
    parts.join(" · ")
}

// --- MARK: Space (ISO flow) ---

/// Where on the chosen disk CrownOS goes, and what filesystem it gets.
fn space(app: &mut App) -> (HeroStep, Boxed, Boxed) {
    let (total, current) = app.step_indicator();
    let Some(disk) = app.target_disk().cloned() else {
        return blocked(app);
    };
    let Some(context) = app.context() else {
        return blocked(app);
    };

    let projection = app.editor.projection(&disk, &context.snapshot);
    let gaps = projection
        .as_ref()
        .map(|projection| projection.free_gaps())
        .unwrap_or_default();

    let mut choices = Vec::new();
    let mut starts = Vec::new();
    for gap in &gaps {
        let size = disk.bytes(gap.sectors);
        choices.push(
            Choice::new(
                format!("{} of free space", bytes(size)),
                format!(
                    "sectors {}–{} on {}",
                    gap.start_sector,
                    gap.end_sector(),
                    disk.path
                ),
            )
            .icon(blinc_icons::icons::LAYERS)
            .enabled(size >= crate::engine::plan::MIN_ROOT_BYTES),
        );
        starts.push(gap.start_sector);
    }

    let selected = app
        .target
        .region_start_sector
        .and_then(|chosen| starts.iter().position(|start| *start == chosen));
    let chosen_starts = starts.clone();
    let has_space = !choices.is_empty();

    let filesystem_index = FILESYSTEM_CHOICES
        .iter()
        .position(|(fs, _)| *fs == app.target.filesystem)
        .unwrap_or(0);
    let distro_index = Distro::ALL
        .iter()
        .position(|distro| *distro == app.target.distro)
        .unwrap_or(0);

    let content = flex_col((
        has_space.then(|| {
            choice_list(choices, selected, move |app: &mut App, index| {
                if let Some(start) = chosen_starts.get(index).copied() {
                    app.target.region_start_sector = Some(start);
                    app.restage_plan();
                }
            })
        }),
        (!has_space).then(|| {
            note(
                "There is no unallocated space on this disk. Use the partition editor to \
                 make some — shrink a partition, or delete one you do not need.",
            )
        }),
        panel(
            flex_col((
                label("Base system".to_owned()).color(theme().text.muted),
                radio_group(
                    Distro::ALL.iter().map(|distro| distro.label()),
                    Some(distro_index),
                    |app: &mut App, index| {
                        if let Some(distro) = Distro::ALL.get(index) {
                            app.target.distro = *distro;
                            app.restage_plan();
                        }
                    },
                ),
            ))
            .gap(ROW_GAP.px())
            .cross_axis_alignment(CrossAxisAlignment::Start),
        ),
        panel(
            flex_col((
                label("Filesystem".to_owned()).color(theme().text.muted),
                radio_group(
                    FILESYSTEM_CHOICES.iter().map(|(_, name)| *name),
                    Some(filesystem_index),
                    |app: &mut App, index| {
                        if let Some((filesystem, _)) = FILESYSTEM_CHOICES.get(index) {
                            app.target.filesystem = *filesystem;
                            app.restage_plan();
                        }
                    },
                ),
            ))
            .gap(ROW_GAP.px())
            .cross_axis_alignment(CrossAxisAlignment::Start),
        ),
        // Whatever stopped the plan being buildable belongs here rather than on
        // a page the user cannot act from: every ISO-flow refusal is about the
        // disk, and this is the page with the disk on it.
        app.refusal().map(|refusal| {
            callout(Tone::Warning, "That will not work")
                .body(refusal.to_string())
                .view()
        }),
    ))
    .gap(BLOCK_GAP.px())
    .cross_axis_alignment(CrossAxisAlignment::Fill);

    let ready = app.plan().is_some();
    (
        HeroStep::new("Where should CrownOS go?")
            .subtitle(
                "Pick the unallocated space to install into. The partition editor can \
                 make more of it.",
            )
            .steps(total, current),
        Box::new(portal(content)),
        Box::new(two_actions(
            button("Edit partitions", |app: &mut App| app.open_editor()),
            button("Next step", App::advance)
                .variant(ButtonVariant::Primary)
                .stretch()
                .disabled(!ready),
        )),
    )
}

/// The filesystems a CrownOS root may be, and what to call them.
const FILESYSTEM_CHOICES: &[(RootFilesystem, &str)] = &[
    (RootFilesystem::Ext4, "ext4 — the safe choice"),
    (RootFilesystem::Btrfs, "btrfs — snapshots, no subvolumes yet"),
];

// --- MARK: The partition editor ---

/// Create, delete, resize and format, on the target disk.
///
/// The one page whose list can be longer than the card, so the one page that
/// wraps its list in a [`portal`]. See the module docs.
fn partitions(app: &mut App) -> (HeroStep, Boxed, Boxed) {
    let Some(disk) = app.target_disk().cloned() else {
        return blocked(app);
    };
    let Some(context) = app.context() else {
        return blocked(app);
    };
    let theme = theme();

    let rows = app.editor.rows(&disk, &context.snapshot);
    let projection_error = app
        .editor
        .projection(&disk, &context.snapshot)
        .err();

    let filter = app.search.trim().to_ascii_lowercase();
    let mut choices = Vec::new();
    let mut indices = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let (title, subtitle, glyph) = describe_row(&disk, row);
        if !filter.is_empty()
            && !title.to_ascii_lowercase().contains(&filter)
            && !subtitle.to_ascii_lowercase().contains(&filter)
        {
            continue;
        }
        choices.push(Choice::new(title, subtitle).icon(glyph));
        indices.push(index);
    }
    let selected = app
        .editor
        .selected
        .and_then(|chosen| indices.iter().position(|index| *index == chosen));
    let mapping = indices.clone();

    let selection = app.editor.selection(&disk, &context.snapshot);
    let on_free = selection.as_ref().is_some_and(Row::is_free);
    let on_partition = selection.as_ref().is_some_and(|row| !row.is_free());

    let queued = app.editor.ops.len();
    let kind_index = app.editor.kind.min(KINDS.len() - 1);
    let fs_index = app.editor.filesystem.min(FILESYSTEMS.len() - 1);

    let content = flex_col((
        search(app.search.clone(), |app: &mut App, value| app.search = value)
            .placeholder("Search partitions…")
            .width(COLUMN_WIDTH),
        sized_box(portal(
            choice_list(choices, selected, move |app: &mut App, index| {
                app.editor.selected = mapping.get(index).copied();
                app.editor.problem = None;
            }),
        ))
        .height(EDITOR_LIST_HEIGHT.px()),
        // The parameter strip. Deliberately always visible rather than swapped
        // per action: a size field that appeared only after picking "Resize"
        // would mean choosing the action before knowing whether the size can be
        // expressed, and these two controls are the only parameters any of the
        // four actions take.
        panel(
            flex_col((
                flex_row((
                    sized_box(label("Size".to_owned()).color(theme.text.muted))
                        .width(46.0.px()),
                    text_input(app.editor.size_text.clone(), |app: &mut App, value| {
                        app.editor.size_text = value;
                    })
                    .placeholder("all of it")
                    .width(SIZE_FIELD_WIDTH),
                    FlexSpacer::Fixed(10.0.px()),
                    sized_box(label("As".to_owned()).color(theme.text.muted)).width(24.0.px()),
                    crownuikit::widgets::select(
                        KINDS.iter().map(|kind| kind.label()),
                        Some(kind_index),
                        |app: &mut App, index| app.editor.kind = index,
                    ),
                ))
                .gap(6.0.px())
                .cross_axis_alignment(CrossAxisAlignment::Center),
                flex_row((
                    sized_box(label("Format".to_owned()).color(theme.text.muted))
                        .width(46.0.px()),
                    crownuikit::widgets::select(
                        FILESYSTEMS.iter().map(|fs| fs.name()),
                        Some(fs_index),
                        |app: &mut App, index| app.editor.filesystem = index,
                    ),
                ))
                .gap(6.0.px())
                .cross_axis_alignment(CrossAxisAlignment::Center),
                divider(),
                flex_row((
                    button("New", |app: &mut App| editor_action(app, EditorAction::Create))
                        .size(ButtonSize::Small)
                        .disabled(!on_free),
                    button("Resize", |app: &mut App| {
                        editor_action(app, EditorAction::Resize)
                    })
                    .size(ButtonSize::Small)
                    .disabled(!on_partition),
                    button("Format", |app: &mut App| {
                        editor_action(app, EditorAction::Format)
                    })
                    .size(ButtonSize::Small)
                    .disabled(!on_partition),
                    button("Delete", |app: &mut App| {
                        editor_action(app, EditorAction::Delete)
                    })
                    .size(ButtonSize::Small)
                    .variant(ButtonVariant::Destructive)
                    .disabled(!on_partition),
                    FlexSpacer::Flex(1.0),
                    (queued > 0).then(|| {
                        button("Undo", |app: &mut App| {
                            app.editor.undo();
                            app.restage_plan();
                        })
                        .size(ButtonSize::Small)
                    }),
                ))
                .gap(6.0.px())
                .cross_axis_alignment(CrossAxisAlignment::Center),
            ))
            .gap(ROW_GAP.px())
            .cross_axis_alignment(CrossAxisAlignment::Fill),
        ),
        // Nothing here has happened yet, and that is the single most important
        // thing this page can say.
        (queued > 0).then(|| {
            callout(
                Tone::Warning,
                format!(
                    "{queued} change{} queued — nothing written yet",
                    if queued == 1 { "" } else { "s" }
                ),
            )
            .body(
                app.editor
                    .ops
                    .iter()
                    .map(|op| format!("• {}", op.label()))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .view()
        }),
        app.editor.problem.clone().map(|problem| {
            callout(Tone::Danger, "That change was not queued")
                .body(problem)
                .view()
        }),
        projection_error.map(|problem| {
            callout(Tone::Danger, "The queued changes are no longer valid")
                .body(problem)
                .view()
        }),
    ))
    .gap(BLOCK_GAP.px())
    .cross_axis_alignment(CrossAxisAlignment::Fill);

    (
        HeroStep::new("Partition editor").subtitle(format!(
            "{} — changes are queued and applied when the install runs, so nothing here \
             is written until you confirm.",
            disk.path
        )),
        Box::new(portal(content)),
        Box::new(two_actions(
            button("Discard changes", |app: &mut App| {
                app.editor.clear();
                app.restage_plan();
            })
            .disabled(queued == 0),
            button("Done", App::retreat)
                .variant(ButtonVariant::Primary)
                .stretch(),
        )),
    )
}

/// Which of the four the user pressed. An enum so [`editor_action`] is one
/// function rather than four near-identical closures, each of which would have
/// to remember to re-plan.
#[derive(Clone, Copy)]
enum EditorAction {
    Create,
    Delete,
    Resize,
    Format,
}

/// Run one editor action and re-plan.
///
/// The re-plan is the point of routing all four through here: a queued operation
/// changes the table the install is laid out against, and a page that queued one
/// without re-planning would show a stale plan on the confirmation page.
fn editor_action(app: &mut App, action: EditorAction) {
    let Some(disk) = app.target_disk().cloned() else {
        return;
    };
    // The snapshot is cloned out because the editor's methods take `&mut self`
    // and `&SystemSnapshot`, and both live inside `app`.
    let Some(snapshot) = app.context().map(|context| context.snapshot.clone()) else {
        return;
    };
    match action {
        EditorAction::Create => app.editor.create(&disk, &snapshot),
        EditorAction::Delete => app.editor.delete(&disk, &snapshot),
        EditorAction::Resize => app.editor.resize(&disk, &snapshot),
        EditorAction::Format => app.editor.format(&disk, &snapshot),
    }
    app.restage_plan();
}

/// A row of the editor's list, as title, subtitle and glyph.
fn describe_row(
    disk: &crate::engine::probe::Disk,
    row: &Row,
) -> (String, String, &'static str) {
    match row {
        Row::Partition {
            path,
            size_bytes,
            fstype,
            label,
            pending,
            start_sector,
            end_sector,
            ..
        } => {
            let mut facts = vec![bytes(*size_bytes)];
            facts.push(match fstype.as_deref() {
                Some(fstype) if !fstype.is_empty() => fstype.to_owned(),
                _ => "no filesystem".to_owned(),
            });
            if let Some(label) = label.as_deref().filter(|l| !l.is_empty()) {
                facts.push(label.to_owned());
            }
            if *pending {
                facts.push("queued, not created yet".to_owned());
            }
            let _ = (start_sector, end_sector, disk);
            (
                path.clone(),
                facts.join(" · "),
                if *pending {
                    blinc_icons::icons::PLUS
                } else {
                    blinc_icons::icons::HARD_DRIVE
                },
            )
        }
        Row::Free {
            size_bytes,
            start_sector,
            end_sector,
        } => (
            format!("{} unallocated", bytes(*size_bytes)),
            format!("sectors {start_sector}–{end_sector}"),
            blinc_icons::icons::LAYERS,
        ),
    }
}

// --- MARK: Account ---

fn account(app: &mut App) -> (HeroStep, Boxed, Boxed) {
    let theme = theme();
    let (total, current) = app.step_indicator();
    let problem = app.form.problem();
    let show_problem = app.form.touched;
    let auto = app.context().is_some_and(|context| context.auto_install);

    let subtitle = if auto {
        "Everything else was set on the Windows side. CrownOS still needs a password — \
         the handoff file never carries one, because it sits on a partition any \
         operating system on this machine can read."
    } else {
        "This is the account you will sign in with."
    };

    let content = flex_col((
        panel(
            flex_col((
                field(
                    "Username",
                    text_input(app.form.username.clone(), |app: &mut App, value| {
                        app.form.username = value;
                        app.restage_plan();
                    })
                    .placeholder("crown"),
                ),
                field(
                    "Full name",
                    text_input(app.form.full_name.clone(), |app: &mut App, value| {
                        app.form.full_name = value;
                    })
                    .placeholder("Optional"),
                ),
                field(
                    "Computer name",
                    text_input(app.form.hostname.clone(), |app: &mut App, value| {
                        app.form.hostname = value;
                        app.restage_plan();
                    })
                    .placeholder("crownos"),
                ),
                field(
                    "Password",
                    // `.secret(true)` keeps the plaintext in our state and a run
                    // of bullets in the widget tree — see
                    // `crownuikit::widgets::text_input`.
                    text_input(app.form.password.clone(), |app: &mut App, value| {
                        app.form.password = value;
                    })
                    .secret(true),
                ),
                field(
                    "Repeat password",
                    text_input(app.form.confirm.clone(), |app: &mut App, value| {
                        app.form.confirm = value;
                    })
                    .secret(true)
                    .on_submit(|app: &mut App, _| advance_from_account(app)),
                ),
            ))
            .gap(ROW_GAP.px())
            .cross_axis_alignment(CrossAxisAlignment::Fill),
        ),
        (show_problem && problem.is_some()).then(|| {
            prose(problem.clone().unwrap_or_default())
                .text_size(13.0)
                .text_color(theme.status.danger)
        }),
    ))
    .gap(BLOCK_GAP.px())
    .cross_axis_alignment(CrossAxisAlignment::Fill);

    let blocked = problem.is_some();
    (
        HeroStep::new("Create your account")
            .subtitle(subtitle)
            .steps(total, current),
        Box::new(portal(content)),
        Box::new(two_actions(
            button("Back", App::retreat),
            button("Next step", advance_from_account)
                .variant(ButtonVariant::Primary)
                .stretch()
                .disabled(blocked),
        )),
    )
}

/// Leave the account page, if it can be left.
///
/// `touched` is set even when it cannot, so that pressing a disabled-looking
/// button — or hitting Return in the last field — explains itself rather than
/// doing nothing.
fn advance_from_account(app: &mut App) {
    app.form.touched = true;
    if app.form.problem().is_some() {
        return;
    }
    app.restage_plan();
    if app.plan().is_some() {
        app.go(Page::Confirm);
    }
}

// --- MARK: Confirm ---

fn confirm(app: &mut App) -> (HeroStep, Boxed, Boxed) {
    let Some(plan) = app.plan().cloned() else {
        return blocked(app);
    };
    let (total, current) = app.step_indicator();
    let theme = theme();
    let dry_run = app.mode.is_dry_run();

    let destructive = plan.destructive_devices().join(", ");
    let file_level = plan.file_level_devices().join(", ");

    let notice = if dry_run {
        callout(Tone::Warning, "Dry run — nothing will be written").body(
            "This build was started with `--dry-run`. Every command below will be \
             printed to the log and none of them will be run.",
        )
    } else {
        callout(Tone::Danger, "This is the point of no return").body(format!(
            "Continuing will write to {destructive}. {file_level} gets files added to it \
             so CrownOS and whatever else is here can share one boot loader; nothing \
             there is erased.{}",
            match &plan.windows {
                Some(windows) => format!(
                    " Your existing {} is not touched at any point.",
                    windows.path
                ),
                None => String::new(),
            }
        ))
    };

    // The real step list, not a summary of it: a summary is a second description
    // of the install that can drift from the one that runs. This is the same
    // `Vec<Step>` the executor walks.
    let steps: Vec<_> = plan
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            flex_row((
                sized_box(label(format!("{}.", index + 1)).color(theme.text.muted))
                    .width(26.0.px()),
                prose(step.label())
                    .text_size(13.0)
                    .text_color(theme.text.body),
            ))
            .gap(6.0.px())
            .cross_axis_alignment(CrossAxisAlignment::Start)
        })
        .collect();

    let warnings: Vec<_> = plan
        .warnings
        .iter()
        .map(|warning| {
            callout(Tone::Warning, "Worth knowing")
                .body(warning.clone())
                .view()
        })
        .collect();

    let content = flex_col((
        notice.view(),
        panel(
            flex_col((
                prose(format!("{} steps, in this order", plan.steps.len()))
                    .text_size(13.0)
                    .text_color(theme.text.primary),
                divider(),
                flex_col(steps)
                    .gap(3.0.px())
                    .cross_axis_alignment(CrossAxisAlignment::Fill),
            ))
            .gap(ROW_GAP.px())
            .cross_axis_alignment(CrossAxisAlignment::Fill),
        ),
        flex_col(warnings)
            .gap(ROW_GAP.px())
            .cross_axis_alignment(CrossAxisAlignment::Fill),
        reclaim_note(&plan),
    ))
    .gap(BLOCK_GAP.px())
    .cross_axis_alignment(CrossAxisAlignment::Fill);

    let primary = if dry_run { "Start dry run" } else { "Install CrownOS" };
    (
        HeroStep::new("Ready to install")
            .subtitle("Read this before continuing.")
            .steps(total, current),
        Box::new(portal(content)),
        Box::new(two_actions(
            button("Back", App::retreat),
            button(primary, |app: &mut App| app.begin_install())
                .variant(if dry_run {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Destructive
                })
                .stretch(),
        )),
    )
}

fn reclaim_note(plan: &Plan) -> Option<impl WidgetView<App> + use<>> {
    match &plan.reclaim {
        Reclaim::GrowRootOver { staging, .. } => Some(note(format!(
            "{} will be reclaimed after the install and added to CrownOS.",
            bytes(staging.size_bytes)
        ))),
        Reclaim::Keep { reason } => Some(note(reason.clone())),
    }
}

// --- MARK: Progress ---

fn running(app: &mut App) -> (HeroStep, Boxed, Boxed) {
    let theme = theme();
    let (total, current) = app.step_indicator();
    let step = (app.run.step + 1).min(app.run.total.max(1));
    let heading = if app.run.total == 0 {
        "Starting…".to_owned()
    } else {
        format!("Step {step} of {}", app.run.total)
    };

    let lines: Vec<_> = app
        .run
        .log
        .iter()
        .map(|line| {
            prose(line.clone())
                .text_size(11.0)
                .text_color(theme.text.muted)
        })
        .collect();

    let content = flex_col((
        panel(
            flex_col((
                prose(if app.run.label.is_empty() {
                    heading.clone()
                } else {
                    app.run.label.clone()
                })
                .text_size(13.0)
                .text_color(theme.text.primary),
                progress(app.run.fraction()).width(COLUMN_WIDTH - PANEL_PADDING * 2.0),
                prose(heading)
                    .text_size(11.0)
                    .text_color(theme.text.muted),
            ))
            .gap(ROW_GAP.px())
            .cross_axis_alignment(CrossAxisAlignment::Fill),
        ),
        app.run.failure.as_ref().map(|failure| {
            callout(Tone::Danger, "The install stopped")
                .body(failure.clone())
                .view()
        }),
        // The log is the one thing on this page that can outgrow the card, so it
        // is the one thing that scrolls.
        sized_box(
            pane(sized_box(portal(
                flex_col(lines)
                    .gap(1.0.px())
                    .cross_axis_alignment(CrossAxisAlignment::Start),
            ))
            .padding(Padding::all(PANEL_PADDING)))
            .fill(theme.surface.sunken)
            .radii(Radii::all(12.0))
            .border(theme.surface.border, 1.0),
        )
        .height(LOG_HEIGHT.px()),
    ))
    .gap(BLOCK_GAP.px())
    .cross_axis_alignment(CrossAxisAlignment::Fill);

    // Nothing to press while it runs. The install cannot be paused, and a Back
    // button that silently did nothing would be worse than no button — so the
    // action row carries only what is true, and only once it is true.
    let action: Boxed = match app.run.failure.is_some() {
        true => Box::new(one_action(
            button("Restart", |app: &mut App| app.restart())
                .variant(ButtonVariant::Primary)
                .stretch(),
        )),
        false => Box::new(
            prose("Do not turn the computer off.")
                .text_size(12.0)
                .text_color(theme.text.muted),
        ),
    };

    (
        HeroStep::new("Installing CrownOS")
            .subtitle("This takes a few minutes.")
            .steps(total, current),
        Box::new(content),
        action,
    )
}

// --- MARK: Done ---

fn done(app: &mut App) -> (HeroStep, Boxed, Boxed) {
    let (total, current) = app.step_indicator();
    let dry_run = app.run.was_dry_run;
    let dual_boot = app
        .plan()
        .and_then(|plan| plan.windows.as_ref())
        .is_some();

    let (title, subtitle, notice) = if dry_run {
        (
            "Dry run finished",
            "Nothing was written to the disk.",
            callout(Tone::Success, "Every step was rehearsed").body(
                "The log listed each command a real install would have run, in order.",
            ),
        )
    } else if dual_boot {
        (
            "CrownOS is installed",
            "Restart, and pick CrownOS from the boot menu.",
            callout(Tone::Success, "Your other operating system is still there").body(
                "The boot menu now offers both. CrownOS is the default; hold the key \
                 your firmware uses for the boot menu to start the other one instead.",
            ),
        )
    } else {
        (
            "CrownOS is installed",
            "Restart to use it.",
            callout(Tone::Success, "Ready to go").body(
                "Remove the installation medium before restarting, or the firmware may \
                 boot it again instead of your new system.",
            ),
        )
    };

    (
        HeroStep::new(title).subtitle(subtitle).steps(total, current),
        Box::new(
            flex_col((notice.view(), FlexSpacer::Flex(1.0)))
                .gap(BLOCK_GAP.px())
                .cross_axis_alignment(CrossAxisAlignment::Fill),
        ),
        Box::new(one_action(
            button("Restart now", |app: &mut App| app.restart())
                .variant(ButtonVariant::Primary)
                .stretch(),
        )),
    )
}

// --- MARK: Blocked ---

fn blocked(app: &mut App) -> (HeroStep, Boxed, Boxed) {
    let theme = theme();
    let (title, detail, notes) = match &app.context {
        Err(blocker) => (
            blocker.title.clone(),
            blocker.detail.clone(),
            blocker.notes.clone(),
        ),
        // Reached when the plan itself was refused on a page that has no way to
        // fix it. The refusal's own message is the explanation.
        Ok(context) => (
            "CrownOS cannot be installed on this computer".to_owned(),
            match app.refusal() {
                Some(refusal) => refusal.to_string(),
                None => "The install plan could not be built.".to_owned(),
            },
            context.notes.clone(),
        ),
    };

    // A flashed image can almost always be got moving again by picking a
    // different disk, so it is offered the way back. A payload-driven install
    // cannot: its target was decided on the other side of a reboot.
    let recoverable = app.context.is_ok() && app.provenance().is_iso();

    let content = flex_col((
        callout(Tone::Danger, title).body(detail).view(),
        (!notes.is_empty()).then(|| {
            panel(
                flex_col(
                    notes
                        .iter()
                        .map(|note| {
                            prose(note.clone())
                                .text_size(11.0)
                                .text_color(theme.text.muted)
                        })
                        .collect::<Vec<_>>(),
                )
                .gap(4.0.px())
                .cross_axis_alignment(CrossAxisAlignment::Start),
            )
        }),
    ))
    .gap(BLOCK_GAP.px())
    .cross_axis_alignment(CrossAxisAlignment::Fill);

    let action: Boxed = if recoverable {
        Box::new(two_actions(
            button("Restart", |app: &mut App| app.restart()),
            button("Choose another disk", |app: &mut App| app.go(Page::Disk))
                .variant(ButtonVariant::Primary)
                .stretch(),
        ))
    } else {
        Box::new(one_action(
            button("Restart", |app: &mut App| app.restart())
                .variant(ButtonVariant::Primary)
                .stretch(),
        ))
    };

    (
        HeroStep::new("Nothing has been changed").subtitle("The disk is exactly as it was."),
        Box::new(portal(content)),
        action,
    )
}

// --- MARK: Pieces ---

/// A single action button across the whole column — the mockup's shape.
///
/// `CrossAxisAlignment::Fill` offers the button the column's width; taking it
/// is [`ButtonView::stretch`]'s job, because a kit button is otherwise exactly
/// as wide as its label whatever it is offered. Both halves are needed, and the
/// call sites say `.stretch()` so that is asserted here.
fn one_action<V: WidgetView<App>>(button: V) -> impl WidgetView<App> + use<V> {
    flex_col((button,))
        .gap(0.0.px())
        .cross_axis_alignment(CrossAxisAlignment::Fill)
}

/// A secondary button beside a primary one, with the primary taking the
/// remaining width so it still reads as the answer.
fn two_actions<S: WidgetView<App>, P: WidgetView<App>>(
    secondary: S,
    primary: P,
) -> impl WidgetView<App> + use<S, P> {
    flex_row((secondary, primary.flex(1.0)))
        .gap(ACTION_GAP.px())
        .cross_axis_alignment(CrossAxisAlignment::Center)
}

/// A block of grouped content inside the column. The card is already a raised
/// surface, so this one is sunken — a raised panel on a raised card has no edge.
fn panel<V: WidgetView<App>>(content: V) -> impl WidgetView<App> + use<V> {
    let theme = theme();
    pane(sized_box(content).padding(Padding::all(PANEL_PADDING)))
        .fill(theme.surface.sunken)
        .radii(Radii::all(12.0))
        .border(theme.surface.border, 1.0)
}

/// A key/value row with the keys in a fixed-width column, so a panel full of
/// them reads as a table rather than as a list of sentences.
fn row(key: &'static str, value: String) -> impl WidgetView<App> + use<> {
    let theme = theme();
    flex_row((
        sized_box(label(key.to_owned()).color(theme.text.muted)).width(KEY_WIDTH.px()),
        prose(value).text_size(13.0).text_color(theme.text.body),
    ))
    .gap(10.0.px())
    .cross_axis_alignment(CrossAxisAlignment::Start)
}

/// A labelled form field.
fn field<V: WidgetView<App>>(name: &'static str, input: V) -> impl WidgetView<App> + use<V> {
    let theme = theme();
    flex_row((
        sized_box(label(name.to_owned()).color(theme.text.muted)).width(KEY_WIDTH.px()),
        input,
    ))
    .gap(10.0.px())
    .cross_axis_alignment(CrossAxisAlignment::Center)
}

fn note<T: Into<String>>(text: T) -> impl WidgetView<App> + use<T> {
    prose(text.into())
        .text_size(12.0)
        .text_color(theme().text.muted)
}
