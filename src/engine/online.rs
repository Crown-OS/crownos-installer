//! The CrownOS packages that the ISO does not carry. The install downloads
//! them once the CrownOS repository is listed, on a best effort basis, and
//! records them, so that `crownos-setup` installs each one that is still
//! missing when the network comes up.

use crownos_config::system::ONLINE_PACKAGES_PATH;

use super::exec::Cmd;

pub fn commands(root: &str, packages: &[String]) -> Vec<Cmd> {
    let mut args = vec!["-Syu", "--needed", "--noconfirm", "--"];
    args.extend(packages.iter().map(String::as_str));
    vec![
        Cmd::WriteFile {
            path: format!("{root}{ONLINE_PACKAGES_PATH}"),
            contents: packages.iter().map(|name| format!("{name}\n")).collect(),
        },
        Cmd::best_effort(Cmd::chroot(root, "pacman", &args)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_recorded_before_a_best_effort_download() {
        let packages = ["crownos-voice".to_owned(), "crownos-ai-cpu".to_owned()];
        let commands = commands("/mnt", &packages);

        assert!(matches!(
            &commands[0],
            Cmd::WriteFile { path, contents }
                if path == "/mnt/etc/crownos/online-packages" && contents == "crownos-voice\ncrownos-ai-cpu\n"
        ));
        assert!(matches!(commands[1], Cmd::BestEffort(_)));
        assert!(
            commands[1]
                .to_string()
                .contains("crownos-voice crownos-ai-cpu")
        );
    }
}
