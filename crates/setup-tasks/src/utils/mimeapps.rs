//! Edits the `[Default Applications]` section of a `mimeapps.list` file. The
//! edit keeps each other section, key and comment.

use std::borrow::Cow;

use crownos_config::system::Browser;

const DEFAULT_APPLICATIONS_HEADER: &str = "[Default Applications]";
const BROWSER_MIME_TYPES: [&str; 4] = [
    "x-scheme-handler/http",
    "x-scheme-handler/https",
    "text/html",
    "application/xhtml+xml",
];

/// The MIME types that a web browser handles, each with `browser`.
pub fn defaults_for(browser: Browser) -> [(&'static str, &'static str); 4] {
    BROWSER_MIME_TYPES.map(|mime_type| (mime_type, browser.desktop_id()))
}

/// True when each MIME type in `defaults` has its desktop file as the first
/// default application in `text`.
pub fn has_defaults(text: &str, defaults: &[(&str, &str)]) -> bool {
    defaults.iter().all(|&(mime_type, desktop_id)| {
        default_application_lines(text)
            .filter_map(key_and_value)
            .find(|&(key, _)| key == mime_type)
            .and_then(|(_, value)| value.split(';').next())
            .is_some_and(|first| first.trim() == desktop_id)
    })
}

/// Sets each MIME type in `defaults` to its desktop file. Replaces a key that
/// exists, appends a missing key to the section and appends the section when
/// it is missing.
pub fn set_defaults(text: &str, defaults: &[(&str, &str)]) -> String {
    let mut lines: Vec<Cow<str>> = text.lines().map(Cow::Borrowed).collect();
    let header_index = match lines
        .iter()
        .position(|line| line.trim() == DEFAULT_APPLICATIONS_HEADER)
    {
        Some(index) => index,
        None => {
            if lines.last().is_some_and(|line| !line.trim().is_empty()) {
                lines.push(Cow::Borrowed(""));
            }
            lines.push(Cow::Borrowed(DEFAULT_APPLICATIONS_HEADER));
            lines.len() - 1
        }
    };
    let section = header_index + 1
        ..lines[header_index + 1..]
            .iter()
            .position(|line| is_section_header(line))
            .map_or(lines.len(), |offset| header_index + 1 + offset);

    let mut missing_lines = Vec::new();
    for &(mime_type, desktop_id) in defaults {
        let entry = Cow::Owned(format!("{mime_type}={desktop_id}"));
        match section
            .clone()
            .find(|&index| key_and_value(&lines[index]).is_some_and(|(key, _)| key == mime_type))
        {
            Some(index) => lines[index] = entry,
            None => missing_lines.push(entry),
        }
    }
    let insert_index = section
        .clone()
        .rev()
        .find(|&index| !lines[index].trim().is_empty())
        .map_or(section.start, |index| index + 1);
    lines.splice(insert_index..insert_index, missing_lines);

    let mut edited = lines.join("\n");
    edited.push('\n');
    edited
}

fn default_application_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .skip_while(|line| line.trim() != DEFAULT_APPLICATIONS_HEADER)
        .skip(1)
        .take_while(|line| !is_section_header(line))
}

fn is_section_header(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

fn key_and_value(line: &str) -> Option<(&str, &str)> {
    if line.trim_start().starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    Some((key.trim(), value.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_gets_the_section() {
        let defaults = defaults_for(Browser::Firefox);
        let edited = set_defaults("", &defaults);
        assert_eq!(
            edited,
            "[Default Applications]\nx-scheme-handler/http=firefox.desktop\nx-scheme-handler/https=firefox.desktop\ntext/html=firefox.desktop\napplication/xhtml+xml=firefox.desktop\n"
        );
        assert!(has_defaults(&edited, &defaults));
        assert!(!has_defaults(&edited, &defaults_for(Browser::Brave)));
    }

    #[test]
    fn other_sections_and_keys_stay() {
        let text = "[Added Associations]\ntext/html=chromium.desktop;\n\n[Default Applications]\n# keep me\ntext/html=chromium.desktop\nimage/png=crownos-preview.desktop\n\n[Removed Associations]\nvideo/mp4=vlc.desktop\n";
        let edited = set_defaults(text, &defaults_for(Browser::Brave));
        assert_eq!(
            edited,
            "[Added Associations]\ntext/html=chromium.desktop;\n\n[Default Applications]\n# keep me\ntext/html=brave-browser.desktop\nimage/png=crownos-preview.desktop\nx-scheme-handler/http=brave-browser.desktop\nx-scheme-handler/https=brave-browser.desktop\napplication/xhtml+xml=brave-browser.desktop\n\n[Removed Associations]\nvideo/mp4=vlc.desktop\n"
        );
        assert!(has_defaults(&edited, &defaults_for(Browser::Brave)));
    }

    #[test]
    fn a_file_without_the_section_gets_it_appended() {
        let edited = set_defaults(
            "[Added Associations]\ntext/html=chromium.desktop;",
            &defaults_for(Browser::Chromium)[..1],
        );
        assert_eq!(
            edited,
            "[Added Associations]\ntext/html=chromium.desktop;\n\n[Default Applications]\nx-scheme-handler/http=chromium.desktop\n"
        );
    }

    #[test]
    fn the_first_entry_of_a_list_is_the_default() {
        let text =
            "[Default Applications]\nx-scheme-handler/http=chromium.desktop;firefox.desktop;\n";
        assert!(has_defaults(
            text,
            &[("x-scheme-handler/http", "chromium.desktop")]
        ));
        assert!(!has_defaults(
            text,
            &[("x-scheme-handler/http", "firefox.desktop")]
        ));
    }

    #[test]
    fn setting_the_same_defaults_twice_changes_nothing() {
        let defaults = defaults_for(Browser::Chrome);
        let once = set_defaults("[Default Applications]\nimage/png=a.desktop\n", &defaults);
        assert_eq!(set_defaults(&once, &defaults), once);
    }
}
