//! How much AI the installed system gets, and where its models may run.
//!
//! The level decides which AI features the new system sets up. The models of
//! every feature except the wallpaper depth effect are large, so they are not
//! on the live image: they download after installation.

use crate::oobe::AiFeatures;

/// How much AI the user asked for, least first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum AiLevel {
    /// Only what needs no download and reads nothing on the screen.
    #[default]
    Off,
    /// Voice tools that run when the user asks.
    Minimal,
    /// Adds the features that see the user's work and act for them.
    Advanced,
}

impl AiLevel {
    pub const ALL: [Self; 3] = [Self::Off, Self::Minimal, Self::Advanced];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Minimal => "Minimal",
            Self::Advanced => "Advanced",
        }
    }

    pub const fn summary(self) -> &'static str {
        match self {
            Self::Off => "No downloads. Nothing reads your screen.",
            Self::Minimal => "Voice tools that run when you ask.",
            Self::Advanced => "Help that knows what you are working on.",
        }
    }

    pub fn includes(self, feature: AiFeature) -> bool {
        self >= feature.minimum_level()
    }

    /// Whether the install sets up AI acceleration at all.
    pub const fn features(self) -> AiFeatures {
        match self {
            Self::Off => AiFeatures::Disabled,
            Self::Minimal | Self::Advanced => AiFeatures::Enabled,
        }
    }
}

/// A payload answers only "AI or not". "AI" means the minimal level, because
/// nobody chose the features that read the screen.
impl From<AiFeatures> for AiLevel {
    fn from(features: AiFeatures) -> Self {
        match features {
            AiFeatures::Disabled => Self::Off,
            AiFeatures::Enabled => Self::Minimal,
        }
    }
}

/// One AI feature of CrownOS, in the order the AI page lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiFeature {
    WallpaperDepth,
    Dictation,
    VoiceEnhancement,
    Assistant,
    ScreenMemory,
    ComputerUse,
}

impl AiFeature {
    pub const ALL: [Self; 6] = [
        Self::WallpaperDepth,
        Self::Dictation,
        Self::VoiceEnhancement,
        Self::Assistant,
        Self::ScreenMemory,
        Self::ComputerUse,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::WallpaperDepth => "Wallpaper depth effect",
            Self::Dictation => "Voice dictation",
            Self::VoiceEnhancement => "Voice enhancement",
            Self::Assistant => "Suja voice assistant (GPU)",
            Self::ScreenMemory => "Screen memory",
            Self::ComputerUse => "Computer use for coding agents",
        }
    }

    /// The least level that includes the feature.
    pub const fn minimum_level(self) -> AiLevel {
        match self {
            Self::WallpaperDepth => AiLevel::Off,
            Self::Dictation | Self::VoiceEnhancement => AiLevel::Minimal,
            Self::Assistant | Self::ScreenMemory | Self::ComputerUse => AiLevel::Advanced,
        }
    }
}

/// Where the models of the AI features may run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModelSource {
    /// Nothing leaves this machine.
    #[default]
    OnDevice,
    /// Larger models in the cloud may be used.
    CloudAllowed,
}

impl ModelSource {
    pub const ALL: [Self; 2] = [Self::OnDevice, Self::CloudAllowed];

    pub const fn name(self) -> &'static str {
        match self {
            Self::OnDevice => "On this device only",
            Self::CloudAllowed => "Allow cloud models",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_level_includes_everything_the_level_below_it_does() {
        for feature in AiFeature::ALL {
            let levels: Vec<bool> = AiLevel::ALL
                .iter()
                .map(|level| level.includes(feature))
                .collect();
            assert!(
                levels.windows(2).all(|pair| pair[0] <= pair[1]),
                "{feature:?}"
            );
        }
    }

    #[test]
    fn off_keeps_only_the_wallpaper_depth_effect_and_advanced_keeps_everything() {
        let off: Vec<AiFeature> = AiFeature::ALL
            .into_iter()
            .filter(|feature| AiLevel::Off.includes(*feature))
            .collect();
        assert_eq!(off, [AiFeature::WallpaperDepth]);
        assert!(
            AiFeature::ALL
                .iter()
                .all(|feature| AiLevel::Advanced.includes(*feature))
        );
    }

    #[test]
    fn only_off_installs_no_ai_acceleration() {
        assert_eq!(AiLevel::Off.features(), AiFeatures::Disabled);
        assert_eq!(AiLevel::Minimal.features(), AiFeatures::Enabled);
        assert_eq!(AiLevel::Advanced.features(), AiFeatures::Enabled);
    }

    #[test]
    fn a_payload_that_asked_for_ai_gets_the_minimal_level() {
        assert_eq!(AiLevel::from(AiFeatures::Disabled), AiLevel::Off);
        assert_eq!(AiLevel::from(AiFeatures::Enabled), AiLevel::Minimal);
    }
}
