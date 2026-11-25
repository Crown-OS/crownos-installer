//! Either flow: how much AI the new system gets, and where its models run.

use crownui::prelude::{
    AnyView, Color, Cx, Memo, Signal, Styled, View, bind, hstack, rgb, rgba, text, vstack,
};

use crate::engine::ai_level::{AiFeature, AiLevel, ModelSource};
use crate::ui::components::{CardMetrics, LevelCard, Orb, Segment, glyph, level_card, segmented};
use crate::ui::layout::{PageParts, back_pill, caption, next_pill};
use crate::ui::{Density, Wizard};

const TITLE: &str = "Choose your AI level";
const SUBTITLE: &str = "Decide how much AI helps you. Change it any time in Settings.";
const RECOMMENDED: AiLevel = AiLevel::Advanced;
const CARD_WIDTH: f32 = 360.0;
const CARD_GAP: f32 = 20.0;
const SOURCE_LABEL: &str = "Where AI runs";
const SOURCE_LABEL_GAP: f32 = 16.0;
const NOTE_SIZE: f32 = 14.0;
const NOTE_LEADING: f32 = 20.0 / NOTE_SIZE;
const NOTE_WIDTH: f32 = 760.0;
const NOTE_COLOUR: Color = rgba(0xffffffb8);
const PRIVACY_NOTE: &str = "CrownOS AI features stay on this device unless you allow cloud \
     models. Coding agents follow their own provider's terms. Screen memory is encrypted, and \
     you can forget any part of it.";

/// The room between the cards and the switch, and inside the switch's group.
const fn gaps(density: Density) -> (f32, f32) {
    match density {
        Density::Roomy => (28.0, 16.0),
        Density::Compact => (20.0, 12.0),
    }
}

pub fn build(cx: &mut Cx, wizard: Signal<Wizard>, density: Memo<Density>) -> PageParts {
    let metrics = cx.memo(move |runtime| CardMetrics::for_density(density.get(runtime)));
    let cards: Vec<AnyView> = AiLevel::ALL
        .iter()
        .map(|&level| level_choice(cx, wizard, level, metrics).boxed())
        .collect();
    let levels = hstack(cards)
        .gap(CARD_GAP)
        .items_stretch()
        .justify_center()
        .w_full();

    let current_source = cx.memo(move |runtime| wizard.with(runtime, |wizard| wizard.model_source));
    let sources: Vec<Segment<ModelSource>> = ModelSource::ALL
        .iter()
        .map(|&source| Segment {
            value: source,
            label: source.name(),
            glyph: source_glyph(source),
        })
        .collect();
    let source_row = hstack((
        caption(SOURCE_LABEL),
        segmented(
            cx,
            SOURCE_LABEL,
            &sources,
            current_source,
            move |cx, source| {
                wizard.update(cx, |wizard| wizard.set_model_source(source));
            },
        ),
    ))
    .gap(SOURCE_LABEL_GAP)
    .items_center();
    let privacy = text(PRIVACY_NOTE)
        .text_size(NOTE_SIZE)
        .leading(NOTE_LEADING)
        .text_color(NOTE_COLOUR)
        .text_center()
        .w_full()
        .max_w(NOTE_WIDTH);

    let content = vstack((
        levels,
        vstack((source_row, privacy))
            .gap(bind(move |runtime| gaps(density.get(runtime)).1))
            .items_center()
            .w_full(),
    ))
    .gap(bind(move |runtime| gaps(density.get(runtime)).0))
    .items_center()
    .w_full();

    PageParts::new(TITLE, SUBTITLE)
        .content(content)
        .leading(back_pill(wizard))
        .actions(next_pill(wizard, |_| false))
}

fn level_choice(
    cx: &mut Cx,
    wizard: Signal<Wizard>,
    level: AiLevel,
    metrics: Memo<CardMetrics>,
) -> impl View + use<> {
    let chosen = cx.memo(move |runtime| wizard.with(runtime, |wizard| wizard.ai_level == level));
    let card = LevelCard {
        name: level.name(),
        summary: level.summary(),
        orb: orb(level),
        badge: (level == RECOMMENDED).then_some("Recommended"),
        features: AiFeature::ALL
            .iter()
            .map(|&feature| (feature.name(), level.includes(feature)))
            .collect(),
    };
    level_card(cx, card, metrics, chosen, move |cx| {
        wizard.update(cx, |wizard| wizard.set_ai_level(level));
    })
    .flex_1()
    .min_w(0.0)
    .max_w(CARD_WIDTH)
}

const fn orb(level: AiLevel) -> Orb {
    match level {
        AiLevel::Off => Orb {
            glyph: glyph::AI_OFF,
            start: rgb(0x3a3a3d),
            end: rgb(0x1c1c1f),
        },
        AiLevel::Minimal => Orb {
            glyph: glyph::AI_MINIMAL,
            start: rgb(0x82bcff),
            end: rgb(0x2483ff),
        },
        AiLevel::Advanced => Orb {
            glyph: glyph::AI_ADVANCED,
            start: rgb(0x2483ff),
            end: rgb(0xff66f4),
        },
    }
}

const fn source_glyph(source: ModelSource) -> &'static str {
    match source {
        ModelSource::OnDevice => glyph::ON_DEVICE,
        ModelSource::CloudAllowed => glyph::CLOUD,
    }
}
