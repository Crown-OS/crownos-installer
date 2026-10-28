//! The window's background: four pink ribbons, blurred into a wash, under a
//! milky veil. Drawn on the GPU from the design's own curves rather than
//! shipped as a full-screen bitmap.

use crownui::ext::{DrawCommand, DrawList, Paint, Path, Stroke, Transform};
use crownui::prelude::{
    Color, CornerRadii, Point, Rect, Runtime, Size, Styled, View, WHITE, canvas, rgba,
};

/// The frame the ribbons were drawn in; they are stretched to the window.
const DESIGN_SIZE: Size = Size::new(1728.0, 1117.0);
/// How far the ribbons are softened.
const BLUR: f32 = 34.0;
const VEIL: Color = rgba(0xfef6f85c);
const FADE_IN_OPACITY: f32 = 0.0;

/// One ribbon: where its curve starts, then cubic segments as
/// `(control, control, end)`, all offset by `origin` into the design frame.
struct Ribbon {
    origin: (f32, f32),
    start: (f32, f32),
    segments: [[(f32, f32); 3]; 4],
    width: f32,
    color: Color,
}

const RIBBONS: [Ribbon; 4] = [
    Ribbon {
        origin: (-176.6, 12.1),
        start: (2018.64, 44.94),
        segments: [
            [(1882.96, 171.68), (1536.02, 501.53), (1358.8, 487.79)],
            [(1181.57, 474.04), (1001.06, 478.92), (736.07, 742.74)],
            [(602.71, 875.51), (668.75, 958.34), (483.79, 1108.54)],
            [(335.82, 1228.7), (114.04, 1328.21), (21.64, 1362.94)],
        ],
        width: 123.0,
        color: rgba(0xef305433),
    },
    Ribbon {
        origin: (-259.2, -121.9),
        start: (2316.2, 135.96),
        segments: [
            [(2162.32, 255.2), (1768.82, 565.53), (1567.81, 552.6)],
            [(1366.79, 539.67), (1293.09, 452.48), (992.54, 700.69)],
            [(841.29, 825.6), (785.16, 995.31), (575.37, 1136.62)],
            [(407.54, 1249.67), (156.0, 1343.28), (51.2, 1375.96)],
        ],
        width: 344.0,
        color: rgba(0xf797a9ad),
    },
    Ribbon {
        origin: (-71.5, -13.8),
        start: (1901.59, 38.78),
        segments: [
            [(1773.59, 153.12), (1583.08, 396.28), (1279.08, 438.28)],
            [(1113.0, 461.23), (812.09, 522.28), (691.59, 668.28)],
            [(571.09, 814.28), (628.09, 862.78), (453.59, 998.28)],
            [(313.99, 1106.68), (104.76, 1196.45), (17.59, 1227.78)],
        ],
        width: 104.0,
        color: rgba(0xfef4f65e),
    },
    Ribbon {
        origin: (-44.9, 12.1),
        start: (1908.02, 52.95),
        segments: [
            [(1780.02, 167.29), (1335.15, 361.87), (1223.51, 486.95)],
            [(1074.01, 654.45), (1006.51, 531.95), (756.51, 769.95)],
            [(630.71, 889.72), (634.51, 876.95), (460.02, 1012.45)],
            [(320.42, 1120.85), (111.18, 1210.62), (24.02, 1241.95)],
        ],
        width: 142.0,
        color: rgba(0xf89eafff),
    },
];

impl Ribbon {
    fn path(&self) -> Path {
        let at = |(x, y): (f32, f32)| Point::new(x + self.origin.0, y + self.origin.1);
        self.segments.iter().fold(
            Path::new().move_to(at(self.start)),
            |path, [first, second, end]| path.cubic_to(at(*first), at(*second), at(*end)),
        )
    }
}

/// Fills its parent, fading in once on the first frame.
pub fn backdrop() -> impl View + Styled {
    canvas(paint)
        .absolute()
        .inset(0.0)
        .pointer_events_none()
        .enter(|style| style.opacity(FADE_IN_OPACITY))
}

fn paint(_: &mut Runtime, size: Size, list: &mut DrawList) {
    let window = Rect::new(Point::new(0.0, 0.0), size);
    list.fill(window, CornerRadii::ZERO, WHITE);
    list.save();
    list.clip(window, CornerRadii::ZERO);
    list.transform(Transform::scale(
        size.width / DESIGN_SIZE.width,
        size.height / DESIGN_SIZE.height,
    ));
    for ribbon in &RIBBONS {
        list.push(DrawCommand::StrokePath(
            ribbon.path(),
            Stroke::new(ribbon.width),
            Paint {
                blur: Some(BLUR),
                ..Paint::solid(ribbon.color)
            },
        ));
    }
    list.restore();
    list.fill(window, CornerRadii::ZERO, VEIL);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ribbon_starts_off_the_right_edge_and_ends_off_the_left() {
        for ribbon in &RIBBONS {
            let first = ribbon.start.0 + ribbon.origin.0;
            let last = ribbon.segments[3][2].0 + ribbon.origin.0;
            assert!(first > DESIGN_SIZE.width * 0.95, "starts at {first}");
            assert!(last < DESIGN_SIZE.width * 0.05, "ends at {last}");
        }
    }

    #[test]
    fn the_wash_is_painted_under_the_veil_and_clipped_to_the_window() {
        let mut list = DrawList::new();
        let mut runtime = Runtime::new();
        paint(&mut runtime, Size::new(1000.0, 700.0), &mut list);
        let commands = list.commands();
        let strokes = commands
            .iter()
            .filter(|command| matches!(command, DrawCommand::StrokePath(_, _, paint) if paint.blur == Some(BLUR)))
            .count();
        assert_eq!(strokes, RIBBONS.len());
        assert!(
            matches!(commands.last(), Some(DrawCommand::FillRect(_, paint)) if paint.brush == crownui::ext::Brush::Solid(VEIL))
        );
    }
}
