//! Chromium, Google Chrome, Brave and Electron apps such as VS Code give the
//! accessibility bus no tree when they only see `IsEnabled`. They give one
//! when they start with `--force-renderer-accessibility=basic`. The visual
//! context engine reads that tree, so while `visual_context.enabled` is on,
//! the flag is in the flags file of each of these programs that is installed.
//! Basic mode costs CPU time and memory in each renderer, so the flag goes
//! away when the engine is off, but only from the files that the setup added
//! it to. A change applies when the program starts again.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crownos_config::patch::from_ron;
use crownos_config::schema::VisualContext;

use crate::context::System;
use crate::error::TaskError;
use crate::utils::flags_file::{with_flag, without_flag};
use crate::utils::home_file;
use crate::utils::user_directories::UserDirectories;

pub const RENDERER_ACCESSIBILITY_FLAG: &str = "--force-renderer-accessibility=basic";

const ELECTRON_RUNTIME_DIRECTORY: &str = "/usr/lib";

/// What shows that the program of a flags file is installed.
enum Reader {
    Program(&'static str),
    /// The Arch Linux `electron<N>` wrappers, in `/usr/lib/electron<N>`. They
    /// read `electron-flags.conf` when no `electron<N>-flags.conf` exists.
    ElectronRuntime,
}

impl Reader {
    fn is_installed(&self, system: &dyn System) -> bool {
        match self {
            Self::Program(program) => system.find_program(program).is_some(),
            Self::ElectronRuntime => system
                .list_dir(Path::new(ELECTRON_RUNTIME_DIRECTORY))
                .is_ok_and(|entries| {
                    entries.iter().any(|entry| {
                        entry
                            .file_name()
                            .and_then(OsStr::to_str)
                            .is_some_and(|name| name.starts_with("electron"))
                    })
                }),
        }
    }
}

const FLAGS_FILES: [(&str, Reader); 5] = [
    ("chromium-flags.conf", Reader::Program("chromium")),
    ("chrome-flags.conf", Reader::Program("google-chrome-stable")),
    ("brave-flags.conf", Reader::Program("brave")),
    ("electron-flags.conf", Reader::ElectronRuntime),
    ("code-flags.conf", Reader::Program("code")),
];

#[derive(Debug, Default)]
pub struct FlagsUpdate {
    pub changed: Vec<PathBuf>,
    pub linked: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, TaskError)>,
}

impl FlagsUpdate {
    /// Keeps the outcome of one file. True when the step succeeded.
    fn record(&mut self, path: PathBuf, result: Result<bool, TaskError>) -> bool {
        match result {
            Ok(true) => {
                self.changed.push(path);
                true
            }
            Ok(false) => true,
            Err(TaskError::SymbolicLink(link)) => {
                self.linked.push(link);
                false
            }
            Err(error) => {
                self.failed.push((path, error));
                false
            }
        }
    }
}

/// Makes the flags files follow `visual_context.enabled`. A
/// `visual_context.ron` that does not parse changes nothing.
pub fn follow_visual_context(system: &dyn System, directories: &UserDirectories) -> FlagsUpdate {
    let mut update = FlagsUpdate::default();
    let visual_context_file = directories.visual_context_file();
    let enabled = match visual_context_switch(system, &visual_context_file) {
        Ok(Some(enabled)) => enabled,
        Ok(None) => return update,
        Err(error) => {
            update.failed.push((visual_context_file, error));
            return update;
        }
    };

    let record_path = directories.accessibility_flags_record();
    let recorded = load_record(system, &record_path);
    let mut added = recorded.clone();
    for (file_name, reader) in &FLAGS_FILES {
        let path = directories.config_home.join(file_name);
        if enabled && reader.is_installed(system) {
            let result = add_flag(system, &path);
            let wrote = matches!(result, Ok(true));
            if update.record(path, result) && wrote {
                added.insert((*file_name).to_owned());
            }
        } else if !enabled && added.contains(*file_name) {
            let result = remove_flag(system, &path);
            if update.record(path, result) {
                added.remove(*file_name);
            }
        }
    }
    if added != recorded
        && let Err(error) = store_record(system, &record_path, &added)
    {
        update.failed.push((record_path, error));
    }
    update
}

/// `Some(enabled)` from `path`, `Some(false)` when it is missing, and `None`
/// when it does not parse.
fn visual_context_switch(system: &dyn System, path: &Path) -> Result<Option<bool>, TaskError> {
    let Some(text) = system.read_file(path)? else {
        return Ok(Some(false));
    };
    Ok(
        from_ron::<VisualContext>("visual_context", "enabled", &text)
            .ok()
            .map(|visual_context| visual_context.enabled),
    )
}

fn add_flag(system: &dyn System, path: &Path) -> Result<bool, TaskError> {
    home_file::refuse_symbolic_link(system, path)?;
    let text = system.read_file(path)?.unwrap_or_default();
    write_if_changed(system, path, with_flag(&text, RENDERER_ACCESSIBILITY_FLAG))
}

fn remove_flag(system: &dyn System, path: &Path) -> Result<bool, TaskError> {
    home_file::refuse_symbolic_link(system, path)?;
    let Some(text) = system.read_file(path)? else {
        return Ok(false);
    };
    write_if_changed(
        system,
        path,
        without_flag(&text, RENDERER_ACCESSIBILITY_FLAG),
    )
}

fn write_if_changed(
    system: &dyn System,
    path: &Path,
    edited_text: Option<String>,
) -> Result<bool, TaskError> {
    let Some(edited_text) = edited_text else {
        return Ok(false);
    };
    home_file::write(system, path, &edited_text)?;
    Ok(true)
}

/// The flags file names that the setup added the flag to. A missing or broken
/// record is empty, so the setup never removes a flag that it did not add.
fn load_record(system: &dyn System, path: &Path) -> BTreeSet<String> {
    system
        .read_file(path)
        .ok()
        .flatten()
        .and_then(|text| ron::from_str(&text).ok())
        .unwrap_or_default()
}

fn store_record(
    system: &dyn System,
    path: &Path,
    file_names: &BTreeSet<String>,
) -> Result<(), TaskError> {
    let quoted_names: Vec<String> = file_names.iter().map(|name| format!("{name:?}")).collect();
    home_file::write(system, path, &format!("[{}]\n", quoted_names.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    const VISUAL_CONTEXT: &str = "/home/ada/.config/crownos/visual_context.ron";
    const CHROMIUM_FLAGS: &str = "/home/ada/.config/chromium-flags.conf";
    const CODE_FLAGS: &str = "/home/ada/.config/code-flags.conf";
    const ELECTRON_FLAGS: &str = "/home/ada/.config/electron-flags.conf";
    const RECORD: &str = "/home/ada/.local/state/crownos/setup/accessibility-flags.ron";
    const FLAG_LINE: &str = "--force-renderer-accessibility=basic\n";

    fn directories() -> UserDirectories {
        UserDirectories::new("/home/ada".into(), None, None)
    }

    fn with_switch(system: FakeSystem, enabled: bool) -> FakeSystem {
        system.with_file(VISUAL_CONTEXT, &format!("(enabled: {enabled})"))
    }

    #[test]
    fn only_installed_programs_get_the_flag_and_other_lines_stay() {
        let system = with_switch(FakeSystem::new(), true)
            .with_program("chromium")
            .with_directory("/usr/lib/electron39")
            .with_file(CHROMIUM_FLAGS, "--ozone-platform=wayland\n");
        let update = follow_visual_context(&system, &directories());
        assert_eq!(
            update.changed,
            [PathBuf::from(CHROMIUM_FLAGS), PathBuf::from(ELECTRON_FLAGS)]
        );
        assert_eq!(
            system.file(CHROMIUM_FLAGS).as_deref(),
            Some("--ozone-platform=wayland\n--force-renderer-accessibility=basic\n")
        );
        assert_eq!(system.file(ELECTRON_FLAGS).as_deref(), Some(FLAG_LINE));
        assert_eq!(system.file(CODE_FLAGS), None);
        assert_eq!(
            system.file(RECORD).as_deref(),
            Some("[\"chromium-flags.conf\", \"electron-flags.conf\"]\n")
        );

        let system = with_switch(system, false);
        let update = follow_visual_context(&system, &directories());
        assert_eq!(update.changed.len(), 2);
        assert_eq!(
            system.file(CHROMIUM_FLAGS).as_deref(),
            Some("--ozone-platform=wayland\n")
        );
        assert_eq!(system.file(RECORD).as_deref(), Some("[]\n"));
    }

    #[test]
    fn a_flag_that_the_user_added_stays_when_the_engine_is_off() {
        let system = with_switch(FakeSystem::new(), false)
            .with_program("code")
            .with_file(CODE_FLAGS, FLAG_LINE);
        let update = follow_visual_context(&system, &directories());
        assert!(update.changed.is_empty());
        assert!(system.journal().is_empty());

        let system = with_switch(system, true);
        follow_visual_context(&system, &directories());
        let system = with_switch(system, false);
        follow_visual_context(&system, &directories());
        assert_eq!(system.file(CODE_FLAGS).as_deref(), Some(FLAG_LINE));
    }

    #[test]
    fn a_settings_file_that_does_not_parse_changes_nothing() {
        let system = FakeSystem::new()
            .with_program("chromium")
            .with_file(VISUAL_CONTEXT, "(enabled: tru")
            .with_file(CHROMIUM_FLAGS, FLAG_LINE)
            .with_file(RECORD, "[\"chromium-flags.conf\"]");
        let update = follow_visual_context(&system, &directories());
        assert!(update.changed.is_empty() && update.failed.is_empty());
        assert!(system.journal().is_empty());
    }

    #[test]
    fn a_missing_settings_file_means_off() {
        let system = FakeSystem::new()
            .with_program("chromium")
            .with_file(CHROMIUM_FLAGS, FLAG_LINE)
            .with_file(RECORD, "[\"chromium-flags.conf\"]");
        let update = follow_visual_context(&system, &directories());
        assert_eq!(update.changed, [PathBuf::from(CHROMIUM_FLAGS)]);
        assert_eq!(system.file(CHROMIUM_FLAGS).as_deref(), Some(""));
    }

    #[test]
    fn a_linked_flags_file_is_not_written() {
        let system = with_switch(FakeSystem::new(), true)
            .with_program("chromium")
            .with_file("/home/ada/dotfiles/chromium-flags.conf", "")
            .with_link(CHROMIUM_FLAGS, "/home/ada/dotfiles/chromium-flags.conf");
        let update = follow_visual_context(&system, &directories());
        assert_eq!(update.linked, [PathBuf::from(CHROMIUM_FLAGS)]);
        assert_eq!(
            system
                .file("/home/ada/dotfiles/chromium-flags.conf")
                .as_deref(),
            Some("")
        );
        assert_eq!(system.file(RECORD), None);
    }
}
