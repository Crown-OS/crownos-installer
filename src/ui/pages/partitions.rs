//! Either flow: create, delete, resize and format on the target disk. Changes
//! are queued, and nothing here is written until the install runs.

use crownui::kit::{
    ButtonKind, ButtonOptions, ButtonSize, ButtonVariant, Choice, TextInputOptions, button,
    choice_list, icons, select, text_input,
};
use crownui::prelude::{
    Cx, Element, Listeners, Memo, Semantic, Signal, Styled, View, bind, hstack, keyed, scroll_view,
    show, spacer, vstack,
};

use crate::engine::plan::bytes;
use crate::ui::Wizard;
use crate::ui::components::{Tone, alert, surface};
use crate::ui::editor::{FILESYSTEMS, KINDS, PartitionAction, Row};
use crate::ui::layout::{BLOCK_GAP, PageParts, TILE_GAP, muted, nothing, wizard_pill};
use crate::util::mirror::mirror;

/// Two rows of the list, so the controls under it stay in view.
const LIST_HEIGHT: f32 = 156.0;
const SIZE_FIELD_WIDTH: f32 = 116.0;
const LABEL_WIDTH: f32 = 46.0;
const STRIP_GAP: f32 = 8.0;
const STRIP_COLUMNS_GAP: f32 = 20.0;

/// One row of the list as it is drawn.
#[derive(Clone, PartialEq)]
struct ListedRow {
    title: String,
    detail: String,
    glyph: &'static str,
}

/// What the selection is, which decides the buttons that apply to it.
#[derive(Clone, Copy, PartialEq)]
enum Selected {
    Nothing,
    Free,
    Partition,
}

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let disk_path =
        wizard.with_untracked(cx, |wizard| wizard.target.disk.clone().unwrap_or_default());

    let list = keyed(
        move |runtime| wizard.with(runtime, listed_rows),
        move |cx, rows| row_list(cx, wizard, rows),
    );

    let selected: Memo<Selected> = cx.memo(move |runtime| {
        wizard.with(runtime, |wizard| match wizard.selected_row() {
            None => Selected::Nothing,
            Some(row) if row.is_free() => Selected::Free,
            Some(_) => Selected::Partition,
        })
    });
    let queued = cx.memo(move |runtime| wizard.with(runtime, |wizard| wizard.editor.ops.len()));

    let size = mirror(
        cx,
        wizard,
        |wizard| wizard.editor.size_text.clone(),
        |wizard, text| wizard.editor.size_text = text,
    );
    let kind = mirror(
        cx,
        wizard,
        |wizard| Some(wizard.editor.kind.min(KINDS.len() - 1)),
        |wizard, index| wizard.editor.kind = index.unwrap_or_default(),
    );
    let filesystem = mirror(
        cx,
        wizard,
        |wizard| Some(wizard.editor.filesystem.min(FILESYSTEMS.len() - 1)),
        |wizard, index| wizard.editor.filesystem = index.unwrap_or_default(),
    );
    let kind_labels = cx.signal(KINDS.iter().map(|kind| kind.label().to_owned()).collect());
    let filesystem_names = cx.signal(FILESYSTEMS.iter().map(|fs| fs.name().to_owned()).collect());

    // Always visible rather than swapped per action: a size field that appeared
    // only after picking "Resize" would mean choosing the action before knowing
    // whether the size can be expressed.
    let strip = surface((
        hstack((
            strip_row(
                "Size",
                text_input(size)
                    .placeholder("all of it")
                    .width(SIZE_FIELD_WIDTH)
                    .label("Size"),
            ),
            strip_row("As", select(kind_labels, kind)),
            strip_row("Format", select(filesystem_names, filesystem)),
        ))
        .gap(STRIP_COLUMNS_GAP)
        .items_center(),
        hstack((
            action(
                wizard,
                "New",
                PartitionAction::Create,
                selected,
                Selected::Free,
            ),
            action(
                wizard,
                "Resize",
                PartitionAction::Resize,
                selected,
                Selected::Partition,
            ),
            action(
                wizard,
                "Format",
                PartitionAction::Format,
                selected,
                Selected::Partition,
            ),
            action(
                wizard,
                "Delete",
                PartitionAction::Delete,
                selected,
                Selected::Partition,
            )
            .variant(ButtonVariant::Destructive),
            spacer(),
            show(
                bind(move |runtime| queued.get(runtime) > 0),
                move || {
                    button("Undo")
                        .control_size(ButtonSize::Small)
                        .on_click(move |cx, _| wizard.update(cx, Wizard::undo_partition_edit))
                },
                nothing,
            ),
        ))
        .gap(STRIP_GAP)
        .items_center(),
    ));

    let content = vstack((
        surface(scroll_view(list).h(LIST_HEIGHT).w_full()),
        strip,
        queued_changes(wizard),
        problem(wizard, "That change was not queued", |wizard| {
            wizard.editor.problem.clone()
        }),
        problem(wizard, "The queued changes are no longer valid", |wizard| {
            let context = wizard.context()?;
            wizard
                .editor
                .projection(wizard.target_disk()?, &context.snapshot)
                .err()
        }),
    ))
    .gap(BLOCK_GAP);

    PageParts::new(
        "Partition editor",
        format!("{disk_path} — nothing is written until the install runs."),
    )
    .content(content)
    .actions((
        wizard_pill(
            wizard,
            "Discard changes",
            ButtonVariant::Secondary,
            Wizard::discard_partition_edits,
        )
        .disabled(bind(move |runtime| queued.get(runtime) == 0)),
        wizard_pill(wizard, "Done", ButtonVariant::Primary, Wizard::retreat),
    ))
}

/// A caption, then its control.
fn strip_row(caption: &'static str, control: impl View) -> impl View {
    hstack((muted(caption).w(LABEL_WIDTH), control))
        .gap(STRIP_GAP)
        .items_center()
}

/// One of the four editor buttons, enabled while the selection is `applies_to`.
fn action(
    wizard: Signal<Wizard>,
    label: &'static str,
    action: PartitionAction,
    selected: Memo<Selected>,
    applies_to: Selected,
) -> Element<ButtonKind> {
    button(label)
        .control_size(ButtonSize::Small)
        .disabled(bind(move |runtime| selected.get(runtime) != applies_to))
        .on_click(move |cx, _| wizard.update(cx, |wizard| wizard.edit_partitions(action)))
}

fn row_list(cx: &mut Cx, wizard: Signal<Wizard>, rows: Vec<ListedRow>) -> impl View + use<> {
    let selected = mirror(
        cx,
        wizard,
        |wizard| wizard.editor.selected,
        |wizard, index| {
            wizard.editor.selected = index;
            wizard.editor.problem = None;
        },
    );
    let choices = rows
        .into_iter()
        .map(|row| Choice::new(row.title, row.detail).icon(row.glyph));
    choice_list(choices, selected)
        .grid()
        .grid_cols(2)
        .gap(TILE_GAP)
}

/// Nothing here has happened yet, and that is the single most important thing
/// this page can say.
fn queued_changes(wizard: Signal<Wizard>) -> impl View {
    keyed(
        move |runtime| {
            wizard.with(runtime, |wizard| {
                wizard
                    .editor
                    .ops
                    .iter()
                    .map(|op| format!("• {}", op.label()))
                    .collect::<Vec<_>>()
            })
        },
        |_, labels| {
            if labels.is_empty() {
                return nothing();
            }
            let plural = if labels.len() == 1 { "" } else { "s" };
            alert(
                Tone::Caution,
                format!(
                    "{} change{plural} queued — nothing written yet",
                    labels.len()
                ),
                labels.join("\n"),
            )
            .boxed()
        },
    )
}

fn problem(
    wizard: Signal<Wizard>,
    title: &'static str,
    read: impl Fn(&Wizard) -> Option<String> + 'static,
) -> impl View {
    keyed(
        move |runtime| wizard.with(runtime, &read),
        move |_, problem| match problem {
            Some(problem) => alert(Tone::Danger, title, problem).boxed(),
            None => nothing(),
        },
    )
}

fn listed_rows(wizard: &Wizard) -> Vec<ListedRow> {
    let (Some(context), Some(disk)) = (wizard.context(), wizard.target_disk()) else {
        return Vec::new();
    };
    wizard
        .editor
        .rows(disk, &context.snapshot)
        .iter()
        .map(|row| {
            let (title, detail, glyph) = describe(row);
            ListedRow {
                title,
                detail,
                glyph,
            }
        })
        .collect()
}

/// A row of the editor's list, as title, detail and glyph.
fn describe(row: &Row) -> (String, String, &'static str) {
    match row {
        Row::Partition {
            path,
            size_bytes,
            fstype,
            label,
            pending,
            ..
        } => {
            let filesystem = fstype
                .as_deref()
                .filter(|fstype| !fstype.is_empty())
                .unwrap_or("no filesystem");
            let mut facts = vec![bytes(*size_bytes), filesystem.to_owned()];
            facts.extend(label.iter().filter(|label| !label.is_empty()).cloned());
            if *pending {
                facts.push("queued, not created yet".to_owned());
            }
            let glyph = if *pending {
                icons::PLUS
            } else {
                icons::HARD_DRIVE
            };
            (path.clone(), facts.join(" · "), glyph)
        }
        Row::Free {
            size_bytes,
            start_sector,
            end_sector,
        } => (
            format!("{} unallocated", bytes(*size_bytes)),
            format!("sectors {start_sector}–{end_sector}"),
            icons::LAYERS,
        ),
    }
}
