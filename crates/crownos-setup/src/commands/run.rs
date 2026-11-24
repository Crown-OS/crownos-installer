//! `crownos-setup run`: the work of `crownos-setup.service`.

use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use crownos_config::system::{SETUP_CONFIG_PATH, SETUP_STATE_DIR};
use crownos_setup_tasks::{Context, PassSummary, StateStore, System, TaskError, config, run_pass};

use crate::utils::task_status;

/// The passes in one run when the choices change during each pass.
const MAX_PASSES: usize = 3;

#[derive(Debug, PartialEq, Eq)]
struct RunSummary {
    passes: usize,
    choices_settled: bool,
    last_pass: PassSummary,
}

impl RunSummary {
    /// True when a task failed and can try again, or when the choices still
    /// changed after the last pass. Waiting for the network and a task that
    /// gave up do not need a retry.
    fn needs_retry(&self) -> bool {
        !self.choices_settled || self.last_pass.has_retryable_failure()
    }
}

/// Exits with a failure only when a retry can help, so that the unit starts
/// again later. NetworkManager starts the unit again when the network comes
/// up.
///
/// A broken `/etc/crownos/setup.ron` runs no task and exits with success:
/// Settings shows the error, and a restart cannot fix the file.
pub fn run(system: &dyn System) -> Result<ExitCode> {
    let summary = match run_until_settled(system) {
        Ok(summary) => summary,
        Err(error) if is_broken_choices(&error) => {
            tracing::warn!("{error:#}; no task runs until the choices are fixed");
            return Ok(ExitCode::SUCCESS);
        }
        Err(error) => return Err(error),
    };
    let store = StateStore::new(system, SETUP_STATE_DIR);
    for (task, state) in store.load_all()? {
        match task_status::message(&state.status) {
            Some(message) if state.gave_up() => tracing::warn!(
                "{task}: failed {} times, stopped until the choices change: {message}",
                state.attempts
            ),
            Some(message) => tracing::warn!("{task}: failed: {message}"),
            None => tracing::info!("{task}: {}", task_status::label(&state.status)),
        }
    }
    let waiting_tasks = &summary.last_pass.waiting_for_network;
    if !waiting_tasks.is_empty() {
        let names: Vec<&str> = waiting_tasks.iter().map(|task| task.id()).collect();
        tracing::info!("offline: {} wait for the network", names.join(", "));
    }
    Ok(if summary.needs_retry() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn is_broken_choices(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<TaskError>(),
        Some(TaskError::Config(_))
    )
}

/// Runs passes until the choices stay the same during a pass, at most
/// [`MAX_PASSES`] times.
fn run_until_settled(system: &dyn System) -> Result<RunSummary> {
    let config_path = Path::new(SETUP_CONFIG_PATH);
    let store = StateStore::new(system, SETUP_STATE_DIR);
    let mut summary = RunSummary {
        passes: 0,
        choices_settled: false,
        last_pass: PassSummary::default(),
    };
    while summary.passes < MAX_PASSES && !summary.choices_settled {
        summary.passes += 1;
        let generation = config::generation(system, config_path);
        let setup_config = config::load_or_default(system, config_path)?;
        let context = Context::new(setup_config, system);
        summary.last_pass = run_pass(&context, &store, generation)?;
        summary.choices_settled = config::generation(system, config_path) == generation
            && config::load_or_default(system, config_path)? == setup_config;
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use crownos_config::system::SetupTask;
    use crownos_setup_tasks::context::fake::FakeSystem;

    use super::*;

    const PLAYWRIGHT_BIN: &str = "/usr/lib/crownos/playwright/node_modules/.bin";
    const CODEX_CONFIG: &str = "(browser: Chromium, developer_tools: (codex: true))";

    fn ready_system() -> FakeSystem {
        FakeSystem::new()
            .with_installed(&["chromium"])
            .with_file("/usr/bin/node", "")
            .with_file("/usr/bin/npm", "")
            .with_file(format!("{PLAYWRIGHT_BIN}/playwright-mcp"), "")
            .with_file(format!("{PLAYWRIGHT_BIN}/playwright-cli"), "")
            .with_link(
                "/usr/local/bin/playwright-cli",
                format!("{PLAYWRIGHT_BIN}/playwright-cli"),
            )
    }

    fn outcome(system: &FakeSystem) -> (usize, bool) {
        let summary = run_until_settled(system).expect("runs");
        (summary.passes, summary.needs_retry())
    }

    #[test]
    fn a_settled_system_completes_in_one_pass() {
        let system = ready_system().with_network();
        assert_eq!(outcome(&system), (1, false));
    }

    #[test]
    fn a_failed_task_needs_a_retry() {
        let system = ready_system()
            .with_network()
            .with_file(SETUP_CONFIG_PATH, CODEX_CONFIG)
            .with_failure(
                "pacman -Syu --needed --noconfirm -- openai-codex",
                "error: target not found: openai-codex",
            );
        assert_eq!(outcome(&system), (1, true));
    }

    #[test]
    fn a_task_that_gave_up_needs_no_retry() {
        let system = ready_system()
            .with_network()
            .with_file(SETUP_CONFIG_PATH, CODEX_CONFIG)
            .with_failure(
                "pacman -Syu --needed --noconfirm -- openai-codex",
                "error: target not found: openai-codex",
            );
        assert_eq!(outcome(&system), (1, true));
        assert_eq!(outcome(&system), (1, true));
        assert_eq!(outcome(&system), (1, false));
        assert_eq!(outcome(&system), (1, false));
    }

    #[test]
    fn waiting_for_the_network_needs_no_retry() {
        let system = ready_system().with_file(SETUP_CONFIG_PATH, CODEX_CONFIG);
        let summary = run_until_settled(&system).expect("runs");
        assert_eq!(
            summary.last_pass.waiting_for_network,
            [SetupTask::DeveloperTools]
        );
        assert!(!summary.needs_retry());
        assert!(!system.is_installed("openai-codex"));
    }

    #[test]
    fn a_change_during_a_pass_runs_one_more_pass() {
        let firefox_with_codex = "(browser: Firefox, developer_tools: (codex: true))";
        let system = ready_system()
            .with_network()
            .with_directory("/usr/share/playwright/firefox-1578")
            .with_file(SETUP_CONFIG_PATH, "(browser: Firefox)")
            .with_command_writes(
                "pacman -Syu --needed --noconfirm -- firefox",
                SETUP_CONFIG_PATH,
                firefox_with_codex,
            );

        assert_eq!(outcome(&system), (2, false));
        assert!(system.is_installed("openai-codex"));
    }

    #[test]
    fn broken_choices_run_no_task_and_exit_with_success() {
        let system = ready_system()
            .with_network()
            .with_file(SETUP_CONFIG_PATH, "(browser: Edge)");
        let error = run_until_settled(&system).expect_err("broken");
        assert!(is_broken_choices(&error));
        assert_eq!(run(&system).expect("runs"), ExitCode::SUCCESS);
        assert!(
            !system
                .commands()
                .iter()
                .any(|command| command.starts_with("pacman")),
            "{:?}",
            system.commands()
        );
    }

    #[test]
    fn a_settled_system_does_not_probe_the_network() {
        let system = ready_system();
        assert_eq!(outcome(&system), (1, false));
        assert_eq!(system.network_probes(), 0);
    }

    #[test]
    fn choices_that_keep_changing_stop_after_three_passes() {
        let system = ready_system().with_network().with_command_writes(
            "pacman -Qq -- chromium",
            SETUP_CONFIG_PATH,
            "(browser: Chromium)",
        );
        assert_eq!(outcome(&system), (3, true));
    }
}
