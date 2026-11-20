//! Reading the release version and the package list out of a published
//! CrownOS manifest.
//!
//! Only the top-level `version = "..."` and `packages = [...]` keys are read.
//! The array is the complete set an installed system must have. Everything
//! after the first table header (`[[package]]` and the like) is ignored.

/// The channel of a testing build, whose version is not a plain `X.Y.Z`.
const TESTING_CHANNEL: &str = "testing";

/// The manifest's top-level `version`, or `None` when it is missing or empty.
pub fn version(manifest: &str) -> Option<String> {
    top_level_lines(manifest)
        .find_map(|line| unquote(value_of(line, "version")?.trim()))
        .filter(|version| !version.is_empty())
        .map(str::to_owned)
}

/// The repository channel that serves exactly the release `version`:
/// `v<X.Y.Z>` for a release, `testing` for anything else.
pub fn channel_for(version: &str) -> String {
    let is_number = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let is_release = version.split('.').count() == 3 && version.split('.').all(is_number);
    if is_release {
        format!("v{version}")
    } else {
        TESTING_CHANNEL.to_owned()
    }
}

/// The names in the manifest's top-level `packages` array.
///
/// `None` when the array is missing, empty, malformed, or names something that
/// is not a pacman package name. A name starting with `-` is refused because it
/// would reach `pacstrap`'s argument vector as an option.
pub fn package_names(manifest: &str) -> Option<Vec<String>> {
    let array = packages_array(manifest)?;
    let names = array
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            unquote(item)
                .filter(|name| is_package_name(name))
                .map(str::to_owned)
        })
        .collect::<Option<Vec<_>>>()?;
    (!names.is_empty()).then_some(names)
}

fn packages_array(manifest: &str) -> Option<String> {
    let mut lines = top_level_lines(manifest);
    let opening = lines
        .by_ref()
        .find_map(|line| value_of(line, "packages")?.strip_prefix('['))?;
    let mut array = String::new();
    for line in std::iter::once(opening).chain(lines) {
        if let Some((inside, _)) = line.split_once(']') {
            array.push_str(inside);
            return Some(array);
        }
        array.push_str(line);
        array.push(',');
    }
    None
}

fn top_level_lines(manifest: &str) -> impl Iterator<Item = &str> {
    manifest
        .lines()
        .map(without_comment)
        .take_while(|line| !line.trim_start().starts_with('['))
}

fn value_of<'line>(line: &'line str, key: &str) -> Option<&'line str> {
    Some(
        line.trim()
            .strip_prefix(key)?
            .trim_start()
            .strip_prefix('=')?
            .trim_start(),
    )
}

fn without_comment(line: &str) -> &str {
    let mut open_quote = None;
    for (index, character) in line.char_indices() {
        match (character, open_quote) {
            ('"' | '\'', None) => open_quote = Some(character),
            (quote, Some(open)) if quote == open => open_quote = None,
            ('#', None) => return &line[..index],
            _ => {}
        }
    }
    line
}

fn unquote(item: &str) -> Option<&str> {
    item.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| item.strip_prefix('\'')?.strip_suffix('\''))
}

fn is_package_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"@._+-".contains(&byte)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_one_line_array_is_read() {
        let manifest = "version = \"0.1.12\"\npackages = [\"crownbar\", \"crowndock\"]\n";
        assert_eq!(
            package_names(manifest),
            Some(vec!["crownbar".to_owned(), "crowndock".to_owned()])
        );
    }

    #[test]
    fn a_multi_line_array_with_comments_and_a_trailing_comma_is_read() {
        let manifest = "version = \"0.1.12\" # the ISO\n\
            packages = [ # every package\n  \"crownbar\",\n  # \"commented-out\",\n  'crowndock', # literal\n  \"lib32-mesa\",\n]\n\
            [[package]]\nname = \"ignored\"\n";
        assert_eq!(
            package_names(manifest),
            Some(vec![
                "crownbar".to_owned(),
                "crowndock".to_owned(),
                "lib32-mesa".to_owned(),
            ])
        );
    }

    #[test]
    fn package_tables_are_not_the_package_list() {
        let manifest = "version = \"1\"\n[[package]]\nname = \"crownbar\"\npackages = [\"late\"]\n";
        assert_eq!(package_names(manifest), None);
    }

    #[test]
    fn a_missing_or_empty_list_is_none() {
        assert_eq!(package_names("version = \"1\"\n"), None);
        assert_eq!(package_names("packages = []\n"), None);
        assert_eq!(package_names("packages = [\n\"crownbar\",\n"), None);
    }

    #[test]
    fn a_name_that_is_not_a_package_refuses_the_whole_list() {
        assert_eq!(package_names("packages = [\"--overwrite=*\"]"), None);
        assert_eq!(package_names("packages = [\"Crownbar\"]"), None);
        assert_eq!(package_names("packages = [crownbar]"), None);
        assert_eq!(package_names("packages = [\"crown bar\"]"), None);
    }

    #[test]
    fn a_key_that_only_starts_with_packages_is_not_the_list() {
        assert_eq!(package_names("packages_extra = [\"x\"]\n"), None);
    }

    #[test]
    fn the_top_level_version_is_read() {
        let manifest = "build_id = \"abc\"\nversion = \"0.4.2\" # the ISO\npackages = []\n";
        assert_eq!(version(manifest).as_deref(), Some("0.4.2"));
        assert_eq!(
            version("version = '0.4.2.r3.g1a2b3c4'\n").as_deref(),
            Some("0.4.2.r3.g1a2b3c4")
        );
    }

    #[test]
    fn a_version_inside_a_table_or_an_empty_one_is_none() {
        assert_eq!(version("[[package]]\nversion = \"0.3.0-1\"\n"), None);
        assert_eq!(version("version = \"\"\n"), None);
        assert_eq!(version("version_note = \"0.4.2\"\n"), None);
        assert_eq!(version("version = 0.4.2\n"), None);
    }

    #[test]
    fn a_release_is_pinned_to_its_own_channel() {
        assert_eq!(channel_for("0.4.2"), "v0.4.2");
        assert_eq!(channel_for("10.0.12"), "v10.0.12");
    }

    #[test]
    fn anything_but_a_plain_triple_follows_testing() {
        for version in [
            "0.4.2.r3.g1a2b3c4",
            "0.4",
            "0.4.2-rc1",
            "v0.4.2",
            "0..2",
            "",
        ] {
            assert_eq!(channel_for(version), "testing", "{version}");
        }
    }
}
