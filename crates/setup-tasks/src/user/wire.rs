//! The steps of `crownos-setup user`, in order.

use std::path::{Path, PathBuf};

use super::{SetupSnapshot, accessibility_flags};
use crate::agents::playwright_cli::{self, ConfigurationChange};
use crate::agents::{self, skills};
use crate::context::System;
use crate::error::TaskError;
use crate::tasks::playwright::{PLAYWRIGHT_CLI_BINARY, prefix_binary};
use crate::utils::browser_fallback::resolve_browser;
use crate::utils::user_directories::UserDirectories;

/// The result of one run. `kept` and `skipped` stay the same from run to
/// run. The other fields are changes or failures.
#[derive(Debug, Default)]
pub struct UserSetupReport {
    /// The skill links that the run made.
    pub linked: Vec<PathBuf>,
    /// Each MCP server that the run added, with its file.
    pub merged: Vec<(PathBuf, &'static str)>,
    /// Each MCP server that a file has with another command or other
    /// arguments. The run did not change it.
    pub kept: Vec<(PathBuf, &'static str)>,
    /// The files that the run did not edit, each with the reason.
    pub skipped: Vec<(PathBuf, SkipReason)>,
    /// Each step that failed, with the file or folder of the step. The steps
    /// after it ran.
    pub failed: Vec<(PathBuf, TaskError)>,
    pub playwright_configured: bool,
    /// The browser and Electron flags files that the run changed.
    pub accessibility_flags_changed: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    SymbolicLink,
    UnexpectedFormat,
}

impl SkipReason {
    /// The reason of a skip that a step reported. A step skips a symbolic
    /// link before it reads the file.
    fn of(system: &dyn System, path: &Path) -> Self {
        if system.read_link(path).is_some() {
            Self::SymbolicLink
        } else {
            Self::UnexpectedFormat
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::SymbolicLink => "it is a symbolic link",
            Self::UnexpectedFormat => "it has comments or an unexpected format",
        }
    }
}

impl UserSetupReport {
    fn record_skip(&mut self, system: &dyn System, path: PathBuf) {
        let reason = SkipReason::of(system, &path);
        self.skipped.push((path, reason));
    }

    fn record_failure<T>(&mut self, path: &Path, result: Result<T, TaskError>) -> Option<T> {
        result
            .map_err(|error| self.failed.push((path.to_owned(), error)))
            .ok()
    }
}

/// Links the skills and adds the MCP servers for each agent on the account,
/// sets the browser of `playwright-cli` when it is installed, and makes the
/// browser and Electron flags files follow `visual_context.enabled`. A second run changes
/// nothing. A step that fails does not stop the steps after it.
pub fn wire(
    system: &dyn System,
    directories: &UserDirectories,
    snapshot: &SetupSnapshot,
) -> UserSetupReport {
    let mut report = UserSetupReport::default();
    let agents = agents::present(system, directories);

    let skill_folders = skills::folders(&agents, directories);
    if !skill_folders.is_empty() {
        let system_skills = report
            .record_failure(
                Path::new(skills::SKILLS_DIRECTORY),
                skills::system_skills(system),
            )
            .unwrap_or_default();
        for folder in &skill_folders {
            if let Some(links) =
                report.record_failure(folder, skills::link(system, &system_skills, folder))
            {
                report.linked.extend(links.linked);
                report.skipped.extend(
                    links
                        .skipped
                        .into_iter()
                        .map(|link| (link, SkipReason::SymbolicLink)),
                );
            }
        }
    }

    for agent in agents {
        let path = agent.configuration_file(directories);
        match report.record_failure(&path, agent.merge_servers(system, &path)) {
            Some(Some(merge)) => {
                report
                    .merged
                    .extend(merge.added.into_iter().map(|name| (path.clone(), name)));
                report
                    .kept
                    .extend(merge.kept.into_iter().map(|name| (path.clone(), name)));
            }
            Some(None) => report.record_skip(system, path),
            None => {}
        }
    }

    if snapshot.config.wants_playwright() && system.exists(&prefix_binary(PLAYWRIGHT_CLI_BINARY)) {
        let path = playwright_cli::configuration_file(&directories.home);
        let browser = resolve_browser(snapshot.config.browser, |executable| {
            system.exists(executable)
        });
        match report.record_failure(&path, playwright_cli::set_browser(system, &path, browser)) {
            Some(ConfigurationChange::Written) => report.playwright_configured = true,
            Some(ConfigurationChange::Skipped) => report.record_skip(system, path),
            Some(ConfigurationChange::Unchanged) | None => {}
        }
    }

    update_accessibility_flags(system, directories, &mut report);
    report
}

fn update_accessibility_flags(
    system: &dyn System,
    directories: &UserDirectories,
    report: &mut UserSetupReport,
) {
    let update = accessibility_flags::follow_visual_context(system, directories);
    report.accessibility_flags_changed.extend(update.changed);
    report.skipped.extend(
        update
            .linked
            .into_iter()
            .map(|link| (link, SkipReason::SymbolicLink)),
    );
    report.failed.extend(update.failed);
}

#[cfg(test)]
mod tests {
    use crownos_config::system::{Browser, SetupConfig, SetupTask, TaskStatus};
    use serde_json::Value;

    use super::*;
    use crate::agents::servers::with_every_server_command;
    use crate::context::fake::FakeSystem;
    use crate::user::snapshot::tests::state;

    const PLAYWRIGHT_CLI: &str = "/usr/lib/crownos/playwright/node_modules/.bin/playwright-cli";
    const SKILL: &str = "/usr/share/crownos/skills/computer-use";

    fn directories() -> UserDirectories {
        UserDirectories::new("/home/ada".into(), None, None)
    }

    fn done_snapshot(browser: Browser) -> SetupSnapshot {
        SetupSnapshot {
            config: SetupConfig {
                browser,
                ..SetupConfig::default()
            },
            generation: 12,
            states: vec![(SetupTask::Browser, state(TaskStatus::Done, 12))],
        }
    }

    #[test]
    fn an_account_without_agents_and_playwright_changes_nothing() {
        let system = FakeSystem::new();
        let report = wire(&system, &directories(), &done_snapshot(Browser::Firefox));
        assert!(report.linked.is_empty() && report.merged.is_empty() && report.skipped.is_empty());
        assert!(report.failed.is_empty() && !report.playwright_configured);
        assert!(report.accessibility_flags_changed.is_empty());
        assert!(system.journal().is_empty(), "{:?}", system.journal());
    }

    #[test]
    fn every_step_runs_and_a_second_run_writes_nothing() {
        let system = with_every_server_command(FakeSystem::new())
            .with_program("codex")
            .with_program("agy")
            .with_directory(SKILL)
            .with_file(PLAYWRIGHT_CLI, "")
            .with_directory("/home/ada/.config/zed")
            .with_file(
                "/home/ada/.config/zed/settings.json",
                "{\n  // comment\n}\n",
            );
        let report = wire(&system, &directories(), &done_snapshot(Browser::Firefox));
        assert_eq!(
            report.linked,
            [
                PathBuf::from("/home/ada/.agents/skills/computer-use"),
                PathBuf::from("/home/ada/.gemini/config/skills/computer-use")
            ]
        );
        assert_eq!(report.merged.len(), 6);
        assert!(report.kept.is_empty() && report.failed.is_empty());
        assert_eq!(
            report.skipped,
            [(
                PathBuf::from("/home/ada/.config/zed/settings.json"),
                SkipReason::UnexpectedFormat
            )]
        );
        assert!(report.playwright_configured);
        assert!(system.file("/home/ada/.codex/config.toml").is_some());
        assert!(
            system
                .file("/home/ada/.gemini/config/mcp_config.json")
                .is_some()
        );

        system.clear_journal();
        let second_report = wire(&system, &directories(), &done_snapshot(Browser::Firefox));
        assert_eq!(second_report.skipped.len(), 1);
        assert!(second_report.merged.is_empty() && second_report.linked.is_empty());
        assert!(system.journal().is_empty(), "{:?}", system.journal());
    }

    #[test]
    fn a_failed_step_does_not_stop_the_next_steps() {
        let system = with_every_server_command(FakeSystem::new())
            .with_program("codex")
            .with_directory(SKILL)
            .with_racing_link("/home/ada/.agents/skills/computer-use", "/opt/other");
        let report = wire(&system, &directories(), &done_snapshot(Browser::Brave));
        assert_eq!(report.failed.len(), 1);
        assert_eq!(
            report.failed[0].0,
            PathBuf::from("/home/ada/.agents/skills")
        );
        assert_eq!(report.merged.len(), 3);
    }

    #[test]
    fn the_flags_files_follow_the_visual_context_switch() {
        let visual_context = "/home/ada/.config/crownos/visual_context.ron";
        let chromium_flags = "/home/ada/.config/chromium-flags.conf";
        let system = FakeSystem::new()
            .with_program("chromium")
            .with_file(visual_context, "(enabled: true)")
            .with_file(chromium_flags, "--ozone-platform=wayland\n");
        let report = wire(&system, &directories(), &done_snapshot(Browser::Chromium));
        assert_eq!(
            report.accessibility_flags_changed,
            [PathBuf::from(chromium_flags)]
        );
        assert_eq!(
            system.file(chromium_flags).as_deref(),
            Some("--ozone-platform=wayland\n--force-renderer-accessibility=basic\n")
        );

        system.clear_journal();
        let second_report = wire(&system, &directories(), &done_snapshot(Browser::Chromium));
        assert!(second_report.accessibility_flags_changed.is_empty());
        assert!(system.journal().is_empty(), "{:?}", system.journal());

        let system = system.with_file(visual_context, "(enabled: false)");
        let report = wire(&system, &directories(), &done_snapshot(Browser::Chromium));
        assert_eq!(report.accessibility_flags_changed.len(), 1);
        assert_eq!(
            system.file(chromium_flags).as_deref(),
            Some("--ozone-platform=wayland\n")
        );
    }

    #[test]
    fn playwright_cli_uses_chromium_while_the_chosen_browser_is_missing() {
        let config_file = "/home/ada/.playwright/cli.config.json";
        let system = FakeSystem::new()
            .with_file(PLAYWRIGHT_CLI, "")
            .with_file("/usr/bin/chromium", "");
        wire(&system, &directories(), &done_snapshot(Browser::Brave));
        let document: Value =
            serde_json::from_str(&system.file(config_file).expect("written")).expect("JSON");
        assert_eq!(
            document["browser"]["launchOptions"]["executablePath"],
            "/usr/bin/chromium"
        );
    }

    #[test]
    fn a_linked_file_is_skipped_as_a_symbolic_link() {
        let system = FakeSystem::new()
            .with_program("codex")
            .with_file("/home/ada/dotfiles/config.toml", "")
            .with_link(
                "/home/ada/.codex/config.toml",
                "/home/ada/dotfiles/config.toml",
            );
        let report = wire(&system, &directories(), &done_snapshot(Browser::Brave));
        assert_eq!(
            report.skipped,
            [(
                PathBuf::from("/home/ada/.codex/config.toml"),
                SkipReason::SymbolicLink
            )]
        );
        assert!(report.failed.is_empty(), "{:?}", report.failed);
    }
}
