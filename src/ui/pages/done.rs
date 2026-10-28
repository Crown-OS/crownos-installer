//! Either flow: the install finished.

use crownui::kit::{ButtonVariant, icons};
use crownui::prelude::{Cx, Signal, Styled, View, vstack};

use crate::engine::plan::{Plan, bytes};
use crate::oobe::RootFilesystem;
use crate::ui::Wizard;
use crate::ui::components::{Outcome, Tone, hero_badge, note, summary_tile, surface};
use crate::ui::layout::{BLOCK_GAP, PageParts, caption, restart_pill, tiles};

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let (dry_run, plan) = wizard.with_untracked(cx, |wizard| {
        (wizard.progress.was_dry_run, wizard.plan().cloned())
    });
    let dual_boot = plan.as_ref().is_some_and(|plan| plan.windows.is_some());

    let (title, subtitle, advice, button) = if dry_run {
        (
            "Dry run finished",
            "Nothing was written to the disk.",
            "The log listed each command a real install would have run, in order.",
            "Close",
        )
    } else {
        (
            "You're all set",
            "CrownOS is installed. Restart to start using it.",
            if dual_boot {
                "CrownOS is the default in the boot menu; hold your firmware's boot menu key \
                 to start Windows instead."
            } else {
                "Remove the installation media before restarting, or the firmware may boot it \
                 again."
            },
            "Restart now",
        )
    };

    let summary = plan
        .as_ref()
        .map(|plan| surface((caption("Summary"), summary_tiles(plan, dual_boot))));

    PageParts::new(title, subtitle)
        .badge(hero_badge(Outcome::Installed))
        .content(vstack((summary, note(Tone::Info, advice))).gap(BLOCK_GAP))
        .actions(restart_pill(wizard, button, ButtonVariant::Primary))
}

/// What was installed, read from the plan that ran.
fn summary_tiles(plan: &Plan, dual_boot: bool) -> impl View + use<> {
    let account = match &plan.account.full_name {
        Some(name) => format!("{} — {name}", plan.account.username),
        None => plan.account.username.clone(),
    };
    let filesystem = match plan.root_filesystem {
        RootFilesystem::Ext4 => "ext4",
        RootFilesystem::Btrfs => "btrfs",
    };
    let startup = if dual_boot {
        "The boot menu offers CrownOS and Windows"
    } else {
        "CrownOS starts by default"
    };
    tiles((
        summary_tile(icons::USER, "Account", account),
        summary_tile(
            icons::LAPTOP,
            "Computer name",
            plan.account.hostname.clone(),
        ),
        summary_tile(
            icons::HARD_DRIVE,
            "Installed on",
            format!(
                "{} · {filesystem} · {}",
                plan.root.path,
                bytes(plan.root.size_bytes())
            ),
        ),
        summary_tile(icons::CROWN, "Startup", startup),
    ))
}
