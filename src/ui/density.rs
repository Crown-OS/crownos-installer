//! How tightly the pages are set. The design is drawn for a tall window; a
//! short one gets smaller headings and less room around them, so the choices
//! stay in view instead of scrolling behind the step track.

/// Below this window height the pages are set compact. The tallest step, the
/// AI level, fits roomy down to about this height.
const COMPACT_BELOW: f32 = 1060.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    /// The design as drawn.
    Roomy,
    /// For short windows.
    Compact,
}

/// The sizes that change with the density.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measures {
    pub title: f32,
    pub subtitle: f32,
    pub heading_gap: f32,
    pub header_to_content: f32,
    pub hero_title: f32,
    /// Room above the heading, as a share of the window's height.
    pub top_share: f32,
    /// Room below the buttons, as a share of the window's height.
    pub bottom_share: f32,
}

impl Density {
    /// The density for a window `height` tall. A window not laid out yet
    /// reads as roomy, so the first frame does not flash compact.
    pub fn for_height(height: f32) -> Self {
        if height > 0.0 && height < COMPACT_BELOW {
            Self::Compact
        } else {
            Self::Roomy
        }
    }

    pub const fn measures(self) -> Measures {
        match self {
            Self::Roomy => Measures {
                title: 60.0,
                subtitle: 30.0,
                heading_gap: 14.0,
                header_to_content: 66.0,
                hero_title: 132.0,
                top_share: 198.0 / 1117.0,
                bottom_share: 100.0 / 1117.0,
            },
            Self::Compact => Measures {
                title: 44.0,
                subtitle: 22.0,
                heading_gap: 8.0,
                header_to_content: 28.0,
                hero_title: 96.0,
                top_share: 0.07,
                bottom_share: 0.05,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_windows_are_compact_and_an_unmeasured_one_is_not() {
        assert_eq!(Density::for_height(0.0), Density::Roomy);
        assert_eq!(Density::for_height(768.0), Density::Compact);
        assert_eq!(Density::for_height(1050.0), Density::Compact);
        assert_eq!(Density::for_height(1080.0), Density::Roomy);
        assert_eq!(Density::for_height(1117.0), Density::Roomy);
    }

    #[test]
    fn compact_is_smaller_in_every_measure() {
        let (roomy, compact) = (Density::Roomy.measures(), Density::Compact.measures());
        assert!(compact.title < roomy.title);
        assert!(compact.subtitle < roomy.subtitle);
        assert!(compact.header_to_content < roomy.header_to_content);
        assert!(compact.top_share < roomy.top_share);
        assert!(compact.bottom_share < roomy.bottom_share);
    }
}
