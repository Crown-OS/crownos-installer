//! Either flow: the install finished.

use crownui::kit::ButtonVariant;
use crownui::prelude::{Cx, Signal};

use crate::ui::Wizard;
use crate::ui::components::{Tone, note};
use crate::ui::layout::{PageParts, restart_pill};

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let (dry_run, dual_boot) = wizard.with_untracked(cx, |wizard| {
        (
            wizard.progress.was_dry_run,
            wizard.plan().is_some_and(|plan| plan.windows.is_some()),
        )
    });

    let (title, subtitle, advice, button) = if dry_run {
        (
            "Dry run finished",
            "Nothing was written to the disk.",
            "The log listed each command a real install would have run, in order.",
            "Close",
        )
    } else {
        (
            "You’re all set",
            "CrownOS is installed. Restart to start using it.",
            if dual_boot {
                "CrownOS is the default in the boot menu. Hold Space while the computer \
                 starts to choose Windows or recovery instead."
            } else {
                "Remove the installation media before restarting, or the firmware may boot it \
                 again."
            },
            "Restart now",
        )
    };

    PageParts::new(title, subtitle)
        .hero()
        .content(note(Tone::Info, advice))
        .actions(restart_pill(wizard, button, ButtonVariant::Primary))
}
