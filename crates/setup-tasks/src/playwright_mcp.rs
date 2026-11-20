//! The command line of the Playwright MCP server for the chosen browser.
//!
//! `playwright-mcp --help` of `@playwright/mcp` 0.0.86 gives these `--browser`
//! values: `chrome`, `firefox`, `webkit` and `msedge`. The server also accepts
//! the value `chromium`, but that value selects the `chrome-for-testing`
//! channel and starts the browser without its sandbox. Thus Brave and Chromium
//! get only `--executable-path`: the server then starts that binary with the
//! sandbox.

use std::ffi::OsString;
use std::path::PathBuf;

use crownos_config::system::{Browser, PLAYWRIGHT_BROWSERS_PATH, PLAYWRIGHT_MCP_SERVER};

#[derive(Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
    pub environment: Vec<(&'static str, String)>,
}

/// The server with the options for `browser`, then `extra`. Give the browser
/// from [`resolve_browser`](crate::utils::browser_fallback::resolve_browser),
/// so that the server starts a browser that is installed.
pub fn invocation(browser: Browser, extra: &[OsString]) -> Invocation {
    let browser_arguments = match browser {
        Browser::Chrome => ["--browser", "chrome"],
        Browser::Firefox => ["--browser", "firefox"],
        browser @ (Browser::Brave | Browser::Chromium) => {
            ["--executable-path", browser.executable()]
        }
    };
    Invocation {
        program: PathBuf::from(PLAYWRIGHT_MCP_SERVER),
        arguments: browser_arguments
            .into_iter()
            .map(OsString::from)
            .chain(extra.iter().cloned())
            .collect(),
        environment: vec![(
            "PLAYWRIGHT_BROWSERS_PATH",
            PLAYWRIGHT_BROWSERS_PATH.to_owned(),
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments_for(browser: Browser) -> Vec<OsString> {
        invocation(browser, &[]).arguments
    }

    #[test]
    fn chrome_uses_the_chrome_channel() {
        assert_eq!(arguments_for(Browser::Chrome), ["--browser", "chrome"]);
    }

    #[test]
    fn firefox_uses_the_playwright_firefox_build() {
        assert_eq!(arguments_for(Browser::Firefox), ["--browser", "firefox"]);
    }

    #[test]
    fn brave_and_chromium_use_their_executable() {
        assert_eq!(
            arguments_for(Browser::Brave),
            ["--executable-path", "/usr/bin/brave"]
        );
        assert_eq!(
            arguments_for(Browser::Chromium),
            ["--executable-path", "/usr/bin/chromium"]
        );
    }

    #[test]
    fn the_extra_arguments_follow_and_the_browsers_path_is_set() {
        let extra = [OsString::from("--headless"), OsString::from("--port=0")];
        let invocation = invocation(Browser::Chromium, &extra);
        assert_eq!(invocation.program, PathBuf::from(PLAYWRIGHT_MCP_SERVER));
        assert_eq!(
            invocation.arguments,
            [
                "--executable-path",
                "/usr/bin/chromium",
                "--headless",
                "--port=0"
            ]
        );
        assert_eq!(
            invocation.environment,
            [(
                "PLAYWRIGHT_BROWSERS_PATH",
                "/usr/share/playwright".to_owned()
            )]
        );
    }
}
