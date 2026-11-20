//! Links each CrownOS skill into the skill folders of the agents that read
//! skills only from the home directory. Claude Code and Codex also read the
//! system skills, but Cursor reads only `~/.agents/skills`.

use std::path::{Path, PathBuf};

use super::Agent;
use crate::context::System;
use crate::error::TaskError;
use crate::utils::home_file;
use crate::utils::user_directories::UserDirectories;

pub const SKILLS_DIRECTORY: &str = "/usr/share/crownos/skills";

/// The skill folders of the agents in `agents`, each one time.
pub fn folders(agents: &[Agent], directories: &UserDirectories) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for folder in agents
        .iter()
        .filter_map(|agent| agent.skill_folder(directories))
    {
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    folders
}

/// The skill directories in [`SKILLS_DIRECTORY`], sorted.
pub fn system_skills(system: &dyn System) -> Result<Vec<PathBuf>, TaskError> {
    let mut skills: Vec<PathBuf> = system
        .list_dir(Path::new(SKILLS_DIRECTORY))?
        .into_iter()
        .filter(|entry| system.is_directory(entry))
        .collect();
    skills.sort();
    Ok(skills)
}

/// The links that one [`link`] call made, and the links of the user that it
/// left as they are.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SkillLinks {
    pub linked: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

/// Links each skill into `folder`. A link into [`SKILLS_DIRECTORY`] that
/// points at another skill is replaced. An entry that is not a symbolic link
/// stays, and a link to another place stays and is reported. A link that
/// another run of `crownos-setup user` made at the same time counts as done.
pub fn link(
    system: &dyn System,
    skills: &[PathBuf],
    folder: &Path,
) -> Result<SkillLinks, TaskError> {
    let mut links = SkillLinks::default();
    if skills.is_empty() {
        return Ok(links);
    }
    home_file::ensure_directory(system, folder)?;
    for skill in skills {
        let Some(skill_name) = skill.file_name() else {
            continue;
        };
        let link = folder.join(skill_name);
        match system.read_link(&link) {
            Some(target) if target == *skill => continue,
            Some(target) if target.starts_with(SKILLS_DIRECTORY) => system.remove_file(&link)?,
            Some(_) => {
                links.skipped.push(link);
                continue;
            }
            None if system.exists(&link) => continue,
            None => {}
        }
        match system.symlink(skill, &link) {
            Ok(()) => links.linked.push(link),
            Err(error)
                if error.is_already_exists() && system.read_link(&link).as_ref() == Some(skill) => {
            }
            Err(error) => return Err(error),
        }
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::fake::FakeSystem;

    const SKILL: &str = "/usr/share/crownos/skills/computer-use";
    const OTHER_SKILL: &str = "/usr/share/crownos/skills/settings";
    const FOLDER: &str = "/home/ada/.agents/skills";

    fn directories() -> UserDirectories {
        UserDirectories::new("/home/ada".into(), None, None)
    }

    fn system_with_skills() -> FakeSystem {
        FakeSystem::new()
            .with_directory(SKILL)
            .with_directory(OTHER_SKILL)
            .with_file("/usr/share/crownos/skills/README.md", "")
    }

    fn link_all(system: &FakeSystem) -> Result<Vec<PathBuf>, TaskError> {
        let skills = system_skills(system)?;
        link(system, &skills, Path::new(FOLDER)).map(|links| links.linked)
    }

    #[test]
    fn the_folders_follow_the_agents_without_repeats() {
        let directories = directories();
        assert!(folders(&[Agent::Zed], &directories).is_empty());
        assert_eq!(
            folders(
                &[
                    Agent::Codex,
                    Agent::Cursor,
                    Agent::GeminiCli,
                    Agent::Antigravity
                ],
                &directories
            ),
            [
                PathBuf::from(FOLDER),
                PathBuf::from("/home/ada/.gemini/config/skills")
            ]
        );
    }

    #[test]
    fn each_skill_directory_is_linked_one_time() {
        let system = system_with_skills();
        let folder = Path::new(FOLDER);
        assert_eq!(
            link_all(&system).expect("links"),
            [folder.join("computer-use"), folder.join("settings")]
        );
        assert_eq!(
            system.read_link(&folder.join("computer-use")),
            Some(PathBuf::from(SKILL))
        );
        system.clear_journal();
        assert!(link_all(&system).expect("links").is_empty());
        assert!(system.journal().is_empty());
    }

    #[test]
    fn a_real_directory_is_never_replaced_and_an_old_link_is() {
        let folder = Path::new(FOLDER);
        let system = system_with_skills()
            .with_directory(folder)
            .with_directory(folder.join("computer-use"))
            .with_link(
                folder.join("settings"),
                "/usr/share/crownos/skills/old-settings",
            );
        assert_eq!(link_all(&system).expect("links"), [folder.join("settings")]);
        assert!(system.is_directory(&folder.join("computer-use")));
        assert_eq!(system.read_link(&folder.join("computer-use")), None);
        assert_eq!(
            system.read_link(&folder.join("settings")),
            Some(PathBuf::from(OTHER_SKILL))
        );
    }

    #[test]
    fn a_link_of_the_user_to_another_place_stays_and_is_reported() {
        let folder = Path::new(FOLDER);
        let system = system_with_skills()
            .with_directory(folder)
            .with_directory("/home/ada/my-skills/settings")
            .with_link(folder.join("settings"), "/home/ada/my-skills/settings");
        let skills = system_skills(&system).expect("lists");
        assert_eq!(
            link(&system, &skills, folder).expect("links"),
            SkillLinks {
                linked: vec![folder.join("computer-use")],
                skipped: vec![folder.join("settings")],
            }
        );
        assert_eq!(
            system.read_link(&folder.join("settings")),
            Some(PathBuf::from("/home/ada/my-skills/settings"))
        );
    }

    #[test]
    fn a_link_that_another_run_made_at_the_same_time_counts_as_done() {
        let folder = Path::new(FOLDER);
        let system = system_with_skills().with_racing_link(folder.join("computer-use"), SKILL);
        assert_eq!(link_all(&system).expect("links"), [folder.join("settings")]);
    }

    #[test]
    fn a_link_to_another_target_made_at_the_same_time_is_an_error() {
        let folder = Path::new(FOLDER);
        let system = system_with_skills()
            .with_racing_link(folder.join("computer-use"), "/opt/other/computer-use");
        assert!(link_all(&system).expect_err("fails").is_already_exists());
    }

    #[test]
    fn the_fake_symlink_fails_when_the_link_exists() {
        let system = FakeSystem::new().with_file("/home/ada/file", "");
        let error = system
            .symlink(Path::new(SKILL), Path::new("/home/ada/file"))
            .expect_err("exists");
        assert!(error.is_already_exists());
    }

    #[test]
    fn without_system_skills_nothing_is_made() {
        let system = FakeSystem::new();
        assert!(link_all(&system).expect("links").is_empty());
        assert!(system.journal().is_empty());
    }
}
