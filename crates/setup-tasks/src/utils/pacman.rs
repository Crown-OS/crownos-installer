//! The pacman calls of the setup tasks. An install always refreshes and
//! upgrades the system (`-Syu`), because `-Sy` without `-u` gives a partial
//! upgrade. A `--` separates the options from the targets. Each call runs
//! under [`INSTALL_TIME_LIMIT`]. When another program holds the database
//! lock, each call waits and tries again. It never deletes `db.lck`.

use std::time::Duration;

use crate::context::{Output, System};
use crate::error::TaskError;
use crate::utils::time_limit::{self, INSTALL_TIME_LIMIT};

const LOCK_RETRY_INTERVAL: Duration = Duration::from_secs(15);
const LOCK_WAIT_LIMIT: Duration = Duration::from_secs(20 * 60);
const LOCKED_DATABASE_MESSAGE: &str = "unable to lock database";

pub fn is_installed(system: &dyn System, package: &str) -> bool {
    time_limit::run(
        system,
        INSTALL_TIME_LIMIT,
        "pacman",
        &["-Qq", "--", package],
    )
    .is_ok_and(|output| output.succeeded())
}

pub fn install(system: &dyn System, packages: &[&str]) -> Result<(), TaskError> {
    run_pacman(system, &["-Syu", "--needed", "--noconfirm"], packages)
}

pub fn upgrade_system(system: &dyn System) -> Result<(), TaskError> {
    run_pacman(system, &["-Syu", "--noconfirm"], &[])
}

/// Installs `packages` as dependencies. Call [`upgrade_system`] first: this
/// call does not refresh the databases.
pub fn install_as_dependencies(system: &dyn System, packages: &[&str]) -> Result<(), TaskError> {
    run_pacman(
        system,
        &["-S", "--needed", "--asdeps", "--noconfirm"],
        packages,
    )
}

pub fn install_file(system: &dyn System, package_file: &str) -> Result<(), TaskError> {
    run_pacman(system, &["-U", "--noconfirm", "--needed"], &[package_file])
}

pub fn remove(system: &dyn System, packages: &[&str]) -> Result<(), TaskError> {
    run_pacman(system, &["-Rs", "--noconfirm"], packages)
}

fn run_pacman(system: &dyn System, options: &[&str], targets: &[&str]) -> Result<(), TaskError> {
    let separator: &[&str] = if targets.is_empty() { &[] } else { &["--"] };
    let args = [options, separator, targets].concat();
    let attempts = LOCK_WAIT_LIMIT.as_secs() / LOCK_RETRY_INTERVAL.as_secs();
    for _ in 0..attempts {
        let output = time_limit::run(system, INSTALL_TIME_LIMIT, "pacman", &args)?;
        if !is_database_locked(&output) {
            return output
                .check(&format!("pacman {}", args.join(" ")))
                .map(drop);
        }
        system.sleep(LOCK_RETRY_INTERVAL);
    }
    Err(TaskError::PacmanLocked)
}

fn is_database_locked(output: &Output) -> bool {
    !output.succeeded() && output.stderr.contains(LOCKED_DATABASE_MESSAGE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    const LOCKED_STDERR: &str = "error: failed to init transaction (unable to lock database)\nerror: could not lock database: File exists\n";
    const INSTALL_LINE: &str = "pacman -Syu --needed --noconfirm -- nodejs npm";

    fn failure(stderr: &str) -> Output {
        Output {
            status: Some(1),
            stderr: stderr.to_owned(),
            ..Output::default()
        }
    }

    #[test]
    fn only_a_failed_run_with_the_lock_message_is_a_locked_database() {
        assert!(is_database_locked(&failure(LOCKED_STDERR)));
        assert!(!is_database_locked(&failure(
            "error: target not found: nodejs"
        )));
        assert!(!is_database_locked(&Output {
            status: Some(0),
            stderr: LOCKED_STDERR.to_owned(),
            ..Output::default()
        }));
    }

    #[test]
    fn a_locked_database_is_retried_until_it_is_free() {
        let system = FakeSystem::new()
            .with_result(INSTALL_LINE, failure(LOCKED_STDERR))
            .with_result(INSTALL_LINE, failure(LOCKED_STDERR))
            .with_result(
                INSTALL_LINE,
                Output {
                    status: Some(0),
                    ..Output::default()
                },
            );
        install(&system, &["nodejs", "npm"]).expect("installs after the lock is free");
        let journal = system.journal();
        assert_eq!(
            journal.iter().filter(|entry| *entry == "sleep 15s").count(),
            2
        );
        assert!(!journal.iter().any(|entry| entry.contains("db.lck")));
    }

    #[test]
    fn a_lock_that_stays_gives_up_after_twenty_minutes() {
        let system = FakeSystem::new().with_failure(INSTALL_LINE, LOCKED_STDERR);
        assert!(matches!(
            install(&system, &["nodejs", "npm"]),
            Err(TaskError::PacmanLocked)
        ));
        assert_eq!(system.commands().len(), 80);
    }

    #[test]
    fn another_failure_is_a_command_error_with_stderr() {
        let system = FakeSystem::new().with_failure(INSTALL_LINE, "error: target not found: npm");
        let error = install(&system, &["nodejs", "npm"]).expect_err("fails");
        assert!(
            error.to_string().contains("target not found: npm"),
            "{error}"
        );
    }

    #[test]
    fn a_system_upgrade_has_no_separator() {
        let system = FakeSystem::new();
        upgrade_system(&system).expect("upgrades");
        assert_eq!(system.commands(), ["pacman -Syu --noconfirm"]);
    }

    #[test]
    fn is_installed_asks_pacman() {
        let system = FakeSystem::new().with_installed(&["firefox"]);
        assert!(is_installed(&system, "firefox"));
        assert!(!is_installed(&system, "brave-bin"));
        assert_eq!(
            system.commands(),
            ["pacman -Qq -- firefox", "pacman -Qq -- brave-bin"]
        );
    }
}
