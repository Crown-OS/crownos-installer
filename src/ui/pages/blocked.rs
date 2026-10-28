//! Where the wizard stops: the machine cannot be installed to, and why.

use crownui::kit::ButtonVariant;
use crownui::prelude::{Cx, Signal, Styled, vstack};

use crate::ui::components::{Outcome, Tone, alert, hero_badge, surface};
use crate::ui::layout::{BLOCK_GAP, PageParts, caption, muted, restart_pill, wizard_pill};
use crate::ui::{Page, Wizard};

const NOTE_GAP: f32 = 4.0;

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let (blocker, recoverable) = wizard.with_untracked(cx, |wizard| {
        // A flashed image can almost always be got moving again by picking a
        // different disk. A payload-driven install cannot: its target was
        // decided on the other side of a reboot.
        (
            wizard.blocker(),
            wizard.context.is_ok() && wizard.provenance().is_iso(),
        )
    });

    let notes = (!blocker.notes.is_empty()).then(|| {
        surface((
            caption("What was found"),
            vstack(blocker.notes.into_iter().map(muted).collect::<Vec<_>>()).gap(NOTE_GAP),
        ))
    });
    let content =
        vstack((alert(Tone::Danger, blocker.title, blocker.detail), notes)).gap(BLOCK_GAP);

    let parts = PageParts::new("Nothing has been changed", "The disk is exactly as it was.")
        .badge(hero_badge(Outcome::Stopped))
        .content(content);
    if recoverable {
        parts.actions((
            restart_pill(wizard, "Restart", ButtonVariant::Secondary),
            wizard_pill(
                wizard,
                "Choose another disk",
                ButtonVariant::Primary,
                |wizard| {
                    wizard.go(Page::Disk);
                },
            ),
        ))
    } else {
        parts.actions(restart_pill(wizard, "Restart", ButtonVariant::Primary))
    }
}
