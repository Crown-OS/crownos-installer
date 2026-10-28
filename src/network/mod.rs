//! Getting the live system online, so the installed system can update straight away.
//!
//! NetworkManager does the work — it is what the installed desktop runs too —
//! and [`nmrs`] is how we talk to it. None of its types leave this module: the
//! wizard sees a [`Snapshot`], asks for a [`Join`], and hears back an [`Event`].
//! The I/O lives in [`worker`], which the network page starts while it shows.

mod worker;

use std::ops::RangeInclusive;

use tokio::sync::mpsc::UnboundedSender;

pub use worker::run;

/// What WPA accepts as a passphrase, in bytes.
pub const PASSPHRASE_LENGTH: RangeInclusive<usize> = 8..=63;

/// One network the Wi-Fi card can see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessPoint {
    pub ssid: String,
    /// `0..=100`.
    pub strength: u8,
    pub secured: bool,
    /// WPA-Enterprise wants a login this page does not ask for.
    pub enterprise: bool,
    pub active: bool,
}

impl AccessPoint {
    pub fn needs_passphrase(&self) -> bool {
        self.secured && !self.enterprise && !self.active
    }
}

/// The machine's network as of the last look.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// NetworkManager reports full connectivity, over any link.
    pub online: bool,
    pub has_wifi: bool,
    /// Strongest first, with the one in use at the top.
    pub access_points: Vec<AccessPoint>,
}

/// Ask NetworkManager to join `ssid`.
#[derive(Debug)]
pub struct Join {
    pub ssid: String,
    pub passphrase: Option<String>,
}

/// What the worker tells the wizard.
#[derive(Debug)]
pub enum Event {
    Snapshot(Snapshot),
    Joined(Result<(), String>),
    /// NetworkManager could not be reached at all.
    Unavailable(String),
}

/// The network page's state.
#[derive(Debug, Default)]
pub struct Network {
    /// `None` until the worker's first report.
    pub snapshot: Option<Snapshot>,
    pub unavailable: Option<String>,
    pub selected: Option<String>,
    pub passphrase: String,
    pub joining: Option<String>,
    pub failure: Option<String>,
    commands: Option<UnboundedSender<Join>>,
}

impl Network {
    pub fn attach(&mut self, commands: UnboundedSender<Join>) {
        self.commands = Some(commands);
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Snapshot(snapshot) => self.snapshot = Some(snapshot),
            Event::Joined(outcome) => {
                self.joining = None;
                self.failure = outcome.err();
                if self.failure.is_none() {
                    self.passphrase.clear();
                }
            }
            Event::Unavailable(reason) => self.unavailable = Some(reason),
        }
    }

    pub fn select(&mut self, ssid: String) {
        if self.selected.as_ref() != Some(&ssid) {
            self.passphrase.clear();
            self.failure = None;
        }
        self.selected = Some(ssid);
    }

    /// The selected network, if it is still in range.
    pub fn selection(&self) -> Option<&AccessPoint> {
        let selected = self.selected.as_deref()?;
        self.snapshot
            .as_ref()?
            .access_points
            .iter()
            .find(|point| point.ssid == selected)
    }

    /// Whether [`join`](Self::join) would send anything.
    pub fn can_join(&self) -> bool {
        self.joining.is_none()
            && self.selection().is_some_and(|point| {
                !point.active
                    && !point.enterprise
                    && (!point.secured || PASSPHRASE_LENGTH.contains(&self.passphrase.len()))
            })
    }

    pub fn join(&mut self) {
        if !self.can_join() {
            return;
        }
        let Some(point) = self.selection() else {
            return;
        };
        let join = Join {
            ssid: point.ssid.clone(),
            passphrase: point.secured.then(|| self.passphrase.clone()),
        };
        let ssid = join.ssid.clone();
        if self
            .commands
            .as_ref()
            .is_some_and(|commands| commands.send(join).is_ok())
        {
            self.joining = Some(ssid);
            self.failure = None;
        }
    }

    pub fn online(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.online)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network_with(point: AccessPoint) -> Network {
        let mut network = Network {
            snapshot: Some(Snapshot {
                online: false,
                has_wifi: true,
                access_points: vec![point.clone()],
            }),
            ..Network::default()
        };
        network.select(point.ssid);
        network
    }

    fn home(secured: bool) -> AccessPoint {
        AccessPoint {
            ssid: "home".to_owned(),
            strength: 80,
            secured,
            enterprise: false,
            active: false,
        }
    }

    #[test]
    fn a_secured_network_waits_for_a_passphrase_wpa_accepts() {
        let mut network = network_with(home(true));
        assert!(!network.can_join());
        network.passphrase = "short".to_owned();
        assert!(!network.can_join());
        network.passphrase = "long enough".to_owned();
        assert!(network.can_join());
    }

    #[test]
    fn an_open_network_joins_without_one() {
        assert!(network_with(home(false)).can_join());
    }

    #[test]
    fn an_enterprise_network_cannot_be_joined_from_here() {
        let mut point = home(true);
        point.enterprise = true;
        let mut network = network_with(point);
        network.passphrase = "long enough".to_owned();
        assert!(!network.can_join());
    }

    #[test]
    fn a_join_is_sent_with_the_passphrase_and_marked_in_flight() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut network = network_with(home(true));
        network.attach(sender);
        network.passphrase = "long enough".to_owned();
        network.join();

        let join = receiver.try_recv().expect("a join");
        assert_eq!(join.ssid, "home");
        assert_eq!(join.passphrase.as_deref(), Some("long enough"));
        assert_eq!(network.joining.as_deref(), Some("home"));
        assert!(!network.can_join(), "one join at a time");

        network.apply(Event::Joined(Err("The password is not right.".to_owned())));
        assert_eq!(network.joining, None);
        assert_eq!(
            network.failure.as_deref(),
            Some("The password is not right.")
        );
    }
}
