//! The browser that the agents can use now. The root runner installs the
//! chosen browser after the first boot. Until then, Chromium, which the image
//! contains, takes the place of a chosen Chromium-based browser.

use std::path::Path;

use crownos_config::system::{Browser, BrowserEngine};

/// `choice`, or Chromium when `choice` is a Chromium-based browser whose
/// executable is not installed. Firefox stays: Playwright uses its own
/// Firefox build.
pub fn resolve_browser(choice: Browser, is_installed: impl Fn(&Path) -> bool) -> Browser {
    if choice.engine() == BrowserEngine::Chromium && !is_installed(Path::new(choice.executable())) {
        return Browser::Chromium;
    }
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installed_choice_stays() {
        assert_eq!(resolve_browser(Browser::Brave, |_| true), Browser::Brave);
    }

    #[test]
    fn a_missing_chromium_based_choice_becomes_chromium() {
        let only_chromium = |path: &Path| path == Path::new("/usr/bin/chromium");
        assert_eq!(
            resolve_browser(Browser::Brave, only_chromium),
            Browser::Chromium
        );
        assert_eq!(
            resolve_browser(Browser::Chrome, only_chromium),
            Browser::Chromium
        );
    }

    #[test]
    fn firefox_stays_when_it_is_missing() {
        assert_eq!(
            resolve_browser(Browser::Firefox, |_| false),
            Browser::Firefox
        );
    }

    #[test]
    fn chromium_stays_chromium() {
        assert_eq!(
            resolve_browser(Browser::Chromium, |_| false),
            Browser::Chromium
        );
    }
}
