//! Plain images: the layers combined into one flat picture, and simple
//! things to do with it (crop, shrink, enlarge, paste, save as PNG).
//!
//! Used for exporting PNGs, and by the command-line renderer for
//! previews, close-ups and contact sheets.

use std::io::Write;
use std::path::Path;

use crate::document::{Background, Document};
use crate::file::{FileError, write_error, write_safely};
use crate::surface::PixelRect;
use crate::tile::{Rgba8, TILE_SIZE};

/// A colour as stored in an image: red, green, blue, opacity, *not*
/// premultiplied (the usual way for image files).
pub type Color = [u8; 4];

/// A flat picture, `width` x `height` pixels, row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<Color>,
}

impl Image {
    /// An image filled with one colour.
    pub fn new(width: u32, height: u32, color: Color) -> Self {
        Self {
            width,
            height,
            pixels: vec![color; width as usize * height as usize],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    fn index(&self, x: u32, y: u32) -> usize {
        y as usize * self.width as usize + x as usize
    }

    /// The pixel at (x, y), which must be inside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Color {
        self.pixels[self.index(x, y)]
    }

    /// Changes one pixel. Ignored outside the image.
    pub fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
            let i = self.index(x as u32, y as u32);
            self.pixels[i] = color;
        }
    }

    /// `area` of the picture as you'd see it: all visible layers combined,
    /// on `background`. Parts of `area` outside the canvas are transparent.
    pub fn from_document(document: &Document, area: PixelRect, background: Background) -> Self {
        let width = (area.right - area.left).max(0) as u32;
        let height = (area.bottom - area.top).max(0) as u32;
        let mut image = Image::new(width, height, [0, 0, 0, 0]);
        let inside = area.intersect(&document.bounds());
        let paper = unpremultiply(document.paper());
        let size = TILE_SIZE as i32;

        for coord in inside.tiles() {
            let tile = document.composite_tile(coord, background);
            let (left, top) = coord.origin();
            let part = inside.intersect(&PixelRect::new(left, top, left + size, top + size));
            for y in part.top..part.bottom {
                for x in part.left..part.right {
                    let color = match &tile {
                        Some(tile) => {
                            unpremultiply(tile.pixel((x - left) as usize, (y - top) as usize))
                        }
                        None if background == Background::Paper => paper,
                        None => [0, 0, 0, 0],
                    };
                    image.set_pixel(x - area.left, y - area.top, color);
                }
            }
        }
        image
    }

    /// The whole canvas as you'd see it.
    pub fn of_document(document: &Document, background: Background) -> Self {
        Self::from_document(document, document.bounds(), background)
    }

    /// The part of the image inside the rectangle (cut to the image).
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Image {
        let x = x.min(self.width);
        let y = y.min(self.height);
        let width = width.min(self.width - x);
        let height = height.min(self.height - y);
        let mut out = Image::new(width, height, [0, 0, 0, 0]);
        for row in 0..height {
            let from = self.index(x, y + row);
            let to = out.index(0, row);
            out.pixels[to..to + width as usize]
                .copy_from_slice(&self.pixels[from..from + width as usize]);
        }
        out
    }

    /// A smaller copy that fits inside `max_width` x `max_height`, keeping
    /// the proportions. Each new pixel is the average of the pixels it
    /// covers, so thin lines fade instead of vanishing. Never enlarges.
    pub fn fit_within(&self, max_width: u32, max_height: u32) -> Image {
        if self.width <= max_width && self.height <= max_height {
            return self.clone();
        }
        let scale =
            (max_width as f64 / self.width as f64).min(max_height as f64 / self.height as f64);
        let width = ((self.width as f64 * scale).round() as u32).max(1);
        let height = ((self.height as f64 * scale).round() as u32).max(1);
        let mut out = Image::new(width, height, [0, 0, 0, 0]);
        for ty in 0..height {
            let y0 = ty as u64 * self.height as u64 / height as u64;
            let y1 = ((ty as u64 + 1) * self.height as u64 / height as u64).max(y0 + 1);
            for tx in 0..width {
                let x0 = tx as u64 * self.width as u64 / width as u64;
                let x1 = ((tx as u64 + 1) * self.width as u64 / width as u64).max(x0 + 1);
                // Average premultiplied, so see-through pixels don't darken the edges.
                let mut sum = [0u64; 4];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let [r, g, b, a] = self.pixel(x as u32, y as u32);
                        let a64 = u64::from(a);
                        sum[0] += u64::from(r) * a64;
                        sum[1] += u64::from(g) * a64;
                        sum[2] += u64::from(b) * a64;
                        sum[3] += a64;
                    }
                }
                let count = (x1 - x0) * (y1 - y0);
                let color = if sum[3] == 0 {
                    [0, 0, 0, 0]
                } else {
                    let channel = |s: u64| ((s + sum[3] / 2) / sum[3]).min(255) as u8;
                    [
                        channel(sum[0]),
                        channel(sum[1]),
                        channel(sum[2]),
                        ((sum[3] + count / 2) / count) as u8,
                    ]
                };
                out.set_pixel(tx as i32, ty as i32, color);
            }
        }
        out
    }

    /// A copy `times` bigger, each pixel becoming a square block, so
    /// single pixels can be seen clearly.
    pub fn magnify(&self, times: u32) -> Image {
        let times = times.max(1);
        let mut out = Image::new(self.width * times, self.height * times, [0, 0, 0, 0]);
        for y in 0..out.height {
            for x in 0..out.width {
                let color = self.pixel(x / times, y / times);
                out.set_pixel(x as i32, y as i32, color);
            }
        }
        out
    }

    /// Copies `other` onto this image with its top-left corner at (x, y),
    /// laying it over what's there (see-through parts let it show).
    pub fn paste(&mut self, other: &Image, x: i32, y: i32) {
        for oy in 0..other.height {
            for ox in 0..other.width {
                let top = other.pixel(ox, oy);
                let (tx, ty) = (x + ox as i32, y + oy as i32);
                if tx < 0 || ty < 0 || tx as u32 >= self.width || ty as u32 >= self.height {
                    continue;
                }
                let below = self.pixel(tx as u32, ty as u32);
                self.set_pixel(tx, ty, over(below, top));
            }
        }
    }

    /// Fills a rectangle with one colour (cut to the image).
    pub fn fill_rect(&mut self, x: i32, y: i32, width: u32, height: u32, color: Color) {
        for py in y..y + height as i32 {
            for px in x..x + width as i32 {
                self.set_pixel(px, py, color);
            }
        }
    }

    /// Writes the image as a PNG file: with an opacity channel if
    /// `with_alpha`, otherwise just red, green and blue.
    pub fn write_png(&self, writer: impl Write, with_alpha: bool) -> Result<(), FileError> {
        let channels = if with_alpha { 4 } else { 3 };
        let bytes: Vec<u8> = self
            .pixels
            .iter()
            .flat_map(|pixel| pixel[..channels].to_vec())
            .collect();
        let mut encoder = png::Encoder::new(writer, self.width, self.height);
        encoder.set_color(if with_alpha {
            png::ColorType::Rgba
        } else {
            png::ColorType::Rgb
        });
        encoder.set_depth(png::BitDepth::Eight);
        let mut png_writer = encoder.write_header().map_err(write_error)?;
        png_writer.write_image_data(&bytes).map_err(write_error)?;
        png_writer.finish().map_err(write_error)
    }

    /// Saves the image as a PNG file (safely, via a temporary file).
    pub fn save_png(&self, path: &Path, with_alpha: bool) -> Result<(), FileError> {
        write_safely(path, |file| self.write_png(file, with_alpha))
    }
}

/// A premultiplied pixel as a normal (straight) colour.
pub fn unpremultiply(pixel: Rgba8) -> Color {
    if pixel.a == 0 {
        return [0, 0, 0, 0];
    }
    let alpha = u32::from(pixel.a);
    let straight = |c: u8| ((u32::from(c) * 255 + alpha / 2) / alpha).min(255) as u8;
    [
        straight(pixel.r),
        straight(pixel.g),
        straight(pixel.b),
        pixel.a,
    ]
}

/// `top` laid over `below` (straight colours).
fn over(below: Color, top: Color) -> Color {
    let ta = u32::from(top[3]);
    if ta == 255 {
        return top;
    }
    if ta == 0 {
        return below;
    }
    let ba = u32::from(below[3]) * (255 - ta) / 255;
    let alpha = ta + ba;
    let channel =
        |t: u8, b: u8| ((u32::from(t) * ta + u32::from(b) * ba + alpha / 2) / alpha) as u8;
    [
        channel(top[0], below[0]),
        channel(top[1], below[1]),
        channel(top[2], below[2]),
        alpha.min(255) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = [255, 0, 0, 255];
    const BLUE: Color = [0, 0, 255, 255];

    #[test]
    fn a_document_becomes_an_image_as_you_see_it() {
        let mut document = Document::new(300, 200);
        document
            .active_layer_mut()
            .surface
            .set_pixel(5, 6, Rgba8::new(0, 0, 128, 128));
        let on_paper = Image::of_document(&document, Background::Paper);
        assert_eq!((on_paper.width(), on_paper.height()), (300, 200));
        assert_eq!(on_paper.pixel(0, 0), [255, 255, 255, 255]);
        assert_eq!(on_paper.pixel(5, 6), [127, 127, 255, 255]);
        let alone = Image::of_document(&document, Background::Transparent);
        assert_eq!(alone.pixel(0, 0), [0, 0, 0, 0]);
        assert_eq!(alone.pixel(5, 6), [0, 0, 255, 128]);
    }

    #[test]
    fn a_part_of_the_canvas_can_be_taken() {
        let mut document = Document::new(300, 200);
        document
            .active_layer_mut()
            .surface
            .set_pixel(260, 10, Rgba8::new(255, 0, 0, 255));
        let part = Image::from_document(
            &document,
            PixelRect::new(250, 0, 350, 20),
            Background::Paper,
        );
        assert_eq!((part.width(), part.height()), (100, 20));
        assert_eq!(part.pixel(10, 10), RED);
        assert_eq!(part.pixel(99, 0), [0, 0, 0, 0], "outside the canvas");
    }

    #[test]
    fn cropping_magnifying_and_shrinking() {
        let mut image = Image::new(4, 4, BLUE);
        image.set_pixel(1, 1, RED);
        let crop = image.crop(1, 1, 10, 10);
        assert_eq!((crop.width(), crop.height()), (3, 3), "cut to the image");
        assert_eq!(crop.pixel(0, 0), RED);

        let big = image.magnify(3);
        assert_eq!((big.width(), big.height()), (12, 12));
        assert_eq!(big.pixel(3, 3), RED);
        assert_eq!(big.pixel(5, 5), RED);
        assert_eq!(big.pixel(6, 6), BLUE);

        let small = Image::new(100, 50, RED).fit_within(20, 20);
        assert_eq!((small.width(), small.height()), (20, 10));
        // Half red, half blue averages to purple.
        let mut halves = Image::new(2, 1, RED);
        halves.set_pixel(1, 0, BLUE);
        assert_eq!(halves.fit_within(1, 1).pixel(0, 0), [128, 0, 128, 255]);
    }

    #[test]
    fn pasting_lays_one_image_over_another() {
        let mut image = Image::new(10, 10, [255, 255, 255, 255]);
        image.paste(&Image::new(3, 3, RED), 8, 8);
        assert_eq!(image.pixel(9, 9), RED);
        assert_eq!(image.pixel(7, 7), [255, 255, 255, 255]);
        image.paste(&Image::new(1, 1, [0, 0, 0, 128]), 0, 0);
        assert_eq!(image.pixel(0, 0), [127, 127, 127, 255]);
    }

    #[test]
    fn png_files_are_written_with_or_without_opacity() {
        let image = Image::new(3, 2, [10, 20, 30, 40]);
        for (alpha, color_type) in [(true, png::ColorType::Rgba), (false, png::ColorType::Rgb)] {
            let mut bytes = Vec::new();
            image.write_png(&mut bytes, alpha).unwrap();
            let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
            let reader = decoder.read_info().unwrap();
            assert_eq!(reader.info().color_type, color_type);
            assert_eq!((reader.info().width, reader.info().height), (3, 2));
        }
    }
}
