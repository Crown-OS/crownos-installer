//! `crownos-setup user --wait` waits for the root runner.

use std::time::Duration;

use super::SetupSnapshot;
use crate::context::System;

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const WAIT_LIMIT: Duration = Duration::from_secs(30 * 60);

/// Reads the choices and the task states every 5 seconds until each wanted
/// task is `Done` for the current choices. `Pending`, `Running` and `Failed`
/// tasks keep the wait. A broken `/etc/crownos/setup.ron` counts as the
/// default choices. `None` after 30 minutes.
pub fn wait_for_wanted_tasks(system: &dyn System) -> Option<SetupSnapshot> {
    let polls = WAIT_LIMIT.as_secs() / POLL_INTERVAL.as_secs();
    for poll in 0..=polls {
        let snapshot = SetupSnapshot::load(system)
            .unwrap_or_else(|_| SetupSnapshot::with_default_choices(system));
        if snapshot.is_complete() {
            return Some(snapshot);
        }
        if poll < polls {
            system.sleep(POLL_INTERVAL);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crownos_config::system::{SETUP_CONFIG_PATH, SETUP_STATE_DIR, SetupTask, TaskStatus};

    use super::*;
    use crate::config;
    use crate::context::fake::FakeSystem;
    use crate::state::StateStore;
    use crate::user::snapshot::tests::state;

    fn sleeps(system: &FakeSystem) -> usize {
        system
            .journal()
            .iter()
            .filter(|entry| *entry == "sleep 5s")
            .count()
    }

    #[test]
    fn the_wait_ends_when_each_wanted_task_is_done() {
        let system = FakeSystem::new().with_file(SETUP_CONFIG_PATH, "(browser: Brave)");
        let generation = config::generation(&system, SETUP_CONFIG_PATH.as_ref());
        let store = StateStore::new(&system, SETUP_STATE_DIR);
        store
            .save(SetupTask::Browser, &state(TaskStatus::Done, generation))
            .expect("saves");
        store
            .save(SetupTask::Playwright, &state(TaskStatus::Done, generation))
            .expect("saves");

        let snapshot = wait_for_wanted_tasks(&system).expect("complete");
        assert_eq!(snapshot.generation, generation);
        assert_eq!(sleeps(&system), 0);
    }

    #[test]
    fn a_failed_or_pending_task_waits_thirty_minutes() {
        let system = FakeSystem::new().with_file(SETUP_CONFIG_PATH, "(browser: Brave)");
        let generation = config::generation(&system, SETUP_CONFIG_PATH.as_ref());
        let store = StateStore::new(&system, SETUP_STATE_DIR);
        store
            .save(SetupTask::Browser, &state(TaskStatus::Done, generation))
            .expect("saves");
        store
            .save(
                SetupTask::Playwright,
                &state(
                    TaskStatus::Failed {
                        message: "npm".to_owned(),
                    },
                    generation,
                ),
            )
            .expect("saves");

        assert_eq!(wait_for_wanted_tasks(&system), None);
        assert_eq!(sleeps(&system), 360);
    }
}
