//! Artwork shipped as PNG, decoded into the straight-alpha RGBA8 pixels the
//! renderer uploads.

use std::io::Cursor;

use png::{BitDepth, ColorType, Decoder, Transformations};

pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// `None` when `encoded` is not a PNG that decodes to RGBA8.
pub fn decode_rgba(encoded: &[u8]) -> Option<Pixels> {
    let mut decoder = Decoder::new(Cursor::new(encoded));
    decoder.set_transformations(Transformations::normalize_to_color8() | Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut rgba).ok()?;
    if (frame.color_type, frame.bit_depth) != (ColorType::Rgba, BitDepth::Eight) {
        return None;
    }
    rgba.truncate(frame.buffer_size());
    Some(Pixels {
        width: frame.width,
        height: frame.height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_png_decodes_to_nothing() {
        assert!(decode_rgba(b"not a png").is_none());
    }
}
