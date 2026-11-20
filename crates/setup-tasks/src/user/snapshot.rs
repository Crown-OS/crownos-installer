//! The choices and the task states that the root runner recorded.

use std::path::Path;

use crownos_config::system::{
    SETUP_CONFIG_PATH, SETUP_STATE_DIR, SetupConfig, SetupTask, TaskState, TaskStatus,
};

use crate::context::System;
use crate::error::TaskError;
use crate::state::StateStore;
use crate::{config, tasks};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetupSnapshot {
    pub config: SetupConfig,
    pub generation: u64,
    pub states: Vec<(SetupTask, TaskState)>,
}

impl SetupSnapshot {
    /// Reads `/etc/crownos/setup.ron` and the task states. A missing file
    /// gives the defaults. A task state that cannot be read counts as
    /// missing.
    pub fn load(system: &dyn System) -> Result<Self, TaskError> {
        let config_path = Path::new(SETUP_CONFIG_PATH);
        let generation = config::generation(system, config_path);
        let config = config::load_or_default(system, config_path)?;
        Ok(Self::with_states(system, config, generation))
    }

    /// The task states with the default choices, for a broken
    /// `/etc/crownos/setup.ron`.
    pub fn with_default_choices(system: &dyn System) -> Self {
        let generation = config::generation(system, Path::new(SETUP_CONFIG_PATH));
        Self::with_states(system, SetupConfig::default(), generation)
    }

    fn with_states(system: &dyn System, config: SetupConfig, generation: u64) -> Self {
        let store = StateStore::new(system, SETUP_STATE_DIR);
        let states = SetupTask::ALL
            .into_iter()
            .filter_map(|task| match store.load(task) {
                Ok(state) => state.map(|state| (task, state)),
                Err(error) => {
                    tracing::warn!("could not read the state of {task}: {error}");
                    None
                }
            })
            .collect();
        Self {
            config,
            generation,
            states,
        }
    }

    /// True when `task` is `Done` for the current choices.
    pub fn is_done(&self, task: SetupTask) -> bool {
        self.states.iter().any(|(recorded_task, state)| {
            *recorded_task == task
                && state.status == TaskStatus::Done
                && state.generation >= self.generation
        })
    }

    pub fn wanted_tasks(&self) -> impl Iterator<Item = SetupTask> + '_ {
        tasks::all()
            .into_iter()
            .filter(|runner| runner.wanted(&self.config))
            .map(|runner| runner.id())
    }

    /// True when `task` failed too often for the current choices, so the
    /// runner does not try it again.
    pub fn has_given_up(&self, task: SetupTask) -> bool {
        self.states.iter().any(|(recorded_task, state)| {
            *recorded_task == task && state.generation >= self.generation && state.gave_up()
        })
    }

    /// True when each wanted task is `Done` for the current choices or gave
    /// up: the runner does nothing more for them.
    pub fn is_complete(&self) -> bool {
        self.wanted_tasks()
            .all(|task| self.is_done(task) || self.has_given_up(task))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    pub(crate) fn state(status: TaskStatus, generation: u64) -> TaskState {
        TaskState {
            status,
            generation,
            started_at_unix: None,
            finished_at_unix: None,
            attempts: 0,
        }
    }

    #[test]
    fn a_task_done_for_an_older_generation_is_not_done() {
        let snapshot = SetupSnapshot {
            generation: 9,
            states: vec![
                (SetupTask::Browser, state(TaskStatus::Done, 8)),
                (SetupTask::Playwright, state(TaskStatus::Done, 9)),
            ],
            ..SetupSnapshot::default()
        };
        assert!(!snapshot.is_done(SetupTask::Browser));
        assert!(snapshot.is_done(SetupTask::Playwright));
        assert!(!snapshot.is_complete());
    }

    #[test]
    fn a_broken_state_file_drops_only_its_own_task() {
        let system = FakeSystem::new()
            .with_file(format!("{SETUP_STATE_DIR}/browser.ron"), "(status: Maybe)")
            .with_file(
                format!("{SETUP_STATE_DIR}/playwright.ron"),
                "(status: Done)",
            );
        let snapshot = SetupSnapshot::load(&system).expect("loads");
        assert_eq!(
            snapshot.states,
            [(SetupTask::Playwright, state(TaskStatus::Done, 0))]
        );
    }

    #[test]
    fn developer_tools_are_wanted_only_when_a_tool_is_on() {
        let mut snapshot = SetupSnapshot {
            generation: 3,
            states: vec![
                (SetupTask::Browser, state(TaskStatus::Done, 3)),
                (SetupTask::Playwright, state(TaskStatus::Done, 3)),
            ],
            ..SetupSnapshot::default()
        };
        assert!(snapshot.is_complete());
        snapshot.config.developer_tools.codex = true;
        assert!(!snapshot.is_complete());
    }

    #[test]
    fn a_task_that_gave_up_completes_the_wait() {
        let gave_up = TaskState {
            attempts: crownos_config::system::MAX_TASK_ATTEMPTS,
            ..state(
                TaskStatus::Failed {
                    message: "target not found".to_owned(),
                },
                3,
            )
        };
        let failed_once = TaskState {
            attempts: 1,
            ..gave_up.clone()
        };
        let mut snapshot = SetupSnapshot {
            generation: 3,
            states: vec![
                (SetupTask::Browser, gave_up),
                (SetupTask::Playwright, state(TaskStatus::Done, 3)),
            ],
            ..SetupSnapshot::default()
        };
        assert!(snapshot.has_given_up(SetupTask::Browser));
        assert!(snapshot.is_complete());
        snapshot.states[0].1 = failed_once;
        assert!(!snapshot.is_complete());
    }
}
