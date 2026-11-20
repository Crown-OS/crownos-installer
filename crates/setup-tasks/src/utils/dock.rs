//! Points the browser entry of a `dock.ron` file at the chosen browser. The
//! edit works on the text, so the rest of the file stays as it is.

use crownos_config::system::Browser;

fn dock_entry(browser: Browser) -> String {
    format!("\"/usr/share/applications/{}\"", browser.desktop_id())
}

/// True when the dock has an entry for `browser`.
pub fn names(dock_text: &str, browser: Browser) -> bool {
    dock_text.contains(&dock_entry(browser))
}

/// The dock with its first browser entry replaced by `browser`. `None` when
/// the dock already names `browser` or has no browser entry: such a dock
/// stays as it is.
pub fn replace_browser(dock_text: &str, browser: Browser) -> Option<String> {
    if names(dock_text, browser) {
        return None;
    }
    let (position, old_entry) = Browser::ALL
        .into_iter()
        .filter_map(|other| {
            let entry = dock_entry(other);
            dock_text.find(&entry).map(|position| (position, entry))
        })
        .min_by_key(|&(position, _)| position)?;
    Some(format!(
        "{}{}{}",
        &dock_text[..position],
        dock_entry(browser),
        &dock_text[position + old_entry.len()..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCK: &str = "(\n    items: [\n        \"/usr/share/applications/kitty.desktop\",\n        \"/usr/share/applications/chromium.desktop\",\n        \"/usr/share/applications/crownos-files.desktop\",\n    ],\n)\n";

    #[test]
    fn the_chromium_entry_becomes_the_chosen_browser() {
        let edited = replace_browser(DOCK, Browser::Firefox).expect("changed");
        assert_eq!(edited, DOCK.replace("chromium.desktop", "firefox.desktop"));
        assert!(names(&edited, Browser::Firefox));
        assert!(!names(&edited, Browser::Chromium));
    }

    #[test]
    fn a_dock_that_names_the_browser_stays() {
        assert_eq!(replace_browser(DOCK, Browser::Chromium), None);
    }

    #[test]
    fn a_dock_without_a_browser_stays() {
        let dock = "(items: [\"/usr/share/applications/kitty.desktop\"])";
        assert_eq!(replace_browser(dock, Browser::Brave), None);
    }

    #[test]
    fn only_the_first_browser_entry_changes() {
        let dock = "(items: [\"/usr/share/applications/brave-browser.desktop\", \"/usr/share/applications/google-chrome.desktop\"])";
        assert_eq!(
            replace_browser(dock, Browser::Firefox).as_deref(),
            Some(
                "(items: [\"/usr/share/applications/firefox.desktop\", \"/usr/share/applications/google-chrome.desktop\"])"
            )
        );
    }
}
