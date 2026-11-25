//! ISO flow only: pick one of the machine's disks.

use crownui::kit::{TextInputOptions, search};
use crownui::prelude::{Cx, Signal, Styled, View, keyed, vstack};

use crate::engine::plan::bytes;
use crate::engine::probe::Disk;
use crate::ui::Wizard;
use crate::ui::components::{Mark, OptionLine, RowSize, Tone, glyph, icon_tile, note, option_row};
use crate::ui::layout::{BLOCK_GAP, PageParts, back_pill, choice_grid, next_pill};

/// More disks than this and the list gets a search field.
const SEARCHABLE_FROM: usize = 4;
const SEARCH_WIDTH: f32 = 320.0;
const TILE: f32 = 48.0;
const TILE_GLYPH: f32 = 22.0;

/// One disk as the list shows it.
#[derive(Clone, PartialEq)]
pub struct DiskRow {
    pub path: String,
    pub summary: String,
    /// How much of it is taken, when that can be told.
    pub used: Option<f32>,
    pub usable: bool,
}

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let query = cx.signal(String::new());
    let searchable = wizard.with_untracked(cx, |wizard| {
        wizard
            .context()
            .is_some_and(|context| context.snapshot.disks.len() > SEARCHABLE_FROM)
    });

    let list = keyed(
        move |runtime| {
            let filter = query.with(runtime, |query| query.trim().to_ascii_lowercase());
            let rows = wizard.with(runtime, |wizard| disk_rows(wizard, &filter));
            (filter.is_empty(), rows)
        },
        move |cx, (unfiltered, rows)| {
            if rows.is_empty() {
                let message = if unfiltered {
                    "No disks were found. This machine may need a storage driver the live \
                     image does not have."
                } else {
                    "No disk matches that."
                };
                return note(Tone::Info, message).boxed();
            }
            disk_list(cx, wizard, rows).boxed()
        },
    );

    let content = vstack((
        searchable.then(|| {
            search(query)
                .placeholder("Search disks…")
                .width(SEARCH_WIDTH)
                .self_center()
        }),
        list,
    ))
    .gap(BLOCK_GAP);

    PageParts::new("Select the disk", "Where should CrownOS live?")
        .content(content)
        .leading(back_pill(wizard))
        .actions(next_pill(wizard, |wizard| wizard.target.disk.is_none()))
}

fn disk_list(cx: &mut Cx, wizard: Signal<Wizard>, rows: Vec<DiskRow>) -> impl View + use<> {
    let rows: Vec<_> = rows
        .into_iter()
        .map(|row| {
            let path = row.path.clone();
            let chosen = {
                let path = path.clone();
                cx.memo(move |runtime| {
                    wizard.with(runtime, |wizard| {
                        wizard.target.disk.as_deref() == Some(path.as_str())
                    })
                })
            };
            let line = OptionLine {
                usage: row.used,
                usable: row.usable,
                ..OptionLine::new(row.path, row.summary)
            };
            option_row(
                line,
                RowSize::Large,
                icon_tile(glyph::HARD_DRIVE, TILE, TILE_GLYPH, move |runtime| {
                    chosen.get(runtime)
                }),
                Mark::Radio,
                chosen,
                move |cx| wizard.update(cx, |wizard| wizard.choose_disk(&path)),
            )
        })
        .collect();
    choice_grid(rows)
}

pub fn disk_rows(wizard: &Wizard, filter: &str) -> Vec<DiskRow> {
    let Some(context) = wizard.context() else {
        return Vec::new();
    };
    context
        .snapshot
        .disks
        .iter()
        .map(|disk| DiskRow {
            path: disk.path.clone(),
            summary: disk_summary(disk),
            used: used_share(disk),
            usable: disk.table == "gpt" || disk.is_blank(),
        })
        .filter(|row| {
            filter.is_empty()
                || row.path.to_ascii_lowercase().contains(filter)
                || row.summary.to_ascii_lowercase().contains(filter)
        })
        .collect()
}

/// How much of a GPT disk its partitions take, from nothing to all of it.
pub fn used_share(disk: &Disk) -> Option<f32> {
    if disk.table != "gpt" {
        return None;
    }
    let size = disk.bytes(disk.last_usable_sector + 1);
    let free: u64 = disk
        .free_gaps()
        .iter()
        .map(|gap| disk.bytes(gap.sectors))
        .sum();
    (size > 0).then(|| 1.0 - free as f32 / size as f32)
}

/// One line describing a disk: how big, what is on it, how much is spare.
pub fn disk_summary(disk: &Disk) -> String {
    let size = bytes(disk.bytes(disk.last_usable_sector + 1));
    if disk.is_blank() {
        return format!("{size} · empty disk");
    }
    if disk.table != "gpt" {
        return format!(
            "{size} · {} partition table — CrownOS needs GPT",
            disk.table
        );
    }
    let free: u64 = disk
        .free_gaps()
        .iter()
        .map(|gap| disk.bytes(gap.sectors))
        .sum();
    format!("{size} · {} free", bytes(free))
}
