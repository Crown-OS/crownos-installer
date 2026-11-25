//! Either flow: the web browser, the coding agents and the apps from other
//! systems the new install gets.

use crownos_config::system::{self, DeveloperTool};
use crownui::prelude::{Cx, Signal, Styled, View, hstack, vstack};

use crate::oobe::{Browser, WindowsApps};
use crate::ui::Wizard;
use crate::ui::components::{Mark, OptionLine, RowSize, app_icon, app_icons, option_row};
use crate::ui::layout::{PageParts, back_pill, caption, muted, next_pill};

const BROWSER_ORDER: [Browser; 4] = [
    Browser::Brave,
    Browser::Chrome,
    Browser::Chromium,
    Browser::Firefox,
];
const CARD_WIDTH: f32 = 268.0;
const CARD_GAP: f32 = 16.0;
const CAPTION_GAP: f32 = 12.0;
const GROUP_GAP: f32 = 32.0;

const BACKGROUND_SETUP: &str = "Windows apps, coding agents, Chrome and Firefox set themselves \
     up in the background the first time CrownOS is online. You can change this later in \
     Settings.";

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>) -> PageParts {
    let setup_shipped = wizard.with_untracked(cx, |wizard| {
        wizard
            .context()
            .is_some_and(|context| context.snapshot.setup_shipped())
    });
    let browsers = setup_shipped.then(|| {
        group(
            "Pick your browser",
            BROWSER_ORDER
                .iter()
                .map(|&browser| browser_card(cx, wizard, browser).boxed())
                .collect(),
        )
    });
    let tools = setup_shipped.then(|| {
        group(
            "Pick your coding agents",
            DeveloperTool::ALL
                .iter()
                .map(|&tool| developer_tool_card(cx, wizard, tool).boxed())
                .collect(),
        )
    });
    let platforms = group(
        "Pick your app platforms",
        vec![windows_apps_card(cx, wizard).boxed()],
    );
    let content = vstack((
        browsers,
        tools,
        platforms,
        muted(BACKGROUND_SETUP).text_center().self_center(),
    ))
    .gap(GROUP_GAP)
    .items_center()
    .w_full();

    let (title, subtitle) = if setup_shipped {
        (
            "Your browser and coding agents",
            "Choose a browser and tools, and we set them up for you.",
        )
    } else {
        (
            "Apps from other systems",
            "You can change this later in Settings.",
        )
    };
    PageParts::new(title, subtitle)
        .content(content)
        .leading(back_pill(wizard))
        .actions(next_pill(wizard, |_| false))
}

fn group(title: &'static str, cards: Vec<crownui::prelude::AnyView>) -> impl View {
    vstack((
        caption(title),
        hstack(cards).gap(CARD_GAP).justify_center().w_full(),
    ))
    .gap(CAPTION_GAP)
    .items_center()
    .w_full()
}

fn card(row: impl View + Styled) -> impl View {
    row.flex_1().max_w(CARD_WIDTH)
}

fn browser_card(cx: &mut Cx, wizard: Signal<Wizard>, browser: Browser) -> impl View + use<> {
    let chosen = cx.memo(move |runtime| wizard.with(runtime, |wizard| wizard.browser == browser));
    card(option_row(
        OptionLine::new(system::Browser::from(browser).name(), summary(browser)),
        RowSize::Compact,
        app_icon(browser_icon(browser)),
        Mark::Radio,
        chosen,
        move |cx| wizard.update(cx, |wizard| wizard.set_browser(browser)),
    ))
}

fn summary(browser: Browser) -> &'static str {
    match browser {
        Browser::Brave => "Blocks ads and trackers",
        Browser::Chrome | Browser::Firefox => "Downloads after install",
        Browser::Chromium => "Already on this image",
    }
}

fn browser_icon(browser: Browser) -> &'static [u8] {
    match browser {
        Browser::Brave => app_icons::BRAVE,
        Browser::Chrome => app_icons::GOOGLE_CHROME,
        Browser::Chromium => app_icons::CHROMIUM,
        Browser::Firefox => app_icons::FIREFOX,
    }
}

fn windows_apps_card(cx: &mut Cx, wizard: Signal<Wizard>) -> impl View + use<> {
    let chosen = cx.memo(move |runtime| {
        wizard.with(runtime, |wizard| {
            wizard.windows_apps == WindowsApps::Enabled
        })
    });
    card(option_row(
        OptionLine::new("Windows apps", ".exe and .msi"),
        RowSize::Compact,
        app_icon(app_icons::WINDOWS_APPS),
        Mark::Checkbox,
        chosen,
        move |cx| {
            wizard.update(cx, |wizard| {
                wizard.set_windows_apps(match wizard.windows_apps {
                    WindowsApps::Enabled => WindowsApps::Disabled,
                    WindowsApps::Disabled => WindowsApps::Enabled,
                });
            });
        },
    ))
}

fn developer_tool_card(
    cx: &mut Cx,
    wizard: Signal<Wizard>,
    tool: DeveloperTool,
) -> impl View + use<> {
    let chosen = cx.memo(move |runtime| {
        wizard.with(runtime, |wizard| {
            system::DeveloperTools::from(wizard.developer_tools).is_enabled(tool)
        })
    });
    card(option_row(
        OptionLine::new(tool.name(), tool_kind(tool)),
        RowSize::Compact,
        app_icon(tool_icon(tool)),
        Mark::Checkbox,
        chosen,
        move |cx| {
            wizard.update(cx, |wizard| {
                let mut tools = system::DeveloperTools::from(wizard.developer_tools);
                tools.set(tool, !tools.is_enabled(tool));
                wizard.set_developer_tools(tools.into());
            });
        },
    ))
}

fn tool_kind(tool: DeveloperTool) -> &'static str {
    match tool {
        DeveloperTool::ClaudeCode | DeveloperTool::Codex => "Coding agent",
        DeveloperTool::Cursor => "Code editor",
    }
}

fn tool_icon(tool: DeveloperTool) -> &'static [u8] {
    match tool {
        DeveloperTool::ClaudeCode => app_icons::CLAUDE_CODE,
        DeveloperTool::Codex => app_icons::CODEX,
        DeveloperTool::Cursor => app_icons::CURSOR,
    }
}
