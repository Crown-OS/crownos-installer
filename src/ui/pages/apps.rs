//! Either flow: the web browser, and the apps from other systems and the
//! developer tools the new install gets.

use crownos_config::system::{self, DeveloperTool};
use crownui::kit::{Choice, ToggleOptions, choice_list, toggle};
use crownui::prelude::{Cx, Signal, Styled, View, vstack};

use crate::oobe::{Browser, WindowsApps};
use crate::ui::Wizard;
use crate::ui::components::surface;
use crate::ui::layout::{BLOCK_GAP, PageParts, TILE_GAP, back_pill, caption, next_pill};
use crate::util::mirror::mirror;

const BROWSER_ORDER: [Browser; 4] = [
    Browser::Brave,
    Browser::Chrome,
    Browser::Chromium,
    Browser::Firefox,
];

const WINDOWS_APPS_SUMMARY: &str = "Run .exe and .msi programs with Wine. It downloads and sets \
     itself up in the background the first time CrownOS is online; after that, open any Windows \
     program to run it.";

const DEVELOPER_TOOLS_SUMMARY: &str = "Developer tools install in the background once CrownOS \
     is online. Coding agents browse the web with the browser you choose.";

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let setup_shipped = wizard.with_untracked(cx, |wizard| {
        wizard
            .context()
            .is_some_and(|context| context.snapshot.setup_shipped())
    });
    let mut switches = vec![windows_apps_toggle(cx, wizard).boxed()];
    if setup_shipped {
        switches
            .extend(DeveloperTool::ALL.map(|tool| developer_tool_toggle(cx, wizard, tool).boxed()));
    }
    let switches = vstack(switches).grid().grid_cols(4).gap(TILE_GAP).w_full();
    let content = vstack((
        setup_shipped.then(|| surface(browser_list(cx, wizard))),
        surface((
            switches,
            caption(WINDOWS_APPS_SUMMARY),
            setup_shipped.then(|| caption(DEVELOPER_TOOLS_SUMMARY)),
        )),
    ))
    .gap(BLOCK_GAP);

    PageParts::new(
        "Apps from other systems",
        "Choose a browser and tools. You can change this later in Settings.",
    )
    .content(content)
    .actions((back_pill(wizard), next_pill(wizard, |_| false)))
}

fn browser_list(cx: &mut Cx, wizard: Signal<Wizard>) -> impl View + use<> {
    let selected = mirror(
        cx,
        wizard,
        |wizard| {
            BROWSER_ORDER
                .iter()
                .position(|&browser| browser == wizard.browser)
        },
        |wizard, index| {
            if let Some(&browser) = index.and_then(|index| BROWSER_ORDER.get(index)) {
                wizard.set_browser(browser);
            }
        },
    );
    let choices = BROWSER_ORDER
        .map(|browser| Choice::new(system::Browser::from(browser).name(), summary(browser)));
    choice_list(choices, selected)
        .grid()
        .grid_cols(4)
        .gap(TILE_GAP)
}

fn summary(browser: Browser) -> &'static str {
    match browser {
        Browser::Brave => "Blocks ads and trackers",
        Browser::Chrome | Browser::Firefox => "Downloads after install",
        Browser::Chromium => "Already on this image",
    }
}

fn windows_apps_toggle(cx: &mut Cx, wizard: Signal<Wizard>) -> impl View + use<> {
    let enabled = mirror(
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
    toggle(enabled).caption("Windows apps")
}

fn developer_tool_toggle(
    cx: &mut Cx,
    wizard: Signal<Wizard>,
    tool: DeveloperTool,
) -> impl View + use<> {
    let enabled = mirror(
        cx,
        wizard,
        move |wizard| system::DeveloperTools::from(wizard.developer_tools).is_enabled(tool),
        move |wizard, enabled| {
            let mut tools = system::DeveloperTools::from(wizard.developer_tools);
            tools.set(tool, enabled);
            wizard.set_developer_tools(tools.into());
        },
    );
    toggle(enabled).caption(tool.name())
}
