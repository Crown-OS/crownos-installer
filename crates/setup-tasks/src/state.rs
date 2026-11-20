//! The task state files in `/var/lib/crownos/setup`, read and written through
//! a [`System`].

use std::path::PathBuf;

use crownos_config::system::{SetupTask, TaskState, TaskStatus};

use crate::context::System;
use crate::error::TaskError;

const STATE_DIRECTORY_MODE: u32 = 0o755;
const STATE_FILE_MODE: u32 = 0o644;

pub struct StateStore<'a> {
    system: &'a dyn System,
    directory: PathBuf,
}

impl<'a> StateStore<'a> {
    pub fn new(system: &'a dyn System, directory: impl Into<PathBuf>) -> Self {
        Self {
            system,
            directory: directory.into(),
        }
    }

    pub fn load(&self, task: SetupTask) -> Result<Option<TaskState>, TaskError> {
        let path = TaskState::path(&self.directory, task);
        let Some(text) = self.system.read_file(&path)? else {
            return Ok(None);
        };
        ron::from_str(&text)
            .map(Some)
            .map_err(|error| TaskError::Parse {
                path,
                message: error.to_string(),
            })
    }

    /// The tasks that have a state file, in [`SetupTask::ALL`] order.
    pub fn load_all(&self) -> Result<Vec<(SetupTask, TaskState)>, TaskError> {
        let mut states = Vec::new();
        for task in SetupTask::ALL {
            if let Some(state) = self.load(task)? {
                states.push((task, state));
            }
        }
        Ok(states)
    }

    pub fn save(&self, task: SetupTask, state: &TaskState) -> Result<(), TaskError> {
        self.system
            .create_dir_all(&self.directory, STATE_DIRECTORY_MODE)?;
        self.system.write_file(
            &TaskState::path(&self.directory, task),
            &state.render()?,
            STATE_FILE_MODE,
        )
    }

    pub fn remove(&self, task: SetupTask) -> Result<(), TaskError> {
        let path = TaskState::path(&self.directory, task);
        if !self.system.exists(&path) {
            return Ok(());
        }
        self.system.remove_file(&path)
    }

    /// Marks `tasks` as `Pending` for `generation`, so that a reader waits
    /// for the runner.
    pub fn reset(&self, tasks: &[SetupTask], generation: u64) -> Result<(), TaskError> {
        let pending = TaskState {
            status: TaskStatus::Pending,
            generation,
            started_at_unix: None,
            finished_at_unix: None,
        };
        tasks.iter().try_for_each(|&task| self.save(task, &pending))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    const DIR: &str = "/var/lib/crownos/setup";

    #[test]
    fn reset_writes_pending_states_that_load_back() {
        let system = FakeSystem::new();
        let store = StateStore::new(&system, DIR);
        store
            .reset(&[SetupTask::Browser, SetupTask::Playwright], 7)
            .expect("resets");

        let states = store.load_all().expect("loads");
        assert_eq!(states.len(), 2);
        for (_, state) in states {
            assert_eq!(state.status, TaskStatus::Pending);
            assert_eq!(state.generation, 7);
        }
        assert!(system.journal().contains(&format!("mkdir {DIR}")));
    }

    #[test]
    fn remove_deletes_the_state_file() {
        let system = FakeSystem::new();
        let store = StateStore::new(&system, DIR);
        store.reset(&[SetupTask::Browser], 1).expect("resets");
        store.remove(SetupTask::Browser).expect("removes");
        assert_eq!(store.load(SetupTask::Browser).expect("loads"), None);
    }

    #[test]
    fn a_broken_state_file_is_a_parse_error() {
        let system = FakeSystem::new().with_file(format!("{DIR}/browser.ron"), "(status: Maybe)");
        let store = StateStore::new(&system, DIR);
        assert!(matches!(
            store.load(SetupTask::Browser),
            Err(TaskError::Parse { .. })
        ));
    }
}
