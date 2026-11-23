//! Chromium, Google Chrome, Brave and Electron apps such as VS Code give the
//! accessibility bus no tree when they only see `IsEnabled`. They give one
//! when they start with `--force-renderer-accessibility=basic`. The visual
//! context engine reads that tree, so while `visual_context.enabled` is on,
//! the flag is in the flags file of each of these programs. Basic mode costs
//! CPU time and memory in each renderer, so the flag goes away when the engine
//! is off. A change applies when the program starts again.

use std::path::{Path, PathBuf};

use crownos_config::schema::VisualContext;
use ron::extensions::Extensions;

use crate::context::System;
use crate::error::TaskError;
use crate::utils::flags_file::{with_flag, without_flag};
use crate::utils::home_file;
use crate::utils::user_directories::UserDirectories;

pub const RENDERER_ACCESSIBILITY_FLAG: &str = "--force-renderer-accessibility=basic";

/// The files that the Arch Linux launchers read: chromium, google-chrome,
/// brave-bin, the electron wrappers (when no `electron<N>-flags.conf` exists)
/// and code.
const FLAGS_FILE_NAMES: [&str; 5] = [
    "chromium-flags.conf",
    "chrome-flags.conf",
    "brave-flags.conf",
    "electron-flags.conf",
    "code-flags.conf",
];

pub fn flags_files(directories: &UserDirectories) -> impl Iterator<Item = PathBuf> + '_ {
    FLAGS_FILE_NAMES
        .iter()
        .map(|name| directories.config_home.join(name))
}

/// Whether `visual_context.ron` turns the engine on. A missing file or one
/// that does not parse means off, as for the engine.
pub fn is_visual_context_enabled(
    system: &dyn System,
    directories: &UserDirectories,
) -> Result<bool, TaskError> {
    let Some(text) = system.read_file(&directories.visual_context_file())? else {
        return Ok(false);
    };
    Ok(ron::Options::default()
        .with_default_extension(Extensions::IMPLICIT_SOME)
        .from_str::<VisualContext>(&text)
        .is_ok_and(|visual_context| visual_context.enabled))
}

/// Adds the flag to `path` when `enabled`, and removes it otherwise. Returns
/// whether the file changed. Only the exact flag is removed, so a flag that
/// the user set with another mode stays.
pub fn update_flags_file(
    system: &dyn System,
    path: &Path,
    enabled: bool,
) -> Result<bool, TaskError> {
    let text = system.read_file(path)?;
    let edited_text = match (enabled, text) {
        (true, text) => with_flag(
            text.as_deref().unwrap_or_default(),
            RENDERER_ACCESSIBILITY_FLAG,
        ),
        (false, Some(text)) => without_flag(&text, RENDERER_ACCESSIBILITY_FLAG),
        (false, None) => None,
    };
    let Some(edited_text) = edited_text else {
        return Ok(false);
    };
    home_file::write(system, path, &edited_text)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    const VISUAL_CONTEXT: &str = "/home/ada/.config/crownos/visual_context.ron";
    const CHROMIUM_FLAGS: &str = "/home/ada/.config/chromium-flags.conf";

    fn directories() -> UserDirectories {
        UserDirectories::new("/home/ada".into(), None, None)
    }

    #[test]
    fn the_engine_is_off_without_a_valid_file_that_turns_it_on() {
        let directories = directories();
        for (text, enabled) in [
            (None, false),
            (Some("(enabled: true, retention_days: 7)"), true),
            (Some("(enabled: false)"), false),
            (Some("(enabled: tru"), false),
        ] {
            let system = match text {
                Some(text) => FakeSystem::new().with_file(VISUAL_CONTEXT, text),
                None => FakeSystem::new(),
            };
            assert_eq!(
                is_visual_context_enabled(&system, &directories).expect("reads"),
                enabled,
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_flag_is_added_then_removed_and_other_lines_stay() {
        let system = FakeSystem::new().with_file(CHROMIUM_FLAGS, "--ozone-platform=wayland\n");
        let path = Path::new(CHROMIUM_FLAGS);
        assert!(update_flags_file(&system, path, true).expect("writes"));
        assert!(!update_flags_file(&system, path, true).expect("reads"));
        assert_eq!(
            system.file(CHROMIUM_FLAGS).as_deref(),
            Some("--ozone-platform=wayland\n--force-renderer-accessibility=basic\n")
        );
        assert!(update_flags_file(&system, path, false).expect("writes"));
        assert_eq!(
            system.file(CHROMIUM_FLAGS).as_deref(),
            Some("--ozone-platform=wayland\n")
        );
    }

    #[test]
    fn a_missing_file_is_made_only_when_the_engine_is_on() {
        let path = "/home/ada/.config/electron-flags.conf";
        let system = FakeSystem::new();
        assert!(!update_flags_file(&system, Path::new(path), false).expect("reads"));
        assert!(system.journal().is_empty());
        assert!(update_flags_file(&system, Path::new(path), true).expect("writes"));
        assert_eq!(
            system.file(path).as_deref(),
            Some("--force-renderer-accessibility=basic\n")
        );
    }

    #[test]
    fn a_linked_flags_file_is_not_written() {
        let system = FakeSystem::new()
            .with_file("/home/ada/dotfiles/chromium-flags.conf", "")
            .with_link(CHROMIUM_FLAGS, "/home/ada/dotfiles/chromium-flags.conf");
        assert!(matches!(
            update_flags_file(&system, Path::new(CHROMIUM_FLAGS), true),
            Err(TaskError::SymbolicLink(_))
        ));
    }
}
