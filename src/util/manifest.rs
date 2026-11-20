//! Reading the package list out of a published CrownOS manifest.
//!
//! Only the top-level `packages = [...]` array is read: it is the complete set
//! an installed system must have. Everything after the first table header
//! (`[[package]]` and the like) is ignored.

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
    let mut lines = manifest
        .lines()
        .map(without_comment)
        .take_while(|line| !line.trim_start().starts_with('['));
    let opening = lines.by_ref().find_map(|line| {
        line.trim()
            .strip_prefix("packages")?
            .trim_start()
            .strip_prefix('=')?
            .trim_start()
            .strip_prefix('[')
    })?;
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
}
