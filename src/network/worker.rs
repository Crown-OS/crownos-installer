//! The async half of [`super`]: one long-lived future per visit to the network
//! page, polling NetworkManager and joining networks when asked. The page
//! starts it with [`spawn_worker`](crate::util::worker::spawn_worker), so it
//! stops when the page goes.
//!
//! Polling rather than subscribing to NetworkManager's signals, because the page
//! is up for a minute at most and a two-second look is cheaper to reason about
//! than a resubscribing event stream. A snapshot is only sent when it differs
//! from the last, so an idle page is not rebuilt every tick.

use std::time::Duration;

use nmrs::{ConnectionError, ConnectivityState, NetworkManager, WifiSecurity};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::{MissedTickBehavior, interval};

use super::{AccessPoint, Event, Join, Snapshot};
use crate::util::worker::MessageProxy;

const REFRESH_EVERY: Duration = Duration::from_secs(2);
const SCAN_EVERY: Duration = Duration::from_secs(15);

/// Polls NetworkManager, posting a [`Snapshot`] whenever it changes, and joins
/// whatever network arrives on `joins`.
pub async fn run(proxy: MessageProxy<Event>, mut joins: UnboundedReceiver<Join>) {
    let manager = match NetworkManager::new().await {
        Ok(manager) => manager,
        Err(error) => {
            let _ = proxy.message(Event::Unavailable(format!(
                "NetworkManager is not running: {error}"
            )));
            return;
        }
    };
    power_on_wifi(&manager).await;

    let mut refresh = interval(REFRESH_EVERY);
    let mut scan = interval(SCAN_EVERY);
    refresh.set_missed_tick_behavior(MissedTickBehavior::Delay);
    scan.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last = None;

    loop {
        tokio::select! {
            _ = scan.tick() => {
                let _ = manager.scan_networks(None).await;
            }
            _ = refresh.tick() => {}
            join = joins.recv() => {
                let Some(join) = join else { return };
                let outcome = connect(&manager, join).await;
                if proxy.message(Event::Joined(outcome)).is_err() {
                    return;
                }
            }
        }

        let current = snapshot(&manager).await;
        if last.as_ref() != Some(&current) {
            if proxy.message(Event::Snapshot(current.clone())).is_err() {
                return;
            }
            last = Some(current);
        }
    }
}

/// A live installer has no reason to keep the radio off, so a soft-blocked
/// card is simply switched on. A hardware switch is the user's to flip.
async fn power_on_wifi(manager: &NetworkManager) {
    if let Ok(radio) = manager.wifi_state().await
        && radio.present
        && radio.hardware_enabled
        && !radio.enabled
    {
        let _ = manager.set_wireless_enabled(true).await;
    }
}

async fn snapshot(manager: &NetworkManager) -> Snapshot {
    let online = matches!(manager.connectivity().await, Ok(ConnectivityState::Full));
    let has_wifi = manager.wifi_state().await.is_ok_and(|radio| radio.present);
    let mut access_points: Vec<AccessPoint> = manager
        .list_networks(None)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|network| !network.ssid.is_empty())
        .map(|network| AccessPoint {
            strength: network.strength.unwrap_or(0),
            secured: network.secured,
            enterprise: network.is_eap,
            active: network.is_active,
            ssid: network.ssid,
        })
        .collect();
    access_points.sort_unstable_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.strength.cmp(&a.strength))
            .then_with(|| a.ssid.cmp(&b.ssid))
    });
    Snapshot {
        online,
        has_wifi,
        access_points,
    }
}

async fn connect(manager: &NetworkManager, join: Join) -> Result<(), String> {
    let security = match join.passphrase {
        Some(psk) => WifiSecurity::WpaPsk { psk },
        None => WifiSecurity::Open,
    };
    manager
        .connect(&join.ssid, None, security)
        .await
        .map_err(|error| describe(&join.ssid, &error))
}

fn describe(ssid: &str, error: &ConnectionError) -> String {
    match error {
        ConnectionError::AuthFailed => format!("The password for {ssid} is not right."),
        ConnectionError::NotFound => format!("{ssid} is out of range."),
        ConnectionError::DhcpFailed => {
            format!("{ssid} accepted the password but gave this computer no address.")
        }
        ConnectionError::Timeout | ConnectionError::SupplicantTimeout => {
            format!("{ssid} did not answer in time. Move closer and try again.")
        }
        other => format!("Could not join {ssid}: {other}."),
    }
}
