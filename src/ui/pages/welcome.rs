//! Windows flow only: what the payload decided, before doing any of it.

use crownui::kit::{ButtonVariant, Segment, icons, segmented_bar};
use crownui::prelude::{Color, Cx, Signal, Styled, View, hstack, rgb, vstack};

use crate::engine::plan::{Plan, bytes};
use crate::oobe::RootFilesystem;
use crate::ui::Wizard;
use crate::ui::components::{summary_tile, surface};
use crate::ui::layout::{BLOCK_GAP, PageParts, caption, muted, tiles, wizard_pill};
use crate::ui::palette::{BRAND, TRACK};

const BAR_HEIGHT: f32 = 12.0;
const LEGEND_GAP: f32 = 18.0;
const DOT: f32 = 8.0;
const FREE: Color = rgb(0xf2f2f6);

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let Some(plan) = wizard.with_untracked(cx, |wizard| wizard.plan().cloned()) else {
        return super::blocked::build(cx, wizard);
    };
    PageParts::new(
        "CrownOS is ready to install",
        "Windows already set aside room for it.",
    )
    .content(vstack((allocation(&plan), facts(&plan))).gap(BLOCK_GAP))
    .actions((
        wizard_pill(
            wizard,
            "Edit partitions",
            ButtonVariant::Secondary,
            Wizard::open_editor,
        ),
        wizard_pill(wizard, "Next", ButtonVariant::Primary, Wizard::advance),
    ))
}

/// The disk as one bar: what Windows keeps, what CrownOS takes, what is left.
fn allocation(plan: &Plan) -> impl View + use<> {
    let crownos = plan.crownos_bytes();
    let kept = plan.windows_bytes;
    let free = plan.disk_size_bytes.saturating_sub(crownos + kept);
    let legend = hstack((
        (kept > 0).then(|| legend_item(TRACK, format!("Kept — {}", bytes(kept)))),
        legend_item(BRAND, format!("CrownOS — {}", bytes(crownos))),
        legend_item(FREE, format!("Free — {}", bytes(free))),
    ))
    .gap(LEGEND_GAP)
    .items_center();
    surface((
        caption(format!("{} — {}", plan.disk, bytes(plan.disk_size_bytes))),
        segmented_bar([
            Segment::new(kept as f32, TRACK),
            Segment::new(crownos as f32, BRAND),
            Segment::new(free as f32, FREE),
        ])
        .h(BAR_HEIGHT),
        legend,
    ))
}

fn legend_item(color: Color, label: String) -> impl View {
    hstack((
        vstack(()).size(DOT).rounded(DOT / 2.0).bg(color),
        muted(label),
    ))
    .gap(6.0)
    .items_center()
}

/// The handful of facts worth checking. The whole description is the step
/// list on the confirmation page; repeating it here would be a second
/// description that can drift from the one that runs.
fn facts(plan: &Plan) -> impl View + use<> {
    let filesystem = match plan.root_filesystem {
        RootFilesystem::Ext4 => "ext4",
        RootFilesystem::Btrfs => "btrfs",
    };
    let swap = match (&plan.swap_partition, plan.swap_file_bytes) {
        (Some(partition), _) => format!("{} swap partition", bytes(partition.size_bytes())),
        (None, Some(size)) => format!("{} swap file", bytes(size)),
        (None, None) => "no swap".to_owned(),
    };
    let untouched = plan.windows.as_ref().map_or_else(
        || "Nothing else is on this disk".to_owned(),
        |windows| format!("{} is left untouched", windows.path),
    );
    tiles((
        summary_tile(
            icons::HARD_DRIVE,
            "CrownOS partition",
            format!("{} · {}", plan.root.path, bytes(plan.root.size_bytes())),
        ),
        summary_tile(
            icons::LAYERS,
            "Filesystem",
            format!("{filesystem} · {swap}"),
        ),
        summary_tile(
            icons::SHIELD_CHECK,
            "Shared EFI partition",
            plan.esp.path.clone(),
        ),
        summary_tile(icons::LAPTOP, "Windows", untouched),
    ))
}
