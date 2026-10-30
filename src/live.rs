//! The image's live desktop. On the ISO the installer is a window inside it,
//! opened by the desktop's own startup, so trying CrownOS out is just this
//! window going away.

/// Close the installer and leave the desktop behind it. The desktop's
/// "Install CrownOS" entry opens it again.
pub fn leave() -> ! {
    std::process::exit(0)
}
