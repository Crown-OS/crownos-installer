//! Points the dock of the user at the chosen browser, one time for each
//! generation of the choices, and only after the root runner installed the
//! browser.

use crownos_config::system::SetupTask;

use super::{SetupSnapshot, UserMarker};
use crate::context::System;
use crate::error::TaskError;
use crate::utils::user_directories::UserDirectories;
use crate::utils::{dock, home_file};

/// True when the dock file changed. Records the generation in the marker
/// when the dock names the browser after the step. While the `browser` task
/// is not `Done`, or while the dock has no file or no browser entry, the
/// marker stays and the next login tries again.
pub fn update_once(
    system: &dyn System,
    directories: &UserDirectories,
    snapshot: &SetupSnapshot,
) -> Result<bool, TaskError> {
    let marker_file = directories.marker_file();
    let mut marker = UserMarker::load(system, &marker_file);
    if marker.dock_generation >= snapshot.generation || !snapshot.is_done(SetupTask::Browser) {
        return Ok(false);
    }
    let dock_file = directories.dock_file();
    let Some(dock_text) = system.read_file(&dock_file)? else {
        return Ok(false);
    };
    let browser = snapshot.config.browser;
    let edited_dock = dock::replace_browser(&dock_text, browser);
    if let Some(edited_text) = &edited_dock {
        home_file::write(system, &dock_file, edited_text)?;
    } else if !dock::names(&dock_text, browser) {
        return Ok(false);
    }
    marker.dock_generation = snapshot.generation;
    marker.store(system, &marker_file)?;
    Ok(edited_dock.is_some())
}

#[cfg(test)]
mod tests {
    use crownos_config::system::{Browser, SetupConfig, TaskStatus};

    use super::*;
    use crate::context::fake::FakeSystem;
    use crate::user::snapshot::tests::state;

    const DOCK: &str = "/home/ada/.config/crownos/dock.ron";
    const MARKER: &str = "/home/ada/.local/state/crownos/setup/user.ron";
    const CHROMIUM_DOCK: &str = "(items: [\"/usr/share/applications/chromium.desktop\"])";

    fn directories() -> UserDirectories {
        UserDirectories::new("/home/ada".into(), None, None)
    }

    fn snapshot(status: TaskStatus) -> SetupSnapshot {
        SetupSnapshot {
            config: SetupConfig {
                browser: Browser::Brave,
                ..SetupConfig::default()
            },
            generation: 40,
            states: vec![(SetupTask::Browser, state(status, 40))],
        }
    }

    #[test]
    fn a_browser_that_is_not_done_leaves_the_dock_and_the_marker() {
        let system = FakeSystem::new().with_file(DOCK, CHROMIUM_DOCK);
        for status in [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::Failed {
                message: "offline".to_owned(),
            },
        ] {
            assert!(!update_once(&system, &directories(), &snapshot(status)).expect("runs"));
        }
        assert!(system.journal().is_empty());
    }

    #[test]
    fn a_done_browser_changes_the_dock_one_time() {
        let system = FakeSystem::new().with_file(DOCK, CHROMIUM_DOCK);
        assert!(update_once(&system, &directories(), &snapshot(TaskStatus::Done)).expect("runs"));
        assert_eq!(
            system.file(DOCK).as_deref(),
            Some("(items: [\"/usr/share/applications/brave-browser.desktop\"])")
        );
        assert_eq!(
            UserMarker::load(&system, MARKER.as_ref()).dock_generation,
            40
        );

        system
            .write_file(DOCK.as_ref(), CHROMIUM_DOCK, 0o600)
            .expect("the user changes the dock back");
        system.clear_journal();
        assert!(!update_once(&system, &directories(), &snapshot(TaskStatus::Done)).expect("runs"));
        assert!(system.journal().is_empty());
    }

    #[test]
    fn a_dock_that_names_the_browser_only_records_the_marker() {
        let brave_dock = "(items: [\"/usr/share/applications/brave-browser.desktop\"])";
        let system = FakeSystem::new().with_file(DOCK, brave_dock);
        assert!(!update_once(&system, &directories(), &snapshot(TaskStatus::Done)).expect("runs"));
        assert_eq!(
            system
                .journal()
                .iter()
                .filter(|entry| entry.starts_with("write"))
                .count(),
            1
        );
        assert_eq!(
            UserMarker::load(&system, MARKER.as_ref()).dock_generation,
            40
        );
    }

    #[test]
    fn a_missing_dock_or_a_dock_without_a_browser_leaves_the_marker() {
        let system = FakeSystem::new();
        assert!(!update_once(&system, &directories(), &snapshot(TaskStatus::Done)).expect("runs"));
        let system = FakeSystem::new()
            .with_file(DOCK, "(items: [\"/usr/share/applications/kitty.desktop\"])");
        assert!(!update_once(&system, &directories(), &snapshot(TaskStatus::Done)).expect("runs"));
        assert!(system.journal().is_empty());
        assert_eq!(
            UserMarker::load(&system, MARKER.as_ref()),
            UserMarker::default()
        );
    }

    #[test]
    fn a_new_generation_changes_the_dock_again() {
        let system = FakeSystem::new()
            .with_file(DOCK, CHROMIUM_DOCK)
            .with_file(MARKER, "(dock_generation: 30)");
        assert!(update_once(&system, &directories(), &snapshot(TaskStatus::Done)).expect("runs"));
    }
}
