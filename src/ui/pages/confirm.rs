//! Either flow: the last look before anything is written.

use crownui::kit::ButtonVariant;
use crownui::prelude::{Cx, Listeners, Signal, Styled, View, vstack};

use crate::engine::plan::{Plan, Reclaim, bytes};
use crate::ui::Wizard;
use crate::ui::components::{Status, Tone, alert, note, pill, stage_row, surface};
use crate::ui::layout::{BLOCK_GAP, PageParts, back_pill, caption, tiles};

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let Some((plan, dry_run)) = wizard.with_untracked(cx, |wizard| {
        Some((wizard.plan()?.clone(), wizard.mode.is_dry_run()))
    }) else {
        return super::blocked::build(cx, wizard);
    };

    let warnings: Vec<_> = plan
        .warnings
        .iter()
        .map(|warning| alert(Tone::Caution, "Worth knowing", warning.clone()))
        .collect();

    let content = vstack((
        point_of_no_return(&plan, dry_run),
        steps(cx, &plan),
        vstack(warnings).gap(BLOCK_GAP),
        note(Tone::Info, reclaim(&plan)),
    ))
    .gap(BLOCK_GAP);

    let primary = if dry_run {
        "Start dry run"
    } else {
        "Install CrownOS"
    };
    PageParts::new("Ready to install", "Read this before anything is written.")
        .content(content)
        .actions((
            back_pill(wizard),
            pill(primary, ButtonVariant::Primary)
                .on_click(move |cx, _| wizard.update(cx, Wizard::begin_install)),
        ))
}

fn point_of_no_return(plan: &Plan, dry_run: bool) -> impl View + use<> {
    if dry_run {
        return alert(
            Tone::Caution,
            "Dry run — nothing will be written",
            "This build was started with `--dry-run`. Every step below will be printed to the \
             log and none of them will be run."
                .to_owned(),
        );
    }
    let destructive = plan.destructive_devices().join(", ");
    let file_level = plan.file_level_devices();
    let shared = if file_level.is_empty() {
        String::new()
    } else {
        format!(
            " {} gets files added to it so CrownOS and whatever else is here can share one \
             boot loader; nothing there is erased.",
            file_level.join(", ")
        )
    };
    let kept = plan.windows.as_ref().map_or_else(String::new, |windows| {
        format!(
            " Your existing {} is not touched at any point.",
            windows.path
        )
    });
    alert(
        Tone::Danger,
        "This is the point of no return",
        format!("Continuing will write to {destructive}.{shared}{kept}"),
    )
}

/// The real step list, not a summary of it: a summary is a second description
/// of the install that can drift from the one that runs. These are the rows
/// the progress page lights up as each step runs.
fn steps(cx: &mut Cx, plan: &Plan) -> impl View + use<> {
    let rows: Vec<_> = plan
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let waiting = cx.memo(move |_| Status::Waiting(index + 1));
            stage_row(step.label(), "Waiting", waiting)
        })
        .collect();
    surface((
        caption(format!("{} steps, in this order", plan.steps.len())),
        tiles(rows),
    ))
}

fn reclaim(plan: &Plan) -> String {
    match &plan.reclaim {
        Reclaim::GrowRootOver { staging, .. } => format!(
            "{} will be reclaimed after the install and added to CrownOS.",
            bytes(staging.size_bytes)
        ),
        Reclaim::Keep { reason } => reason.clone(),
    }
}
