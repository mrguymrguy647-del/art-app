//! Contact sheets: every snapshot of a render side by side in one image,
//! each with its number and name, so progress can be judged at a glance.

use quill_engine::image::{Color, Image};

use crate::font::{draw_text, fit_text, text_height};

/// Each picture on the sheet fits in this box.
pub const THUMB_WIDTH: u32 = 480;
pub const THUMB_HEIGHT: u32 = 360;
/// At most this many pictures side by side.
const MAX_COLUMNS: usize = 4;
/// Space around and between the pictures.
const GAP: u32 = 16;
/// Letters are drawn at this size (2 = each font pixel is 2x2).
const TEXT_SCALE: u32 = 2;

const BACKGROUND: Color = [38, 38, 38, 255];
const TEXT: Color = [230, 230, 230, 255];
const DIM_TEXT: Color = [150, 150, 150, 255];

/// One picture on the sheet, already shrunk to fit the box.
pub struct Entry {
    pub label: String,
    pub image: Image,
}

/// Lays the pictures out in rows, under a title.
pub fn contact_sheet(title: &str, entries: &[Entry]) -> Image {
    let columns = (entries.len() as f64)
        .sqrt()
        .ceil()
        .clamp(1.0, MAX_COLUMNS as f64) as usize;
    let rows = entries.len().div_ceil(columns).max(1);
    let label_height = text_height(TEXT_SCALE) + 8;
    let cell_width = THUMB_WIDTH;
    let cell_height = label_height + THUMB_HEIGHT;
    let title_height = text_height(TEXT_SCALE) + GAP;

    let width = GAP + columns as u32 * (cell_width + GAP);
    let height = GAP + title_height + rows as u32 * (cell_height + GAP);
    let mut sheet = Image::new(width, height, BACKGROUND);
    let title = fit_text(title, width - 2 * GAP, TEXT_SCALE);
    draw_text(
        &mut sheet, &title, GAP as i32, GAP as i32, TEXT_SCALE, DIM_TEXT,
    );

    for (i, entry) in entries.iter().enumerate() {
        let (column, row) = ((i % columns) as u32, (i / columns) as u32);
        let left = GAP + column * (cell_width + GAP);
        let top = GAP + title_height + row * (cell_height + GAP);
        let label = fit_text(
            &format!("{} {}", i + 1, entry.label),
            cell_width,
            TEXT_SCALE,
        );
        draw_text(
            &mut sheet,
            &label,
            left as i32,
            top as i32,
            TEXT_SCALE,
            TEXT,
        );
        // Centre the picture in its box; a checkerboard shows see-through parts.
        let image = &entry.image;
        let x = left + (cell_width - image.width().min(cell_width)) / 2;
        let y = top + label_height + (THUMB_HEIGHT - image.height().min(THUMB_HEIGHT)) / 2;
        let mut backing = checkerboard(image.width(), image.height());
        backing.paste(image, 0, 0);
        sheet.paste(&backing, x as i32, y as i32);
    }
    sheet
}

/// Every frame of an animation side by side (`columns` per row), each
/// with its number above it, at the frames' own size.
pub fn frame_sheet(title: &str, frames: &[(u32, Image)], columns: usize) -> Image {
    let columns = columns.clamp(1, frames.len().max(1));
    let rows = frames.len().div_ceil(columns).max(1);
    let cell_width = frames
        .iter()
        .map(|(_, f)| f.width())
        .max()
        .unwrap_or(1)
        .max(24);
    let cell_height = frames.iter().map(|(_, f)| f.height()).max().unwrap_or(1);
    let gap = 8;
    let label_height = text_height(1) + 4;
    let title_height = text_height(TEXT_SCALE) + gap;
    let width = gap + columns as u32 * (cell_width + gap);
    let height = gap + title_height + rows as u32 * (label_height + cell_height + gap);
    let mut sheet = Image::new(width, height, BACKGROUND);
    let title = fit_text(title, width - 2 * gap, TEXT_SCALE);
    draw_text(
        &mut sheet, &title, gap as i32, gap as i32, TEXT_SCALE, DIM_TEXT,
    );
    for (i, (number, frame)) in frames.iter().enumerate() {
        let (column, row) = ((i % columns) as u32, (i / columns) as u32);
        let left = gap + column * (cell_width + gap);
        let top = gap + title_height + row * (label_height + cell_height + gap);
        draw_text(
            &mut sheet,
            &number.to_string(),
            left as i32,
            top as i32,
            1,
            TEXT,
        );
        let mut backing = checkerboard(frame.width(), frame.height());
        backing.paste(frame, 0, 0);
        sheet.paste(&backing, left as i32, (top + label_height) as i32);
    }
    sheet
}

/// Light and darker grey squares, the usual sign for "see-through".
fn checkerboard(width: u32, height: u32) -> Image {
    let mut image = Image::new(width, height, [204, 204, 204, 255]);
    for y in 0..height {
        for x in 0..width {
            if (x / 8 + y / 8) % 2 == 1 {
                image.set_pixel(x as i32, y as i32, [160, 160, 160, 255]);
            }
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_laid_out_in_rows() {
        let entry = |label: &str| Entry {
            label: label.to_owned(),
            image: Image::new(THUMB_WIDTH, 200, [255, 0, 0, 255]),
        };
        let one = contact_sheet("test", &[entry("a")]);
        let five = contact_sheet(
            "test",
            &(0..5).map(|i| entry(&i.to_string())).collect::<Vec<_>>(),
        );
        // Five pictures: 3 columns, 2 rows.
        assert_eq!(five.width(), GAP + 3 * (THUMB_WIDTH + GAP));
        assert!(five.height() > one.height());
        assert_eq!(one.width(), GAP + THUMB_WIDTH + GAP);
    }

    #[test]
    fn frames_sit_in_rows_at_their_own_size() {
        let frame = |n: u32| (n, Image::new(32, 32, [255, 0, 0, 255]));
        let sheet = frame_sheet("walk", &(0..10).map(frame).collect::<Vec<_>>(), 8);
        // 8 per row, two rows.
        assert_eq!(sheet.width(), 8 + 8 * (32 + 8));
        let one_row = frame_sheet("walk", &(0..3).map(frame).collect::<Vec<_>>(), 8);
        assert_eq!(one_row.width(), 8 + 3 * (32 + 8));
        assert!(sheet.height() > one_row.height());
    }
}
