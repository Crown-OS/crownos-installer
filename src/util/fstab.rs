//! Editing what `genfstab` writes.

use std::borrow::Cow;

/// `fstab` text with `subvol=` and `subvolid=` removed from the options of the
/// line that mounts `/`.
///
/// The root is mounted by whatever subvolume is the filesystem's default, so a
/// rollback is one `btrfs subvolume set-default`. An fstab line that names the
/// subvolume would pin it and quietly undo the rollback at the next remount.
pub fn without_root_subvolume(fstab: &str) -> Cow<'_, str> {
    if !fstab.lines().any(is_root_line_with_subvolume) {
        return Cow::Borrowed(fstab);
    }
    let mut edited = String::with_capacity(fstab.len());
    for line in fstab.split_inclusive('\n') {
        let (content, ending) = split_line_ending(line);
        if is_root_line_with_subvolume(content) {
            edited.push_str(&strip_subvolume_options(content));
        } else {
            edited.push_str(content);
        }
        edited.push_str(ending);
    }
    Cow::Owned(edited)
}

fn split_line_ending(line: &str) -> (&str, &str) {
    let content = line.trim_end_matches(['\n', '\r']);
    (content, &line[content.len()..])
}

fn is_root_line_with_subvolume(line: &str) -> bool {
    let mut fields = line.split_whitespace();
    let device = fields.next();
    let mount_point = fields.next();
    let options = fields.nth(1);
    match (device, mount_point, options) {
        (Some(device), Some("/"), Some(options)) if !device.starts_with('#') => {
            options.split(',').any(is_subvolume_option)
        }
        _ => false,
    }
}

fn is_subvolume_option(option: &str) -> bool {
    option.starts_with("subvol=") || option.starts_with("subvolid=")
}

/// Rewrites the fourth field in place, keeping the separators around every
/// field exactly as `genfstab` wrote them.
fn strip_subvolume_options(line: &str) -> String {
    let mut rewritten = String::with_capacity(line.len());
    let mut field_index = 0;
    let mut rest = line;
    while !rest.is_empty() {
        let separator_len = rest.len() - rest.trim_start().len();
        rewritten.push_str(&rest[..separator_len]);
        rest = &rest[separator_len..];
        let field_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let field = &rest[..field_len];
        if field_index == 3 {
            let kept: Vec<&str> = field
                .split(',')
                .filter(|option| !is_subvolume_option(option))
                .collect();
            if kept.is_empty() {
                rewritten.push_str("defaults");
            } else {
                rewritten.push_str(&kept.join(","));
            }
        } else {
            rewritten.push_str(field);
        }
        rest = &rest[field_len..];
        field_index += 1;
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENFSTAB: &str = "# Static information about the filesystems.\n\
        # /dev/sda6 LABEL=crownos\n\
        UUID=1111\t/         \tbtrfs     \trw,noatime,compress=zstd:3,space_cache=v2,subvolid=256,subvol=/@\t0 0\n\
        \n\
        # /dev/sda6 LABEL=crownos\n\
        UUID=1111\t/home     \tbtrfs     \trw,noatime,compress=zstd:3,subvol=/@home,subvolid=257\t0 0\n\
        UUID=2222\t/efi      \tvfat      \trw,fmask=0137,dmask=0027\t0 2\n";

    #[test]
    fn the_root_line_loses_its_subvolume_and_nothing_else_changes() {
        let edited = without_root_subvolume(GENFSTAB);
        let root = edited
            .lines()
            .find(|line| line.starts_with("UUID=1111\t/ "))
            .expect("a root line");
        assert_eq!(
            root,
            "UUID=1111\t/         \tbtrfs     \trw,noatime,compress=zstd:3,space_cache=v2\t0 0"
        );
        let home = edited
            .lines()
            .find(|line| line.contains("/home"))
            .expect("a home line");
        assert!(home.contains("subvol=/@home,subvolid=257"), "{home}");
        assert_eq!(edited.lines().count(), GENFSTAB.lines().count());
        assert!(edited.ends_with("0 2\n"));
    }

    #[test]
    fn subvolid_before_subvol_is_removed_too() {
        let edited = without_root_subvolume("UUID=1 / btrfs subvol=/@,subvolid=256,rw 0 0\n");
        assert_eq!(edited, "UUID=1 / btrfs rw 0 0\n");
    }

    #[test]
    fn options_made_only_of_a_subvolume_become_defaults() {
        let edited = without_root_subvolume("UUID=1 / btrfs subvol=/@ 0 0");
        assert_eq!(edited, "UUID=1 / btrfs defaults 0 0");
    }

    #[test]
    fn an_fstab_without_a_root_subvolume_is_borrowed() {
        let plain = "UUID=1 / ext4 rw,relatime 0 1\n# UUID=2 / btrfs subvol=/@ 0 0\n";
        assert!(matches!(without_root_subvolume(plain), Cow::Borrowed(_)));
    }
}
