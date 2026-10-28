//! What the user types on the account page.

use crate::engine::plan::Account;
use crate::oobe::Oobe;

const DEFAULT_USERNAME: &str = "crown";
const DEFAULT_HOSTNAME: &str = "crownos";

#[derive(Clone, Debug, Default)]
pub struct Form {
    pub username: String,
    pub full_name: String,
    pub hostname: String,
    pub password: String,
    pub confirm: String,
    /// Set once the user has tried to continue, so the page does not shout about
    /// an empty password before they have had a chance to type one.
    pub touched: bool,
}

impl Form {
    /// Pre-filled from the payload where it had something to say, and from
    /// sensible defaults where it did not.
    ///
    /// Defaults rather than blanks so that the wizard always has a valid account
    /// to plan against, which is what lets the disk-level refusals be reported on
    /// the very first frame instead of after the user has filled in a form for a
    /// disk we were never going to touch.
    pub fn from_payload(oobe: Option<&Oobe>) -> Self {
        let user = oobe.and_then(|oobe| oobe.user.as_ref());
        Self {
            username: user.map_or(DEFAULT_USERNAME, |u| &u.username).to_owned(),
            full_name: user.and_then(|u| u.full_name.clone()).unwrap_or_default(),
            hostname: user.map_or(DEFAULT_HOSTNAME, |u| &u.hostname).to_owned(),
            ..Self::default()
        }
    }

    pub fn account(&self, autologin: bool) -> Account {
        let full_name = self.full_name.trim();
        Account {
            username: self.username.trim().to_owned(),
            full_name: (!full_name.is_empty()).then(|| full_name.to_owned()),
            hostname: self.hostname.trim().to_owned(),
            autologin,
        }
    }

    /// Why the account page cannot be left, if it cannot.
    pub fn problem(&self) -> Option<&'static str> {
        if self.username.trim().is_empty() {
            return Some("Choose a username.");
        }
        if self.hostname.trim().is_empty() {
            return Some("Give this computer a name.");
        }
        if self.password.is_empty() {
            return Some("Choose a password.");
        }
        if self.password != self.confirm {
            return Some("The two passwords do not match.");
        }
        None
    }
}
