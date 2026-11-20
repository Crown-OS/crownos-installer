//! Builds and installs a package from the AUR. The unprivileged system user
//! `crownos-build` clones and builds the package. Root installs only the
//! build dependencies and the built package.

use std::io;
use std::path::{Path, PathBuf};

use super::Context;
use crate::context::System;
use crate::error::TaskError;
use crate::utils::{pacman, srcinfo};

const BUILD_USER: &str = "crownos-build";
const BUILD_ROOT: &str = "/var/lib/crownos/setup/build";
const BUILD_DIRECTORY_MODE: u32 = 0o755;
const BUILD_TOOLS: [&str; 5] = ["fakeroot", "binutils", "debugedit", "file", "git"];
const PACKAGE_FILE_SUFFIX: &str = ".pkg.tar.zst";

pub fn install(context: &Context, package: &str) -> Result<(), TaskError> {
    let system = context.system;
    if !system.run("id", &["-u", BUILD_USER])?.succeeded() {
        return Err(TaskError::MissingUser(BUILD_USER));
    }

    let build_directory = Path::new(BUILD_ROOT).join(package);
    let build_directory_text = build_directory.to_string_lossy();
    let build_environment = [("HOME", build_directory_text.as_ref())];
    let run_as_build_user = |program: &str, args: &[&str]| {
        system.run_as(
            BUILD_USER,
            &build_directory,
            &build_environment,
            program,
            args,
        )
    };

    pacman::upgrade_system(system)?;
    pacman::install_as_dependencies(system, &BUILD_TOOLS)?;

    let is_updated = system.exists(&build_directory.join(".git"))
        && run_as_build_user("git", &["-C", &build_directory_text, "pull", "--ff-only"])?
            .succeeded();
    if !is_updated {
        system.remove_dir_all(&build_directory)?;
        system.create_dir_all(&build_directory, BUILD_DIRECTORY_MODE)?;
        system.chown(&build_directory, BUILD_USER)?;
        let repository = format!("https://aur.archlinux.org/{package}.git");
        run_as_build_user("git", &["clone", &repository, &build_directory_text])?
            .check("git clone")?;
    }

    let srcinfo_path = build_directory.join(".SRCINFO");
    let srcinfo_text = system
        .read_file(&srcinfo_path)?
        .ok_or_else(|| TaskError::Parse {
            path: srcinfo_path,
            message: "the AUR repository has no .SRCINFO".to_owned(),
        })?;
    let dependencies = srcinfo::dependencies(&srcinfo_text);
    if !dependencies.is_empty() {
        pacman::install_as_dependencies(system, &dependencies)?;
    }

    run_as_build_user(
        "makepkg",
        &["--nodeps", "--nocheck", "--noconfirm", "--force"],
    )?
    .check("makepkg")?;

    let package_file = newest_package_file(system, &build_directory, package)?;
    pacman::install_file(system, &package_file.to_string_lossy())
}

fn newest_package_file(
    system: &dyn System,
    build_directory: &Path,
    package: &str,
) -> Result<PathBuf, TaskError> {
    let package_prefix = format!("{package}-");
    let debug_prefix = format!("{package}-debug-");
    system
        .list_dir(build_directory)?
        .into_iter()
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy())
                .is_some_and(|name| {
                    name.starts_with(&package_prefix)
                        && !name.starts_with(&debug_prefix)
                        && name.ends_with(PACKAGE_FILE_SUFFIX)
                })
        })
        .max_by_key(|path| system.modified(path))
        .ok_or_else(|| {
            TaskError::io(
                build_directory,
                io::Error::new(io::ErrorKind::NotFound, "makepkg built no package"),
            )
        })
}
