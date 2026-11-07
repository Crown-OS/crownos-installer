//! Either flow: which apps from other systems the new install can run.

use crownui::kit::{setting_row, toggle};
use crownui::prelude::{Cx, Signal};

use crate::oobe::WindowsApps;
use crate::ui::Wizard;
use crate::ui::components::surface;
use crate::ui::layout::{PageParts, back_pill, caption, next_pill};
use crate::util::mirror::mirror;

const WINDOWS_APPS_SUMMARY: &str = "Run .exe and .msi programs with Wine. It downloads and sets \
     itself up in the background the first time CrownOS is online; after that, open any Windows \
     program to run it.";

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let windows_apps = mirror(
        cx,
        wizard,
        |wizard| wizard.windows_apps == WindowsApps::Enabled,
        |wizard, enabled| {
            wizard.set_windows_apps(if enabled {
                WindowsApps::Enabled
            } else {
                WindowsApps::Disabled
            });
        },
    );
    let content = surface((
        setting_row("Windows apps support", toggle(windows_apps)),
        caption(WINDOWS_APPS_SUMMARY),
    ));

    PageParts::new(
        "Apps from other systems",
        "Choose what else CrownOS can run. You can change this later in Settings.",
    )
    .content(content)
    .actions((back_pill(wizard), next_pill(wizard, |_| false)))
}
