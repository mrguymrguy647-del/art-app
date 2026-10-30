//! Filling areas: the paint bucket (flood fill), gradients, and painting
//! the inside of any area with a colour or a gradient.
//!
//! Filling happens in two parts:
//! 1. Work out *where* to paint: a [`Mask`] that says, for each pixel, how
//!    much of it is inside the area (0 = not at all, 255 = completely).
//!    The paint bucket makes one by spreading out from a starting point;
//!    shapes make one from their outline.
//! 2. Paint that area with a [`Paint`]: one colour, or a gradient.
//!
//! Like strokes, filling returns the changed tiles as they were before,
//! so it can be undone.

use crate::brush::BrushMode;
use crate::color::over_paper;
use crate::document::{Background, Document};
use crate::layer::LayerId;
use crate::stroke::TilesBefore;
use crate::surface::{PixelRect, Surface, TileCoord};
use crate::tile::{Rgba8, TILE_SIZE, Tile};

/// A colour with its opacity: `[red, green, blue, alpha]`, each 0..255.
/// Not premultiplied: `[255, 0, 0, 128]` is half-transparent pure red.
pub type Rgba = [u8; 4];

// ---- Where to paint --------------------------------------------------------

/// For each pixel of a rectangle: how much of it is inside an area,
/// from 0 (outside) to 255 (completely inside). Values in between give
/// smooth ("anti-aliased") edges.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    rect: PixelRect,
    values: Vec<u8>,
}

impl Mask {
    /// A mask over `rect` with nothing inside yet.
    pub fn new(rect: PixelRect) -> Self {
        let rect = if rect.is_empty() {
            PixelRect::new(0, 0, 0, 0)
        } else {
            rect
        };
        let len = (rect.right - rect.left) as usize * (rect.bottom - rect.top) as usize;
        Self {
            rect,
            values: vec![0; len],
        }
    }

    /// The rectangle this mask covers. Everything outside it is outside the area.
    pub fn rect(&self) -> PixelRect {
        self.rect
    }

    fn width(&self) -> usize {
        (self.rect.right - self.rect.left) as usize
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let r = &self.rect;
        (x >= r.left && x < r.right && y >= r.top && y < r.bottom)
            .then(|| (y - r.top) as usize * self.width() + (x - r.left) as usize)
    }

    /// How much of pixel (x, y) is inside: 0..255.
    pub fn get(&self, x: i32, y: i32) -> u8 {
        self.index(x, y).map_or(0, |i| self.values[i])
    }

    /// Sets how much of pixel (x, y) is inside. Ignored outside `rect`.
    pub fn set(&mut self, x: i32, y: i32, value: u8) {
        if let Some(i) = self.index(x, y) {
            self.values[i] = value;
        }
    }

    /// True if no pixel is even partly inside.
    pub fn is_empty(&self) -> bool {
        self.values.iter().all(|&v| v == 0)
    }

    /// How many pixels are at least partly inside.
    pub fn covered_pixels(&self) -> usize {
        self.values.iter().filter(|&&v| v > 0).count()
    }

    /// The smallest rectangle holding every pixel that is at least partly
    /// inside (empty if none is).
    pub fn content_bounds(&self) -> PixelRect {
        let width = self.width();
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (row, values) in self.values.chunks(width.max(1)).enumerate() {
            let Some(first) = values.iter().position(|&v| v > 0) else {
                continue;
            };
            let last = values.iter().rposition(|&v| v > 0).unwrap_or(first);
            let y = self.rect.top + row as i32;
            top = top.min(y);
            bottom = bottom.max(y + 1);
            left = left.min(self.rect.left + first as i32);
            right = right.max(self.rect.left + last as i32 + 1);
        }
        if top > bottom {
            PixelRect::new(0, 0, 0, 0)
        } else {
            PixelRect::new(left, top, right, bottom)
        }
    }

    /// The values of row `y` from `left` to `right` (which must be inside
    /// `rect`).
    fn row(&self, y: i32, left: i32, right: i32) -> &[u8] {
        let start = (y - self.rect.top) as usize * self.width() + (left - self.rect.left) as usize;
        &self.values[start..start + (right - left) as usize]
    }

    /// A copy of the part of this mask inside `rect`.
    pub fn cropped(&self, rect: PixelRect) -> Mask {
        let mut cropped = Mask::new(rect.intersect(&self.rect));
        let r = cropped.rect;
        let width = cropped.width();
        for y in r.top..r.bottom {
            let start = (y - r.top) as usize * width;
            cropped.values[start..start + width].copy_from_slice(self.row(y, r.left, r.right));
        }
        cropped
    }

    /// Grows the area by `pixels` in every direction (staying inside
    /// `rect`), as a square: each pixel takes the highest value within
    /// `pixels` of it. Used so a fill tucks under the soft edges of the
    /// lines around it instead of leaving a light fringe.
    pub fn expand(&mut self, pixels: u32) {
        let reach = pixels as usize;
        let width = self.width();
        if reach == 0 || width == 0 {
            return;
        }
        let height = self.values.len() / width;
        // A square can be done in two quick passes: across, then down.
        let mut across = vec![0u8; self.values.len()];
        for y in 0..height {
            let row = &self.values[y * width..(y + 1) * width];
            for x in 0..width {
                across[y * width + x] = if row[x] == 255 {
                    255 // can't get any higher
                } else {
                    let window = &row[x.saturating_sub(reach)..(x + reach + 1).min(width)];
                    window.iter().copied().max().unwrap_or(0)
                };
            }
        }
        for y in 0..height {
            let (first, last) = (y.saturating_sub(reach), (y + reach).min(height - 1));
            for x in 0..width {
                let mut most = across[y * width + x];
                if most < 255 {
                    for row in first..=last {
                        most = most.max(across[row * width + x]);
                    }
                }
                self.values[y * width + x] = most;
            }
        }
    }
}

// ---- The paint bucket ------------------------------------------------------

/// Which pixels the paint bucket looks at to decide where to stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFrom {
    /// Only the layer being filled.
    Layer,
    /// Everything you see: all visible layers on the paper.
    Visible,
}

/// Settings for the paint bucket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloodFill {
    /// Where to start, in canvas pixels.
    pub x: f32,
    pub y: f32,
    /// How different a colour may be from the colour at the start and
    /// still be filled: 0 = only exactly the same colour, 1 = everything.
    /// It's measured on the biggest difference in red, green, blue or
    /// opacity, so 0.1 allows a difference of about 25 out of 255.
    pub tolerance: f32,
    pub sample: SampleFrom,
    /// Grow the filled area by this many pixels, so it tucks under the
    /// soft edges of the lines around it.
    pub expand: u32,
}

impl Default for FloodFill {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            tolerance: 0.1,
            sample: SampleFrom::Layer,
            expand: 1,
        }
    }
}

/// Works out where the paint bucket would fill on `layer`: every pixel
/// connected to the starting point (left, right, up or down) whose colour
/// is close enough to the starting colour. `None` if the start is
/// outside the canvas.
pub fn flood_region(document: &Document, layer: LayerId, fill: &FloodFill) -> Option<Mask> {
    if !document.contains(fill.x, fill.y) {
        return None;
    }
    let bounds = document.bounds();
    let width = (bounds.right - bounds.left) as usize;
    let height = (bounds.bottom - bounds.top) as usize;
    let pixels = sampled_pixels(document, layer, fill.sample, bounds);

    let start = (fill.y as usize) * width + fill.x as usize;
    let seed = pixels[start];
    let limit = (fill.tolerance.clamp(0.0, 1.0) * 255.0).round() as i32;
    let matches = |p: Rgba8| {
        let diff = |a: u8, b: u8| (i32::from(a) - i32::from(b)).abs();
        diff(p.r, seed.r)
            .max(diff(p.g, seed.g))
            .max(diff(p.b, seed.b))
            .max(diff(p.a, seed.a))
            <= limit
    };

    // A "scanline" fill: fill a whole run of a row at once, then look for
    // runs to continue in the rows above and below it.
    let mut mask = Mask::new(bounds);
    let inside = &mut mask.values; // 255 = filled
    let mut todo = vec![(fill.x as usize, fill.y as usize)];
    while let Some((x, y)) = todo.pop() {
        let row = y * width;
        if inside[row + x] != 0 || !matches(pixels[row + x]) {
            continue;
        }
        let mut left = x;
        while left > 0 && inside[row + left - 1] == 0 && matches(pixels[row + left - 1]) {
            left -= 1;
        }
        let mut right = x;
        while right + 1 < width && inside[row + right + 1] == 0 && matches(pixels[row + right + 1])
        {
            right += 1;
        }
        inside[row + left..=row + right].fill(255);

        for next_y in [y.wrapping_sub(1), y + 1] {
            if next_y >= height {
                continue; // above the top (wrapped round) or below the bottom
            }
            let next_row = next_y * width;
            let mut in_run = false;
            for next_x in left..=right {
                let fillable = inside[next_row + next_x] == 0 && matches(pixels[next_row + next_x]);
                if fillable && !in_run {
                    todo.push((next_x, next_y)); // the start of a run
                }
                in_run = fillable;
            }
        }
    }

    // Keep only the part that matters, plus room to grow.
    let content = mask.content_bounds();
    let margin = fill.expand as i32;
    let mut mask = mask.cropped(PixelRect::new(
        content.left - margin,
        content.top - margin,
        content.right + margin,
        content.bottom + margin,
    ));
    mask.expand(fill.expand);
    Some(mask)
}

/// The pixels the paint bucket looks at, row by row over `bounds`.
fn sampled_pixels(
    document: &Document,
    layer: LayerId,
    sample: SampleFrom,
    bounds: PixelRect,
) -> Vec<Rgba8> {
    let width = (bounds.right - bounds.left) as usize;
    let height = (bounds.bottom - bounds.top) as usize;
    let paper = over_paper(Rgba8::TRANSPARENT, document.paper());
    let empty = match sample {
        SampleFrom::Layer => Rgba8::TRANSPARENT,
        SampleFrom::Visible => Rgba8::new(paper[0], paper[1], paper[2], 255),
    };
    let mut pixels = vec![empty; width * height];

    let mut copy_tile = |coord: TileCoord, tile: &Tile| {
        let (left, top) = coord.origin();
        let size = TILE_SIZE as i32;
        let area = bounds.intersect(&PixelRect::new(left, top, left + size, top + size));
        let row_width = (area.right - area.left) as usize;
        for y in area.top..area.bottom {
            let from = (y - top) as usize * TILE_SIZE + (area.left - left) as usize;
            let source = &tile.pixels()[from..from + row_width];
            let to = (y - bounds.top) as usize * width + (area.left - bounds.left) as usize;
            pixels[to..to + row_width].copy_from_slice(source);
        }
    };
    for coord in bounds.tiles() {
        match sample {
            SampleFrom::Layer => {
                if let Some(tile) = document.layer(layer).and_then(|l| l.surface.tile(coord)) {
                    copy_tile(coord, tile);
                }
            }
            SampleFrom::Visible => {
                if let Some(tile) = document.composite_tile(coord, Background::Paper) {
                    copy_tile(coord, &tile);
                }
            }
        }
    }
    pixels
}

// ---- What to paint with ----------------------------------------------------

/// What fills an area: one colour, or a gradient.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Color(Rgba),
    Gradient(Gradient),
}

/// The shape of a gradient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientShape {
    /// Colour changes along the line from `start` to `end`, and stays the
    /// same across it (like a sky).
    Linear,
    /// Rings around `start`: the first colour in the middle, the last one
    /// at the distance of `end` and beyond (like a glow).
    Radial,
}

/// One colour of a gradient, and where it is: 0 = at the start, 1 = at the end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub position: f32,
    pub color: Rgba,
}

/// A smooth change between two or more colours.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub shape: GradientShape,
    /// In canvas pixels.
    pub start: (f32, f32),
    pub end: (f32, f32),
    /// At least one, sorted by position.
    stops: Vec<GradientStop>,
}

impl Gradient {
    /// A gradient through `colors`, spread evenly from start to end.
    /// With no colours at all, it's transparent.
    pub fn new(shape: GradientShape, start: (f32, f32), end: (f32, f32), colors: &[Rgba]) -> Self {
        let last = colors.len().saturating_sub(1).max(1) as f32;
        let stops = colors
            .iter()
            .enumerate()
            .map(|(i, &color)| GradientStop {
                position: i as f32 / last,
                color,
            })
            .collect();
        Self::with_stops(shape, start, end, stops)
    }

    /// A gradient with colours at chosen positions (0..1). They are
    /// sorted, and positions outside 0..1 are moved to the nearest end.
    pub fn with_stops(
        shape: GradientShape,
        start: (f32, f32),
        end: (f32, f32),
        mut stops: Vec<GradientStop>,
    ) -> Self {
        for stop in &mut stops {
            stop.position = if stop.position.is_finite() {
                stop.position.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        if stops.is_empty() {
            stops.push(GradientStop {
                position: 0.0,
                color: [0, 0, 0, 0],
            });
        }
        Self {
            shape,
            start,
            end,
            stops,
        }
    }

    pub fn stops(&self) -> &[GradientStop] {
        &self.stops
    }

    /// Where point (x, y) is along the gradient: 0 at the start, 1 at the
    /// end (and beyond).
    pub fn position_at(&self, x: f32, y: f32) -> f32 {
        let (dx, dy) = (self.end.0 - self.start.0, self.end.1 - self.start.1);
        let length_squared = dx * dx + dy * dy;
        if length_squared < 1e-6 {
            return 1.0; // start and end in the same place: all end colour
        }
        let (px, py) = (x - self.start.0, y - self.start.1);
        let t = match self.shape {
            GradientShape::Linear => (px * dx + py * dy) / length_squared,
            GradientShape::Radial => (px * px + py * py).sqrt() / length_squared.sqrt(),
        };
        t.clamp(0.0, 1.0)
    }

    /// The colour at point (x, y), premultiplied, each 0..255.
    ///
    /// Colours are mixed premultiplied, so fading a colour out to
    /// transparent keeps its colour instead of turning greyish.
    pub fn color_at(&self, x: f32, y: f32) -> [f32; 4] {
        self.color_at_position(self.position_at(x, y))
    }

    /// The colour at position `t` along the gradient (0..1), premultiplied.
    pub fn color_at_position(&self, t: f32) -> [f32; 4] {
        let after = self.stops.partition_point(|stop| stop.position <= t);
        if after == 0 {
            return premultiplied(self.stops[0].color);
        }
        if after == self.stops.len() {
            return premultiplied(self.stops[after - 1].color);
        }
        let (a, b) = (self.stops[after - 1], self.stops[after]);
        let w = (t - a.position) / (b.position - a.position).max(1e-6);
        let (ca, cb) = (premultiplied(a.color), premultiplied(b.color));
        std::array::from_fn(|i| ca[i] + (cb[i] - ca[i]) * w)
    }
}

/// A straight colour as premultiplied floats (0..255).
fn premultiplied(color: Rgba) -> [f32; 4] {
    let alpha = f32::from(color[3]) / 255.0;
    [
        f32::from(color[0]) * alpha,
        f32::from(color[1]) * alpha,
        f32::from(color[2]) * alpha,
        f32::from(color[3]),
    ]
}

// ---- Painting an area ------------------------------------------------------

/// A 4x4 "ordered dither" pattern. Gradients only have 256 steps per
/// colour, which can show as visible bands on big smooth gradients. Adding
/// this tiny, regular pattern (less than half a step) before rounding
/// breaks the bands up. It's the same every time, so results stay
/// predictable.
const DITHER: [[f32; 4]; 4] = [
    [0.0, 8.0, 2.0, 10.0],
    [12.0, 4.0, 14.0, 6.0],
    [3.0, 11.0, 1.0, 9.0],
    [15.0, 7.0, 13.0, 5.0],
];

fn dither_offset(x: i32, y: i32) -> f32 {
    (DITHER[(y & 3) as usize][(x & 3) as usize] + 0.5) / 16.0 - 0.5
}

/// How finely a gradient is worked out in advance: this many colours
/// from start to end. Far finer than the 256 steps of a colour channel.
const GRADIENT_STEPS: usize = 4096;

/// A paint, prepared for painting many pixels quickly.
enum Prepared<'a> {
    /// One colour, premultiplied (as floats and as a pixel).
    Color([f32; 4], Rgba8),
    /// The gradient's colours worked out in advance at `GRADIENT_STEPS`
    /// positions, so each pixel only has to look its colour up.
    Gradient(
        Positions,
        Vec<[f32; 4]>,
        std::marker::PhantomData<&'a Gradient>,
    ),
}

/// Works out positions along a gradient quickly: the sums that are the
/// same for every pixel are done once, in advance.
struct Positions {
    shape: GradientShape,
    start: (f32, f32),
    /// Linear: the direction divided by its length squared.
    /// Radial: 1 / radius in `along.0`.
    along: (f32, f32),
    /// Start and end are in the same place.
    degenerate: bool,
}

impl Positions {
    fn new(gradient: &Gradient) -> Self {
        let (dx, dy) = (
            gradient.end.0 - gradient.start.0,
            gradient.end.1 - gradient.start.1,
        );
        let length_squared = dx * dx + dy * dy;
        let along = match gradient.shape {
            GradientShape::Linear => (dx / length_squared, dy / length_squared),
            GradientShape::Radial => (1.0 / length_squared.sqrt(), 0.0),
        };
        Self {
            shape: gradient.shape,
            start: gradient.start,
            along,
            degenerate: length_squared < 1e-6,
        }
    }

    /// The same as `Gradient::position_at`.
    fn at(&self, x: f32, y: f32) -> f32 {
        if self.degenerate {
            return 1.0;
        }
        let (px, py) = (x - self.start.0, y - self.start.1);
        let t = match self.shape {
            GradientShape::Linear => px * self.along.0 + py * self.along.1,
            GradientShape::Radial => (px * px + py * py).sqrt() * self.along.0,
        };
        t.clamp(0.0, 1.0)
    }
}

impl<'a> Prepared<'a> {
    fn new(paint: &'a Paint) -> Self {
        match paint {
            Paint::Color(color) => {
                let [r, g, b, a] = *color;
                Prepared::Color(premultiplied(*color), Rgba8::from_straight(r, g, b, a))
            }
            Paint::Gradient(gradient) => {
                let colors = (0..GRADIENT_STEPS)
                    .map(|i| gradient.color_at_position(i as f32 / (GRADIENT_STEPS - 1) as f32))
                    .collect();
                Prepared::Gradient(Positions::new(gradient), colors, std::marker::PhantomData)
            }
        }
    }

    fn color_at(&self, x: f32, y: f32) -> [f32; 4] {
        match self {
            Prepared::Color(color, _) => *color,
            Prepared::Gradient(positions, colors, _) => {
                let t = positions.at(x, y);
                colors[(t * (GRADIENT_STEPS - 1) as f32 + 0.5) as usize]
            }
        }
    }
}

/// How much of each pixel to paint: all of it, or what a mask says.
#[derive(Clone, Copy)]
enum Coverage<'a> {
    Full,
    Mask(&'a Mask),
}

/// Paints (or, in erase mode, rubs out) `paint` inside `mask` on
/// `surface`, never outside `limits`. `opacity` (0..1) makes it fainter.
/// Returns the changed tiles as they were before.
pub fn paint_mask(
    surface: &mut Surface,
    mask: &Mask,
    paint: &Paint,
    opacity: f32,
    mode: BrushMode,
    limits: PixelRect,
) -> TilesBefore {
    let rect = mask.rect().intersect(&limits);
    paint_where(surface, rect, Coverage::Mask(mask), paint, opacity, mode)
}

/// Paints (or rubs out) `paint` over the whole of `rect`, e.g. a gradient
/// across the canvas. Returns the changed tiles as they were before.
pub fn paint_rect(
    surface: &mut Surface,
    rect: PixelRect,
    paint: &Paint,
    opacity: f32,
    mode: BrushMode,
) -> TilesBefore {
    paint_where(surface, rect, Coverage::Full, paint, opacity, mode)
}

fn paint_where(
    surface: &mut Surface,
    rect: PixelRect,
    coverage: Coverage,
    paint: &Paint,
    opacity: f32,
    mode: BrushMode,
) -> TilesBefore {
    let mut before = TilesBefore::new();
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 || rect.is_empty() {
        return before;
    }
    let prepared = Prepared::new(paint);
    // Painting an opaque colour at full strength simply replaces pixels.
    let replace_with = match prepared {
        Prepared::Color(_, pixel)
            if pixel.a == 255 && opacity >= 1.0 && mode == BrushMode::Paint =>
        {
            Some(pixel)
        }
        _ => None,
    };
    let dither = matches!(prepared, Prepared::Gradient(..));

    for coord in rect.tiles() {
        let existing = surface.tile(coord);
        if mode == BrushMode::Erase && existing.is_none() {
            continue; // nothing here to rub out
        }
        let mut tile = existing.cloned().unwrap_or_default();
        let (left, top) = coord.origin();
        let size = TILE_SIZE as i32;
        let area = rect.intersect(&PixelRect::new(left, top, left + size, top + size));

        let mut changed = false;
        for y in area.top..area.bottom {
            let start = (y - top) as usize * TILE_SIZE + (area.left - left) as usize;
            let row = &mut tile.pixels_mut()[start..start + (area.right - area.left) as usize];
            let row_coverage = match coverage {
                Coverage::Full => None,
                Coverage::Mask(mask) => Some(mask.row(y, area.left, area.right)),
            };
            for (i, pixel) in row.iter_mut().enumerate() {
                let covered = row_coverage.map_or(255, |c| c[i]);
                if covered == 0 {
                    continue;
                }
                let new = match replace_with {
                    Some(color) if covered == 255 => color,
                    _ => {
                        let x = area.left + i as i32;
                        let strength = f32::from(covered) / 255.0 * opacity;
                        let color = prepared.color_at(x as f32 + 0.5, y as f32 + 0.5);
                        let offset = if dither { dither_offset(x, y) } else { 0.0 };
                        mix(*pixel, color, strength, mode, offset)
                    }
                };
                if new != *pixel {
                    *pixel = new;
                    changed = true;
                }
            }
        }
        if changed {
            let tile = (!tile.is_transparent()).then_some(tile);
            let old = surface.replace_tile(coord, tile);
            before.insert(coord, old);
        }
    }
    before
}

/// One pixel: `color` (premultiplied, 0..255) laid over `under` with the
/// given strength, or in erase mode, `under` rubbed out by the colour's
/// opacity times the strength. `dither` (under half a step) is added
/// before rounding.
fn mix(under: Rgba8, color: [f32; 4], strength: f32, mode: BrushMode, dither: f32) -> Rgba8 {
    let keep = 1.0 - color[3] / 255.0 * strength;
    let add = match mode {
        BrushMode::Paint => strength,
        BrushMode::Erase => 0.0,
    };
    // Adding 0.5 and cutting off the fraction rounds to the nearest
    // whole number; `as u8` also keeps the result within 0..255.
    let channel =
        |paint: f32, under: u8| (paint * add + f32::from(under) * keep + dither + 0.5) as u8;
    let a = channel(color[3], under.a);
    // Premultiplied: a colour can never be stronger than its opacity.
    Rgba8::new(
        channel(color[0], under.r).min(a),
        channel(color[1], under.g).min(a),
        channel(color[2], under.b).min(a),
        a,
    )
}

// ---- Whole operations on a document ------------------------------------------

/// The paint bucket: fills the area around the starting point on
/// `layer`. Returns the changed tiles as they were before (empty if
/// nothing changed).
pub fn flood_fill(
    document: &mut Document,
    layer: LayerId,
    fill: &FloodFill,
    paint: &Paint,
    opacity: f32,
) -> TilesBefore {
    let Some(mask) = flood_region(document, layer, fill) else {
        return TilesBefore::new();
    };
    let limits = document.bounds();
    match document.layer_mut(layer) {
        Some(target) => paint_mask(
            &mut target.surface,
            &mask,
            paint,
            opacity,
            BrushMode::Paint,
            limits,
        ),
        None => TilesBefore::new(),
    }
}

/// Covers the whole of `layer` with `paint` (usually a gradient).
/// Returns the changed tiles as they were before.
pub fn fill_layer(
    document: &mut Document,
    layer: LayerId,
    paint: &Paint,
    opacity: f32,
) -> TilesBefore {
    let bounds = document.bounds();
    match document.layer_mut(layer) {
        Some(target) => paint_rect(
            &mut target.surface,
            bounds,
            paint,
            opacity,
            BrushMode::Paint,
        ),
        None => TilesBefore::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::BrushSettings;
    use crate::stroke::{Stroke, StrokePoint};

    const RED: Rgba = [255, 0, 0, 255];
    const BLUE: Rgba = [0, 0, 255, 255];

    /// A 300x200 document with a black ring (a closed outline) on Layer 1.
    fn document_with_ring() -> Document {
        let mut document = Document::new(300, 200);
        let brush = BrushSettings {
            size: 6.0,
            hardness: 1.0,
            color: [0, 0, 0],
            ..BrushSettings::default()
        };
        let mut stroke = Stroke::new(brush, document.bounds());
        let layer = &mut document.active_layer_mut().surface;
        for i in 0..=64 {
            let angle = i as f32 / 64.0 * std::f32::consts::TAU;
            let point =
                StrokePoint::new(150.0 + 60.0 * angle.cos(), 100.0 + 60.0 * angle.sin(), 1.0);
            stroke.add_point(layer, point);
        }
        stroke.finish(layer);
        document
    }

    fn pixel(document: &Document, x: i32, y: i32) -> Rgba8 {
        document.active_layer().surface.pixel(x, y)
    }

    #[test]
    fn the_bucket_fills_inside_a_closed_outline_only() {
        let mut document = document_with_ring();
        let id = document.active_id();
        let fill = FloodFill {
            x: 150.0,
            y: 100.0,
            expand: 0,
            ..FloodFill::default()
        };
        let before = flood_fill(&mut document, id, &fill, &Paint::Color(RED), 1.0);
        assert!(!before.is_empty());
        assert_eq!(
            pixel(&document, 150, 100),
            Rgba8::new(255, 0, 0, 255),
            "the middle"
        );
        assert_eq!(
            pixel(&document, 120, 100),
            Rgba8::new(255, 0, 0, 255),
            "inside"
        );
        assert_eq!(
            pixel(&document, 10, 10),
            Rgba8::TRANSPARENT,
            "outside stays empty"
        );
        assert_eq!(
            pixel(&document, 150, 40),
            Rgba8::new(0, 0, 0, 255),
            "the ring itself"
        );
    }

    #[test]
    fn expanding_tucks_the_fill_under_soft_edges() {
        let mut plain = document_with_ring();
        let mut grown = document_with_ring();
        let id = plain.active_id();
        let fill = FloodFill {
            x: 150.0,
            y: 100.0,
            expand: 0,
            ..FloodFill::default()
        };
        flood_fill(&mut plain, id, &fill, &Paint::Color(RED), 1.0);
        let grown_fill = FloodFill { expand: 2, ..fill };
        flood_fill(&mut grown, id, &grown_fill, &Paint::Color(RED), 1.0);
        // Count reddish pixels: growing must cover more of the ring's inner edge.
        let red = |d: &Document| {
            (0..200)
                .flat_map(|y| (0..300).map(move |x| (x, y)))
                .filter(|&(x, y)| pixel(d, x, y).r > 100)
                .count()
        };
        assert!(red(&grown) > red(&plain));
    }

    #[test]
    fn tolerance_decides_how_far_the_fill_spreads() {
        // Left half dark grey, right half a slightly lighter grey.
        let mut document = Document::new(100, 50);
        let id = document.active_id();
        for y in 0..50 {
            for x in 0..100 {
                let v = if x < 50 { 100 } else { 110 };
                document
                    .active_layer_mut()
                    .surface
                    .set_pixel(x, y, Rgba8::new(v, v, v, 255));
            }
        }
        let strict = FloodFill {
            x: 10.0,
            y: 10.0,
            tolerance: 0.0,
            expand: 0,
            ..FloodFill::default()
        };
        let mask = flood_region(&document, id, &strict).unwrap();
        assert_eq!(mask.covered_pixels(), 50 * 50, "only the left half");

        let loose = FloodFill {
            tolerance: 0.1,
            ..strict
        };
        let mask = flood_region(&document, id, &loose).unwrap();
        assert_eq!(mask.covered_pixels(), 100 * 50, "both halves");
    }

    #[test]
    fn sampling_all_layers_stops_at_lines_on_other_layers() {
        // The ring is on Layer 1; we fill an empty Layer 2 above it.
        let mut document = document_with_ring();
        let layer = document.new_layer();
        let id = layer.id();
        document.insert_layer(1, layer);

        let only_this_layer = FloodFill {
            x: 150.0,
            y: 100.0,
            sample: SampleFrom::Layer,
            expand: 0,
            ..FloodFill::default()
        };
        let mask = flood_region(&document, id, &only_this_layer).unwrap();
        assert_eq!(
            mask.covered_pixels(),
            300 * 200,
            "an empty layer fills everywhere"
        );

        let everything = FloodFill {
            sample: SampleFrom::Visible,
            ..only_this_layer
        };
        let mask = flood_region(&document, id, &everything).unwrap();
        assert!(mask.covered_pixels() < 120 * 120, "stops at the ring");
        assert_eq!(mask.get(150, 100), 255);
        assert_eq!(mask.get(10, 10), 0);
    }

    #[test]
    fn starting_outside_the_canvas_fills_nothing() {
        let mut document = Document::new(100, 100);
        let id = document.active_id();
        let fill = FloodFill {
            x: -5.0,
            y: 20.0,
            ..FloodFill::default()
        };
        assert!(flood_fill(&mut document, id, &fill, &Paint::Color(RED), 1.0).is_empty());
    }

    #[test]
    fn a_linear_gradient_goes_from_start_colour_to_end_colour() {
        let gradient = Gradient::new(
            GradientShape::Linear,
            (0.0, 0.0),
            (100.0, 0.0),
            &[RED, BLUE],
        );
        assert_eq!(
            gradient.color_at(-10.0, 5.0),
            [255.0, 0.0, 0.0, 255.0],
            "before the start"
        );
        assert_eq!(
            gradient.color_at(50.0, 77.0),
            [127.5, 0.0, 127.5, 255.0],
            "half way"
        );
        assert_eq!(
            gradient.color_at(150.0, 0.0),
            [0.0, 0.0, 255.0, 255.0],
            "past the end"
        );
    }

    #[test]
    fn a_radial_gradient_makes_rings() {
        let gradient = Gradient::new(
            GradientShape::Radial,
            (50.0, 50.0),
            (50.0, 150.0),
            &[RED, BLUE],
        );
        // Same distance from the centre, any direction: same colour.
        assert_eq!(gradient.color_at(100.0, 50.0), gradient.color_at(50.0, 0.0));
        assert_eq!(gradient.position_at(50.0, 50.0), 0.0);
        assert_eq!(gradient.position_at(50.0, 100.0), 0.5);
    }

    #[test]
    fn fading_to_transparent_keeps_the_colour() {
        let gradient = Gradient::new(
            GradientShape::Linear,
            (0.0, 0.0),
            (100.0, 0.0),
            &[RED, [0, 0, 0, 0]],
        );
        let [r, g, b, a] = gradient.color_at(50.0, 0.0);
        assert_eq!(a, 127.5);
        assert_eq!(
            (r, g, b),
            (127.5, 0.0, 0.0),
            "premultiplied pure red, not grey"
        );
    }

    #[test]
    fn stops_can_be_placed_anywhere_and_are_sorted() {
        let stops = vec![
            GradientStop {
                position: 1.0,
                color: BLUE,
            },
            GradientStop {
                position: 0.2,
                color: RED,
            },
        ];
        let gradient = Gradient::with_stops(GradientShape::Linear, (0.0, 0.0), (100.0, 0.0), stops);
        assert_eq!(gradient.stops()[0].color, RED);
        assert_eq!(
            gradient.color_at(10.0, 0.0),
            [255.0, 0.0, 0.0, 255.0],
            "red up to 20%"
        );
    }

    #[test]
    fn a_gradient_across_the_layer_can_be_undone() {
        let mut document = Document::new(600, 300);
        let id = document.active_id();
        let gradient = Gradient::new(
            GradientShape::Linear,
            (0.0, 0.0),
            (600.0, 0.0),
            &[RED, BLUE],
        );
        let before = fill_layer(&mut document, id, &Paint::Gradient(gradient), 1.0);
        assert_eq!(before.len(), 3 * 2, "every tile of a 600x300 canvas");
        assert!(
            before.values().all(Option::is_none),
            "they were all empty before"
        );
        let left = pixel(&document, 5, 100);
        let right = pixel(&document, 595, 100);
        assert!(left.r > 240 && left.b < 15, "{left:?}");
        assert!(right.b > 240 && right.r < 15, "{right:?}");

        // Putting the old tiles back is exactly what undo does.
        let surface = &mut document.active_layer_mut().surface;
        for (coord, tile) in before {
            surface.replace_tile(coord, tile);
        }
        assert_eq!(surface.tile_count(), 0);
    }

    #[test]
    fn fill_opacity_lets_the_layer_show_through() {
        let mut document = Document::new(64, 64);
        let id = document.active_id();
        fill_layer(&mut document, id, &Paint::Color(BLUE), 1.0);
        fill_layer(&mut document, id, &Paint::Color(RED), 0.5);
        assert_eq!(pixel(&document, 10, 10), Rgba8::new(128, 0, 128, 255));
    }

    #[test]
    fn erasing_with_a_mask_removes_emptied_tiles() {
        let mut document = Document::new(64, 64);
        let id = document.active_id();
        fill_layer(&mut document, id, &Paint::Color(RED), 1.0);
        let mut mask = Mask::new(document.bounds());
        for y in 0..64 {
            for x in 0..64 {
                mask.set(x, y, 255);
            }
        }
        let surface = &mut document.layer_mut(id).unwrap().surface;
        let before = paint_mask(
            surface,
            &mask,
            &Paint::Color(RED),
            1.0,
            BrushMode::Erase,
            PixelRect::new(0, 0, 64, 64),
        );
        assert_eq!(before.len(), 1);
        assert_eq!(surface.tile_count(), 0, "the empty tile was removed");
    }

    #[test]
    fn dithering_never_changes_exact_colours() {
        for y in 0..4 {
            for x in 0..4 {
                let offset = dither_offset(x, y);
                assert!(offset.abs() < 0.5, "{offset}");
                let out = mix(
                    Rgba8::TRANSPARENT,
                    [10.0, 20.0, 30.0, 255.0],
                    1.0,
                    BrushMode::Paint,
                    offset,
                );
                assert_eq!(out, Rgba8::new(10, 20, 30, 255));
            }
        }
    }

    #[test]
    fn mask_bounds_and_cropping() {
        let mut mask = Mask::new(PixelRect::new(0, 0, 100, 100));
        assert!(mask.is_empty());
        assert!(mask.content_bounds().is_empty());
        mask.set(10, 20, 255);
        mask.set(30, 25, 100);
        assert_eq!(mask.content_bounds(), PixelRect::new(10, 20, 31, 26));
        let cropped = mask.cropped(mask.content_bounds());
        assert_eq!(cropped.rect(), PixelRect::new(10, 20, 31, 26));
        assert_eq!(cropped.get(30, 25), 100);
        assert_eq!(cropped.get(0, 0), 0, "outside the mask");
    }
}
