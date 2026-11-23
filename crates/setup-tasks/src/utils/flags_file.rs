//! Edits a `<program>-flags.conf` file: one command line flag on each line.

/// `text` with `flag` on a new last line. `None` when a line already sets the
/// same switch, with this value or another one.
pub fn with_flag(text: &str, flag: &str) -> Option<String> {
    let switch = flag.split_once('=').map_or(flag, |(switch, _)| switch);
    let sets_switch = |line: &str| {
        let line = line.trim();
        line == switch || line.starts_with(&format!("{switch}="))
    };
    if text.lines().any(sets_switch) {
        return None;
    }
    let separator = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    Some(format!("{text}{separator}{flag}\n"))
}

/// `text` without the lines that are exactly `flag`. `None` when it has no
/// such line.
pub fn without_flag(text: &str, flag: &str) -> Option<String> {
    if !text.lines().any(|line| line.trim() == flag) {
        return None;
    }
    Some(
        text.lines()
            .filter(|line| line.trim() != flag)
            .map(|line| format!("{line}\n"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLAG: &str = "--force-renderer-accessibility=basic";

    #[test]
    fn a_flag_goes_on_a_new_last_line() {
        assert_eq!(
            with_flag("--ozone-platform=wayland", FLAG).as_deref(),
            Some("--ozone-platform=wayland\n--force-renderer-accessibility=basic\n")
        );
        assert_eq!(
            with_flag("", FLAG).as_deref(),
            Some("--force-renderer-accessibility=basic\n")
        );
    }

    #[test]
    fn a_switch_that_is_set_already_stays_as_it_is() {
        for text in [
            "--force-renderer-accessibility=basic\n",
            "--force-renderer-accessibility\n",
            "  --force-renderer-accessibility=complete\n",
        ] {
            assert_eq!(with_flag(text, FLAG), None, "{text}");
        }
    }

    #[test]
    fn only_the_exact_flag_is_removed() {
        assert_eq!(
            without_flag("--a\n--force-renderer-accessibility=basic\n--b\n", FLAG).as_deref(),
            Some("--a\n--b\n")
        );
        assert_eq!(
            without_flag("--force-renderer-accessibility=complete\n", FLAG),
            None
        );
    }
}
