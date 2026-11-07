//! `quill render --tile`: for pictures that repeat (game backgrounds,
//! floor tiles), the picture 2 x 2 to look at, and where opposite edges
//! don't meet.

use quill_engine::image::Image;

/// The picture repeated `across` x `down` times.
pub fn repeated(image: &Image, across: u32, down: u32) -> Image {
    let (w, h) = (image.width(), image.height());
    let mut out = Image::new(w * across, h * down, [0, 0, 0, 0]);
    for y in 0..h * down {
        for x in 0..w * across {
            out.set_pixel(x as i32, y as i32, image.pixel(x % w, y % h));
        }
    }
    out
}

/// How different two pixels look, 0..255 (the biggest difference in any
/// channel, see-through counted too).
fn difference(a: [u8; 4], b: [u8; 4]) -> u8 {
    (0..4).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0)
}

/// A seam is a jump this many times bigger than the picture's usual step
/// from one pixel to the next (and at least `MIN_JUMP`), so a noisy
/// texture isn't flagged for being noisy.
const JUMP_FACTOR: u32 = 3;
const MIN_JUMP: u8 = 24;
/// Edges are reported when more than this share of them jumps.
const REPORT_SHARE: f32 = 0.05;

/// One line per pair of edges that doesn't meet, saying how much of it
/// jumps and where the worst stretch is (in canvas pixels: `scale` is how
/// big `image` was painted). Empty when it repeats cleanly.
pub fn edge_mismatches(image: &Image, scale: f32) -> Vec<String> {
    let (w, h) = (image.width(), image.height());
    if w < 2 || h < 2 {
        return Vec::new();
    }
    // The usual step between neighbours, across and down (the median).
    let usual = |steps: &mut Vec<u8>| {
        steps.sort_unstable();
        steps.get(steps.len() / 2).copied().unwrap_or(0)
    };
    let stride = ((w * h) / 200_000).max(1); // a sample is plenty on big pictures
    let mut across: Vec<u8> = Vec::new();
    let mut down: Vec<u8> = Vec::new();
    for y in (0..h - 1).step_by(stride as usize) {
        for x in 0..w - 1 {
            across.push(difference(image.pixel(x, y), image.pixel(x + 1, y)));
            down.push(difference(image.pixel(x, y), image.pixel(x, y + 1)));
        }
    }
    let threshold = |usual: u8| (u32::from(usual) * JUMP_FACTOR).max(u32::from(MIN_JUMP));
    let mut findings = Vec::new();
    // Left and right: each row's last pixel next to its first.
    let limit = threshold(usual(&mut across));
    let rows: Vec<bool> = (0..h)
        .map(|y| u32::from(difference(image.pixel(w - 1, y), image.pixel(0, y))) > limit)
        .collect();
    if let Some(line) = describe("left and right edges", "y", &rows, scale) {
        findings.push(line);
    }
    // Top and bottom.
    let limit = threshold(usual(&mut down));
    let columns: Vec<bool> = (0..w)
        .map(|x| u32::from(difference(image.pixel(x, h - 1), image.pixel(x, 0))) > limit)
        .collect();
    if let Some(line) = describe("top and bottom edges", "x", &columns, scale) {
        findings.push(line);
    }
    findings
}

/// "left and right edges don't meet: 40% of rows jump (most of all at y 120..180)".
fn describe(edges: &str, axis: &str, jumps: &[bool], scale: f32) -> Option<String> {
    let count = jumps.iter().filter(|&&j| j).count();
    let share = count as f32 / jumps.len() as f32;
    if share <= REPORT_SHARE {
        return None;
    }
    // The longest run of jumps: where to look first.
    let (mut best, mut start) = ((0, 0), None);
    for (i, &jump) in jumps.iter().chain([false].iter()).enumerate() {
        match (jump, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s > best.1 - best.0 {
                    best = (s, i);
                }
                start = None;
            }
            _ => {}
        }
    }
    Some(format!(
        "{edges} don't meet: {:.0}% of the {} jump (most of all at {axis} {}..{})",
        share * 100.0,
        if axis == "y" { "rows" } else { "columns" },
        (best.0 as f32 / scale).round(),
        (best.1 as f32 / scale).round()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture that repeats cleanly: stripes whose colour wraps round.
    fn seamless(w: u32, h: u32) -> Image {
        let mut image = Image::new(w, h, [0, 0, 0, 255]);
        for y in 0..h {
            for x in 0..w {
                let t = (x as f32 / w as f32 * std::f32::consts::TAU).sin() * 0.5 + 0.5;
                let v = (t * 200.0) as u8 + 20;
                image.set_pixel(x as i32, y as i32, [v, v / 2, 255 - v, 255]);
            }
        }
        image
    }

    #[test]
    fn a_seamless_picture_has_nothing_to_report() {
        assert!(edge_mismatches(&seamless(64, 32), 1.0).is_empty());
        // Noise repeats cleanly too if it's the same noise: and noisy
        // inside isn't a seam.
        let mut noisy = Image::new(64, 64, [0, 0, 0, 255]);
        let mut r = 7u32;
        for y in 0..64 {
            for x in 0..64 {
                r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let v = (r >> 24) as u8;
                noisy.set_pixel(x, y, [v, v, v, 255]);
            }
        }
        // (Its edges jump no more than anywhere else in it.)
        assert!(
            edge_mismatches(&noisy, 1.0).is_empty(),
            "{:?}",
            edge_mismatches(&noisy, 1.0)
        );
    }

    #[test]
    fn a_seam_is_found_and_located() {
        // A sky gradient from top to bottom: the bottom row is dark, the
        // top light, so stacked copies show a hard line.
        let mut sky = Image::new(40, 100, [0, 0, 0, 255]);
        for y in 0..100 {
            for x in 0..40 {
                let v = 255 - (y as u32 * 2) as u8;
                sky.set_pixel(x, y, [v / 2, v / 2, v, 255]);
            }
        }
        let findings = edge_mismatches(&sky, 1.0);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(
            findings[0].starts_with("top and bottom edges don't meet: 100%"),
            "{findings:?}"
        );
        // A bump on the right edge only, rows 10 to 29.
        let mut bump = seamless(64, 40);
        for y in 10..30 {
            bump.set_pixel(63, y, [255, 255, 255, 255]);
        }
        let findings = edge_mismatches(&bump, 1.0);
        assert!(
            findings[0].contains("left and right") && findings[0].contains("y 10..30"),
            "{findings:?}"
        );
    }

    #[test]
    fn repeating_puts_copies_side_by_side() {
        let image = seamless(8, 4);
        let two = repeated(&image, 2, 2);
        assert_eq!((two.width(), two.height()), (16, 8));
        assert_eq!(two.pixel(9, 5), image.pixel(1, 1));
    }
}
