use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write as _};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use super::{Output, System};
use crate::error::TaskError;

/// The machine: subprocesses and the filesystem.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealSystem;

impl RealSystem {
    /// Runs `command` in the C locale, so that the messages that the tasks
    /// match, such as the pacman lock error, are in English.
    fn output_of(mut command: Command, program: &str) -> Result<Output, TaskError> {
        let output = command
            .env("LC_ALL", "C")
            .output()
            .map_err(|source| TaskError::io(program, source))?;
        Ok(Output {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            time_limit: None,
        })
    }
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

impl System for RealSystem {
    fn run(&self, program: &str, args: &[&str]) -> Result<Output, TaskError> {
        let mut command = Command::new(program);
        command.args(args);
        Self::output_of(command, program)
    }

    fn run_as(
        &self,
        user: &str,
        working_directory: &Path,
        environment: &[(&str, &str)],
        program: &str,
        args: &[&str],
    ) -> Result<Output, TaskError> {
        let mut command = Command::new("runuser");
        command.args(["-u", user, "--", "env"]);
        command.args(
            environment
                .iter()
                .map(|(name, value)| format!("{name}={value}")),
        );
        command
            .arg(program)
            .args(args)
            .current_dir(working_directory);
        Self::output_of(command, program)
    }

    fn read_file(&self, path: &Path) -> Result<Option<String>, TaskError> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(TaskError::io(path, source)),
        }
    }

    fn write_file(&self, path: &Path, contents: &str, mode: u32) -> Result<(), TaskError> {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        let temporary_path =
            path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
        let write = || -> io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .open(&temporary_path)?;
            file.set_permissions(fs::Permissions::from_mode(mode))?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary_path, path)
        };
        write().map_err(|source| {
            let _ = fs::remove_file(&temporary_path);
            TaskError::io(path, source)
        })
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_directory(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        let search_path = std::env::var_os("PATH")?;
        std::env::split_paths(&search_path)
            .map(|directory| directory.join(name))
            .find(|candidate| is_executable_file(candidate))
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>, TaskError> {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(TaskError::io(path, source)),
        };
        entries
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<_>>()
            .map_err(|source| TaskError::io(path, source))
    }

    fn remove_file(&self, path: &Path) -> Result<(), TaskError> {
        match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(TaskError::io(path, error))
            }
            _ => Ok(()),
        }
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), TaskError> {
        match fs::remove_dir_all(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(TaskError::io(path, error))
            }
            _ => Ok(()),
        }
    }

    fn create_dir_all(&self, path: &Path, mode: u32) -> Result<(), TaskError> {
        DirBuilder::new()
            .recursive(true)
            .mode(mode)
            .create(path)
            .and_then(|()| fs::set_permissions(path, fs::Permissions::from_mode(mode)))
            .map_err(|source| TaskError::io(path, source))
    }

    fn chown(&self, path: &Path, user: &str) -> Result<(), TaskError> {
        let owner = format!("{user}:");
        let path_text = path.to_string_lossy();
        self.run("chown", &["--", &owner, &path_text])?
            .check("chown")
            .map(drop)
    }

    fn modified(&self, path: &Path) -> Option<SystemTime> {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    }

    fn symlink(&self, target: &Path, link: &Path) -> Result<(), TaskError> {
        std::os::unix::fs::symlink(target, link).map_err(|source| TaskError::io(link, source))
    }

    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        fs::read_link(path).ok()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn can_connect(&self, address: &str, timeout: Duration) -> bool {
        address.to_socket_addrs().is_ok_and(|mut addresses| {
            addresses.any(|address| TcpStream::connect_timeout(&address, timeout).is_ok())
        })
    }
}
