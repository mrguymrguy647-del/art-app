//! Filters: blur, glow, drop shadow, colour adjustments and a gradient
//! map, applied to everything on a surface (a layer, or a group's
//! scratch drawing).
//!
//! A filter works on a block of premultiplied pixels copied off the
//! surface (the painted area, grown by how far the filter reaches), and
//! the result is written back a tile at a time, returning the tiles as
//! they were for undo.

use crate::color::Hsv;
use crate::fill::{GradientStop, color_through_stops};
use crate::surface::{PixelRect, Surface, TileCoord};
use crate::tile::{Rgba8, TILE_SIZE, Tile};

/// The biggest blur radius, in pixels.
pub const MAX_BLUR: f32 = 500.0;

/// One filter.
#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    /// Softens everything by about this many pixels.
    Blur(f32),
    /// Light pixels (lightness above `threshold`, 0..1) spread `radius`
    /// pixels, `strength` times, and are added on top.
    Glow {
        radius: f32,
        threshold: f32,
        strength: f32,
    },
    /// A copy of the drawing's silhouette in `color` (not premultiplied),
    /// moved by `offset` and softened by `blur`, put behind it.
    Shadow {
        offset: (f32, f32),
        blur: f32,
        color: [u8; 4],
    },
    /// `hue` in degrees; the rest -1..1 (0 = no change).
    Adjust {
        hue: f32,
        saturation: f32,
        brightness: f32,
        contrast: f32,
    },
    /// Every pixel recoloured by its lightness through the stops.
    GradientMap(Vec<GradientStop>),
}

impl Filter {
    /// How many pixels beyond the painted area the filter can reach.
    pub fn reach(&self) -> i32 {
        match self {
            Filter::Blur(radius) => blur_reach(*radius),
            Filter::Glow { radius, .. } => blur_reach(*radius),
            Filter::Shadow { offset, blur, .. } => {
                blur_reach(*blur) + offset.0.abs().ceil() as i32 + offset.1.abs().ceil() as i32
            }
            Filter::Adjust { .. } | Filter::GradientMap(_) => 0,
        }
    }

    /// True if it softens pixels (so it's a mistake on a pixel canvas).
    pub fn blurs(&self) -> bool {
        match self {
            Filter::Blur(radius) | Filter::Glow { radius, .. } => *radius > 0.0,
            Filter::Shadow { blur, .. } => *blur > 0.0,
            Filter::Adjust { .. } | Filter::GradientMap(_) => false,
        }
    }
}

fn blur_reach(radius: f32) -> i32 {
    if radius <= 0.0 {
        0
    } else {
        (radius * 2.0).ceil() as i32 + 2
    }
}

/// Applies `filters`, in order, to everything on `surface`, never
/// painting outside `limits` (the canvas). Returns the changed tiles as
/// they were before (empty if nothing changed).
pub fn apply(surface: &mut Surface, filters: &[Filter], limits: PixelRect) -> TilesBefore {
    let Some(painted) = surface.painted_area() else {
        return TilesBefore::new();
    };
    if filters.is_empty() {
        return TilesBefore::new();
    }
    let reach: i32 = filters.iter().map(Filter::reach).sum();
    let area = PixelRect::new(
        painted.left - reach,
        painted.top - reach,
        painted.right + reach,
        painted.bottom + reach,
    )
    .intersect(&limits);
    let mut block = Block::copy(surface, area);
    for filter in filters {
        block.apply(filter);
    }
    block.write_back(surface)
}

/// The changed tiles as they were, for undo (the same as a stroke's).
pub type TilesBefore = crate::stroke::TilesBefore;

/// A block of premultiplied pixels (each channel 0..255, as floats).
struct Block {
    area: PixelRect,
    width: usize,
    height: usize,
    pixels: Vec<[f32; 4]>,
}

impl Block {
    fn copy(surface: &Surface, area: PixelRect) -> Self {
        let width = (area.right - area.left).max(0) as usize;
        let height = (area.bottom - area.top).max(0) as usize;
        let mut pixels = vec![[0.0; 4]; width * height];
        let size = TILE_SIZE as i32;
        for coord in area.tiles() {
            let Some(tile) = surface.tile(coord) else {
                continue;
            };
            let (left, top) = coord.origin();
            let part = area.intersect(&PixelRect::new(left, top, left + size, top + size));
            for y in part.top..part.bottom {
                for x in part.left..part.right {
                    let p = tile.pixel((x - left) as usize, (y - top) as usize);
                    pixels[(y - area.top) as usize * width + (x - area.left) as usize] = [
                        f32::from(p.r),
                        f32::from(p.g),
                        f32::from(p.b),
                        f32::from(p.a),
                    ];
                }
            }
        }
        Self {
            area,
            width,
            height,
            pixels,
        }
    }

    /// Writes the block onto `surface`, tile by tile, keeping each
    /// changed tile as it was.
    fn write_back(&self, surface: &mut Surface) -> TilesBefore {
        let mut before = TilesBefore::new();
        let size = TILE_SIZE as i32;
        for coord in self.area.tiles() {
            let (left, top) = coord.origin();
            let part = self
                .area
                .intersect(&PixelRect::new(left, top, left + size, top + size));
            let mut tile = surface.tile(coord).cloned().unwrap_or_default();
            let mut changed = false;
            for y in part.top..part.bottom {
                for x in part.left..part.right {
                    let p = self.pixels
                        [(y - self.area.top) as usize * self.width + (x - self.area.left) as usize];
                    let new = to_pixel(p);
                    let (tx, ty) = ((x - left) as usize, (y - top) as usize);
                    if tile.pixel(tx, ty) != new {
                        tile.set_pixel(tx, ty, new);
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

    fn apply(&mut self, filter: &Filter) {
        match filter {
            Filter::Blur(radius) => blur(&mut self.pixels, self.width, self.height, *radius),
            Filter::Glow {
                radius,
                threshold,
                strength,
            } => self.glow(*radius, *threshold, *strength),
            Filter::Shadow {
                offset,
                blur,
                color,
            } => self.shadow(*offset, *blur, *color),
            Filter::Adjust {
                hue,
                saturation,
                brightness,
                contrast,
            } => self.adjust(*hue, *saturation, *brightness, *contrast),
            Filter::GradientMap(stops) => self.gradient_map(stops),
        }
    }

    fn glow(&mut self, radius: f32, threshold: f32, strength: f32) {
        if strength <= 0.0 {
            return;
        }
        // The light parts, with a soft start just below the threshold.
        let mut light: Vec<[f32; 4]> = self
            .pixels
            .iter()
            .map(|&p| {
                let l = lightness(p);
                let amount = ((l - threshold) / 0.1 + 1.0).clamp(0.0, 1.0) * strength;
                [p[0] * amount, p[1] * amount, p[2] * amount, p[3] * amount]
            })
            .collect();
        blur(&mut light, self.width, self.height, radius);
        for (p, g) in self.pixels.iter_mut().zip(&light) {
            // Added on top: brighter inside, a halo outside.
            let a = p[3] + g[3] * (1.0 - p[3] / 255.0);
            p[0] = (p[0] + g[0]).min(a);
            p[1] = (p[1] + g[1]).min(a);
            p[2] = (p[2] + g[2]).min(a);
            p[3] = a;
        }
    }

    /// Replaces the pixels by their shadow alone.
    fn shadow_only(&mut self, offset: (f32, f32), radius: f32, color: [u8; 4]) {
        let shadow = self.make_shadow(offset, radius, color);
        self.pixels = shadow;
    }

    fn shadow(&mut self, offset: (f32, f32), radius: f32, color: [u8; 4]) {
        let shadow = self.make_shadow(offset, radius, color);
        // The drawing over its shadow.
        for (p, s) in self.pixels.iter_mut().zip(&shadow) {
            let keep = 1.0 - p[3] / 255.0;
            for i in 0..4 {
                p[i] += s[i] * keep;
            }
        }
    }

    /// The shadow of the pixels: their silhouette in `color`, moved and
    /// softened.
    fn make_shadow(&self, offset: (f32, f32), radius: f32, color: [u8; 4]) -> Vec<[f32; 4]> {
        let [r, g, b, a] = color.map(f32::from);
        let tint = [r * a / 255.0, g * a / 255.0, b * a / 255.0, a];
        // The silhouette, moved by the offset (whole pixels; the blur
        // hides the rounding).
        let (dx, dy) = (offset.0.round() as i64, offset.1.round() as i64);
        let mut shadow = vec![[0.0; 4]; self.pixels.len()];
        for y in 0..self.height as i64 {
            for x in 0..self.width as i64 {
                let (sx, sy) = (x - dx, y - dy);
                if sx < 0 || sy < 0 || sx >= self.width as i64 || sy >= self.height as i64 {
                    continue;
                }
                let alpha = self.pixels[sy as usize * self.width + sx as usize][3] / 255.0;
                if alpha > 0.0 {
                    shadow[y as usize * self.width + x as usize] = [
                        tint[0] * alpha,
                        tint[1] * alpha,
                        tint[2] * alpha,
                        tint[3] * alpha,
                    ];
                }
            }
        }
        blur(&mut shadow, self.width, self.height, radius);
        shadow
    }

    fn adjust(&mut self, hue: f32, saturation: f32, brightness: f32, contrast: f32) {
        for p in &mut self.pixels {
            if p[3] <= 0.0 {
                continue;
            }
            let [r, g, b] = straight(*p);
            let mut hsv = Hsv::from_rgb([r, g, b]);
            hsv.h += hue / 360.0;
            hsv.s = (hsv.s * (1.0 + saturation)).clamp(0.0, 1.0);
            hsv.v = (hsv.v + brightness).clamp(0.0, 1.0);
            let rgb = hsv.to_rgb().map(|c| {
                let c = f32::from(c) / 255.0;
                (((c - 0.5) * (1.0 + contrast) + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8
            });
            *p = premultiply(rgb, p[3]);
        }
    }

    fn gradient_map(&mut self, stops: &[GradientStop]) {
        if stops.is_empty() {
            return;
        }
        for p in &mut self.pixels {
            if p[3] <= 0.0 {
                continue;
            }
            let mapped = color_through_stops(stops, lightness(*p));
            // The mapped colour's own opacity, times the pixel's.
            let a = p[3] / 255.0;
            *p = [mapped[0] * a, mapped[1] * a, mapped[2] * a, mapped[3] * a];
        }
    }
}

/// How light a premultiplied pixel's colour is, 0..1 (its colour as if
/// it were opaque).
fn lightness(p: [f32; 4]) -> f32 {
    if p[3] <= 0.0 {
        return 0.0;
    }
    ((p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722) / p[3]).clamp(0.0, 1.0)
}

/// A premultiplied pixel's plain colour (each 0..255).
fn straight(p: [f32; 4]) -> [u8; 3] {
    let un = |c: f32| (c / p[3] * 255.0).round().clamp(0.0, 255.0) as u8;
    [un(p[0]), un(p[1]), un(p[2])]
}

fn premultiply([r, g, b]: [u8; 3], alpha: f32) -> [f32; 4] {
    let a = alpha / 255.0;
    [f32::from(r) * a, f32::from(g) * a, f32::from(b) * a, alpha]
}

fn to_pixel(p: [f32; 4]) -> Rgba8 {
    let a = (p[3] + 0.5).clamp(0.0, 255.0) as u8;
    let c = |v: f32| ((v + 0.5).clamp(0.0, 255.0) as u8).min(a);
    Rgba8::new(c(p[0]), c(p[1]), c(p[2]), a)
}

/// Softens `pixels` (a `width` x `height` block) by about `radius`
/// pixels: three box blurs across, then three down, which together are
/// very close to a Gaussian blur and cost the same whatever the radius.
pub fn blur(pixels: &mut [[f32; 4]], width: usize, height: usize, radius: f32) {
    if radius <= 0.0 || width == 0 || height == 0 {
        return;
    }
    let radius = radius.min(MAX_BLUR);
    // The box half-widths that add up to a Gaussian of this spread.
    let sigma = radius / 2.0;
    let boxes = boxes_for_gaussian(sigma);
    let mut scratch = vec![[0.0; 4]; pixels.len()];
    for half in boxes {
        box_blur_rows(pixels, &mut scratch, width, height, half);
        box_blur_columns(pixels, &mut scratch, width, height, half);
    }
}

/// Three box half-widths whose combined blur is close to a Gaussian
/// with the given sigma (the usual "boxes for Gauss" sums).
fn boxes_for_gaussian(sigma: f32) -> [usize; 3] {
    let n = 3.0;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as usize;
    if wl.is_multiple_of(2) {
        wl = wl.saturating_sub(1);
    }
    let wu = wl + 2;
    let m = (12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n)
        / (-4.0 * wl as f32 - 4.0);
    let m = m.round().clamp(0.0, n) as usize;
    let mut sizes = [0; 3];
    for (i, size) in sizes.iter_mut().enumerate() {
        *size = (if i < m { wl } else { wu }).saturating_sub(1) / 2;
    }
    sizes
}

/// A running-sum box blur along each row: each pixel becomes the
/// average of the pixels within `half` of it (edges count as empty).
fn box_blur_rows(
    pixels: &mut [[f32; 4]],
    scratch: &mut [[f32; 4]],
    width: usize,
    height: usize,
    half: usize,
) {
    if half == 0 {
        return;
    }
    let span = (2 * half + 1) as f32;
    for y in 0..height {
        let row = &pixels[y * width..(y + 1) * width];
        let out = &mut scratch[y * width..(y + 1) * width];
        let mut sum = [0.0; 4];
        for &pixel in &row[..half.min(width)] {
            add(&mut sum, pixel);
        }
        for x in 0..width {
            if x + half < width {
                add(&mut sum, row[x + half]);
            }
            if x > half {
                subtract(&mut sum, row[x - half - 1]);
            }
            out[x] = sum.map(|v| v / span);
        }
    }
    pixels.copy_from_slice(scratch);
}

fn box_blur_columns(
    pixels: &mut [[f32; 4]],
    scratch: &mut [[f32; 4]],
    width: usize,
    height: usize,
    half: usize,
) {
    if half == 0 {
        return;
    }
    let span = (2 * half + 1) as f32;
    for x in 0..width {
        let mut sum = [0.0; 4];
        for y in 0..half.min(height) {
            add(&mut sum, pixels[y * width + x]);
        }
        for y in 0..height {
            if y + half < height {
                add(&mut sum, pixels[(y + half) * width + x]);
            }
            if y > half {
                subtract(&mut sum, pixels[(y - half - 1) * width + x]);
            }
            scratch[y * width + x] = sum.map(|v| v / span);
        }
    }
    pixels.copy_from_slice(scratch);
}

fn add(sum: &mut [f32; 4], p: [f32; 4]) {
    for i in 0..4 {
        sum[i] += p[i];
    }
}

fn subtract(sum: &mut [f32; 4], p: [f32; 4]) {
    for i in 0..4 {
        sum[i] -= p[i];
    }
}

/// The shadow alone of everything on `surface`: its silhouette in
/// `color`, moved by `offset`, softened by `blur`, inside `limits`.
pub fn shadow_of(
    surface: &Surface,
    offset: (f32, f32),
    blur: f32,
    color: [u8; 4],
    limits: PixelRect,
) -> Surface {
    let mut shadow = Surface::new();
    let Some(painted) = surface.painted_area() else {
        return shadow;
    };
    let filter = Filter::Shadow {
        offset,
        blur,
        color,
    };
    let reach = filter.reach();
    let area = PixelRect::new(
        painted.left - reach,
        painted.top - reach,
        painted.right + reach,
        painted.bottom + reach,
    )
    .intersect(&limits);
    let mut block = Block::copy(surface, area);
    block.shadow_only(offset, blur, color);
    block.write_back(&mut shadow);
    shadow
}

/// Lays everything on `part` over `onto` (normal blending), never
/// outside `limits`. With `onto`'s alpha lock on, only the colour of
/// pixels already there changes. Returns the changed tiles as they were.
pub fn lay_over(onto: &mut Surface, part: &Surface, limits: PixelRect) -> TilesBefore {
    let mut before = TilesBefore::new();
    let locked = onto.locked;
    let size = TILE_SIZE as i32;
    for (coord, top) in part.tiles() {
        let (left, tile_top) = coord.origin();
        let area = limits.intersect(&PixelRect::new(
            left,
            tile_top,
            left + size,
            tile_top + size,
        ));
        if area.is_empty() {
            continue;
        }
        let existing = onto.tile(coord);
        if locked && existing.is_none() {
            continue;
        }
        let mut tile = existing.cloned().unwrap_or_default();
        let mut changed = false;
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                let (tx, ty) = ((x - left) as usize, (y - tile_top) as usize);
                let over = top.pixel(tx, ty);
                if over.a == 0 {
                    continue;
                }
                let under = tile.pixel(tx, ty);
                let new = if locked {
                    if under.a == 0 {
                        continue;
                    }
                    // Recolour, keeping the pixel's opacity.
                    let amount = f32::from(over.a) / 255.0;
                    let scale = f32::from(under.a) / f32::from(over.a);
                    let mix = |o: u8, u: u8| {
                        (f32::from(o) * scale * amount + f32::from(u) * (1.0 - amount) + 0.5) as u8
                    };
                    Rgba8::new(
                        mix(over.r, under.r).min(under.a),
                        mix(over.g, under.g).min(under.a),
                        mix(over.b, under.b).min(under.a),
                        under.a,
                    )
                } else {
                    let keep = 1.0 - f32::from(over.a) / 255.0;
                    let mix = |o: u8, u: u8| (f32::from(o) + f32::from(u) * keep + 0.5) as u8;
                    let a = mix(over.a, under.a);
                    Rgba8::new(
                        mix(over.r, under.r).min(a),
                        mix(over.g, under.g).min(a),
                        mix(over.b, under.b).min(a),
                        a,
                    )
                };
                if new != under {
                    tile.set_pixel(tx, ty, new);
                    changed = true;
                }
            }
        }
        if changed {
            let old = onto.replace_tile(coord, Some(tile));
            before.insert(coord, old);
        }
    }
    before
}

/// A mask-like copy of where `surfaces` have paint, combined: each
/// pixel's opacity is the most opaque of them (the silhouette of a
/// group drawn on several layers).
pub fn silhouette(surfaces: &[&Surface]) -> Surface {
    let mut out = Surface::new();
    let mut coords: Vec<TileCoord> = surfaces
        .iter()
        .flat_map(|s| s.tiles().map(|(c, _)| c))
        .collect();
    coords.sort_unstable_by_key(|c| (c.y, c.x));
    coords.dedup();
    for coord in coords {
        let mut tile: Tile = Tile::new();
        let mut any = false;
        for surface in surfaces {
            if let Some(part) = surface.tile(coord) {
                for (out, p) in tile.pixels_mut().iter_mut().zip(part.pixels()) {
                    if p.a > out.a {
                        *out = Rgba8::new(0, 0, 0, p.a);
                        any = true;
                    }
                }
            }
        }
        if any {
            out.replace_tile(coord, Some(tile));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: PixelRect = PixelRect::new(0, 0, 200, 200);

    fn square(surface: &mut Surface, left: i32, top: i32, side: i32, pixel: Rgba8) {
        for y in top..top + side {
            for x in left..left + side {
                surface.set_pixel(x, y, pixel);
            }
        }
    }

    #[test]
    fn blur_spreads_paint_and_keeps_its_amount() {
        let mut surface = Surface::new();
        square(&mut surface, 90, 90, 20, Rgba8::new(255, 0, 0, 255));
        let before = apply(&mut surface, &[Filter::Blur(6.0)], CANVAS);
        assert!(!before.is_empty());
        let middle = surface.pixel(100, 100);
        assert!(middle.a > 200, "{middle:?}");
        let edge = surface.pixel(110, 100);
        assert!(edge.a > 50 && edge.a < 200, "the edge is soft: {edge:?}");
        let outside = surface.pixel(113, 100);
        assert!(outside.a > 0 && outside.a < 100, "spread out: {outside:?}");
        assert_eq!(surface.pixel(140, 100).a, 0, "but not that far");
        // Blurring moves paint around without making more or less of it.
        let total: u32 = (0..200)
            .flat_map(|y| (0..200).map(move |x| (x, y)))
            .map(|(x, y)| u32::from(surface.pixel(x, y).a))
            .sum();
        assert!((total as i64 - 400 * 255).abs() < 400 * 255 / 50, "{total}");
        // Pure red stays pure red (premultiplied blur: no dark fringe).
        assert_eq!(edge.g, 0);
        assert_eq!(edge.r, edge.a);
        // The same radius, the same blur: deterministic.
        let mut again = Surface::new();
        square(&mut again, 90, 90, 20, Rgba8::new(255, 0, 0, 255));
        apply(&mut again, &[Filter::Blur(6.0)], CANVAS);
        assert_eq!(again.pixel(110, 100), edge);
    }

    #[test]
    fn blur_zero_changes_nothing_and_blur_stops_at_the_canvas() {
        let mut surface = Surface::new();
        square(&mut surface, 0, 0, 10, Rgba8::BLACK);
        assert!(apply(&mut surface, &[Filter::Blur(0.0)], CANVAS).is_empty());
        apply(&mut surface, &[Filter::Blur(4.0)], CANVAS);
        assert_eq!(surface.pixel(-1, 0).a, 0, "nothing outside the canvas");
        assert!(surface.pixel(0, 0).a > 0);
    }

    #[test]
    fn glow_brightens_and_adds_a_halo_only_around_light_pixels() {
        let mut surface = Surface::new();
        square(&mut surface, 50, 50, 20, Rgba8::new(255, 255, 200, 255)); // light
        square(&mut surface, 120, 50, 20, Rgba8::new(20, 20, 40, 255)); // dark
        apply(
            &mut surface,
            &[Filter::Glow {
                radius: 8.0,
                threshold: 0.7,
                strength: 1.0,
            }],
            CANVAS,
        );
        assert!(
            surface.pixel(74, 60).a > 0,
            "a halo beside the light square"
        );
        assert_eq!(surface.pixel(144, 60).a, 0, "none beside the dark one");
        assert_eq!(
            surface.pixel(130, 60),
            Rgba8::new(20, 20, 40, 255),
            "dark unchanged"
        );
    }

    #[test]
    fn shadow_falls_behind_offset_and_soft() {
        let mut surface = Surface::new();
        square(&mut surface, 50, 50, 20, Rgba8::new(255, 0, 0, 255));
        apply(
            &mut surface,
            &[Filter::Shadow {
                offset: (10.0, 10.0),
                blur: 2.0,
                color: [0, 0, 0, 128],
            }],
            CANVAS,
        );
        assert_eq!(
            surface.pixel(60, 60),
            Rgba8::new(255, 0, 0, 255),
            "the shape is on top"
        );
        let shadow = surface.pixel(75, 75);
        assert!(
            shadow.a > 60 && shadow.a < 140 && shadow.r == 0,
            "{shadow:?}"
        );
        assert_eq!(surface.pixel(45, 45).a, 0, "no shadow the other way");
    }

    #[test]
    fn adjust_and_gradient_map_recolour_without_changing_opacity() {
        let mut surface = Surface::new();
        square(
            &mut surface,
            0,
            0,
            4,
            Rgba8::from_straight(200, 100, 100, 128),
        );
        apply(
            &mut surface,
            &[Filter::Adjust {
                hue: 0.0,
                saturation: -1.0,
                brightness: 0.0,
                contrast: 0.0,
            }],
            CANVAS,
        );
        let grey = surface.pixel(1, 1);
        assert_eq!(grey.a, 128);
        assert_eq!((grey.r, grey.g, grey.b), (100, 100, 100), "{grey:?}");
        apply(
            &mut surface,
            &[Filter::GradientMap(vec![
                GradientStop {
                    position: 0.0,
                    color: [0, 0, 255, 255],
                },
                GradientStop {
                    position: 1.0,
                    color: [255, 255, 0, 255],
                },
            ])],
            CANVAS,
        );
        let mapped = surface.pixel(1, 1);
        assert_eq!(mapped.a, 128);
        // Light grey (0.78) lands near the yellow end, still a little blue.
        assert!(
            mapped.r > mapped.b && mapped.g > mapped.b && mapped.b > 10,
            "{mapped:?}"
        );
        // Brightness up on an opaque colour.
        let mut bright = Surface::new();
        square(&mut bright, 0, 0, 2, Rgba8::new(100, 100, 100, 255));
        apply(
            &mut bright,
            &[Filter::Adjust {
                hue: 0.0,
                saturation: 0.0,
                brightness: 0.2,
                contrast: 0.0,
            }],
            CANVAS,
        );
        assert_eq!(bright.pixel(0, 0).r, 151);
    }

    #[test]
    fn laying_over_respects_the_alpha_lock_and_the_silhouette_combines() {
        let mut part = Surface::new();
        square(&mut part, 0, 0, 4, Rgba8::new(255, 0, 0, 255));
        let mut onto = Surface::new();
        square(&mut onto, 2, 2, 4, Rgba8::new(0, 0, 255, 255));
        let before = lay_over(&mut onto, &part, CANVAS);
        assert_eq!(before.len(), 1);
        assert_eq!(onto.pixel(1, 1), Rgba8::new(255, 0, 0, 255));
        assert_eq!(
            onto.pixel(3, 3),
            Rgba8::new(255, 0, 0, 255),
            "over the blue"
        );
        assert_eq!(onto.pixel(5, 5), Rgba8::new(0, 0, 255, 255));
        let mut locked = Surface::new();
        square(&mut locked, 2, 2, 4, Rgba8::new(0, 0, 255, 255));
        locked.locked = true;
        lay_over(&mut locked, &part, CANVAS);
        assert_eq!(locked.pixel(1, 1).a, 0, "nothing new under the lock");
        assert_eq!(locked.pixel(3, 3), Rgba8::new(255, 0, 0, 255), "recoloured");
        let combined = silhouette(&[&part, &onto]);
        assert_eq!(combined.pixel(1, 1), Rgba8::new(0, 0, 0, 255));
        assert_eq!(combined.pixel(5, 5), Rgba8::new(0, 0, 0, 255));
        assert_eq!(combined.pixel(7, 7).a, 0);
    }
}
