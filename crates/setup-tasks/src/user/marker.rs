//! `$XDG_STATE_HOME/crownos/setup/user.ron`: the generation for which the
//! notification is done.

use std::path::Path;

use serde::Deserialize;

use crate::context::System;
use crate::error::TaskError;
use crate::utils::home_file;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct UserMarker {
    pub notified_generation: u64,
}

impl UserMarker {
    /// The marker in `path`. A missing or broken file gives zeros, so that the
    /// steps run again.
    pub fn load(system: &dyn System, path: &Path) -> Self {
        system
            .read_file(path)
            .ok()
            .flatten()
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn store(&self, system: &dyn System, path: &Path) -> Result<(), TaskError> {
        let Self {
            notified_generation,
        } = self;
        home_file::write(
            system,
            path,
            &format!("(notified_generation: {notified_generation})\n"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    const PATH: &str = "/home/ada/.local/state/crownos/setup/user.ron";

    #[test]
    fn a_stored_marker_loads_back() {
        let system = FakeSystem::new();
        let marker = UserMarker {
            notified_generation: 5,
        };
        marker.store(&system, Path::new(PATH)).expect("stores");
        assert_eq!(UserMarker::load(&system, Path::new(PATH)), marker);
    }

    #[test]
    fn a_marker_of_the_old_format_keeps_its_notified_generation() {
        let system =
            FakeSystem::new().with_file(PATH, "(dock_generation: 7, notified_generation: 5)\n");
        assert_eq!(
            UserMarker::load(&system, Path::new(PATH)),
            UserMarker {
                notified_generation: 5
            }
        );
    }

    #[test]
    fn a_missing_or_broken_marker_is_zero() {
        let system = FakeSystem::new().with_file(PATH, "(notified_generation: soon)");
        assert_eq!(
            UserMarker::load(&system, Path::new(PATH)),
            UserMarker::default()
        );
        assert_eq!(
            UserMarker::load(&FakeSystem::new(), Path::new(PATH)),
            UserMarker::default()
        );
    }
}
