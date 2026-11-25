//! The window's background: black, with a row of glass columns rising along
//! its foot, tallest in the middle, each lit from night blue through cyan to
//! pink. Drawn on the GPU from the design's numbers rather than shipped as a
//! full-screen bitmap.

use crownui::ext::{Brush, DrawList, GradientStop, Paint};
use crownui::prelude::{
    Color, CornerRadii, Point, Rect, Runtime, Size, Styled, View, canvas, rgb, rgba,
};

use crate::ui::palette::NIGHT;

/// The frame the columns were drawn in; they are stretched to the window.
const DESIGN_SIZE: Size = Size::new(1728.0, 1117.0);
const COLUMN_WIDTH: f32 = 99.69;
const FIRST_COLUMN_LEFT: f32 = -83.08;
const COLUMN_EDGE: f32 = 1.0;
const COLUMN_EDGE_COLOR: Color = rgba(0x0000002e);
const FADE_IN_OPACITY: f32 = 0.0;

/// Where each column starts and how tall it is, from the middle column out.
/// The row is symmetrical.
const HALF_ROW: [(f32, f32); 10] = [
    (531.91, 790.89),
    (539.24, 783.44),
    (583.63, 738.29),
    (662.81, 657.76),
    (735.14, 584.18),
    (774.98, 543.66),
    (788.37, 530.04),
    (790.87, 527.50),
    (791.10, 527.27),
    (791.11, 527.26),
];

/// The light down one column, from its top.
const GLOW: [(f32, Color); 19] = [
    (0.0, rgb(0x040404)),
    (0.074_79, rgb(0x0a0721)),
    (0.131_93, rgb(0x0f063c)),
    (0.2, rgb(0x111e5e)),
    (0.251_26, rgb(0x003a7f)),
    (0.325_21, rgb(0x0076b2)),
    (0.387_39, rgb(0x00a2ca)),
    (0.410_08, rgb(0x3cafcf)),
    (0.437_82, rgb(0x6cb8d0)),
    (0.512_61, rgb(0xcdb8c6)),
    (0.577_31, rgb(0xfb9aac)),
    (0.673_11, rgb(0xfe4e72)),
    (0.709_24, rgb(0xf32462)),
    (0.726_05, rgb(0xec005b)),
    (0.781_51, rgb(0xdf004d)),
    (0.857_14, rgb(0x6b0a3f)),
    (0.882_35, rgb(0x530831)),
    (0.924_37, rgba(0x2a0619b5)),
    (1.0, rgba(0x04040400)),
];

/// Fills its parent, fading in once on the first frame.
pub fn backdrop() -> impl View + Styled {
    canvas(paint)
        .absolute()
        .inset(0.0)
        .pointer_events_none()
        .enter(|style| style.opacity(FADE_IN_OPACITY))
}

/// Each column's left edge, top and height in the design frame, left to right.
fn columns() -> impl Iterator<Item = (f32, f32, f32)> {
    let middle = HALF_ROW.len() - 1;
    (0..2 * HALF_ROW.len() - 1).map(move |index| {
        let (top, height) = HALF_ROW[index.abs_diff(middle)];
        let left = FIRST_COLUMN_LEFT + index as f32 * COLUMN_WIDTH;
        (left, top, height)
    })
}

fn glow(top: f32, bottom: f32) -> Brush {
    Brush::LinearGradient {
        start: Point::new(0.0, top),
        end: Point::new(0.0, bottom),
        stops: GLOW
            .iter()
            .map(|&(offset, color)| GradientStop { offset, color })
            .collect(),
    }
}

fn paint(_: &mut Runtime, size: Size, list: &mut DrawList) {
    let window = Rect::new(Point::new(0.0, 0.0), size);
    list.fill(window, CornerRadii::ZERO, NIGHT);
    let scale_x = size.width / DESIGN_SIZE.width;
    let scale_y = size.height / DESIGN_SIZE.height;
    list.save();
    list.clip(window, CornerRadii::ZERO);
    for (left, top, height) in columns() {
        let column = Rect::from_xywh(
            left * scale_x,
            top * scale_y,
            COLUMN_WIDTH * scale_x,
            height * scale_y,
        );
        list.fill(
            column,
            CornerRadii::ZERO,
            Paint {
                brush: glow(column.min_y(), column.max_y()),
                blur: None,
            },
        );
        list.stroke(column, CornerRadii::ZERO, COLUMN_EDGE, COLUMN_EDGE_COLOR);
    }
    list.restore();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crownui::ext::DrawCommand;

    #[test]
    fn the_row_spans_the_window_and_peaks_in_the_middle() {
        let row: Vec<_> = columns().collect();
        assert_eq!(row.len(), 19);
        let (first_left, ..) = row[0];
        let (last_left, ..) = row[row.len() - 1];
        assert!(first_left < 0.0);
        assert!(last_left + COLUMN_WIDTH > DESIGN_SIZE.width);
        let tallest = row
            .iter()
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|column| column.0);
        assert_eq!(tallest, Some(row[9].0));
        assert_eq!(row[0].1, row[18].1, "the row is symmetrical");
    }

    #[test]
    fn every_column_runs_past_the_foot_of_the_window() {
        assert!(
            columns().all(|(_, top, height)| top + height > DESIGN_SIZE.height),
            "no column ends in sight"
        );
    }

    #[test]
    fn the_glow_fades_out_at_both_ends() {
        assert_eq!(GLOW[0].1, NIGHT);
        assert_eq!(GLOW[GLOW.len() - 1].1.a, 0.0);
        assert!(GLOW.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn the_night_is_painted_under_the_columns() {
        let mut list = DrawList::new();
        let mut runtime = Runtime::new();
        paint(&mut runtime, Size::new(1000.0, 700.0), &mut list);
        let commands = list.commands();
        assert!(
            matches!(commands.first(), Some(DrawCommand::FillRect(_, paint)) if paint.brush == Brush::Solid(NIGHT))
        );
        let gradients = commands
            .iter()
            .filter(|command| {
                matches!(command, DrawCommand::FillRect(_, paint) if matches!(paint.brush, Brush::LinearGradient { .. }))
            })
            .count();
        assert_eq!(gradients, 19);
    }
}
