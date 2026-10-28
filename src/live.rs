//! The image's live desktop. On the ISO the installer is a window inside it,
//! opened by the desktop's own startup, so trying CrownOS out is just this
//! window going away. crownos-live.service marks that session with
//! `CROWNOS_LIVE_SESSION`; the safe-graphics kiosk, which has no desktop
//! underneath, does not.

const SESSION_MARKER: &str = "CROWNOS_LIVE_SESSION";

pub fn available() -> bool {
    std::env::var_os(SESSION_MARKER).is_some()
}

/// Close the installer and leave the desktop behind it. The desktop's
/// "Install CrownOS" entry opens it again.
pub fn leave() -> ! {
    std::process::exit(0)
}
