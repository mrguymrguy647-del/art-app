//! Pixels and tiles.
//!
//! A layer's pixels are stored in square blocks called tiles, each
//! `TILE_SIZE` x `TILE_SIZE` pixels. Only tiles that have been painted on
//! exist (see `surface.rs`), so big empty areas cost no memory.

use std::fmt::Debug;

/// Width and height of a tile, in pixels.
pub const TILE_SIZE: usize = 256;

/// Number of pixels in one tile.
pub const TILE_PIXELS: usize = TILE_SIZE * TILE_SIZE;

/// The smallest preview size we make of a tile: 256 >> 6 = 4 pixels across.
pub const MAX_PREVIEW_LEVEL: u32 = 6;

/// A pixel format the canvas can store.
///
/// Today there is only [`Rgba8`]. Later a 16-bit format can implement this
/// trait too, and tiles and surfaces will work with it unchanged.
pub trait Pixel: Copy + PartialEq + Debug + 'static {
    /// A fully transparent pixel.
    const TRANSPARENT: Self;

    /// This pixel as 8-bit premultiplied RGBA, the format screens use.
    fn to_display(self) -> [u8; 4];
}

/// An 8-bit RGBA pixel with **premultiplied alpha**.
///
/// "Premultiplied" means the colour is stored already multiplied by its
/// opacity: 50%-transparent red is (128, 0, 0, 128), not (255, 0, 0, 128).
/// It makes blending simpler and faster, and graphics cards expect it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);
    pub const BLACK: Self = Self::new(0, 0, 0, 255);
    pub const WHITE: Self = Self::new(255, 255, 255, 255);

    /// Makes a pixel from values that are already premultiplied.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Makes a pixel from a normal ("straight") colour plus an opacity,
    /// for example `from_straight(255, 0, 0, 128)` for 50%-transparent red.
    pub fn from_straight(r: u8, g: u8, b: u8, a: u8) -> Self {
        let premultiply = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
        Self::new(premultiply(r), premultiply(g), premultiply(b), a)
    }
}

impl Pixel for Rgba8 {
    const TRANSPARENT: Self = Rgba8::TRANSPARENT;

    fn to_display(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// One `TILE_SIZE` x `TILE_SIZE` block of pixels.
///
/// The `P = Rgba8` part means "the pixel type is Rgba8 unless you say
/// otherwise", so most code can just write `Tile`.
#[derive(Clone, PartialEq)]
pub struct Tile<P: Pixel = Rgba8> {
    /// Row by row, top to bottom. Always exactly `TILE_PIXELS` long.
    pixels: Box<[P]>,
}

impl<P: Pixel> Tile<P> {
    /// A tile where every pixel is transparent.
    pub fn new() -> Self {
        Self {
            pixels: vec![P::TRANSPARENT; TILE_PIXELS].into_boxed_slice(),
        }
    }

    /// The pixel at (x, y) inside this tile. Both must be below `TILE_SIZE`.
    pub fn pixel(&self, x: usize, y: usize) -> P {
        self.pixels[y * TILE_SIZE + x]
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, pixel: P) {
        self.pixels[y * TILE_SIZE + x] = pixel;
    }

    /// All pixels, row by row.
    pub fn pixels(&self) -> &[P] {
        &self.pixels
    }

    /// The tile as 8-bit premultiplied RGBA bytes for the screen, shrunk by
    /// half `level` times (0 = full size, 1 = 128x128, 2 = 64x64, ...).
    ///
    /// When you zoom out, the app shows these smaller versions instead of
    /// the full tile. That looks smoother (no shimmering) and sends less
    /// data to the graphics card. Each small pixel is the average of the
    /// block of big pixels it covers.
    pub fn display_bytes(&self, level: u32) -> Vec<u8> {
        let level = level.min(MAX_PREVIEW_LEVEL);
        let block = 1usize << level; // big pixels per small pixel, across
        let size = TILE_SIZE >> level; // small pixels across
        let count = (block * block) as u32;

        let mut bytes = Vec::with_capacity(size * size * 4);
        for small_y in 0..size {
            for small_x in 0..size {
                let mut sum = [0u32; 4];
                for y in small_y * block..(small_y + 1) * block {
                    for x in small_x * block..(small_x + 1) * block {
                        for (total, value) in sum.iter_mut().zip(self.pixel(x, y).to_display()) {
                            *total += u32::from(value);
                        }
                    }
                }
                // + count / 2 rounds to the nearest whole number.
                bytes.extend(sum.map(|total| ((total + count / 2) / count) as u8));
            }
        }
        bytes
    }
}

impl<P: Pixel> Default for Tile<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Pixel> Debug for Tile<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Printing 65 536 pixels isn't useful, so just say what it is.
        write!(f, "Tile({TILE_SIZE}x{TILE_SIZE})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tile_is_transparent() {
        let tile: Tile = Tile::new();
        assert_eq!(tile.pixels().len(), TILE_PIXELS);
        assert!(tile.pixels().iter().all(|&p| p == Rgba8::TRANSPARENT));
    }

    #[test]
    fn set_and_read_a_pixel() {
        let mut tile: Tile = Tile::new();
        tile.set_pixel(10, 20, Rgba8::WHITE);
        assert_eq!(tile.pixel(10, 20), Rgba8::WHITE);
        assert_eq!(tile.pixel(20, 10), Rgba8::TRANSPARENT);
        // Row by row: (10, 20) is at index 20 * 256 + 10.
        assert_eq!(tile.pixels()[20 * TILE_SIZE + 10], Rgba8::WHITE);
    }

    #[test]
    fn straight_colour_is_premultiplied() {
        assert_eq!(
            Rgba8::from_straight(255, 0, 0, 128),
            Rgba8::new(128, 0, 0, 128)
        );
        assert_eq!(
            Rgba8::from_straight(200, 100, 50, 255),
            Rgba8::new(200, 100, 50, 255)
        );
        assert_eq!(Rgba8::from_straight(200, 100, 50, 0), Rgba8::TRANSPARENT);
    }

    #[test]
    fn full_size_display_bytes_match_the_pixels() {
        let mut tile: Tile = Tile::new();
        tile.set_pixel(1, 0, Rgba8::new(10, 20, 30, 40));
        let bytes = tile.display_bytes(0);
        assert_eq!(bytes.len(), TILE_PIXELS * 4);
        assert_eq!(&bytes[4..8], &[10, 20, 30, 40]);
    }

    #[test]
    fn smaller_previews_average_the_pixels() {
        let mut tile: Tile = Tile::new();
        // Top-left 2x2 block: two white pixels, two transparent ones.
        tile.set_pixel(0, 0, Rgba8::WHITE);
        tile.set_pixel(1, 1, Rgba8::WHITE);

        let half = tile.display_bytes(1);
        assert_eq!(half.len(), 128 * 128 * 4);
        // Average of 255, 255, 0, 0 is 127.5, rounded to 128.
        assert_eq!(&half[0..4], &[128, 128, 128, 128]);
        // The next small pixel covers an empty block.
        assert_eq!(&half[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn preview_levels_get_smaller() {
        let tile: Tile = Tile::new();
        for level in 0..=MAX_PREVIEW_LEVEL {
            let size = TILE_SIZE >> level;
            assert_eq!(tile.display_bytes(level).len(), size * size * 4);
        }
        // Asking for a level beyond the smallest gives the smallest.
        let smallest = TILE_SIZE >> MAX_PREVIEW_LEVEL;
        assert_eq!(tile.display_bytes(99).len(), smallest * smallest * 4);
    }
}
