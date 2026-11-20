//! `crownos-setup user [--wait]`: the per-user half of the setup. The fish
//! login hook runs it without `--wait`, `crownos-setup-user.service` with
//! `--wait`. It uses only local files, never the network.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use crownos_setup_tasks::System;
use crownos_setup_tasks::user::{
    self, SetupSnapshot, UserSetupReport, notify_once, wait_for_wanted_tasks,
};
use crownos_setup_tasks::utils::user_directories::UserDirectories;

/// Runs the steps one time. With `wait`, it also waits for the root runner,
/// runs the steps again and posts the summary. A failed step does not stop
/// the wait or the summary. When the wait times out, it returns quietly.
pub fn set_up_user(system: &dyn System, wait: bool) -> Result<()> {
    let directories = user_directories()?;
    let snapshot = SetupSnapshot::load(system)?;
    log_report(&user::wire(system, &directories, &snapshot));
    if !wait {
        return Ok(());
    }
    let Some(snapshot) = wait_for_wanted_tasks(system) else {
        return Ok(());
    };
    log_report(&user::wire(system, &directories, &snapshot));
    if notify_once(system, &directories, &snapshot)? {
        tracing::info!("posted the setup summary");
    }
    Ok(())
}

fn user_directories() -> Result<UserDirectories> {
    let home = environment_path("HOME").context("HOME is not set")?;
    Ok(UserDirectories::new(
        home,
        environment_path("XDG_CONFIG_HOME"),
        environment_path("XDG_STATE_HOME"),
    ))
}

fn environment_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Logs each change at the info level and each failure at the warn level. A
/// file that the run did not edit is the same at each login, so it goes to
/// the debug level.
fn log_report(report: &UserSetupReport) {
    for line in change_lines(report) {
        tracing::info!("{line}");
    }
    for (path, error) in &report.failed {
        tracing::warn!("could not set up {}: {error}", path.display());
    }
    for line in unchanged_file_lines(report) {
        tracing::debug!("{line}");
    }
}

/// One line for each change. No lines when nothing changed.
fn change_lines(report: &UserSetupReport) -> Vec<String> {
    let mut lines: Vec<String> = report
        .linked
        .iter()
        .map(|link| format!("linked the skill {}", link.display()))
        .collect();
    lines.extend(
        report
            .merged
            .iter()
            .map(|(path, server)| format!("added the {server} MCP server to {}", path.display())),
    );
    if report.playwright_configured {
        lines.push("set the browser of playwright-cli".to_owned());
    }
    if report.dock_changed {
        lines.push("put the chosen browser in the dock".to_owned());
    }
    lines
}

fn unchanged_file_lines(report: &UserSetupReport) -> Vec<String> {
    let kept = report.kept.iter().map(|(path, server)| {
        format!(
            "kept the {server} MCP server in {}: it has other settings",
            path.display()
        )
    });
    let skipped = report.skipped.iter().map(|path| {
        format!(
            "did not change {}: it has comments or an unexpected format",
            path.display()
        )
    });
    kept.chain(skipped).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_without_changes_prints_nothing_at_the_info_level() {
        let report = UserSetupReport {
            kept: vec![("/home/ada/.codex/config.toml".into(), "playwright")],
            skipped: vec!["/home/ada/.config/zed/settings.json".into()],
            ..UserSetupReport::default()
        };
        assert!(change_lines(&report).is_empty());
        assert_eq!(
            unchanged_file_lines(&report),
            [
                "kept the playwright MCP server in /home/ada/.codex/config.toml: it has other settings",
                "did not change /home/ada/.config/zed/settings.json: it has comments or an unexpected format",
            ]
        );
    }

    #[test]
    fn each_change_is_one_line() {
        let report = UserSetupReport {
            linked: vec!["/home/ada/.agents/skills/computer-use".into()],
            merged: vec![("/home/ada/.cursor/mcp.json".into(), "playwright")],
            playwright_configured: true,
            dock_changed: true,
            ..UserSetupReport::default()
        };
        assert_eq!(
            change_lines(&report),
            [
                "linked the skill /home/ada/.agents/skills/computer-use",
                "added the playwright MCP server to /home/ada/.cursor/mcp.json",
                "set the browser of playwright-cli",
                "put the chosen browser in the dock",
            ]
        );
    }
}
