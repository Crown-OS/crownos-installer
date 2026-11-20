//! The post-install tasks and the pass that runs them in order.

pub mod aur;
pub mod browser;
pub mod developer_tools;
pub mod playwright;

#[cfg(test)]
mod tests;

use crownos_config::system::{SetupConfig, SetupTask, TaskState, TaskStatus};

use crate::context::System;
use crate::error::TaskError;
use crate::state::StateStore;
use crate::utils::time::now_unix;

pub struct Context<'a> {
    pub config: SetupConfig,
    pub system: &'a dyn System,
    /// Whether the machine could reach the mirrors when the pass started.
    pub online: bool,
}

pub trait SetupTaskRunner {
    fn id(&self) -> SetupTask;

    fn wanted(&self, config: &SetupConfig) -> bool;

    /// Whether [`run`](Self::run) needs the network. The pass asks only when
    /// the task is not done.
    fn needs_network(&self, context: &Context) -> bool;

    fn is_done(&self, context: &Context) -> Result<bool, TaskError>;

    fn run(&self, context: &Context) -> Result<(), TaskError>;
}

/// Each task, in the order of a pass.
pub fn all() -> [&'static dyn SetupTaskRunner; 3] {
    [
        &browser::BrowserTask,
        &developer_tools::DeveloperToolsTask,
        &playwright::PlaywrightTask,
    ]
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PassSummary {
    pub failed: Vec<(SetupTask, String)>,
    pub waiting_for_network: Vec<SetupTask>,
}

impl PassSummary {
    pub fn is_complete(&self) -> bool {
        self.failed.is_empty() && self.waiting_for_network.is_empty()
    }
}

/// Runs each wanted task that is not done and records its state. A task is
/// `Done` only when it is done after its run. A task that fails does not stop
/// the tasks after it. A task that needs the network
/// stays `Pending` while the machine is offline.
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

        let previous_state = store.load(task).unwrap_or_default();
        let is_recorded = |status: &TaskStatus| {
            previous_state
                .as_ref()
                .is_some_and(|state| state.status == *status && state.generation == generation)
        };

        let started_at_unix = now_unix();
        let outcome = match runner.is_done(context) {
            Ok(true) => {
                if !is_recorded(&TaskStatus::Done) {
                    store.save(
                        task,
                        &TaskState {
                            status: TaskStatus::Done,
                            generation,
                            started_at_unix: None,
                            finished_at_unix: Some(started_at_unix),
                        },
                    )?;
                }
                continue;
            }
            Ok(false) if !context.online && runner.needs_network(context) => {
                if !is_recorded(&TaskStatus::Pending) {
                    store.reset(&[task], generation)?;
                }
                summary.waiting_for_network.push(task);
                continue;
            }
            Ok(false) => {
                store.save(
                    task,
                    &TaskState {
                        status: TaskStatus::Running,
                        generation,
                        started_at_unix: Some(started_at_unix),
                        finished_at_unix: None,
                    },
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

        let status = match outcome {
            Ok(()) => TaskStatus::Done,
            Err(error) => {
                let message = error.to_string();
                summary.failed.push((task, message.clone()));
                TaskStatus::Failed { message }
            }
        };
        store.save(
            task,
            &TaskState {
                status,
                generation,
                started_at_unix: Some(started_at_unix),
                finished_at_unix: Some(now_unix()),
            },
        )?;
    }
    Ok(summary)
}
