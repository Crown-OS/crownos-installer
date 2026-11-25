//! The post-install tasks and the pass that runs them in order.

pub mod aur;
pub mod browser;
pub mod developer_tools;
pub mod online_packages;
pub mod playwright;

#[cfg(test)]
mod tests;

use std::cell::Cell;

use crownos_config::system::{SetupConfig, SetupTask, TaskState, TaskStatus};

use crate::context::System;
use crate::error::TaskError;
use crate::state::StateStore;
use crate::utils::network;
use crate::utils::time::now_unix;

pub struct Context<'a> {
    pub config: SetupConfig,
    pub system: &'a dyn System,
    online: Cell<Option<bool>>,
}

impl<'a> Context<'a> {
    pub fn new(config: SetupConfig, system: &'a dyn System) -> Self {
        Self {
            config,
            system,
            online: Cell::new(None),
        }
    }

    /// Whether the machine can reach the mirrors. The first call probes the
    /// network; the later calls of the pass use that result, so a pass where
    /// no task needs the network does not probe it.
    pub fn is_online(&self) -> bool {
        self.online.get().unwrap_or_else(|| self.probe_again())
    }

    /// Probes the network and keeps the result for the rest of the pass.
    pub fn probe_again(&self) -> bool {
        let online = network::is_online(self.system);
        self.online.set(Some(online));
        online
    }
}

pub trait SetupTaskRunner {
    fn id(&self) -> SetupTask;

    fn wanted(&self, config: &SetupConfig) -> bool;

    /// Whether the user's session waits for this task before it sets up the
    /// user's own files.
    fn user_session_waits(&self) -> bool {
        true
    }

    /// Whether [`run`](Self::run) needs the network. The pass asks only when
    /// the task is not done.
    fn needs_network(&self, context: &Context) -> bool;

    fn is_done(&self, context: &Context) -> Result<bool, TaskError>;

    fn run(&self, context: &Context) -> Result<(), TaskError>;
}

/// Each task, in the order of a pass.
pub fn all() -> [&'static dyn SetupTaskRunner; 4] {
    [
        &online_packages::OnlinePackagesTask,
        &browser::BrowserTask,
        &developer_tools::DeveloperToolsTask,
        &playwright::PlaywrightTask,
    ]
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PassSummary {
    /// The tasks that failed and that the next run tries again.
    pub failed: Vec<(SetupTask, String)>,
    pub waiting_for_network: Vec<SetupTask>,
    /// The tasks that failed [`MAX_TASK_ATTEMPTS`](crownos_config::system::MAX_TASK_ATTEMPTS) times for this generation.
    /// The runner does not try them again until the choices change.
    pub given_up: Vec<SetupTask>,
}

impl PassSummary {
    pub fn is_complete(&self) -> bool {
        self.failed.is_empty() && self.waiting_for_network.is_empty() && self.given_up.is_empty()
    }

    pub fn has_retryable_failure(&self) -> bool {
        !self.failed.is_empty()
    }
}

/// Runs each wanted task that is not done and records its state. A task is
/// `Done` only when it is done after its run. A task that fails does not stop
/// the tasks after it. A task that needs the network stays `Pending` while
/// the machine is offline, and a task whose run fails while the machine is
/// offline after the run goes back to `Pending` without an attempt. A task
/// that failed [`MAX_TASK_ATTEMPTS`](crownos_config::system::MAX_TASK_ATTEMPTS)
/// times for `generation` is not run again, and its state stays as it is.
///
/// Known limit: when `RuntimeMaxSec=` stops the service during a pass, the
/// task stays `Running` and the stop does not count as an attempt. The time
/// limit of each command makes this rare.
pub fn run_pass(
    context: &Context,
    store: &StateStore,
    generation: u64,
) -> Result<PassSummary, TaskError> {
    let mut summary = PassSummary::default();
    for runner in all() {
        let task = runner.id();
        if !runner.wanted(&context.config) {
            store.remove(task)?;
            continue;
        }

        let current_state = store
            .load(task)
            .unwrap_or_default()
            .filter(|state| state.generation == generation);
        if current_state.as_ref().is_some_and(TaskState::gave_up) {
            summary.given_up.push(task);
            continue;
        }
        let attempts = current_state.as_ref().map_or(0, |state| state.attempts);
        let is_recorded = |status: &TaskStatus| {
            current_state
                .as_ref()
                .is_some_and(|state| state.status == *status)
        };
        let started_at_unix = now_unix();
        let state = |status, started_at_unix, finished_at_unix| TaskState {
            status,
            generation,
            started_at_unix,
            finished_at_unix,
            attempts,
        };

        let outcome = match runner.is_done(context) {
            Ok(true) => {
                if !is_recorded(&TaskStatus::Done) {
                    store.save(task, &state(TaskStatus::Done, None, Some(started_at_unix)))?;
                }
                continue;
            }
            Ok(false) if runner.needs_network(context) && !context.is_online() => {
                if !is_recorded(&TaskStatus::Pending) {
                    store.save(task, &state(TaskStatus::Pending, None, None))?;
                }
                summary.waiting_for_network.push(task);
                continue;
            }
            Ok(false) => {
                store.save(
                    task,
                    &state(TaskStatus::Running, Some(started_at_unix), None),
                )?;
                runner.run(context).and_then(|()| {
                    if runner.is_done(context)? {
                        Ok(())
                    } else {
                        Err(TaskError::Incomplete)
                    }
                })
            }
            Err(error) => Err(error),
        };

        let finished_state = match outcome {
            Ok(()) => state(TaskStatus::Done, Some(started_at_unix), Some(now_unix())),
            Err(error) if runner.needs_network(context) && !context.probe_again() => {
                tracing::debug!("{task}: {error}; offline, kept pending");
                summary.waiting_for_network.push(task);
                state(TaskStatus::Pending, None, None)
            }
            Err(error) => {
                let message = error.to_string();
                let failed_state = TaskState {
                    attempts: attempts + 1,
                    ..state(
                        TaskStatus::Failed {
                            message: message.clone(),
                        },
                        Some(started_at_unix),
                        Some(now_unix()),
                    )
                };
                if failed_state.gave_up() {
                    summary.given_up.push(task);
                } else {
                    summary.failed.push((task, message));
                }
                failed_state
            }
        };
        store.save(task, &finished_state)?;
    }
    Ok(summary)
}
