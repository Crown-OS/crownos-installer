//! ISO flow only: where on the chosen disk CrownOS goes, and what filesystem it
//! gets.

use crownui::kit::{ButtonVariant, Choice, choice_list, icons};
use crownui::prelude::{Cx, Signal, Styled, View, hstack, keyed, vstack};

use crate::engine::plan::{MIN_ROOT_BYTES, bytes};
use crate::oobe::RootFilesystem;
use crate::ui::Wizard;
use crate::ui::components::{Tone, alert, note, option_tile, surface};
use crate::ui::layout::{BLOCK_GAP, PageParts, TILE_GAP, caption, next_pill, nothing, wizard_pill};
use crate::util::mirror::mirror;

/// The filesystems a CrownOS root may be, and what to call them.
pub const FILESYSTEMS: [(RootFilesystem, &str); 2] = [
    (RootFilesystem::Ext4, "ext4 — the safe choice"),
    (RootFilesystem::Btrfs, "btrfs — snapshots"),
];

/// One free region as the list shows it.
#[derive(Clone, PartialEq)]
pub struct RegionRow {
    pub start_sector: u64,
    pub title: String,
    pub detail: String,
    pub usable: bool,
}

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let filesystems: Vec<_> = FILESYSTEMS
        .iter()
        .map(|&(filesystem, label)| {
            let chosen = cx.memo(move |runtime| {
                wizard.with(runtime, |wizard| wizard.target.filesystem == filesystem)
            });
            option_tile(label, chosen, move |cx| {
                wizard.update(cx, |wizard| {
                    wizard.target.filesystem = filesystem;
                    wizard.restage_plan();
                });
            })
        })
        .collect();

    let regions = keyed(
        move |runtime| wizard.with(runtime, region_rows),
        move |cx, rows| {
            if rows.is_empty() {
                return note(
                    Tone::Info,
                    "There is no unallocated space on this disk. Use the partition editor to \
                     make some — shrink a partition, or delete one you do not need.",
                )
                .boxed();
            }
            region_list(cx, wizard, rows).boxed()
        },
    );

    // Every ISO-flow refusal is about the disk, and this is the page with the
    // disk on it, so whatever stops the plan is said here.
    let refusal = keyed(
        move |runtime| wizard.with(runtime, |wizard| wizard.refusal().map(ToString::to_string)),
        |_, refusal| match refusal {
            Some(refusal) => alert(Tone::Caution, "That will not work", refusal).boxed(),
            None => nothing(),
        },
    );

    let content = vstack((
        surface((
            caption("Filesystem"),
            hstack(filesystems).gap(TILE_GAP).w_full(),
        )),
        surface((caption("Free space"), regions)),
        refusal,
    ))
    .gap(BLOCK_GAP);

    PageParts::new(
        "Where should CrownOS go?",
        "Pick the free space to install into.",
    )
    .content(content)
    .actions((
        wizard_pill(
            wizard,
            "Edit partitions",
            ButtonVariant::Secondary,
            Wizard::open_editor,
        ),
        next_pill(wizard, |wizard| wizard.plan().is_none()),
    ))
}

fn region_list(cx: &mut Cx, wizard: Signal<Wizard>, rows: Vec<RegionRow>) -> impl View + use<> {
    let starts: Vec<u64> = rows.iter().map(|row| row.start_sector).collect();
    let chosen_starts = starts.clone();
    let selected = mirror(
        cx,
        wizard,
        move |wizard| {
            let chosen = wizard.target.region_start_sector?;
            starts.iter().position(|start| *start == chosen)
        },
        move |wizard, index| {
            if let Some(start) = index.and_then(|index| chosen_starts.get(index)) {
                wizard.target.region_start_sector = Some(*start);
                wizard.restage_plan();
            }
        },
    );
    let choices = rows.into_iter().map(|row| {
        let choice = Choice::new(row.title, row.detail).icon(icons::LAYERS);
        if row.usable {
            choice
        } else {
            choice.disabled()
        }
    });
    choice_list(choices, selected)
        .grid()
        .grid_cols(2)
        .gap(TILE_GAP)
}

/// The free regions of the target disk as the queued edits will leave it.
pub fn region_rows(wizard: &Wizard) -> Vec<RegionRow> {
    let (Some(context), Some(disk)) = (wizard.context(), wizard.target_disk()) else {
        return Vec::new();
    };
    let Ok(projection) = wizard.editor.projection(disk, &context.snapshot) else {
        return Vec::new();
    };
    projection
        .free_gaps()
        .into_iter()
        .map(|gap| {
            let size = disk.bytes(gap.sectors);
            RegionRow {
                start_sector: gap.start_sector,
                title: format!("{} of free space", bytes(size)),
                detail: format!(
                    "sectors {}–{} on {}",
                    gap.start_sector,
                    gap.end_sector(),
                    disk.path
                ),
                usable: size >= MIN_ROOT_BYTES,
            }
        })
        .collect()
}
