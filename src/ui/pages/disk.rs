//! ISO flow only: pick one of the machine's disks.

use crownui::kit::{Choice, TextInputOptions, choice_list, icons, search};
use crownui::prelude::{Cx, Signal, Styled, View, keyed, vstack};

use crate::engine::plan::bytes;
use crate::engine::probe::Disk;
use crate::ui::components::{Tone, note, surface};
use crate::ui::layout::{BLOCK_GAP, PageParts, TILE_GAP, back_pill, next_pill};
use crate::ui::{Wizard, esp_of};
use crate::util::mirror::mirror;

/// More disks than this and the list gets a search field.
const SEARCHABLE_FROM: usize = 4;
const SEARCH_WIDTH: f32 = 320.0;

/// One disk as the list shows it.
#[derive(Clone, PartialEq)]
pub struct DiskRow {
    pub path: String,
    pub summary: String,
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
            surface(disk_list(cx, wizard, rows)).boxed()
        },
    );

    let content = vstack((
        searchable.then(|| {
            search(query)
                .placeholder("Search disks…")
                .width(SEARCH_WIDTH)
        }),
        list,
    ))
    .gap(BLOCK_GAP);

    PageParts::new(
        "Choose a disk",
        "Select the disk CrownOS will be installed to.",
    )
    .content(content)
    .actions((
        back_pill(wizard),
        next_pill(wizard, |wizard| wizard.target.disk.is_none()),
    ))
}

fn disk_list(cx: &mut Cx, wizard: Signal<Wizard>, rows: Vec<DiskRow>) -> impl View + use<> {
    let paths: Vec<String> = rows.iter().map(|row| row.path.clone()).collect();
    let chosen_paths = paths.clone();
    let selected = mirror(
        cx,
        wizard,
        move |wizard| {
            let chosen = wizard.target.disk.as_deref()?;
            paths.iter().position(|path| path == chosen)
        },
        move |wizard, index| {
            if let Some(path) = index.and_then(|index| chosen_paths.get(index)) {
                wizard.choose_disk(path);
            }
        },
    );
    let choices = rows.into_iter().map(|row| {
        let choice = Choice::new(row.path, row.summary).icon(icons::HARD_DRIVE);
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
            usable: disk.table == "gpt" || disk.is_blank(),
        })
        .filter(|row| {
            filter.is_empty()
                || row.path.to_ascii_lowercase().contains(filter)
                || row.summary.to_ascii_lowercase().contains(filter)
        })
        .collect()
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
    let esp = if esp_of(disk).is_some() {
        "EFI partition"
    } else {
        "no EFI partition"
    };
    format!("{size} · {esp} · {} free", bytes(free))
}
