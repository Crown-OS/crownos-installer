//! The design's own icons, as SVG source. Each is drawn with
//! [`icon`](crownui::prelude::icon) in whatever colour its place calls for.

/// The orb of the AI level that turns AI off.
pub const AI_OFF: &str = include_str!("../../../assets/icons/ai_off.svg");
/// The orb of the minimal AI level.
pub const AI_MINIMAL: &str = include_str!("../../../assets/icons/ai_minimal.svg");
/// The orb of the advanced AI level.
pub const AI_ADVANCED: &str = include_str!("../../../assets/icons/ai_advanced.svg");
pub const ARROW_LEFT: &str = include_str!("../../../assets/icons/arrow_left.svg");
pub const ARROW_RIGHT: &str = include_str!("../../../assets/icons/arrow_right.svg");
pub const ARROW_SQUARE_DOWN: &str = include_str!("../../../assets/icons/arrow_square_down.svg");
/// The check inside a radio.
pub const CHECK_SMALL: &str = include_str!("../../../assets/icons/check.svg");
/// The check inside a checkbox.
pub const CHECK_LARGE: &str = include_str!("../../../assets/icons/checkbox_check.svg");
/// The check on a reached step and on a finished stage.
pub const CHECK: &str = include_str!("../../../assets/icons/step_check.svg");
pub const CLOUD: &str = include_str!("../../../assets/icons/cloud.svg");
/// A feature an AI level includes.
pub const FEATURE_INCLUDED: &str = include_str!("../../../assets/icons/feature_included.svg");
/// A feature an AI level leaves out.
pub const FEATURE_EXCLUDED: &str = include_str!("../../../assets/icons/feature_excluded.svg");
pub const HARD_DRIVE: &str = include_str!("../../../assets/icons/hard_drive.svg");
pub const LOCK: &str = include_str!("../../../assets/icons/lock.svg");
/// AI that stays on this machine.
pub const ON_DEVICE: &str = include_str!("../../../assets/icons/on_device.svg");
pub const MONITOR: &str = include_str!("../../../assets/icons/monitor.svg");
pub const REFRESH: &str = include_str!("../../../assets/icons/refresh.svg");
pub const WIFI: &str = include_str!("../../../assets/icons/wifi.svg");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_an_svg() {
        for source in [
            AI_OFF,
            AI_MINIMAL,
            AI_ADVANCED,
            ARROW_LEFT,
            ARROW_RIGHT,
            ARROW_SQUARE_DOWN,
            CHECK_SMALL,
            CHECK_LARGE,
            CHECK,
            CLOUD,
            FEATURE_INCLUDED,
            FEATURE_EXCLUDED,
            HARD_DRIVE,
            LOCK,
            MONITOR,
            ON_DEVICE,
            REFRESH,
            WIFI,
        ] {
            assert!(source.trim_start().starts_with("<svg"));
        }
    }
}
