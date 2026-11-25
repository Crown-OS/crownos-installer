//! A round app icon from the design, shipped as PNG.

use crownui::prelude::{Styled, View, image, vstack};

use crate::util::png::decode_rgba;

const SIZE: f32 = 50.0;

/// `encoded` (an RGBA PNG) at the design's icon size. An icon that does not
/// decode leaves its room empty rather than moving the row.
pub fn app_icon(encoded: &'static [u8]) -> impl View {
    let picture = decode_rgba(encoded)
        .map(|pixels| image(pixels.width, pixels.height, pixels.rgba).size(SIZE));
    vstack(picture).size(SIZE).shrink(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_app_icon_decodes() {
        for encoded in [
            crate::ui::components::app_icons::BRAVE,
            crate::ui::components::app_icons::GOOGLE_CHROME,
            crate::ui::components::app_icons::CHROMIUM,
            crate::ui::components::app_icons::FIREFOX,
            crate::ui::components::app_icons::CLAUDE_CODE,
            crate::ui::components::app_icons::CODEX,
            crate::ui::components::app_icons::CURSOR,
            crate::ui::components::app_icons::WINDOWS_APPS,
        ] {
            assert!(decode_rgba(encoded).is_some());
        }
    }
}
