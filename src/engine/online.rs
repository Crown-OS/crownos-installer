//! The CrownOS packages that the ISO does not carry. The install downloads
//! them once the CrownOS repository is listed, on a best effort basis, and
//! records them, so that `crownos-setup` installs each one that is still
//! missing when the network comes up.

use crownos_config::system::ONLINE_PACKAGES_PATH;

use super::boot;
use super::exec::Cmd;
use super::plan::ESP_MOUNT_REL;

/// The `-Syu` may replace the kernel, so it is copied onto the ESP again
/// afterwards, as for the graphics packages.
pub fn commands(root: &str, packages: &[String]) -> Vec<Cmd> {
    let mut args = vec!["-Syu", "--needed", "--noconfirm", "--"];
    args.extend(packages.iter().map(String::as_str));
    vec![
        Cmd::WriteFile {
            path: format!("{root}{ONLINE_PACKAGES_PATH}"),
            contents: packages.iter().map(|name| format!("{name}\n")).collect(),
        },
        Cmd::best_effort(Cmd::chroot(root, "pacman", &args)),
        boot::copy_kernel_to_esp(root, ESP_MOUNT_REL),
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
        assert_eq!(
            commands[2].to_string(),
            boot::copy_kernel_to_esp("/mnt", ESP_MOUNT_REL).to_string(),
            "the kernel goes onto the ESP again"
        );
    }
}
