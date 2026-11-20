//! The `browser` section of `~/.playwright/cli.config.json`, so that
//! `playwright-cli` drives the chosen browser. The edit keeps each other key
//! and removes only the browser values that CrownOS writes for another
//! browser.

use std::path::{Path, PathBuf};

use crownos_config::system::Browser;
use serde_json::{Map, Value};

use crate::context::System;
use crate::error::TaskError;
use crate::utils::json_file;

const CHROME_CHANNEL: &str = "chrome";
const FIREFOX_BROWSER_NAME: &str = "firefox";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigurationChange {
    Unchanged,
    Written,
    /// The file is not plain JSON, or its `browser` section is not an object.
    Skipped,
}

pub fn configuration_file(home: &Path) -> PathBuf {
    home.join(".playwright/cli.config.json")
}

/// Sets the browser section of `path` for `browser`. Give the browser from
/// [`resolve_browser`](crate::utils::browser_fallback::resolve_browser).
pub fn set_browser(
    system: &dyn System,
    path: &Path,
    browser: Browser,
) -> Result<ConfigurationChange, TaskError> {
    if system.read_link(path).is_some() {
        return Ok(ConfigurationChange::Skipped);
    }
    let Some(mut document) = json_file::read(system, path)? else {
        return Ok(ConfigurationChange::Skipped);
    };
    match apply_browser(&mut document, browser) {
        None => Ok(ConfigurationChange::Skipped),
        Some(false) => Ok(ConfigurationChange::Unchanged),
        Some(true) => {
            json_file::write(system, path, &document)?;
            Ok(ConfigurationChange::Written)
        }
    }
}

/// True when the document changed. `None` when the `browser` section or its
/// `launchOptions` is not an object.
fn apply_browser(document: &mut Value, browser: Browser) -> Option<bool> {
    let browser_section = object_entry(document.as_object_mut()?, "browser")?;
    let firefox_name = (browser == Browser::Firefox).then_some(FIREFOX_BROWSER_NAME);
    let name_changed = set_or_clear(browser_section, "browserName", firefox_name, |name| {
        name == FIREFOX_BROWSER_NAME
    });

    let (channel, executable_path) = match browser {
        Browser::Chrome => (Some(CHROME_CHANNEL), None),
        Browser::Brave | Browser::Chromium => (None, Some(browser.executable())),
        Browser::Firefox => (None, None),
    };
    if channel.is_none()
        && executable_path.is_none()
        && !browser_section.contains_key("launchOptions")
    {
        return Some(name_changed);
    }
    let launch_options = object_entry(browser_section, "launchOptions")?;
    let channel_changed = set_or_clear(launch_options, "channel", channel, |channel| {
        channel == CHROME_CHANNEL
    });
    let executable_changed =
        set_or_clear(launch_options, "executablePath", executable_path, |path| {
            Browser::ALL.iter().any(|other| other.executable() == path)
        });
    let launch_options_changed = channel_changed || executable_changed;
    if launch_options_changed && launch_options.is_empty() {
        browser_section.remove("launchOptions");
    }
    Some(name_changed || launch_options_changed)
}

/// Sets `key` to `wanted`. Without `wanted`, removes a value that CrownOS
/// could have written, as `is_crownos_value` tells. True when `object`
/// changed.
fn set_or_clear(
    object: &mut Map<String, Value>,
    key: &str,
    wanted: Option<&str>,
    is_crownos_value: impl Fn(&str) -> bool,
) -> bool {
    let current = object.get(key).and_then(Value::as_str);
    let is_wanted = wanted.is_some() && current == wanted;
    let is_stale = wanted.is_none() && current.is_some_and(is_crownos_value);
    match wanted {
        Some(value) if !is_wanted => {
            object.insert(key.to_owned(), value.into());
            true
        }
        None if is_stale => {
            object.remove(key);
            true
        }
        _ => false,
    }
}

fn object_entry<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Option<&'a mut Map<String, Value>> {
    object
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::context::fake::FakeSystem;

    const PATH: &str = "/home/ada/.playwright/cli.config.json";

    fn browser_section_after(steps: &[Browser]) -> Value {
        let system = FakeSystem::new().with_file(PATH, r#"{"outputDir": "/tmp/pw"}"#);
        for &browser in steps {
            set_browser(&system, Path::new(PATH), browser).expect("sets");
        }
        let document: Value =
            serde_json::from_str(&system.file(PATH).expect("written")).expect("JSON");
        assert_eq!(document["outputDir"], "/tmp/pw");
        document["browser"].clone()
    }

    #[test]
    fn each_browser_gets_its_section() {
        assert_eq!(
            browser_section_after(&[Browser::Chrome]),
            json!({ "launchOptions": { "channel": "chrome" } })
        );
        assert_eq!(
            browser_section_after(&[Browser::Brave]),
            json!({ "launchOptions": { "executablePath": "/usr/bin/brave" } })
        );
        assert_eq!(
            browser_section_after(&[Browser::Firefox]),
            json!({ "browserName": "firefox" })
        );
    }

    #[test]
    fn a_change_of_browser_removes_the_values_of_the_old_browser() {
        assert_eq!(
            browser_section_after(&[Browser::Chrome, Browser::Chromium]),
            json!({ "launchOptions": { "executablePath": "/usr/bin/chromium" } })
        );
        assert_eq!(
            browser_section_after(&[Browser::Brave, Browser::Firefox]),
            json!({ "browserName": "firefox" })
        );
        assert_eq!(
            browser_section_after(&[Browser::Firefox, Browser::Chrome]),
            json!({ "launchOptions": { "channel": "chrome" } })
        );
    }

    #[test]
    fn values_of_the_user_stay() {
        let system = FakeSystem::new().with_file(
            PATH,
            r#"{"browser": {"launchOptions": {"channel": "msedge", "headless": true}}}"#,
        );
        set_browser(&system, Path::new(PATH), Browser::Firefox).expect("sets");
        let document: Value =
            serde_json::from_str(&system.file(PATH).expect("written")).expect("JSON");
        assert_eq!(
            document["browser"],
            json!({ "launchOptions": { "channel": "msedge", "headless": true }, "browserName": "firefox" })
        );
    }

    #[test]
    fn the_same_browser_again_writes_nothing() {
        let system = FakeSystem::new();
        assert_eq!(
            set_browser(&system, Path::new(PATH), Browser::Brave).expect("sets"),
            ConfigurationChange::Written
        );
        system.clear_journal();
        assert_eq!(
            set_browser(&system, Path::new(PATH), Browser::Brave).expect("sets"),
            ConfigurationChange::Unchanged
        );
        assert!(system.journal().is_empty());
    }

    #[test]
    fn a_symbolic_link_is_skipped() {
        let system = FakeSystem::new().with_link(PATH, "/home/ada/dotfiles/cli.config.json");
        assert_eq!(
            set_browser(&system, Path::new(PATH), Browser::Brave).expect("sets"),
            ConfigurationChange::Skipped
        );
        assert!(system.journal().is_empty());
    }

    #[test]
    fn a_broken_file_is_skipped() {
        let system = FakeSystem::new().with_file(PATH, r#"{"browser": "chrome"}"#);
        assert_eq!(
            set_browser(&system, Path::new(PATH), Browser::Brave).expect("sets"),
            ConfigurationChange::Skipped
        );
        assert!(system.journal().is_empty());
    }
}
