//! Animated GIFs: the quick way to show an animation to a person.
//!
//! A GIF holds at most 256 colours per frame, so each frame's colours are
//! reduced (with the `gif` crate's quantizer) unless there are already
//! few enough, as in pixel art. Fully see-through pixels stay see-through;
//! anything in between is treated as opaque (a GIF can't do half).

use std::io::Write;
use std::path::Path;

use crate::file::{FileError, write_error, write_safely};
use crate::image::Image;

/// Writes `frames` (all the same size) as an animated GIF playing at
/// `fps`, looping forever if `looping` (otherwise once).
pub fn write_gif(
    frames: &[Image],
    fps: f32,
    looping: bool,
    writer: impl Write,
) -> Result<(), FileError> {
    let Some(first) = frames.first() else {
        return Err(write_error("a GIF needs at least one frame"));
    };
    let (width, height) = (first.width(), first.height());
    if width > u32::from(u16::MAX) || height > u32::from(u16::MAX) {
        return Err(write_error(
            "a GIF frame can be at most 65535 pixels wide or high",
        ));
    }
    let mut encoder =
        gif::Encoder::new(writer, width as u16, height as u16, &[]).map_err(write_error)?;
    encoder
        .set_repeat(if looping {
            gif::Repeat::Infinite
        } else {
            gif::Repeat::Finite(0)
        })
        .map_err(write_error)?;
    // GIF time is in hundredths of a second, rounded.
    let delay = (100.0 / fps).round().clamp(1.0, 65535.0) as u16;
    for frame in frames {
        if frame.width() != width || frame.height() != height {
            return Err(write_error("every frame of a GIF must be the same size"));
        }
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let [r, g, b, a] = frame.pixel(x, y);
                // Half see-through becomes opaque: a GIF can't blend.
                pixels.extend_from_slice(&[r, g, b, if a < 128 { 0 } else { 255 }]);
            }
        }
        let mut encoded = gif::Frame::from_rgba_speed(width as u16, height as u16, &mut pixels, 10);
        encoded.delay = delay;
        encoded.dispose = gif::DisposalMethod::Background;
        encoder.write_frame(&encoded).map_err(write_error)?;
    }
    Ok(())
}

/// Saves an animated GIF (safely, via a temporary file).
pub fn save_gif(frames: &[Image], fps: f32, looping: bool, path: &Path) -> Result<(), FileError> {
    write_safely(path, |file| write_gif(frames, fps, looping, file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gif_holds_every_frame_at_the_right_speed() {
        let mut frames = Vec::new();
        for i in 0..3u8 {
            let mut image = Image::new(8, 6, [0, 0, 0, 0]);
            image.fill_rect(i as i32 * 2, 1, 2, 2, [255, i * 100, 0, 255]);
            frames.push(image);
        }
        let mut bytes = Vec::new();
        write_gif(&frames, 12.5, true, &mut bytes).unwrap();
        assert!(bytes.starts_with(b"GIF89a"));

        let mut decoder = gif::DecodeOptions::new();
        decoder.set_color_output(gif::ColorOutput::RGBA);
        let mut reader = decoder.read_info(bytes.as_slice()).unwrap();
        let mut count = 0;
        while let Some(frame) = reader.read_next_frame().unwrap() {
            assert_eq!(frame.delay, 8, "1/12.5 s is 8 hundredths");
            assert_eq!((frame.width, frame.height), (8, 6));
            // The square is where it was painted, and the rest see-through.
            let at = |x: usize, y: usize| &frame.buffer[(y * 8 + x) * 4..][..4];
            assert_eq!(at(count * 2, 1)[3], 255);
            assert_eq!(at(count * 2, 1)[0], 255);
            assert_eq!(at(7, 5)[3], 0);
            count += 1;
        }
        assert_eq!(count, 3);
    }

    #[test]
    fn frames_must_match() {
        let frames = [
            Image::new(4, 4, [0, 0, 0, 255]),
            Image::new(5, 4, [0, 0, 0, 255]),
        ];
        assert!(write_gif(&frames, 10.0, false, Vec::new()).is_err());
        assert!(write_gif(&[], 10.0, false, Vec::new()).is_err());
    }
}
