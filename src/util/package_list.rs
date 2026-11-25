//! A package list file: one name on each line, with `#` comments and blank
//! lines between them.

use super::manifest::is_package_name;

/// The names of `list`. A line that is not a package name is left out, so it
/// can never reach `pacstrap` as an option.
pub fn names(list: &str) -> Vec<String> {
    list.lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|name| is_package_name(name))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_skip_comments_blank_lines_and_options() {
        let list = "# the base\nbase\n\nlinux-cachyos # kernel\n--overwrite=*\n";
        assert_eq!(names(list), ["base", "linux-cachyos"]);
    }
}
