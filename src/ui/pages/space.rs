//! ISO flow only: which free space on the chosen disk CrownOS goes in, and the
//! partitions it will be split into.

use crownui::kit::{ButtonVariant, Choice, choice_list, icons};
use crownui::prelude::{Cx, Signal, Styled, View, keyed, vstack};

use crate::engine::plan::{BOOT_BYTES, MIN_ROOT_BYTES, Plan, bytes, filesystem_name};
use crate::ui::Wizard;
use crate::ui::components::{Tone, alert, note, surface};
use crate::ui::layout::{BLOCK_GAP, PageParts, TILE_GAP, caption, next_pill, nothing, wizard_pill};
use crate::util::mirror::mirror;

/// One free region as the list shows it.
#[derive(Clone, PartialEq)]
pub struct RegionRow {
    pub start_sector: u64,
    pub title: String,
    pub detail: String,
    pub usable: bool,
}

pub fn build(wizard: Signal<Wizard>) -> PageParts {
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

    let layout = keyed(
        move |runtime| wizard.with(runtime, |wizard| wizard.plan().map(layout_summary)),
        |_, summary| match summary {
            Some(summary) => note(Tone::Info, summary).boxed(),
            None => nothing(),
        },
    );

    let content =
        vstack((surface((caption("Free space"), regions)), layout, refusal)).gap(BLOCK_GAP);

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

/// The partitions the plan creates, in the order they sit on the disk.
pub fn layout_summary(plan: &Plan) -> String {
    let swap = plan
        .swap_partition
        .as_ref()
        .map_or_else(String::new, |swap| {
            format!("{} swap, ", bytes(swap.size_bytes()))
        });
    format!(
        "CrownOS will create a {} boot partition, {swap}and a {} {} system partition.",
        bytes(
            plan.boot_partition
                .as_ref()
                .map_or(BOOT_BYTES, |boot| boot.size_bytes())
        ),
        bytes(plan.root.size_bytes()),
        filesystem_name(plan.root_filesystem),
    )
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
                usable: size >= BOOT_BYTES + MIN_ROOT_BYTES,
            }
        })
        .collect()
}
