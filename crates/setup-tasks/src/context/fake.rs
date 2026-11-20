//! A [`System`] that keeps files and installed packages in memory and records
//! each call in [`FakeSystem::journal`]. `pacman` behaves like the real one:
//! an install adds its targets, a removal removes them and `pacman -Qq`
//! queries them, and `pacman -U` installs the package that its file name
//! names. A scripted result replaces this behaviour for one command line. A
//! command line can also write files, like `npm install` does. Any other
//! program runs only when [`FakeSystem::with_program`] installed it;
//! otherwise `run` fails like a missing binary.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{Output, System};
use crate::error::TaskError;

const FIRST_MODIFICATION_UNIX: u64 = 1_700_000_000;
/// The kernel stops following symbolic links after 40 hops.
const MAX_LINK_HOPS: usize = 40;

#[derive(Debug, Default)]
pub struct FakeSystem {
    scripted_results: RefCell<HashMap<String, VecDeque<Output>>>,
    command_writes: HashMap<String, Vec<(PathBuf, String)>>,
    is_online: bool,
    installed_packages: RefCell<BTreeSet<String>>,
    programs: BTreeSet<String>,
    files: RefCell<BTreeMap<PathBuf, String>>,
    directories: RefCell<BTreeSet<PathBuf>>,
    links: RefCell<BTreeMap<PathBuf, PathBuf>>,
    racing_links: BTreeMap<PathBuf, PathBuf>,
    modification_times: RefCell<BTreeMap<PathBuf, SystemTime>>,
    modification_count: Cell<u64>,
    /// Each call, in order: `run <command line>`, `write <path>`,
    /// `remove <path>`, `remove-all <path>`, `mkdir <path>`, `chown <user> <path>`,
    /// `symlink <link> -> <target>`, `sleep <seconds>s`.
    pub journal: RefCell<Vec<String>>,
}

impl FakeSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_file(self, path: impl Into<PathBuf>, contents: &str) -> Self {
        self.store_file(path.into(), contents);
        self
    }

    pub fn with_directory(self, path: impl Into<PathBuf>) -> Self {
        self.directories.borrow_mut().insert(path.into());
        self
    }

    pub fn with_link(self, link: impl Into<PathBuf>, target: impl Into<PathBuf>) -> Self {
        self.links.borrow_mut().insert(link.into(), target.into());
        self
    }

    /// Puts the program `name` on `PATH`, as `/usr/bin/<name>`. `run` then
    /// succeeds for it.
    pub fn with_program(mut self, name: &str) -> Self {
        self.programs.insert(name.to_owned());
        self
    }

    /// Makes the link `link` to `target` just before a [`System::symlink`]
    /// call for `link`, like another process that links at the same time.
    pub fn with_racing_link(
        mut self,
        link: impl Into<PathBuf>,
        target: impl Into<PathBuf>,
    ) -> Self {
        self.racing_links.insert(link.into(), target.into());
        self
    }

    pub fn with_installed(self, packages: &[&str]) -> Self {
        self.installed_packages
            .borrow_mut()
            .extend(packages.iter().map(|&package| package.to_owned()));
        self
    }

    /// Queues `output` for `command_line`. The last queued result repeats.
    pub fn with_result(self, command_line: &str, output: Output) -> Self {
        self.scripted_results
            .borrow_mut()
            .entry(command_line.to_owned())
            .or_default()
            .push_back(output);
        self
    }

    /// Writes `contents` to `path` each time `command_line` runs.
    pub fn with_command_writes(
        mut self,
        command_line: &str,
        path: impl Into<PathBuf>,
        contents: &str,
    ) -> Self {
        self.command_writes
            .entry(command_line.to_owned())
            .or_default()
            .push((path.into(), contents.to_owned()));
        self
    }

    /// Lets [`System::can_connect`] succeed.
    pub fn with_network(mut self) -> Self {
        self.is_online = true;
        self
    }

    pub fn with_failure(self, command_line: &str, stderr: &str) -> Self {
        self.with_result(
            command_line,
            Output {
                status: Some(1),
                stderr: stderr.to_owned(),
                ..Output::default()
            },
        )
    }

    pub fn file(&self, path: impl AsRef<Path>) -> Option<String> {
        self.files.borrow().get(path.as_ref()).cloned()
    }

    pub fn is_installed(&self, package: &str) -> bool {
        self.installed_packages.borrow().contains(package)
    }

    pub fn journal(&self) -> Vec<String> {
        self.journal.borrow().clone()
    }

    /// The command lines that ran, without the `run ` prefix.
    pub fn commands(&self) -> Vec<String> {
        self.journal
            .borrow()
            .iter()
            .filter_map(|entry| entry.strip_prefix("run "))
            .map(str::to_owned)
            .collect()
    }

    pub fn clear_journal(&self) {
        self.journal.borrow_mut().clear();
    }

    fn record(&self, entry: String) {
        self.journal.borrow_mut().push(entry);
    }

    fn store_file(&self, path: PathBuf, contents: &str) {
        let count = self.modification_count.get() + 1;
        self.modification_count.set(count);
        let time = UNIX_EPOCH + Duration::from_secs(FIRST_MODIFICATION_UNIX + count);
        self.modification_times
            .borrow_mut()
            .insert(path.clone(), time);
        self.files.borrow_mut().insert(path, contents.to_owned());
    }

    /// The file or directory that `path` names after its symbolic links, or
    /// `None` when there is none.
    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        let mut resolved = path.to_owned();
        for _ in 0..MAX_LINK_HOPS {
            if self.files.borrow().contains_key(&resolved)
                || self.directories.borrow().contains(&resolved)
            {
                return Some(resolved);
            }
            let target = self.links.borrow().get(&resolved).cloned()?;
            resolved = match resolved.parent() {
                Some(parent) => parent.join(target),
                None => target,
            };
        }
        None
    }

    fn scripted_result(&self, command_line: &str) -> Option<Output> {
        let mut scripted_results = self.scripted_results.borrow_mut();
        let queue = scripted_results.get_mut(command_line)?;
        if queue.len() > 1 {
            queue.pop_front()
        } else {
            queue.front().cloned()
        }
    }

    fn apply_command_writes(&self, command_line: &str) {
        for (path, contents) in self.command_writes.get(command_line).into_iter().flatten() {
            self.store_file(path.clone(), contents);
        }
    }

    fn simulate_pacman(&self, args: &[&str]) -> Output {
        let Some((operation, rest)) = args.split_first() else {
            return Output::default();
        };
        let targets = rest
            .iter()
            .filter(|arg| !arg.starts_with('-'))
            .map(|&target| package_of_file(target));
        let mut installed_packages = self.installed_packages.borrow_mut();
        if operation.starts_with("-Q") {
            let missing = targets
                .filter(|package| !installed_packages.contains(*package))
                .map(|package| format!("error: package '{package}' was not found\n"))
                .collect::<String>();
            return Output {
                status: Some(if missing.is_empty() { 0 } else { 1 }),
                stderr: missing,
                ..Output::default()
            };
        }
        if operation.starts_with("-R") {
            for package in targets {
                installed_packages.remove(package);
            }
        } else {
            installed_packages.extend(targets.map(str::to_owned));
        }
        Output {
            status: Some(0),
            ..Output::default()
        }
    }
}

/// The package name in a package file path such as
/// `/build/google-chrome-131.0-1-x86_64.pkg.tar.zst`, or `target` itself when
/// it is not a package file.
fn package_of_file(target: &str) -> &str {
    let Some(stem) = target
        .rsplit('/')
        .next()
        .and_then(|file_name| file_name.strip_suffix(".pkg.tar.zst"))
    else {
        return target;
    };
    stem.rsplitn(4, '-').nth(3).unwrap_or(stem)
}

fn command_line(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

impl System for FakeSystem {
    fn run(&self, program: &str, args: &[&str]) -> Result<Output, TaskError> {
        let line = command_line(program, args);
        self.record(format!("run {line}"));
        self.apply_command_writes(&line);
        if let Some(output) = self.scripted_result(&line) {
            return Ok(output);
        }
        if program == "pacman" {
            return Ok(self.simulate_pacman(args));
        }
        if !self.programs.contains(program) && !self.command_writes.contains_key(&line) {
            return Err(TaskError::io(program, io::ErrorKind::NotFound.into()));
        }
        Ok(Output {
            status: Some(0),
            ..Output::default()
        })
    }

    fn run_as(
        &self,
        user: &str,
        working_directory: &Path,
        environment: &[(&str, &str)],
        program: &str,
        args: &[&str],
    ) -> Result<Output, TaskError> {
        let assignments = environment
            .iter()
            .map(|(name, value)| format!("{name}={value} "))
            .collect::<String>();
        let line = format!(
            "as {user} in {}: {assignments}{}",
            working_directory.display(),
            command_line(program, args)
        );
        self.record(format!("run {line}"));
        self.apply_command_writes(&line);
        Ok(self.scripted_result(&line).unwrap_or(Output {
            status: Some(0),
            ..Output::default()
        }))
    }

    fn read_file(&self, path: &Path) -> Result<Option<String>, TaskError> {
        Ok(self.resolve(path).and_then(|resolved| self.file(resolved)))
    }

    fn write_file(&self, path: &Path, contents: &str, _mode: u32) -> Result<(), TaskError> {
        self.record(format!("write {}", path.display()));
        self.store_file(path.to_owned(), contents);
        Ok(())
    }

    /// Follows symbolic links like the real filesystem: a dangling link does
    /// not exist.
    fn exists(&self, path: &Path) -> bool {
        self.resolve(path).is_some()
    }

    fn is_directory(&self, path: &Path) -> bool {
        self.directories.borrow().contains(path)
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        self.programs
            .contains(name)
            .then(|| Path::new("/usr/bin").join(name))
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, TaskError> {
        let files = self.files.borrow();
        let directories = self.directories.borrow();
        let links = self.links.borrow();
        Ok(files
            .keys()
            .chain(directories.iter())
            .chain(links.keys())
            .filter(|entry| entry.parent() == Some(path))
            .cloned()
            .collect())
    }

    fn remove_file(&self, path: &Path) -> Result<(), TaskError> {
        self.record(format!("remove {}", path.display()));
        self.files.borrow_mut().remove(path);
        self.links.borrow_mut().remove(path);
        Ok(())
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), TaskError> {
        self.record(format!("remove-all {}", path.display()));
        self.files
            .borrow_mut()
            .retain(|entry, _| !entry.starts_with(path));
        self.directories
            .borrow_mut()
            .retain(|entry| !entry.starts_with(path));
        self.links
            .borrow_mut()
            .retain(|entry, _| !entry.starts_with(path));
        Ok(())
    }

    fn create_dir_all(&self, path: &Path, _mode: u32) -> Result<(), TaskError> {
        self.record(format!("mkdir {}", path.display()));
        self.directories.borrow_mut().insert(path.to_owned());
        Ok(())
    }

    fn chown(&self, path: &Path, user: &str) -> Result<(), TaskError> {
        self.record(format!("chown {user} {}", path.display()));
        Ok(())
    }

    fn modified(&self, path: &Path) -> Option<SystemTime> {
        self.modification_times.borrow().get(path).copied()
    }

    fn symlink(&self, target: &Path, link: &Path) -> Result<(), TaskError> {
        if let Some(racing_target) = self.racing_links.get(link) {
            self.links
                .borrow_mut()
                .insert(link.to_owned(), racing_target.clone());
        }
        if self.exists(link) || self.links.borrow().contains_key(link) {
            return Err(TaskError::io(link, io::ErrorKind::AlreadyExists.into()));
        }
        self.record(format!(
            "symlink {} -> {}",
            link.display(),
            target.display()
        ));
        self.links
            .borrow_mut()
            .insert(link.to_owned(), target.to_owned());
        Ok(())
    }

    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        self.links.borrow().get(path).cloned()
    }

    fn sleep(&self, duration: Duration) {
        self.record(format!("sleep {}s", duration.as_secs()));
    }

    fn can_connect(&self, _address: &str, _timeout: Duration) -> bool {
        self.is_online
    }
}
