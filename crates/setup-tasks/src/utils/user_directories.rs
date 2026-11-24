//! The XDG base directories of the user.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserDirectories {
    pub home: PathBuf,
    pub config_home: PathBuf,
    pub state_home: PathBuf,
}

impl UserDirectories {
    /// The XDG base directories. A missing or relative `config_home` or
    /// `state_home` gives the default under `home`, as the XDG specification
    /// tells.
    pub fn new(home: PathBuf, config_home: Option<PathBuf>, state_home: Option<PathBuf>) -> Self {
        let or_default = |directory: Option<PathBuf>, default: &str| {
            directory
                .filter(|directory| directory.is_absolute())
                .unwrap_or_else(|| home.join(default))
        };
        Self {
            config_home: or_default(config_home, ".config"),
            state_home: or_default(state_home, ".local/state"),
            home,
        }
    }

    pub fn marker_file(&self) -> PathBuf {
        self.state_home.join("crownos/setup/user.ron")
    }

    pub fn visual_context_file(&self) -> PathBuf {
        self.config_home.join("crownos/visual_context.ron")
    }

    pub fn accessibility_flags_record(&self) -> PathBuf {
        self.state_home
            .join("crownos/setup/accessibility-flags.ron")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_relative_xdg_directories_use_the_defaults() {
        let directories =
            UserDirectories::new("/home/ada".into(), None, Some("relative/state".into()));
        assert_eq!(directories.config_home, PathBuf::from("/home/ada/.config"));
        assert_eq!(
            directories.marker_file(),
            PathBuf::from("/home/ada/.local/state/crownos/setup/user.ron")
        );
    }

    #[test]
    fn absolute_xdg_directories_are_used() {
        let directories = UserDirectories::new(
            "/home/ada".into(),
            Some("/srv/config".into()),
            Some("/srv/state".into()),
        );
        assert_eq!(
            directories.visual_context_file(),
            PathBuf::from("/srv/config/crownos/visual_context.ron")
        );
        assert_eq!(
            directories.marker_file(),
            PathBuf::from("/srv/state/crownos/setup/user.ron")
        );
    }
}
