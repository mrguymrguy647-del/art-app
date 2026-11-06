//! The critique: the artist's eye in numbers. It looks at a painted
//! picture (and, when known, the painter's intent and the named parts'
//! silhouettes) and reports what a person spots in a second: flat
//! values, a focal point that isn't where it was meant to be, a colour
//! that's foreign to the key, noise where there should be shapes, a part
//! that vanishes at thumbnail size, a lopsided composition.
//!
//! Every number is deterministic and every threshold is a named constant
//! here, so a report can be read back to the rule that made it. The
//! numbers are warnings, never goals: a finding is a reason to look.
//! They can't judge beauty, and the report says so.

use crate::fill::Mask;
use crate::image::{Color, Image};
use crate::senses::{self, GRID, Grey, Heat, THUMB_WIDTH, cell_box, hsv, luminance, on_white};
use crate::surface::PixelRect;

// ---- The thresholds ---------------------------------------------------------------

/// A value band of the squint with at least this share of the picture
/// is a value group (a big shape of one value).
pub const GROUP_BAND: f32 = 0.04;
/// The squint's value bands: this many, equal, from black to white.
pub const BANDS: usize = 5;
/// This share of the pixels in the middle bins (4 to 6) is "flat".
pub const FLAT_PICTURE: f32 = 0.70;
/// The focal point's contrast against the median cell: good from here...
pub const FOCAL_CONTRAST_GOOD: f32 = 1.5;
/// ...and lost under here.
pub const FOCAL_CONTRAST_LOST: f32 = 1.0;
/// Pixels count for the hue histogram above this saturation and value.
pub const HUE_SATURATION: f32 = 0.15;
pub const HUE_VALUE: f32 = 0.1;
/// A cell is saturated when at least half its pixels are this saturated.
pub const SATURATED: f32 = 0.35;
/// A saturated cell whose hue is further than this (degrees) from every
/// key colour, and not between two of them (the short way round), is
/// out of key.
pub const OUT_OF_KEY_DEGREES: f32 = 40.0;
/// A key colour sets a hue only if it's at least this saturated: a pale
/// cream or a grey is a value, not a hue.
pub const KEY_SATURATION: f32 = 0.35;
/// The blur that takes away grain (noise, speckle, dither)...
pub const GRAIN_BLUR: f32 = 1.5;
/// ...and the blur that takes away marks (strokes, blades, feathers).
pub const MARKS_BLUR: f32 = 6.0;
/// A part smaller than this share of the canvas isn't judged for texture.
pub const SMALL_PART: f32 = 0.01;
/// Grain above this share of the part's value range is noise...
pub const DIRTY_GRAIN: f32 = 0.06;
/// ...unless the marks there are at least this many times the grain.
pub const MARKS_OVER_GRAIN: f32 = 2.0;
/// A part is dirty when noise covers more than this share of it.
pub const DIRTY_SHARE: f32 = 0.5;
/// A cell with a value range under this is flat...
pub const FLAT_CELL: f32 = 0.02;
/// ...and a part with more than this share of flat cells is flat.
pub const FLAT_SHARE: f32 = 0.25;
/// A part whose squint has less value range than this is a smudge.
pub const LOW_STRUCTURE: f32 = 0.05;
/// Detail counts as "at the focal point" within this many cells of it.
pub const NEAR_FOCAL_CELLS: i32 = 4;
/// The marks near the focal point should be at least this many times
/// their area's share of the picture (the area is about 8%).
pub const DETAIL_AT_FOCAL: f32 = 1.5;
/// At thumbnail size, a part merges with its surroundings under this
/// value difference and (when both are coloured) this hue difference.
pub const MERGE_VALUE: f32 = 0.12;
pub const MERGE_HUE: f32 = 30.0;
/// A part under this many thumbnail pixels is too small to read.
pub const TOO_SMALL: usize = 4;
/// The visual weight's centre further than this from the middle (as a
/// share of the width or height) is heavy on one side.
pub const HEAVY: f32 = 0.08;
/// The focal point within this share of the canvas centre is dead centre.
pub const DEAD_CENTRE: f32 = 0.05;
/// A horizon within this share of the middle cuts the picture in half.
pub const HORIZON_MIDDLE: f32 = 0.05;

/// Twelve hue names around the wheel, 30 degrees apart from red.
pub const HUE_NAMES: [&str; 12] = [
    "red",
    "orange",
    "yellow",
    "yellow-green",
    "green",
    "blue-green",
    "cyan",
    "azure",
    "blue",
    "violet",
    "magenta",
    "rose",
];

/// The squint's value bands, darkest first.
pub const BAND_NAMES: [&str; BANDS] = ["black", "dark", "mid", "light", "white"];

/// The name of a hue in degrees.
pub fn hue_name(hue: f32) -> &'static str {
    let i = ((hue.rem_euclid(360.0) + 15.0) / 30.0).floor() as usize % 12;
    HUE_NAMES[i]
}

/// The smallest angle between two hues, 0 to 180.
pub fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

// ---- What's judged ------------------------------------------------------------------

/// A named part of the picture with its silhouette (canvas pixels).
#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub mask: Mask,
    /// The box around what it drew, in canvas pixels.
    pub area: PixelRect,
}

/// The picture and what's known about it.
#[derive(Clone, Debug)]
pub struct Subject<'a> {
    pub image: &'a Image,
    /// The intent's focal point, on the canvas.
    pub focal: Option<(f32, f32)>,
    /// The focal part's name and box, when the focal point is a part.
    pub focal_part: Option<(String, PixelRect)>,
    /// The intent's key colours (palette name, colour).
    pub key_colors: Vec<(String, Color)>,
    /// The outermost named parts, in script order.
    pub parts: Vec<Part>,
}

// ---- The report -------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Values {
    /// The share of pixels in each of 10 luminance bins (0 = darkest).
    pub bins: [f32; 10],
    /// The value groups: the squint's `BANDS` bands (0 = darkest) that
    /// cover at least `GROUP_BAND` of the picture, with their shares.
    pub groups: Vec<(usize, f32)>,
    /// Shares in the dark (bins 0-2), mid (3-6) and light (7-9) bins.
    pub dark: f32,
    pub mid: f32,
    pub light: f32,
    /// The share in the three middle bins (4-6), what `flat` judges.
    pub middle: f32,
    pub flat: bool,
    /// The strongest cell contrast within `NEAR_FOCAL_CELLS` of the focal
    /// point over the median cell's (at least `FLAT_CELL`), if there's a
    /// focal point.
    pub focal_contrast: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Focal {
    /// Where the eye goes (the heat peak), on the canvas.
    pub peak: (f32, f32),
    /// The part the peak is in, if any.
    pub peak_part: Option<String>,
    /// How far the peak is from the intent's focal point.
    pub distance: Option<f32>,
    /// Whether the peak lies in the focal part's box.
    pub inside: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Blob {
    /// Its middle, on the canvas.
    pub at: (f32, f32),
    pub hue: f32,
    /// Its share of the picture.
    pub share: f32,
    /// The nearest part.
    pub part: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Colour {
    /// How many of the 24 hue bins carry 90% of the colour.
    pub hues: usize,
    pub warm: f32,
    pub cool: f32,
    pub neutral: f32,
    /// The key colours' hues (name, degrees); grey key colours have none.
    pub key_hues: Vec<(String, f32)>,
    pub out_of_key: Vec<Blob>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub name: String,
    /// Its share of the canvas.
    pub share: f32,
    /// Mean grain and marks over the part, as shares of its value range.
    pub grain: f32,
    pub marks: f32,
    /// The share of the part that's noise without structure.
    pub dirty_share: f32,
    pub dirty: bool,
    pub flat_share: f32,
    pub flat: bool,
    /// The value range of the part's squint.
    pub squint_range: f32,
    pub low_structure: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Readability {
    pub name: String,
    /// How many thumbnail pixels the part keeps.
    pub pixels: usize,
    pub too_small: bool,
    pub value_gap: f32,
    pub hue_gap: Option<f32>,
    pub merges: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Composition {
    /// The visual weight's centre as shares of the width and height.
    pub weight: (f32, f32),
    pub dead_centre: bool,
    pub cut_by_edge: bool,
    /// Where a part called "horizon" sits, as a share of the height.
    pub horizon: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Critique {
    pub values: Values,
    pub focal: Focal,
    pub colour: Colour,
    pub textures: Vec<Texture>,
    /// The share of the marks within `NEAR_FOCAL_CELLS` of the focal
    /// point, and that area's share of the picture.
    pub detail_at_focal: Option<(f32, f32)>,
    pub readability: Vec<Readability>,
    pub composition: Composition,
}

/// The pixel measurements everything is read from.
struct Measured {
    width: u32,
    height: u32,
    grey: Grey,
    squint: Grey,
    heat: Heat,
    /// Per pixel: what a small blur removes, and what a bigger one does.
    grain: Vec<f32>,
    marks: Vec<f32>,
}

impl Critique {
    pub fn of(subject: &Subject) -> Critique {
        let image = subject.image;
        let grey = Grey::of(image);
        let squint = grey.squint();
        let heat = Heat::of_grey(image, &grey, &squint);
        let fine = grey.blurred(GRAIN_BLUR);
        let coarse = grey.blurred(MARKS_BLUR);
        let grain: Vec<f32> = grey
            .values
            .iter()
            .zip(&fine.values)
            .map(|(a, b)| (a - b).abs())
            .collect();
        let marks: Vec<f32> = fine
            .values
            .iter()
            .zip(&coarse.values)
            .map(|(a, b)| (a - b).abs())
            .collect();
        let m = Measured {
            width: image.width(),
            height: image.height(),
            grey,
            squint,
            heat,
            grain,
            marks,
        };
        let focal_cell = subject.focal.map(|(x, y)| m.heat.cell_at(x, y));
        Critique {
            values: values(&m, focal_cell),
            focal: focal(&m, subject),
            colour: colour(&m, subject),
            textures: textures(&m, subject),
            detail_at_focal: focal_cell.map(|cell| detail_near(&m, cell)),
            readability: readability(&m, subject),
            composition: composition(&m, subject),
        }
    }

    /// The report, one finding per line, in plain words with the numbers.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let v = &self.values;
        let name = |g: &(usize, f32)| {
            let (band, share) = *g;
            format!("{} {:.0}%", BAND_NAMES[band.min(BANDS - 1)], share * 100.0)
        };
        if v.flat {
            out.push(format!(
                "values: mostly mid-greys ({:.0}% in bins 4-6): the picture looks flat; \
                 push the darks down and the lights up",
                v.middle * 100.0
            ));
        } else {
            let groups: Vec<String> = v.groups.iter().map(name).collect();
            out.push(format!(
                "values: {} value group{} when squinted ({}); dark {:.0}%, mid {:.0}%, light {:.0}%",
                v.groups.len(),
                if v.groups.len() == 1 { "" } else { "s" },
                groups.join(", "),
                v.dark * 100.0,
                v.mid * 100.0,
                v.light * 100.0
            ));
        }
        if let Some(ratio) = v.focal_contrast {
            let judge = if ratio >= FOCAL_CONTRAST_GOOD {
                "good"
            } else if ratio < FOCAL_CONTRAST_LOST {
                "lost: it has less contrast than the rest"
            } else {
                "weak"
            };
            out.push(format!(
                "focal contrast: {ratio:.1}x the rest ({judge}; {FOCAL_CONTRAST_GOOD}x or more \
                 stands out, under {FOCAL_CONTRAST_LOST:.0}x is lost)"
            ));
        }
        let f = &self.focal;
        let peak = match &f.peak_part {
            Some(part) => format!("{part} (x {:.0}, y {:.0})", f.peak.0, f.peak.1),
            None => format!("x {:.0}, y {:.0}", f.peak.0, f.peak.1),
        };
        match (f.distance, f.inside) {
            (Some(distance), inside) => {
                let verdict = match inside {
                    Some(true) => "inside the focal part".to_owned(),
                    _ => format!("{distance:.0} px from the intent's focal point"),
                };
                out.push(format!("focal point: the eye goes to {peak}; {verdict}"));
            }
            _ => out.push(format!(
                "focal point: the eye goes to {peak} (no focal point in the intent to compare)"
            )),
        }
        let c = &self.colour;
        out.push(format!(
            "colour: {} hue{} 90% of the colour; warm {:.0}%, cool {:.0}%, neutral {:.0}%",
            c.hues,
            if c.hues == 1 { " carries" } else { "s carry" },
            c.warm * 100.0,
            c.cool * 100.0,
            c.neutral * 100.0
        ));
        if c.key_hues.is_empty() {
            out.push("out of key: not judged (no coloured key colours in the intent)".into());
        } else if c.out_of_key.is_empty() {
            out.push(format!(
                "out of key: nothing; the key is {}",
                c.key_hues
                    .iter()
                    .map(|(n, h)| format!("{n} ({})", hue_name(*h)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else {
            for blob in &c.out_of_key {
                out.push(format!(
                    "out of key: a saturated {} at x {:.0}, y {:.0} ({}{:.1}% of the picture) \
                     is far from every key colour; tint it towards the key or mute it",
                    hue_name(blob.hue),
                    blob.at.0,
                    blob.at.1,
                    blob.part
                        .as_ref()
                        .map_or(String::new(), |p| format!("{p}, ")),
                    blob.share * 100.0
                ));
            }
        }
        for t in &self.textures {
            if t.dirty {
                out.push(format!(
                    "texture: {} is noisy inside: fine grain over {:.0}% of it with no marks; \
                     shading wants big soft shapes and a few marks, not noise everywhere",
                    t.name,
                    t.dirty_share * 100.0
                ));
            }
            if t.flat {
                out.push(format!(
                    "texture: {} is flat over {:.0}% of it (no change in value); a gradient, \
                     a noise fill or a few marks would give it life",
                    t.name,
                    t.flat_share * 100.0
                ));
            }
            if t.low_structure {
                out.push(format!(
                    "structure: {} has no light and dark side (squint range {:.2}): a smudge; \
                     give it a lit side and a shadow side",
                    t.name, t.squint_range
                ));
            }
        }
        if let Some((near, area)) = self.detail_at_focal {
            out.push(format!(
                "detail: {:.0}% of the marks are within {NEAR_FOCAL_CELLS} cells of the focal \
                 point (an area of {:.0}%), {:.0}% elsewhere{}",
                near * 100.0,
                area * 100.0,
                (1.0 - near) * 100.0,
                if near < DETAIL_AT_FOCAL * area {
                    "; put the marks where the eye should go"
                } else {
                    ""
                }
            ));
        }
        let readable = self
            .readability
            .iter()
            .filter(|r| !r.too_small && !r.merges)
            .count();
        if !self.readability.is_empty() {
            out.push(format!(
                "readability at {THUMB_WIDTH} px: {readable} of {} parts still read",
                self.readability.len()
            ));
        }
        // The same finding for parts of the same name is said once.
        let mut said: Vec<(&str, bool)> = Vec::new();
        for r in &self.readability {
            if r.too_small {
                if said.contains(&(r.name.as_str(), true)) {
                    continue;
                }
                said.push((r.name.as_str(), true));
                let same: Vec<&Readability> = self
                    .readability
                    .iter()
                    .filter(|o| o.name == r.name && o.too_small)
                    .collect();
                let (least, most) = same.iter().fold((usize::MAX, 0), |(lo, hi), o| {
                    (lo.min(o.pixels), hi.max(o.pixels))
                });
                out.push(format!(
                    "readability: {}{} too small to read at thumbnail size ({} px)",
                    r.name,
                    if same.len() > 1 {
                        format!(" (x{}) is", same.len())
                    } else {
                        " is".to_owned()
                    },
                    if least == most {
                        least.to_string()
                    } else {
                        format!("{least} to {most}")
                    }
                ));
            } else if r.merges {
                if said.contains(&(r.name.as_str(), false)) {
                    continue;
                }
                said.push((r.name.as_str(), false));
                out.push(format!(
                    "readability: {} merges with its surroundings at thumbnail size (value gap \
                     {:.2}{}); separate it by value",
                    r.name,
                    r.value_gap,
                    r.hue_gap
                        .map_or(String::new(), |h| format!(", hue gap {h:.0} degrees"))
                ));
            }
        }
        let comp = &self.composition;
        let (wx, wy) = comp.weight;
        let side = |share: f32, low: &str, high: &str| {
            if share < 0.5 - HEAVY {
                format!(" ({low}-heavy)")
            } else if share > 0.5 + HEAVY {
                format!(" ({high}-heavy)")
            } else {
                String::new()
            }
        };
        out.push(format!(
            "composition: visual weight sits at {:.0}% across{}, {:.0}% down{}",
            wx * 100.0,
            side(wx, "left", "right"),
            wy * 100.0,
            side(wy, "top", "bottom")
        ));
        if comp.dead_centre {
            out.push("composition: the focal point is dead centre; move it to a third".to_owned());
        }
        if comp.cut_by_edge {
            out.push(
                "composition: the focal part touches the canvas edge; give it room or cut it \
                 on purpose"
                    .to_owned(),
            );
        }
        if let Some(h) = comp.horizon {
            if (h - 0.5).abs() <= HORIZON_MIDDLE {
                out.push(format!(
                    "composition: the horizon sits in the middle ({:.0}% down); move it up or \
                     down",
                    h * 100.0
                ));
            } else {
                out.push(format!(
                    "composition: the horizon is {:.0}% down",
                    h * 100.0
                ));
            }
        }
        out.push(
            "Numbers can't judge beauty: look at the views, run the blind-read test, and ask \
             for a rating."
                .to_owned(),
        );
        out
    }

    /// Every part's numbers, one line each, for `--detail`.
    pub fn detail_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in &self.textures {
            out.push(format!(
                "{}: {:.1}% of the canvas; grain {:.3}, marks {:.3} (of its value range); noisy \
                 over {:.0}%, flat over {:.0}%; squint range {:.2}",
                t.name,
                t.share * 100.0,
                t.grain,
                t.marks,
                t.dirty_share * 100.0,
                t.flat_share * 100.0,
                t.squint_range
            ));
        }
        for r in &self.readability {
            out.push(format!(
                "{}: {} thumbnail px; value gap {:.2}{}",
                r.name,
                r.pixels,
                r.value_gap,
                r.hue_gap
                    .map_or(String::new(), |h| format!(", hue gap {h:.0} degrees"))
            ));
        }
        out
    }

    /// The findings that are warnings (the lines a painter should act on).
    pub fn warnings(&self) -> usize {
        self.lines()
            .iter()
            .filter(|line| {
                line.contains("looks flat")
                    || line.contains("lost:")
                    || line.contains("px from the intent")
                    || line.contains("out of key: a ")
                    || line.contains("is noisy inside")
                    || line.contains("is flat over")
                    || line.contains("a smudge")
                    || line.contains("put the marks")
                    || line.contains("too small to read")
                    || line.contains("merges with")
                    || line.contains("-heavy")
                    || line.contains("dead centre")
                    || line.contains("touches the canvas edge")
                    || line.contains("sits in the middle")
            })
            .count()
    }
}

// ---- Values -------------------------------------------------------------------------

fn values(m: &Measured, focal_cell: Option<(usize, usize)>) -> Values {
    let mut counts = [0usize; 10];
    for &v in &m.grey.values {
        let bin = ((v * 10.0).floor() as usize).min(9);
        counts[bin] += 1;
    }
    let total = m.grey.values.len().max(1) as f32;
    let bins = counts.map(|c| c as f32 / total);
    // Groups: the squint's value bands with real area, so a gradient
    // between two values doesn't count as a third.
    let mut band_counts = [0usize; BANDS];
    for &v in &m.squint.values {
        band_counts[((v * BANDS as f32).floor() as usize).min(BANDS - 1)] += 1;
    }
    let groups: Vec<(usize, f32)> = band_counts
        .iter()
        .enumerate()
        .map(|(band, &c)| (band, c as f32 / total))
        .filter(|&(_, share)| share >= GROUP_BAND)
        .collect();
    let dark: f32 = bins[0..3].iter().sum();
    let mid: f32 = bins[3..7].iter().sum();
    let light: f32 = bins[7..10].iter().sum();
    let middle: f32 = bins[4..7].iter().sum();
    let focal_contrast = focal_cell.map(|(column, row)| {
        let mut all: Vec<f32> = m.heat.cells.iter().map(|c| c.contrast).collect();
        all.sort_by(|a, b| a.total_cmp(b));
        // A flat picture's median is 0: the floor keeps the ratio finite.
        let median = all[all.len() / 2].max(FLAT_CELL);
        // The strongest contrast near the focal point: the middle of a
        // shape is often flat, its edges are where the contrast is.
        let mut strongest = 0.0f32;
        for r in 0..GRID {
            for c in 0..GRID {
                if (c as i32 - column as i32).abs() <= NEAR_FOCAL_CELLS
                    && (r as i32 - row as i32).abs() <= NEAR_FOCAL_CELLS
                {
                    strongest = strongest.max(m.heat.cell(c, r).contrast);
                }
            }
        }
        strongest / median
    });
    Values {
        bins,
        groups,
        dark,
        mid,
        light,
        middle,
        flat: middle >= FLAT_PICTURE,
        focal_contrast,
    }
}

// ---- Focal point --------------------------------------------------------------------

fn part_at(subject: &Subject, x: f32, y: f32) -> Option<String> {
    // The smallest part whose box holds the point.
    subject
        .parts
        .iter()
        .filter(|p| {
            x >= p.area.left as f32
                && x < p.area.right as f32
                && y >= p.area.top as f32
                && y < p.area.bottom as f32
        })
        .min_by_key(|p| (p.area.right - p.area.left) as i64 * (p.area.bottom - p.area.top) as i64)
        .map(|p| p.name.clone())
}

fn focal(m: &Measured, subject: &Subject) -> Focal {
    let (column, row) = m.heat.peak;
    let peak = m.heat.centre_of(column, row);
    let distance = subject.focal.map(|(x, y)| (peak.0 - x).hypot(peak.1 - y));
    let inside = subject.focal_part.as_ref().map(|(_, area)| {
        peak.0 >= area.left as f32
            && peak.0 < area.right as f32
            && peak.1 >= area.top as f32
            && peak.1 < area.bottom as f32
    });
    Focal {
        peak,
        peak_part: part_at(subject, peak.0, peak.1),
        distance,
        inside,
    }
}

// ---- Colour -------------------------------------------------------------------------

/// A hue is in key when it's within `OUT_OF_KEY_DEGREES` of a key hue,
/// or between two key hues the short way round (a mix of them).
pub fn in_key(hue: f32, key_hues: &[(String, f32)]) -> bool {
    if key_hues
        .iter()
        .any(|(_, key)| hue_distance(hue, *key) <= OUT_OF_KEY_DEGREES)
    {
        return true;
    }
    key_hues.iter().any(|(_, a)| {
        key_hues.iter().any(|(_, b)| {
            let arc = hue_distance(*a, *b);
            arc > 0.0
                && arc < 180.0
                && (hue_distance(hue, *a) + hue_distance(hue, *b) - arc).abs() < 0.01
        })
    })
}

/// The circular mean of hues (degrees) with weights.
fn mean_hue(hues: impl Iterator<Item = (f32, f32)>) -> f32 {
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    for (hue, weight) in hues {
        sx += hue.to_radians().cos() * weight;
        sy += hue.to_radians().sin() * weight;
    }
    sy.atan2(sx).to_degrees().rem_euclid(360.0)
}

fn colour(m: &Measured, subject: &Subject) -> Colour {
    let image = subject.image;
    let mut bins = [0.0f32; 24];
    let (mut warm, mut cool, mut neutral) = (0.0f32, 0.0f32, 0.0f32);
    // Per cell: the hues of its strongly saturated pixels.
    let mut cell_hues: Vec<Vec<f32>> = vec![Vec::new(); GRID * GRID];
    let mut cell_counts = vec![0usize; GRID * GRID];
    for y in 0..m.height {
        for x in 0..m.width {
            let (h, s, v) = hsv(on_white(image.pixel(x, y)));
            let cell = cell_index(x, y, m.width, m.height);
            cell_counts[cell] += 1;
            if s < HUE_SATURATION || v < HUE_VALUE {
                continue;
            }
            bins[((h / 15.0).floor() as usize).min(23)] += s;
            if hue_distance(h, 15.0) <= 45.0 {
                warm += s;
            } else if hue_distance(h, 210.0) <= 60.0 {
                cool += s;
            } else {
                neutral += s;
            }
            if s >= SATURATED {
                cell_hues[cell].push(h);
            }
        }
    }
    let weight: f32 = bins.iter().sum();
    let hues = if weight > 0.0 {
        let mut sorted = bins.to_vec();
        sorted.sort_by(|a, b| b.total_cmp(a));
        let mut sum = 0.0;
        let mut n = 0;
        for b in sorted {
            if sum >= weight * 0.9 {
                break;
            }
            sum += b;
            n += 1;
        }
        n
    } else {
        0
    };
    let balance = warm + cool + neutral;
    let (warm, cool, neutral) = if balance > 0.0 {
        (warm / balance, cool / balance, neutral / balance)
    } else {
        (0.0, 0.0, 0.0)
    };
    let key_hues: Vec<(String, f32)> = subject
        .key_colors
        .iter()
        .filter_map(|(name, color)| {
            let (h, s, v) = hsv(*color);
            (s >= KEY_SATURATION && v >= HUE_VALUE).then(|| (name.clone(), h))
        })
        .collect();
    // Cells out of key: saturated over half their pixels, with a dominant
    // hue far from every key hue; grouped into blobs of touching cells.
    let mut out = vec![false; GRID * GRID];
    let mut dominant = vec![0.0f32; GRID * GRID];
    if !key_hues.is_empty() {
        for i in 0..GRID * GRID {
            if cell_counts[i] == 0 || cell_hues[i].len() * 2 < cell_counts[i] {
                continue;
            }
            let mut hist = [0usize; 24];
            for &h in &cell_hues[i] {
                hist[((h / 15.0).floor() as usize).min(23)] += 1;
            }
            let top = (0..24).max_by_key(|&b| hist[b]).unwrap_or(0) as f32 * 15.0 + 7.5;
            let hue = mean_hue(
                cell_hues[i]
                    .iter()
                    .filter(|&&h| hue_distance(h, top) <= 30.0)
                    .map(|&h| (h, 1.0)),
            );
            dominant[i] = hue;
            out[i] = !in_key(hue, &key_hues);
        }
    }
    let mut seen = vec![false; GRID * GRID];
    let mut blobs = Vec::new();
    for start in 0..GRID * GRID {
        if !out[start] || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        let mut cells = Vec::new();
        seen[start] = true;
        while let Some(i) = stack.pop() {
            cells.push(i);
            let (c, r) = ((i % GRID) as i32, (i / GRID) as i32);
            for (dc, dr) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nc, nr) = (c + dc, r + dr);
                if nc < 0 || nr < 0 || nc >= GRID as i32 || nr >= GRID as i32 {
                    continue;
                }
                let j = nr as usize * GRID + nc as usize;
                if out[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        let n = cells.len() as f32;
        let (mut sx, mut sy) = (0.0, 0.0);
        for &i in &cells {
            let (x, y) = m.heat.centre_of(i % GRID, i / GRID);
            sx += x;
            sy += y;
        }
        let at = (sx / n, sy / n);
        let hue = mean_hue(cells.iter().map(|&i| (dominant[i], 1.0)));
        blobs.push(Blob {
            at,
            hue,
            share: n / (GRID * GRID) as f32,
            part: part_at(subject, at.0, at.1),
        });
    }
    blobs.sort_by(|a, b| b.share.total_cmp(&a.share));
    Colour {
        hues,
        warm,
        cool,
        neutral,
        key_hues,
        out_of_key: blobs,
    }
}

// ---- Texture ------------------------------------------------------------------------

fn cell_index(x: u32, y: u32, width: u32, height: u32) -> usize {
    let column = (u64::from(x) * GRID as u64 / u64::from(width)) as usize;
    let row = (u64::from(y) * GRID as u64 / u64::from(height)) as usize;
    row.min(GRID - 1) * GRID + column.min(GRID - 1)
}

/// The 95th minus the 5th percentile.
fn range_of(values: &mut [f32]) -> f32 {
    if values.len() < 2 {
        return 0.0;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let at = |p: f32| values[((values.len() - 1) as f32 * p).round() as usize];
    (at(0.95) - at(0.05)).max(0.0)
}

/// The part's pixels inside the canvas: (x, y) for every covered pixel.
fn pixels_of(part: &Part, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> + '_ {
    let rect = part
        .mask
        .rect()
        .intersect(&PixelRect::new(0, 0, width as i32, height as i32));
    (rect.top.max(0)..rect.bottom.max(0))
        .flat_map(move |y| (rect.left.max(0)..rect.right.max(0)).map(move |x| (x, y)))
        .filter(move |&(x, y)| part.mask.get(x, y) >= 128)
        .map(|(x, y)| (x as u32, y as u32))
}

fn textures(m: &Measured, subject: &Subject) -> Vec<Texture> {
    let canvas = (m.width as f32) * (m.height as f32);
    let mut out = Vec::new();
    for part in &subject.parts {
        // Per cell: the part's pixels' greys, grain and marks.
        let mut cell_greys: Vec<Vec<f32>> = vec![Vec::new(); GRID * GRID];
        let mut cell_grain = vec![0.0f32; GRID * GRID];
        let mut cell_marks = vec![0.0f32; GRID * GRID];
        let mut greys = Vec::new();
        let mut squints = Vec::new();
        let (mut grain_sum, mut marks_sum, mut count) = (0.0f32, 0.0f32, 0usize);
        for (x, y) in pixels_of(part, m.width, m.height) {
            let i = (y * m.width + x) as usize;
            let cell = cell_index(x, y, m.width, m.height);
            cell_greys[cell].push(m.grey.values[i]);
            cell_grain[cell] += m.grain[i];
            cell_marks[cell] += m.marks[i];
            greys.push(m.grey.values[i]);
            squints.push(m.squint.values[i]);
            grain_sum += m.grain[i];
            marks_sum += m.marks[i];
            count += 1;
        }
        let share = count as f32 / canvas;
        if share < SMALL_PART {
            continue;
        }
        let range = range_of(&mut greys).max(1e-3);
        let squint_range = range_of(&mut squints);
        let (mut dirty_cells, mut flat_cells, mut cells) = (0usize, 0usize, 0usize);
        for i in 0..GRID * GRID {
            let n = cell_greys[i].len();
            // A cell counts when the part fills at least half of it.
            let (l, t, r, b) = cell_box(i, m.width, m.height);
            let cell_pixels = ((r - l) * (b - t)) as usize;
            if cell_pixels == 0 || n * 2 < cell_pixels {
                continue;
            }
            cells += 1;
            let grain = cell_grain[i] / n as f32;
            let marks = cell_marks[i] / n as f32;
            if grain > DIRTY_GRAIN * range && marks < MARKS_OVER_GRAIN * grain {
                dirty_cells += 1;
            }
            if range_of(&mut cell_greys[i]) < FLAT_CELL {
                flat_cells += 1;
            }
        }
        let cells = cells.max(1) as f32;
        let dirty_share = dirty_cells as f32 / cells;
        let flat_share = flat_cells as f32 / cells;
        out.push(Texture {
            name: part.name.clone(),
            share,
            grain: grain_sum / count.max(1) as f32 / range,
            marks: marks_sum / count.max(1) as f32 / range,
            dirty_share,
            dirty: dirty_share > DIRTY_SHARE,
            flat_share,
            flat: flat_share > FLAT_SHARE,
            squint_range,
            low_structure: squint_range < LOW_STRUCTURE,
        });
    }
    out
}

/// The share of the picture's marks within `NEAR_FOCAL_CELLS` of a cell,
/// and that area's share of the picture.
fn detail_near(m: &Measured, (column, row): (usize, usize)) -> (f32, f32) {
    let (mut near, mut total) = (0.0f32, 0.0f32);
    let (mut near_pixels, mut pixels) = (0u64, 0u64);
    for y in 0..m.height {
        for x in 0..m.width {
            let i = (y * m.width + x) as usize;
            let cell = cell_index(x, y, m.width, m.height);
            let (c, r) = ((cell % GRID) as i32, (cell / GRID) as i32);
            let close = (c - column as i32).abs() <= NEAR_FOCAL_CELLS
                && (r - row as i32).abs() <= NEAR_FOCAL_CELLS;
            total += m.marks[i];
            pixels += 1;
            if close {
                near += m.marks[i];
                near_pixels += 1;
            }
        }
    }
    (
        if total > 0.0 { near / total } else { 0.0 },
        near_pixels as f32 / pixels.max(1) as f32,
    )
}

// ---- Readability --------------------------------------------------------------------

fn readability(m: &Measured, subject: &Subject) -> Vec<Readability> {
    let thumb = senses::thumbnail(subject.image);
    let (tw, th) = (thumb.width(), thumb.height());
    let canvas = (m.width as f32) * (m.height as f32);
    let mut out = Vec::new();
    for part in &subject.parts {
        // Small parts (a reed, a glint) are small on purpose: only the
        // focal part is judged however small it is.
        let share = part.mask.covered_pixels() as f32 / canvas;
        let is_focal = subject
            .focal_part
            .as_ref()
            .is_some_and(|(name, _)| *name == part.name);
        if share < SMALL_PART && !is_focal {
            continue;
        }
        // How much of each thumbnail pixel the part covers.
        let mut covered = vec![0u32; (tw * th) as usize];
        let mut all = vec![0u32; (tw * th) as usize];
        for y in 0..m.height {
            let ty = (u64::from(y) * u64::from(th) / u64::from(m.height)) as u32;
            for x in 0..m.width {
                let tx = (u64::from(x) * u64::from(tw) / u64::from(m.width)) as u32;
                all[(ty * tw + tx) as usize] += 1;
            }
        }
        for (x, y) in pixels_of(part, m.width, m.height) {
            let tx = (u64::from(x) * u64::from(tw) / u64::from(m.width)) as u32;
            let ty = (u64::from(y) * u64::from(th) / u64::from(m.height)) as u32;
            covered[(ty * tw + tx) as usize] += 1;
        }
        let inside: Vec<bool> = covered
            .iter()
            .zip(&all)
            .map(|(&c, &a)| a > 0 && c * 2 >= a)
            .collect();
        let pixels = inside.iter().filter(|&&b| b).count();
        if pixels == 0 {
            // Not even one thumbnail pixel is mostly this part.
            out.push(Readability {
                name: part.name.clone(),
                pixels,
                too_small: true,
                value_gap: 0.0,
                hue_gap: None,
                merges: false,
            });
            continue;
        }
        let mut ring = vec![false; (tw * th) as usize];
        for y in 0..th as i32 {
            for x in 0..tw as i32 {
                if inside[(y as u32 * tw + x as u32) as usize] {
                    continue;
                }
                let near = (-1..=1).any(|dy| {
                    (-1..=1).any(|dx| {
                        let (nx, ny) = (x + dx, y + dy);
                        nx >= 0
                            && ny >= 0
                            && nx < tw as i32
                            && ny < th as i32
                            && inside[(ny as u32 * tw + nx as u32) as usize]
                    })
                });
                ring[(y as u32 * tw + x as u32) as usize] = near;
            }
        }
        let stats = |chosen: &[bool]| {
            let (mut value, mut n, mut sat_n) = (0.0f32, 0usize, 0usize);
            let mut hues = Vec::new();
            for y in 0..th {
                for x in 0..tw {
                    if !chosen[(y * tw + x) as usize] {
                        continue;
                    }
                    let color = thumb.pixel(x, y);
                    value += luminance(color);
                    n += 1;
                    let (h, s, v) = hsv(color);
                    if s >= HUE_SATURATION && v >= HUE_VALUE {
                        sat_n += 1;
                        hues.push((h, s));
                    }
                }
            }
            let hue = (n > 0 && sat_n * 2 >= n).then(|| mean_hue(hues.into_iter()));
            (if n > 0 { value / n as f32 } else { 0.0 }, hue)
        };
        let (part_value, part_hue) = stats(&inside);
        let (ring_value, ring_hue) = stats(&ring);
        let value_gap = (part_value - ring_value).abs();
        let hue_gap = match (part_hue, ring_hue) {
            (Some(a), Some(b)) => Some(hue_distance(a, b)),
            _ => None,
        };
        let too_small = pixels < TOO_SMALL;
        let merges =
            !too_small && value_gap < MERGE_VALUE && hue_gap.is_none_or(|gap| gap < MERGE_HUE);
        out.push(Readability {
            name: part.name.clone(),
            pixels,
            too_small,
            value_gap,
            hue_gap,
            merges,
        });
    }
    out
}

// ---- Composition --------------------------------------------------------------------

fn composition(m: &Measured, subject: &Subject) -> Composition {
    let weight = m.heat.centre_of_weight();
    let dead_centre = subject.focal.is_some_and(|(x, y)| {
        (x / m.width as f32 - 0.5).abs() <= DEAD_CENTRE
            && (y / m.height as f32 - 0.5).abs() <= DEAD_CENTRE
    });
    let cut_by_edge = subject.focal_part.as_ref().is_some_and(|(_, area)| {
        area.left <= 0
            || area.top <= 0
            || area.right >= m.width as i32
            || area.bottom >= m.height as i32
    });
    let horizon = subject
        .parts
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case("horizon"))
        .map(|p| (p.area.top + p.area.bottom) as f32 / 2.0 / m.height as f32);
    Composition {
        weight,
        dead_centre,
        cut_by_edge,
        horizon,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc(image: &mut Image, cx: i32, cy: i32, r: i32, color: Color) -> Mask {
        let mut mask = Mask::new(PixelRect::new(cx - r, cy - r, cx + r + 1, cy + r + 1));
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                if (x - cx).pow(2) + (y - cy).pow(2) <= r * r {
                    image.set_pixel(x, y, color);
                    mask.set(x, y, 255);
                }
            }
        }
        mask
    }

    fn part(name: &str, mask: Mask) -> Part {
        let area = mask.rect();
        Part {
            name: name.to_owned(),
            mask,
            area,
        }
    }

    #[test]
    fn hues_have_names_and_distances() {
        assert_eq!(hue_name(0.0), "red");
        assert_eq!(hue_name(350.0), "red");
        assert_eq!(hue_name(120.0), "green");
        assert_eq!(hue_name(100.0), "yellow-green");
        assert_eq!(hue_distance(10.0, 350.0), 20.0);
        assert_eq!(hue_distance(0.0, 180.0), 180.0);
    }

    #[test]
    fn a_flat_grey_picture_is_flat_and_a_dark_disc_on_light_reads() {
        let mut image = Image::new(320, 200, [128, 128, 128, 255]);
        let subject = Subject {
            image: &image,
            focal: None,
            focal_part: None,
            key_colors: vec![],
            parts: vec![],
        };
        let flat = Critique::of(&subject);
        assert!(flat.values.flat);
        assert_eq!(flat.values.groups, [(2, 1.0)]);
        assert!(flat.lines().iter().any(|l| l.contains("looks flat")));
        assert_eq!(flat.colour.hues, 0);

        let mut light = Image::new(320, 200, [230, 230, 230, 255]);
        let mask = disc(&mut light, 200, 100, 40, [20, 20, 20, 255]);
        image = light;
        let subject = Subject {
            image: &image,
            focal: Some((200.0, 100.0)),
            focal_part: Some(("disc".into(), mask.rect())),
            key_colors: vec![],
            parts: vec![part("disc", mask)],
        };
        let c = Critique::of(&subject);
        assert!(!c.values.flat);
        assert_eq!(c.values.groups.len(), 2, "{:?}", c.values.groups);
        assert!(
            in_key(300.0, &[("a".into(), 227.0), ("b".into(), 34.0)]),
            "between, the short way"
        );
        assert!(
            !in_key(120.0, &[("a".into(), 227.0), ("b".into(), 34.0)]),
            "the long way round"
        );
        assert!(
            c.values
                .focal_contrast
                .is_some_and(|r| r >= FOCAL_CONTRAST_GOOD)
        );
        assert_eq!(c.focal.inside, Some(true));
        assert!(c.focal.distance.unwrap() < 60.0, "{:?}", c.focal);
        assert_eq!(c.focal.peak_part.as_deref(), Some("disc"));
        let read = &c.readability[0];
        assert!(!read.too_small && !read.merges, "{read:?}");
        assert!(c.textures[0].flat, "a solid disc is flat inside");
        assert!(!c.composition.dead_centre);
        assert!(!c.composition.cut_by_edge);
        assert!(
            c.lines()
                .last()
                .unwrap()
                .starts_with("Numbers can't judge beauty")
        );
    }

    #[test]
    fn a_green_blob_in_an_orange_key_is_out_of_key_and_noise_is_dirty() {
        let mut image = Image::new(256, 256, [230, 140, 60, 255]);
        let green = disc(&mut image, 64, 64, 30, [40, 200, 60, 255]);
        // A noisy disc: a checker of two greys, pixel by pixel.
        let mut noisy = disc(&mut image, 180, 180, 50, [120, 120, 120, 255]);
        for y in 130..=230 {
            for x in 130..=230 {
                if noisy.get(x, y) >= 128 {
                    let v = if (x + y) % 2 == 0 { 90 } else { 150 };
                    image.set_pixel(x, y, [v, v, v, 255]);
                }
            }
        }
        noisy.set(0, 0, 0);
        let subject = Subject {
            image: &image,
            focal: Some((180.0, 180.0)),
            focal_part: None,
            key_colors: vec![
                ("orange".into(), [230, 140, 60, 255]),
                ("ink".into(), [20, 20, 20, 255]),
            ],
            parts: vec![part("leaf", green), part("stone", noisy)],
        };
        let c = Critique::of(&subject);
        assert_eq!(c.colour.key_hues.len(), 1, "grey keys have no hue");
        assert_eq!(c.colour.out_of_key.len(), 1, "{:?}", c.colour.out_of_key);
        let blob = &c.colour.out_of_key[0];
        assert_eq!(hue_name(blob.hue), "green");
        assert_eq!(blob.part.as_deref(), Some("leaf"));
        assert!((blob.at.0 - 64.0).abs() < 20.0 && (blob.at.1 - 64.0).abs() < 20.0);
        let stone = c.textures.iter().find(|t| t.name == "stone").unwrap();
        assert!(stone.dirty, "{stone:?}");
        let leaf = c.textures.iter().find(|t| t.name == "leaf").unwrap();
        assert!(!leaf.dirty, "{leaf:?}");
        assert!(
            c.lines()
                .iter()
                .any(|l| l.contains("out of key: a saturated green"))
        );
        assert!(
            c.lines()
                .iter()
                .any(|l| l.contains("stone is noisy inside"))
        );
    }

    #[test]
    fn a_part_that_merges_or_is_tiny_is_reported() {
        let mut image = Image::new(400, 300, [128, 128, 128, 255]);
        let faint = disc(&mut image, 100, 150, 40, [140, 140, 140, 255]);
        let tiny = disc(&mut image, 300, 150, 2, [0, 0, 0, 255]);
        let subject = Subject {
            image: &image,
            focal: Some((200.0, 150.0)),
            focal_part: Some(("tiny".into(), tiny.rect())),
            key_colors: vec![],
            parts: vec![part("faint", faint), part("tiny", tiny)],
        };
        let c = Critique::of(&subject);
        let faint = c.readability.iter().find(|r| r.name == "faint").unwrap();
        assert!(faint.merges, "{faint:?}");
        let tiny = c.readability.iter().find(|r| r.name == "tiny").unwrap();
        assert!(tiny.too_small, "{tiny:?}");
        assert!(c.composition.dead_centre);
        let same = Critique::of(&subject);
        assert_eq!(c, same, "deterministic");
    }
}

/// The senses against reality: the reference pictures (good
/// photographs) should mostly pass.
#[cfg(test)]
mod reference_tests {
    use super::*;

    fn critique_of(file: &str) -> Critique {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../skills/art-app/references");
        let image = Image::load(&folder.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        let subject = Subject {
            image: &image,
            focal: None,
            focal_part: None,
            key_colors: vec![],
            parts: vec![],
        };
        let c = Critique::of(&subject);
        assert_eq!(c, Critique::of(&subject), "{file}: deterministic");
        eprintln!(
            "{file}: flat {} (middle {:.2}), groups {:?}, hues {}, warm {:.2}, weight ({:.2}, {:.2}), peak {:?}",
            c.values.flat,
            c.values.middle,
            c.values.groups,
            c.colour.hues,
            c.colour.warm,
            c.composition.weight.0,
            c.composition.weight.1,
            c.focal.peak
        );
        c
    }

    #[test]
    fn the_reference_pictures_mostly_pass() {
        for file in [
            "astronaut.png",
            "chelsea.png",
            "coffee.png",
            "rocket.jpg",
            "van-gogh-starry-night.jpg",
            "vermeer-girl-with-a-pearl-earring.jpg",
            "friedrich-wanderer.jpg",
            "hokusai-great-wave.jpg",
        ] {
            let c = critique_of(file);
            assert!(!c.values.flat, "{file} is called flat: {:?}", c.values);
            assert!(c.values.groups.len() >= 3, "{file}: {:?}", c.values.groups);
            assert!(
                c.colour.hues >= 1 && c.colour.hues <= 12,
                "{file}: {} hues",
                c.colour.hues
            );
            // The visual weight is never far from the middle in these.
            let (x, y) = c.composition.weight;
            assert!(
                (0.3..0.7).contains(&x) && (0.3..0.7).contains(&y),
                "{file}: {x} {y}"
            );
        }
    }

    /// The known false alarm: Monet's sunrise is nearly one value on
    /// purpose, and the numbers say "flat". Recorded in decisions.md.
    #[test]
    fn monets_sunrise_is_the_known_false_alarm() {
        let c = critique_of("monet-impression-sunrise.jpg");
        assert!(c.values.middle >= 0.6, "{:?}", c.values);
        assert!(c.values.groups.len() <= 2, "{:?}", c.values.groups);
    }
}
