//! Reads and writes `/etc/crownos/setup.ron` through a [`System`].

use std::path::Path;
use std::time::UNIX_EPOCH;

use crownos_config::system::SetupConfig;

use crate::context::System;
use crate::error::TaskError;

const CONFIG_MODE: u32 = 0o644;

/// The choices in `path`, or the defaults when the file does not exist.
pub fn load_or_default(system: &dyn System, path: &Path) -> Result<SetupConfig, TaskError> {
    match system.read_file(path)? {
        Some(text) => Ok(SetupConfig::parse(&text)?),
        None => Ok(SetupConfig::default()),
    }
}

pub fn store(system: &dyn System, path: &Path, config: &SetupConfig) -> Result<(), TaskError> {
    system.write_file(path, &config.render(), CONFIG_MODE)
}

/// The modification time of `path` in Unix seconds, or 0 when it is unknown.
/// Each change of the choices gives a new generation.
pub fn generation(system: &dyn System, path: &Path) -> u64 {
    system
        .modified(path)
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since_epoch| since_epoch.as_secs())
}

#[cfg(test)]
mod tests {
    use crownos_config::system::{Browser, DeveloperTools};

    use super::*;
    use crate::context::fake::FakeSystem;

    const PATH: &str = "/etc/crownos/setup.ron";

    #[test]
    fn a_missing_file_gives_the_defaults_and_generation_zero() {
        let system = FakeSystem::new();
        let config = load_or_default(&system, Path::new(PATH)).expect("loads");
        assert_eq!(config, SetupConfig::default());
        assert_eq!(generation(&system, Path::new(PATH)), 0);
    }

    #[test]
    fn a_stored_config_loads_back_with_a_new_generation() {
        let system = FakeSystem::new();
        let config = SetupConfig {
            browser: Browser::Firefox,
            developer_tools: DeveloperTools {
                codex: true,
                ..DeveloperTools::default()
            },
        };
        store(&system, Path::new(PATH), &config).expect("stores");
        let first_generation = generation(&system, Path::new(PATH));
        store(&system, Path::new(PATH), &config).expect("stores");

        assert_eq!(
            load_or_default(&system, Path::new(PATH)).expect("loads"),
            config
        );
        assert!(generation(&system, Path::new(PATH)) > first_generation);
    }

    #[test]
    fn a_broken_file_is_an_error() {
        let system = FakeSystem::new().with_file(PATH, "(browser: Edge)");
        assert!(matches!(
            load_or_default(&system, Path::new(PATH)),
            Err(TaskError::Config(_))
        ));
    }
}
