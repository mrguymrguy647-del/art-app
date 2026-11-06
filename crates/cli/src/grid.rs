//! A light grid with coordinates over a picture (`--grid`), so positions
//! can be read straight off a preview or a close-up.

use quill_engine::image::{Color, Image};

use crate::font::{draw_text, text_height, text_width};

/// Grid lines are never closer than this, in picture pixels: closer ones
/// are skipped (only every 2nd, 5th, ... line is drawn).
const MIN_GAP: f32 = 6.0;
/// Labels: dark letters on a light box.
const LABEL_TEXT: Color = [30, 30, 30, 255];
const LABEL_BOX: Color = [240, 240, 240, 255];

/// Which part of the canvas a picture shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The canvas position at the picture's top-left corner.
    pub left: f32,
    pub top: f32,
    /// How many picture pixels one canvas pixel is (0.5 for a half-size
    /// preview, 3 for a close-up zoomed 3 times).
    pub zoom: f32,
}

/// Draws lines every `spacing` canvas pixels, labelled with their canvas
/// coordinates along the top and left edges.
pub fn draw_grid(image: &mut Image, spacing: u32, view: View) {
    let gap = spacing as f32 * view.zoom;
    if gap <= 0.0 {
        return;
    }
    // Lines at every `every`-th multiple of the spacing, far enough apart.
    let every = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000]
        .into_iter()
        .find(|&n| gap * n as f32 >= MIN_GAP)
        .unwrap_or(1000);
    let step = spacing * every;
    let (width, height) = (image.width() as f32, image.height() as f32);
    let right = view.left + width / view.zoom;
    let bottom = view.top + height / view.zoom;

    let xs = multiples(view.left, right, step);
    let ys = multiples(view.top, bottom, step);
    for &x in &xs {
        let px = ((x as f32 - view.left) * view.zoom).round() as i32;
        for py in 0..image.height() as i32 {
            mark(image, px, py);
        }
    }
    for &y in &ys {
        let py = ((y as f32 - view.top) * view.zoom).round() as i32;
        for px in 0..image.width() as i32 {
            mark(image, px, py);
        }
    }
    // Labels, on as many lines as there's room for.
    let widest = xs
        .iter()
        .chain(&ys)
        .map(|v| text_width(&v.to_string(), 1))
        .max()
        .unwrap_or(0);
    let room = (widest as f32 + 8.0).max(text_height(1) as f32 + 6.0);
    let label_every = (1..)
        .find(|n| gap * (every * n) as f32 >= room)
        .unwrap_or(1) as i64;
    for &x in &xs {
        if (x / i64::from(step)) % label_every == 0 {
            let px = ((x as f32 - view.left) * view.zoom).round() as i32;
            label(image, &x.to_string(), px + 2, 1);
        }
    }
    for &y in &ys {
        if (y / i64::from(step)) % label_every == 0 {
            let py = ((y as f32 - view.top) * view.zoom).round() as i32;
            label(image, &y.to_string(), 1, py + 2);
        }
    }
}

/// The multiples of `step` from `from` up to (not including) `to`.
fn multiples(from: f32, to: f32, step: u32) -> Vec<i64> {
    let step = i64::from(step.max(1));
    let first = (from / step as f32).ceil() as i64;
    (first..)
        .map(|k| k * step)
        .take_while(|&value| (value as f32) < to)
        .collect()
}

/// Makes one pixel of a grid line: a little darker on light colours and
/// a little lighter on dark ones, so lines show on any picture without
/// hiding it.
fn mark(image: &mut Image, x: i32, y: i32) {
    if x < 0 || y < 0 || x as u32 >= image.width() || y as u32 >= image.height() {
        return;
    }
    let [r, g, b, a] = image.pixel(x as u32, y as u32);
    let light = u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 > 140_000;
    let toward: u8 = if light { 0 } else { 255 };
    let mix = |c: u8| ((u16::from(c) * 6 + u16::from(toward) * 4) / 10) as u8;
    image.set_pixel(x, y, [mix(r), mix(g), mix(b), a.max(128)]);
}

/// A coordinate on a small light box.
fn label(image: &mut Image, text: &str, x: i32, y: i32) {
    image.fill_rect(
        x - 1,
        y - 1,
        text_width(text, 1) + 2,
        text_height(1) + 2,
        LABEL_BOX,
    );
    draw_text(image, text, x, y, 1, LABEL_TEXT);
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Color = [255, 255, 255, 255];

    #[test]
    fn lines_land_on_canvas_coordinates() {
        // A close-up of the canvas from (150, 0), zoomed twice.
        let mut image = Image::new(200, 100, WHITE);
        let view = View {
            left: 150.0,
            top: 0.0,
            zoom: 2.0,
        };
        draw_grid(&mut image, 50, view);
        // x = 200 is 50 canvas pixels in: picture pixel 100.
        assert_ne!(image.pixel(100, 60), WHITE);
        assert_eq!(image.pixel(101, 60), WHITE);
        // y = 50 is picture row 100: just outside. y = 0 is row 0.
        assert_ne!(image.pixel(30, 0), WHITE);
        assert_eq!(image.pixel(30, 60), WHITE);
    }

    #[test]
    fn crowded_lines_are_thinned_out() {
        // A grid every 10 pixels on a quarter-size preview would put lines
        // 2.5 pixels apart: only every 5th is drawn (every 50 pixels).
        let mut image = Image::new(100, 100, WHITE);
        let view = View {
            left: 0.0,
            top: 0.0,
            zoom: 0.25,
        };
        draw_grid(&mut image, 10, view);
        let row: Vec<bool> = (0..100).map(|x| image.pixel(x, 90) != WHITE).collect();
        let lines: Vec<usize> = (0..100).filter(|&x| row[x]).collect();
        assert_eq!(lines.first(), Some(&0));
        assert!(lines.windows(2).all(|w| w[1] - w[0] >= 6), "{lines:?}");
    }

    #[test]
    fn multiples_cover_the_view() {
        assert_eq!(multiples(150.0, 260.0, 50), [150, 200, 250]);
        assert_eq!(multiples(-20.0, 30.0, 25), [0, 25]);
    }
}
