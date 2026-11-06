//! The critique: the artist's eye in numbers. It looks at a painted
//! picture (and, when known, the painter's intent, the named parts'
//! silhouettes and own paint, and the layers) and reports what a person
//! spots in a second: flat values, a focal point that isn't where it was
//! meant to be, a colour that's foreign to the key, noise or blotches
//! where there should be shapes, a part that vanishes at thumbnail size,
//! a lopsided composition.
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
/// Grain (the median over a cell, so a few sparkles don't count) above
/// this share of the part's value range is noise...
pub const DIRTY_GRAIN: f32 = 0.06;
/// ...unless the marks there are at least this many times the grain.
pub const MARKS_OVER_GRAIN: f32 = 2.0;
/// A part is dirty when noise covers more than this share of it.
pub const DIRTY_SHARE: f32 = 0.5;
/// The blurs that take away blotches (6 to 24 px) and soft blobs (24 to
/// 96 px): with `GRAIN_BLUR` and `MARKS_BLUR`, four bands of fineness.
pub const BLOBS_BLUR: f32 = 24.0;
pub const SHAPES_BLUR: f32 = 96.0;
/// A band (marks, blotches, blobs) with at least this much energy in a
/// cell (mean change in value) is looked at for direction...
pub const BLOTCH_ENERGY: f32 = 0.015;
/// ...and counts as blotches (noise bigger than grain) when it is soft
/// (`BLOTCH_SHARPNESS`) and the coherence of its gradients (the
/// structure tensor's: 1 = one direction, 0 = none) is under this.
/// Feathers, grass and strokes have a direction and edges; noise fills
/// have neither.
pub const BLOTCH_COHERENCE: f32 = 0.5;
/// Stillness: the share of pixels whose gradient is under half the
/// mean, over the band's direction window, each band on the picture
/// blurred to its own scale. Marks and flat fills are still between
/// their edges (a high share); a smooth blob field is a gradient
/// everywhere (a low share). Under this share a band is soft.
pub const BLOTCH_STILL: f32 = 0.5;
/// Gradient magnitudes are counted in this many log-scale bins (four
/// per doubling from `SLOPE_FINEST`), so windows can be summed.
const SLOPE_BINS: usize = 96;
const SLOPE_FINEST: f32 = 1.0 / 4096.0;
const SLOPE_BINS_PER_DOUBLING: f32 = 4.0;
/// A part whose own paint is one colour (less value range than this)
/// and mostly see-through (mean opacity under `TINT_ALPHA` where it
/// counts) is a tint (a vignette, a shadow, a glow): nothing to judge.
pub const TINT: f32 = 0.02;
pub const TINT_ALPHA: f32 = 220.0;
/// Texture is judged this many pixels in from a part's edge, per band:
/// the edge itself (an outline, the silhouette) is a shape, not a
/// texture, and a band's blur drags it that far inside.
pub const EDGE_MARGIN: [f32; 3] = [8.0, 12.0, 30.0];
/// A part is blotchy when blotches cover more than this share of it.
/// (The gallery's marbled duck body and smudge clouds sit at 42%; a
/// smooth-form painting as one part at 26 to 34%.)
pub const BLOTCHY_SHARE: f32 = 0.4;
/// A cell with a value range under this is flat...
pub const FLAT_CELL: f32 = 0.02;
/// ...and a part with more than this share of flat cells is flat.
pub const FLAT_SHARE: f32 = 0.25;
/// A part whose squint has less value range than this is a smudge.
pub const LOW_STRUCTURE: f32 = 0.05;
/// A part this much lighter (mean value) than the ring around it is a
/// light: a light source may be flat and has no shadow side.
pub const LIGHT_STANDOUT: f32 = 0.3;
/// The fringe just inside a silhouette's edge (this many pixels in)...
pub const FRINGE: (f32, f32) = (2.0, 6.0);
/// ...against further in.
pub const INTERIOR: (f32, f32) = (14.0, 30.0);
/// An edge is soft when the part's own opacity ramps: over the first
/// pixels inside its silhouette (`SOFT_DEPTH`) it is this far (0-255)
/// under the opacity further in (`SOFT_INSIDE`). A blurred silhouette
/// ramps; a see-through shadow is even all the way in, and isn't soft.
pub const SOFT_EDGE: f32 = 40.0;
pub const SOFT_DEPTH: (f32, f32) = (1.0, 3.0);
pub const SOFT_MIDDLE: (f32, f32) = (4.0, 6.0);
pub const SOFT_INSIDE: (f32, f32) = (7.0, 12.0);
/// A ramp keeps rising: the middle depth is at least this much more
/// opaque than the first pixels (a see-through shadow under an opaque
/// body is a step, not a ramp).
pub const SOFT_RISE: f32 = 10.0;
/// A soft edge through which the picture is this much lighter than the
/// part's own paint (the background showing through) is a halo: a blur
/// on the silhouette reads as a haze band or a smudge.
pub const HALO: f32 = 0.05;
/// How many cells around a cell the direction of each band is judged
/// over: a blotch is bigger than a cell, so one cell sees one slope,
/// and a few blobs in a small window look like a direction by chance.
pub const DIRECTION_WINDOW: [i32; 3] = [0, 2, 4];
/// The sides a fringe is judged on: this many, around the clock.
pub const SIDES: usize = 8;
pub const SIDE_NAMES: [&str; SIDES] = [
    "right",
    "lower right",
    "bottom",
    "lower left",
    "left",
    "upper left",
    "top",
    "upper right",
];
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

/// The squint's value bands, darkest first.
pub const BAND_NAMES: [&str; BANDS] = ["black", "dark", "mid", "light", "white"];

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

/// A named part of the picture, or a layer, with its silhouette (canvas
/// pixels) and, when known, its paint on its own.
#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub mask: Mask,
    /// The box around what it drew, in canvas pixels.
    pub area: PixelRect,
    /// Its paint alone, the size of `mask`'s box, see-through where it
    /// has none. Texture and structure are judged on this when it's
    /// here (so a paper grain or a gradient behind doesn't count), on
    /// the picture inside the mask when it isn't.
    pub own: Option<Image>,
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
    /// The visible layers, bottom to top, each as a part with its own
    /// paint (texture and structure are judged on them too: a cloud on
    /// the sky layer, a haze over the hills).
    pub layers: Vec<Part>,
}

// ---- The report -------------------------------------------------------------------

/// The contrast at the focal point against the strongest elsewhere.
#[derive(Clone, Debug, PartialEq)]
pub struct FocalContrast {
    /// The strongest cell contrast within `NEAR_FOCAL_CELLS` of the focal
    /// point or inside the focal part's box.
    pub near: f32,
    /// The strongest cell contrast further away, and where.
    pub elsewhere: f32,
    pub elsewhere_at: (f32, f32),
    pub elsewhere_part: Option<String>,
}

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
    /// If there's a focal point: its contrast against the rest.
    pub focal_contrast: Option<FocalContrast>,
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
    /// Judged on its own paint (true) or on the picture inside it.
    pub own: bool,
    /// Grain (median) and marks (mean) over the part, as shares of its value range.
    pub grain: f32,
    pub marks: f32,
    /// The share of the part that's fine grain without marks.
    pub dirty_share: f32,
    pub dirty: bool,
    /// Energy (mean change in value) in the blotch bands: marks (1.5 to
    /// 6 px), blotches (6 to 24 px), blobs (24 to 96 px).
    pub bands: [f32; 3],
    /// The share of the part that's blotches (a band with energy and no
    /// direction), and the mean coherence of the bands with energy
    /// (1 = one direction).
    pub blotchy_share: f32,
    pub blotchy: bool,
    pub coherence: Option<f32>,
    /// The mean coherence of each band on its own, over the cells where
    /// it has energy.
    pub coherences: [Option<f32>; 3],
    /// The mean stillness (the share of pixels with a gradient under
    /// half the mean) of each band over the cells where it has energy:
    /// edges and flats, or a gradient everywhere; and the least of them.
    pub stills: [Option<f32>; 3],
    pub still: Option<f32>,
    pub flat_share: f32,
    pub flat: bool,
    /// The value range of the part's squint.
    pub squint_range: f32,
    pub low_structure: bool,
    /// Much lighter than the ring around it: a light source, so flat,
    /// dirty, blotchy and low_structure are not findings for it.
    pub light: bool,
    /// A tint: one colour with a soft alpha (a vignette, a glow), so
    /// its structure isn't judged.
    pub tint: bool,
    /// How soft its edges are: how much the own opacity ramps up from
    /// the first pixels inside the silhouette to further in, on the
    /// softest side (own paint only).
    pub soft_edge: Option<f32>,
    /// The strongest light fringe on a soft side: how much lighter the
    /// picture is than the part's own paint just inside that edge (the
    /// background showing through), and which side.
    pub halo: Option<(f32, &'static str)>,
    pub light_fringe: bool,
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
    /// The named parts' textures, then the layers'.
    pub textures: Vec<Texture>,
    /// The share of the marks within `NEAR_FOCAL_CELLS` of the focal
    /// point, and that area's share of the picture.
    pub detail_at_focal: Option<(f32, f32)>,
    pub readability: Vec<Readability>,
    pub composition: Composition,
}

/// The pixel measurements of one picture (or one part's own paint),
/// over a box of the canvas.
struct Field {
    /// The box's top-left on the canvas.
    origin: (i32, i32),
    width: u32,
    height: u32,
    /// Which pixels count (a part's own paint; everything for the picture).
    inside: Vec<bool>,
    grey: Grey,
    squint: Grey,
    /// Per pixel: what a small blur removes (grain), and what each bigger
    /// one removes beyond the last (marks, blotches, blobs).
    grain: Vec<f32>,
    marks: Vec<f32>,
    bands: [Vec<f32>; 3],
    /// The gradients of the grain-free picture, the marks-free one and
    /// the blotch-free one, for direction in each band.
    gradients: [(Vec<f32>, Vec<f32>); 3],
}

impl Field {
    /// Of `image`, placed at `origin`; `inside` says which pixels count
    /// (`None`: all of them).
    fn of(
        image: &Image,
        origin: (i32, i32),
        inside: Option<Vec<bool>>,
        squint_radius: f32,
    ) -> Field {
        let (w, h) = (image.width(), image.height());
        let all = inside.is_none();
        let inside = inside.unwrap_or_else(|| vec![true; (w * h) as usize]);
        let grey = if all {
            Grey::of(image)
        } else {
            // The part's own colours, not laid on anything.
            let values = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .zip(&inside)
                .map(|((x, y), &i)| if i { luminance(image.pixel(x, y)) } else { 0.0 })
                .collect();
            Grey {
                width: w,
                height: h,
                values,
            }
        };
        let blur = |radius: f32| {
            if all {
                grey.blurred(radius)
            } else {
                grey.blurred_where(radius, &inside)
            }
        };
        let fine = blur(GRAIN_BLUR);
        let coarse = blur(MARKS_BLUR);
        let coarser = blur(BLOBS_BLUR);
        let coarsest = blur(SHAPES_BLUR);
        let squint = blur(squint_radius);
        let difference = |a: &Grey, b: &Grey| -> Vec<f32> {
            a.values
                .iter()
                .zip(&b.values)
                .map(|(a, b)| (a - b).abs())
                .collect()
        };
        let grain = difference(&grey, &fine);
        let marks = difference(&fine, &coarse);
        let bands = [
            marks.clone(),
            difference(&coarse, &coarser),
            difference(&coarser, &coarsest),
        ];
        let gradient = |of: &Grey| -> (Vec<f32>, Vec<f32>) {
            let mut gx = vec![0.0f32; (w * h) as usize];
            let mut gy = vec![0.0f32; (w * h) as usize];
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    if !inside[i] {
                        continue;
                    }
                    if x + 1 < w && inside[i + 1] {
                        gx[i] = of.values[i + 1] - of.values[i];
                    }
                    if y + 1 < h && inside[i + w as usize] {
                        gy[i] = of.values[i + w as usize] - of.values[i];
                    }
                }
            }
            (gx, gy)
        };
        let gradients = [gradient(&fine), gradient(&coarse), gradient(&coarser)];
        Field {
            origin,
            width: w,
            height: h,
            inside,
            grey,
            squint,
            grain,
            marks,
            bands,
            gradients,
        }
    }

    /// The index of a canvas pixel in this field, if it's inside the box
    /// and counts.
    fn at(&self, x: i32, y: i32) -> Option<usize> {
        let (lx, ly) = (x - self.origin.0, y - self.origin.1);
        if lx < 0 || ly < 0 || lx >= self.width as i32 || ly >= self.height as i32 {
            return None;
        }
        let i = (ly as u32 * self.width + lx as u32) as usize;
        self.inside[i].then_some(i)
    }
}

/// The picture's measurements: the field over the whole canvas, and the heat.
struct Measured {
    width: u32,
    height: u32,
    field: Field,
    heat: Heat,
}

impl Measured {
    fn grey(&self) -> &Grey {
        &self.field.grey
    }
}

impl Critique {
    pub fn of(subject: &Subject) -> Critique {
        let image = subject.image;
        let grey = Grey::of(image);
        let radius = grey.squint_radius();
        let field = Field::of(image, (0, 0), None, radius);
        let heat = Heat::of_grey(image, &field.grey, &field.squint);
        let m = Measured {
            width: image.width(),
            height: image.height(),
            field,
            heat,
        };
        let focal_cell = subject.focal.map(|(x, y)| m.heat.cell_at(x, y));
        let mut textures: Vec<Texture> = subject
            .parts
            .iter()
            .filter_map(|part| texture_of(&m, part, radius))
            .collect();
        textures.extend(
            subject
                .layers
                .iter()
                .filter_map(|layer| texture_of(&m, layer, radius)),
        );
        Critique {
            values: values(&m, subject, focal_cell),
            focal: focal(&m, subject),
            colour: colour(&m, subject),
            textures,
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
        if let Some(fc) = &v.focal_contrast {
            let elsewhere = match &fc.elsewhere_part {
                Some(part) => format!(
                    "{part} (x {:.0}, y {:.0})",
                    fc.elsewhere_at.0, fc.elsewhere_at.1
                ),
                None => format!("x {:.0}, y {:.0}", fc.elsewhere_at.0, fc.elsewhere_at.1),
            };
            if fc.near >= fc.elsewhere {
                out.push(format!(
                    "focal contrast: the focal point has the strongest contrast in the picture \
                     ({:.1}x the next strongest, at {elsewhere})",
                    if fc.elsewhere > 0.0 {
                        fc.near / fc.elsewhere
                    } else {
                        1.0
                    }
                ));
            } else {
                out.push(format!(
                    "focal contrast: the strongest contrast is elsewhere: {elsewhere} is {:.1}x \
                     the focal point's; the eye may go there first",
                    if fc.near > 0.0 {
                        fc.elsewhere / fc.near
                    } else {
                        f32::INFINITY
                    }
                ));
            }
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
            if t.blotchy {
                out.push(format!(
                    "texture: {} is blotchy inside: soft blobs with no direction over {:.0}% \
                     of it (coherence {:.2}, still {:.2}); feathers, grass and strokes have \
                     a direction and edges, noise fills have neither: shade with big soft \
                     shapes and a few directed marks",
                    t.name,
                    t.blotchy_share * 100.0,
                    t.coherence.unwrap_or(0.0),
                    t.still.unwrap_or(0.0)
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
            if t.light_fringe {
                let (bleed, side) = t.halo.unwrap_or((0.0, "top"));
                out.push(format!(
                    "edge: {}'s {side} edge is soft and the lighter background shows through it \
                     ({bleed:+.2} just inside): it reads as a haze band or a smudge; a blur on \
                     the silhouette does this: blur the fill, not the edge, or give it a crisp \
                     side",
                    t.name
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

    /// Every part's and layer's numbers, one line each, for `--detail`.
    pub fn detail_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in &self.textures {
            out.push(format!(
                "{}: {:.1}% of the canvas{}; grain {:.3}, marks {:.3} (of its value range); \
                 bands {:.3} {:.3} {:.3}; noisy over {:.0}%, blotchy over {:.0}%{}, flat over \
                 {:.0}%; squint range {:.2}{}{}{}{}",
                t.name,
                t.share * 100.0,
                if t.own { ", its own paint" } else { "" },
                t.grain,
                t.marks,
                t.bands[0],
                t.bands[1],
                t.bands[2],
                t.dirty_share * 100.0,
                t.blotchy_share * 100.0,
                t.coherence.map_or(String::new(), |c| {
                    let per: Vec<String> = t
                        .coherences
                        .iter()
                        .map(|b| b.map_or("-".to_owned(), |v| format!("{v:.2}")))
                        .collect();
                    let sharp: Vec<String> = t
                        .stills
                        .iter()
                        .map(|b| b.map_or("-".to_owned(), |v| format!("{v:.2}")))
                        .collect();
                    format!(
                        " (coherence {c:.2}: {}; still {:.2}: {})",
                        per.join(" "),
                        t.still.unwrap_or(0.0),
                        sharp.join(" ")
                    )
                }),
                t.flat_share * 100.0,
                t.squint_range,
                t.soft_edge
                    .map_or(String::new(), |e| format!("; softest edge {e:.0}")),
                t.halo.map_or(String::new(), |(h, side)| format!(
                    ", shows through {h:+.2} ({side})"
                )),
                if t.light { "; a light" } else { "" },
                if t.tint { "; a tint" } else { "" }
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
                    || line.contains("is elsewhere:")
                    || line.contains("px from the intent")
                    || line.contains("out of key: a ")
                    || line.contains("is noisy inside")
                    || line.contains("is blotchy inside")
                    || line.contains("is flat over")
                    || line.contains("a smudge")
                    || line.contains("edge is soft and")
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

fn values(m: &Measured, subject: &Subject, focal_cell: Option<(usize, usize)>) -> Values {
    let mut counts = [0usize; 10];
    for &v in &m.grey().values {
        let bin = ((v * 10.0).floor() as usize).min(9);
        counts[bin] += 1;
    }
    let total = m.grey().values.len().max(1) as f32;
    let bins = counts.map(|c| c as f32 / total);
    // Groups: the squint's value bands with real area, so a gradient
    // between two values doesn't count as a third.
    let mut band_counts = [0usize; BANDS];
    for &v in &m.field.squint.values {
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
    // The strongest contrast near the focal point (the middle of a shape
    // is often flat, its edges carry the contrast) against the strongest
    // anywhere else.
    let focal_contrast = focal_cell.map(|(column, row)| {
        let (mut near, mut elsewhere, mut at) = (0.0f32, 0.0f32, (0usize, 0usize));
        for r in 0..GRID {
            for c in 0..GRID {
                let contrast = m.heat.cell(c, r).contrast;
                let (cx, cy) = m.heat.centre_of(c, r);
                let in_part = subject.focal_part.as_ref().is_some_and(|(_, a)| {
                    cx >= a.left as f32
                        && cx < a.right as f32
                        && cy >= a.top as f32
                        && cy < a.bottom as f32
                });
                let close = in_part
                    || ((c as i32 - column as i32).abs() <= NEAR_FOCAL_CELLS
                        && (r as i32 - row as i32).abs() <= NEAR_FOCAL_CELLS);
                if close {
                    near = near.max(contrast);
                } else if contrast > elsewhere {
                    elsewhere = contrast;
                    at = (c, r);
                }
            }
        }
        let elsewhere_at = m.heat.centre_of(at.0, at.1);
        // The part at that cell, or within a cell of it (a shape's edge
        // cells have their centres just outside its box).
        let (cw, ch) = (m.width as f32 / GRID as f32, m.height as f32 / GRID as f32);
        let elsewhere_part = part_at(subject, elsewhere_at.0, elsewhere_at.1).or_else(|| {
            subject
                .parts
                .iter()
                .filter(|p| {
                    elsewhere_at.0 >= p.area.left as f32 - cw
                        && elsewhere_at.0 < p.area.right as f32 + cw
                        && elsewhere_at.1 >= p.area.top as f32 - ch
                        && elsewhere_at.1 < p.area.bottom as f32 + ch
                })
                .min_by_key(|p| {
                    (p.area.right - p.area.left) as i64 * (p.area.bottom - p.area.top) as i64
                })
                .map(|p| p.name.clone())
        });
        FocalContrast {
            near,
            elsewhere,
            elsewhere_at,
            elsewhere_part,
        }
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

/// The middle value.
fn median_of(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
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

/// How far inside the part each pixel of its box is, in pixels (0
/// outside the mask; the canvas edge isn't an edge of the part): a
/// chamfer distance, two passes.
fn depth_inside(part: &Part, width: u32, height: u32) -> (PixelRect, Vec<f32>) {
    let rect = part
        .mask
        .rect()
        .intersect(&PixelRect::new(0, 0, width as i32, height as i32));
    let (w, h) = (
        (rect.right - rect.left).max(0) as usize,
        (rect.bottom - rect.top).max(0) as usize,
    );
    let big = 1e9f32;
    let mut d: Vec<f32> = vec![big; w * h];
    for y in 0..h {
        for x in 0..w {
            if part.mask.get(rect.left + x as i32, rect.top + y as i32) < 128 {
                d[y * w + x] = 0.0;
            }
        }
    }
    // A pixel on the box's edge next to the canvas edge keeps `big`; on
    // the box's edge inside the canvas it's next to outside-the-mask
    // pixels (the box is the mask's bounds), so the passes reach it.
    let (straight, diagonal) = (3.0f32, 4.0f32);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if d[i] == 0.0 {
                continue;
            }
            let mut best = d[i];
            if x > 0 {
                best = best.min(d[i - 1] + straight);
            }
            if y > 0 {
                best = best.min(d[i - w] + straight);
                if x > 0 {
                    best = best.min(d[i - w - 1] + diagonal);
                }
                if x + 1 < w {
                    best = best.min(d[i - w + 1] + diagonal);
                }
            }
            d[i] = best;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if d[i] == 0.0 {
                continue;
            }
            let mut best = d[i];
            if x + 1 < w {
                best = best.min(d[i + 1] + straight);
            }
            if y + 1 < h {
                best = best.min(d[i + w] + straight);
                if x > 0 {
                    best = best.min(d[i + w - 1] + diagonal);
                }
                if x + 1 < w {
                    best = best.min(d[i + w + 1] + diagonal);
                }
            }
            d[i] = best;
        }
    }
    (rect, d.into_iter().map(|v| v / straight).collect())
}

/// Layers are named `layer "..."` by whoever builds the subject.
fn is_layer(part: &Part) -> bool {
    part.name.starts_with("layer \"")
}

/// A part's texture: on its own paint when it has it, else on the
/// picture inside its mask. `None` for a part too small to judge.
fn texture_of(m: &Measured, part: &Part, squint_radius: f32) -> Option<Texture> {
    let canvas = (m.width as f32) * (m.height as f32);
    let count = pixels_of(part, m.width, m.height).count();
    let share = count as f32 / canvas;
    if share < SMALL_PART {
        return None;
    }
    // The field the texture is read from: the part's own paint, or the
    // picture.
    let own_field = part.own.as_ref().map(|own| {
        let rect = part.mask.rect();
        let inside: Vec<bool> = (0..own.height())
            .flat_map(|y| (0..own.width()).map(move |x| (x, y)))
            .map(|(x, y)| {
                own.pixel(x, y)[3] >= 128
                    && part.mask.get(rect.left + x as i32, rect.top + y as i32) >= 128
            })
            .collect();
        Field::of(own, (rect.left, rect.top), Some(inside), squint_radius)
    });
    let field = own_field.as_ref().unwrap_or(&m.field);

    // How far in from the edge each pixel is, for the margin and the halo.
    let (depth_rect, depth) = depth_inside(part, m.width, m.height);
    let depth_w = (depth_rect.right - depth_rect.left).max(1) as usize;
    let depth_at = |x: u32, y: u32| -> f32 {
        let (lx, ly) = (x as i32 - depth_rect.left, y as i32 - depth_rect.top);
        if lx < 0 || ly < 0 || lx as usize >= depth_w {
            return 0.0;
        }
        depth
            .get(ly as usize * depth_w + lx as usize)
            .copied()
            .unwrap_or(0.0)
    };
    // Per cell: the part's pixels' greys and grain (for medians), marks,
    // and the structure tensor for direction (inside the margin only).
    let mut cell_greys: Vec<Vec<f32>> = vec![Vec::new(); GRID * GRID];
    let mut cell_grain: Vec<Vec<f32>> = vec![Vec::new(); GRID * GRID];
    let mut cell_bands = vec![[0.0f32; 3]; GRID * GRID];
    let mut tensors = vec![[[0.0f32; 3]; 3]; GRID * GRID];
    let mut greys = Vec::new();
    let mut squints = Vec::new();
    let mut grains = Vec::new();
    let mut cell_inner = vec![[0usize; 3]; GRID * GRID];
    // Per cell and band: gradient magnitudes as a log histogram, their
    // sum and count (summed over windows for stillness).
    let mut cell_slopes: Vec<[([u32; SLOPE_BINS], f32, u32); 3]> =
        vec![[([0; SLOPE_BINS], 0.0, 0); 3]; GRID * GRID];
    let (mut marks_sum, mut band_sums, mut n) = (0.0f32, [0.0f32; 3], 0usize);
    let (mut alpha_sum, mut alpha_n) = (0.0f32, 0usize);
    for (x, y) in pixels_of(part, m.width, m.height) {
        let Some(i) = field.at(x as i32, y as i32) else {
            continue;
        };
        let cell = cell_index(x, y, m.width, m.height);
        cell_greys[cell].push(field.grey.values[i]);
        cell_grain[cell].push(field.grain[i]);
        let depth = depth_at(x, y);
        for band in 0..3 {
            if depth >= EDGE_MARGIN[band] {
                cell_inner[cell][band] += 1;
                let (fx, fy) = (field.gradients[band].0[i], field.gradients[band].1[i]);
                let slope = fx.hypot(fy);
                let bin = if slope <= SLOPE_FINEST {
                    0
                } else {
                    (((slope / SLOPE_FINEST).log2() * SLOPE_BINS_PER_DOUBLING).floor() as usize)
                        .min(SLOPE_BINS - 1)
                };
                let (hist, sum, n) = &mut cell_slopes[cell][band];
                hist[bin] += 1;
                *sum += slope;
                *n += 1;
                cell_bands[cell][band] += field.bands[band][i];
                band_sums[band] += field.bands[band][i];
                let (gx, gy) = (field.gradients[band].0[i], field.gradients[band].1[i]);
                tensors[cell][band][0] += gx * gx;
                tensors[cell][band][1] += gy * gy;
                tensors[cell][band][2] += gx * gy;
            }
        }
        if let Some(own) = &part.own {
            let r = part.mask.rect();
            alpha_sum +=
                f32::from(own.pixel((x as i32 - r.left) as u32, (y as i32 - r.top) as u32)[3]);
            alpha_n += 1;
        }
        greys.push(field.grey.values[i]);
        squints.push(field.squint.values[i]);
        grains.push(field.grain[i]);
        marks_sum += field.marks[i];
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let inner_total: [usize; 3] = [0, 1, 2].map(|b| cell_inner.iter().map(|c| c[b]).sum());
    let range = range_of(&mut greys).max(1e-3);
    let squint_range = range_of(&mut squints);
    let (mut dirty_cells, mut blotchy_cells, mut flat_cells, mut cells) =
        (0usize, 0usize, 0usize, 0usize);
    let (mut coherence_sum, mut coherence_n) = (0.0f32, 0usize);
    let (mut still_sum, mut still_n) = (0.0f32, 0usize);
    let mut band_coherence = [(0.0f32, 0usize); 3];
    let mut band_still = [(0.0f32, 0usize); 3];
    for i in 0..GRID * GRID {
        let count = cell_greys[i].len();
        // A cell counts when the part fills at least half of it.
        let (l, t, r, b) = cell_box(i, m.width, m.height);
        let cell_pixels = ((r - l) * (b - t)) as usize;
        if cell_pixels == 0 || count * 2 < cell_pixels {
            continue;
        }
        cells += 1;
        let grain = median_of(&mut cell_grain[i]);
        let marks = cell_bands[i][0] / cell_inner[i][0].max(1) as f32;
        if grain > DIRTY_GRAIN * range && marks < MARKS_OVER_GRAIN * grain {
            dirty_cells += 1;
        }
        // Direction and edges: for each band with energy here, the
        // coherence over a window as big as its blotches, and the
        // stillness of the picture blurred to the band's scale; only on
        // pixels in from the edge. The cell is blotchy when some band
        // is both soft and directionless.
        let mut least: Option<f32> = None;
        let mut softest: Option<f32> = None;
        let mut blotchy_here = false;
        for band in 0..3 {
            let inner = cell_inner[i][band];
            if inner * 2 < count || cell_bands[i][band] / inner.max(1) as f32 <= BLOTCH_ENERGY {
                continue;
            }
            let (column, row) = ((i % GRID) as i32, (i / GRID) as i32);
            let radius = DIRECTION_WINDOW[band];
            // Sharpness over the window: the 95th percentile of the
            // gradient (from the summed histograms) over its mean.
            let mut hist = [0u32; SLOPE_BINS];
            let (mut sum, mut n) = (0.0f32, 0u32);
            for r in (row - radius).max(0)..=(row + radius).min(GRID as i32 - 1) {
                for c in (column - radius).max(0)..=(column + radius).min(GRID as i32 - 1) {
                    let (h, s, k) = &cell_slopes[r as usize * GRID + c as usize][band];
                    for (a, b) in hist.iter_mut().zip(h) {
                        *a += b;
                    }
                    sum += s;
                    n += k;
                }
            }
            let mean = sum / n.max(1) as f32;
            // Still pixels: a gradient under half the mean.
            let half = mean * 0.5;
            let half_bin = if half <= SLOPE_FINEST {
                0
            } else {
                (((half / SLOPE_FINEST).log2() * SLOPE_BINS_PER_DOUBLING).floor() as usize)
                    .min(SLOPE_BINS - 1)
            };
            let still_count: u32 = hist[..half_bin].iter().sum();
            let still = still_count as f32 / n.max(1) as f32;
            band_still[band] = (band_still[band].0 + still, band_still[band].1 + 1);
            softest = Some(softest.map_or(still, |s: f32| s.min(still)));
            let mut sum = [0.0f32; 3];
            for r in (row - radius).max(0)..=(row + radius).min(GRID as i32 - 1) {
                for c in (column - radius).max(0)..=(column + radius).min(GRID as i32 - 1) {
                    let t = tensors[r as usize * GRID + c as usize][band];
                    sum = [sum[0] + t[0], sum[1] + t[1], sum[2] + t[2]];
                }
            }
            let [xx, yy, xy] = sum;
            let coherence = ((xx - yy).powi(2) + 4.0 * xy * xy).sqrt() / (xx + yy + 1e-12);
            band_coherence[band] = (
                band_coherence[band].0 + coherence,
                band_coherence[band].1 + 1,
            );
            least = Some(least.map_or(coherence, |l: f32| l.min(coherence)));
            if coherence < BLOTCH_COHERENCE && still < BLOTCH_STILL {
                blotchy_here = true;
            }
        }
        if let (Some(coherence), Some(still)) = (least, softest) {
            coherence_sum += coherence;
            coherence_n += 1;
            still_sum += still;
            still_n += 1;
            if blotchy_here {
                blotchy_cells += 1;
            }
        }
        if range_of(&mut cell_greys[i]) < FLAT_CELL {
            flat_cells += 1;
        }
    }
    let cells = cells.max(1) as f32;
    let dirty_share = dirty_cells as f32 / cells;
    let blotchy_share = blotchy_cells as f32 / cells;
    let flat_share = flat_cells as f32 / cells;
    let coherence = (coherence_n > 0).then(|| coherence_sum / coherence_n as f32);
    let coherences = band_coherence.map(|(sum, n)| (n > 0).then(|| sum / n as f32));
    let still = (still_n > 0).then(|| still_sum / still_n as f32);
    let stills = band_still.map(|(sum, n)| (n > 0).then(|| sum / n as f32));
    let bands = [0, 1, 2].map(|b| band_sums[b] / inner_total[b].max(1) as f32);
    // One colour with a soft alpha (a vignette, a glow, a shadow) is a
    // tint: nothing to judge inside it.
    let tint = part.own.is_some()
        && range < TINT
        && alpha_n > 0
        && (alpha_sum / alpha_n as f32) < TINT_ALPHA;

    // A light: much lighter than the ring around it (in the picture).
    let light = {
        let rect = part.mask.rect();
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        let (dx, dy) = ((w / 4).max(8), (h / 4).max(8));
        let ring = PixelRect::new(
            rect.left - dx,
            rect.top - dy,
            rect.right + dx,
            rect.bottom + dy,
        )
        .intersect(&PixelRect::new(0, 0, m.width as i32, m.height as i32));
        let (mut inside, mut outside) = ((0.0f32, 0usize), (0.0f32, 0usize));
        for y in ring.top..ring.bottom {
            for x in ring.left..ring.right {
                let v = m.grey().get(x as u32, y as u32);
                if part.mask.get(x, y) >= 128 {
                    inside = (inside.0 + v, inside.1 + 1);
                } else {
                    outside = (outside.0 + v, outside.1 + 1);
                }
            }
        }
        inside.1 > 0
            && outside.1 > 0
            && inside.0 / inside.1 as f32 - outside.0 / outside.1 as f32 >= LIGHT_STANDOUT
    };

    // The halo: on each side of the part, how soft its own edge is (the
    // own opacity over the first pixels inside) and how much lighter the
    // picture is than the own paint just inside it (the background
    // showing through). Only with own paint: without it, nothing says
    // what's the part and what's behind.
    let (soft_edge, halo) = match (&own_field, &part.own) {
        (Some(field), Some(own)) => {
            let rect = depth_rect;
            let w = depth_w;
            let h = (rect.bottom - rect.top).max(0) as usize;
            let own_rect = part.mask.rect();
            // Per side: (opacity sum, count) over the first pixels and
            // over the pixels further in, and (picture sum, own sum,
            // count) over the fringe.
            let mut softness = [(0.0f32, 0usize); SIDES];
            let mut middle = [(0.0f32, 0usize); SIDES];
            let mut deeper = [(0.0f32, 0usize); SIDES];
            let mut fringes = [(0.0f32, 0.0f32, 0usize); SIDES];
            for (i, &d) in depth.iter().enumerate() {
                if d < SOFT_DEPTH.0 || d > SOFT_INSIDE.1 {
                    continue;
                }
                let (lx, ly) = ((i % w.max(1)) as i32, (i / w.max(1)) as i32);
                let (x, y) = (rect.left + lx, rect.top + ly);
                // Which side: the way out is where the depth falls.
                let sample = |dx: i32, dy: i32| -> f32 {
                    let (sx, sy) = (lx + dx, ly + dy);
                    if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                        0.0
                    } else {
                        depth[sy as usize * w + sx as usize]
                    }
                };
                let (ox, oy) = (sample(-1, 0) - sample(1, 0), sample(0, -1) - sample(0, 1));
                let angle = oy.atan2(ox).rem_euclid(std::f32::consts::TAU);
                let side =
                    ((angle / std::f32::consts::TAU * SIDES as f32 + 0.5).floor() as usize) % SIDES;
                let (ox, oy) = ((x - own_rect.left) as u32, (y - own_rect.top) as u32);
                if ox >= own.width() || oy >= own.height() {
                    continue;
                }
                let a = f32::from(own.pixel(ox, oy)[3]);
                if d <= SOFT_DEPTH.1 {
                    softness[side] = (softness[side].0 + a, softness[side].1 + 1);
                } else if d >= SOFT_MIDDLE.0 && d <= SOFT_MIDDLE.1 {
                    middle[side] = (middle[side].0 + a, middle[side].1 + 1);
                } else if d >= SOFT_INSIDE.0 {
                    deeper[side] = (deeper[side].0 + a, deeper[side].1 + 1);
                }
                if d >= FRINGE.0
                    && d <= FRINGE.1
                    && let Some(j) = field.at(x, y)
                {
                    let v = m.grey().get(x as u32, y as u32);
                    let f = &mut fringes[side];
                    *f = (f.0 + v, f.1 + field.grey.values[j], f.2 + 1);
                }
            }
            let mut softest: Option<f32> = None;
            let mut strongest: Option<(f32, &'static str)> = None;
            for side in 0..SIDES {
                let (a, n) = softness[side];
                let (mid, mn) = middle[side];
                let (deep, dn) = deeper[side];
                let (pv, ov, fn_) = fringes[side];
                if n < 50 || mn < 50 || dn < 50 || fn_ < 100 {
                    continue;
                }
                let shallow = a / n as f32;
                let rises = mid / mn as f32 - shallow >= SOFT_RISE;
                let soft = if rises {
                    deep / dn as f32 - shallow
                } else {
                    0.0
                };
                softest = Some(softest.map_or(soft, |s: f32| s.max(soft)));
                if soft < SOFT_EDGE {
                    continue;
                }
                let bleed = pv / fn_ as f32 - ov / fn_ as f32;
                if strongest.is_none_or(|(b, _)| bleed > b) {
                    strongest = Some((bleed, SIDE_NAMES[side]));
                }
            }
            (softest, strongest)
        }
        _ => (None, None),
    };

    Some(Texture {
        name: part.name.clone(),
        share,
        own: part.own.is_some(),
        grain: median_of(&mut grains) / range,
        marks: marks_sum / n as f32 / range,
        bands,
        dirty_share,
        dirty: !light && !tint && dirty_share > DIRTY_SHARE,
        blotchy_share,
        blotchy: !light && !tint && blotchy_share > BLOTCHY_SHARE,
        coherence,
        coherences,
        stills,
        still,
        flat_share,
        // A layer is allowed big smooth areas (a sky, a wall); a named
        // part that's flat is a shape with nothing inside.
        flat: !light && !tint && !is_layer(part) && flat_share > FLAT_SHARE,
        squint_range,
        low_structure: !light && !tint && squint_range < LOW_STRUCTURE,
        light,
        tint,
        soft_edge,
        halo,
        light_fringe: !light && !tint && halo.is_some_and(|(h, _)| h >= HALO),
    })
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
            total += m.field.marks[i];
            pixels += 1;
            if close {
                near += m.field.marks[i];
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
            own: None,
        }
    }

    fn subject<'a>(image: &'a Image, focal: Option<(f32, f32)>, parts: Vec<Part>) -> Subject<'a> {
        Subject {
            image,
            focal,
            focal_part: None,
            key_colors: vec![],
            parts,
            layers: vec![],
        }
    }

    fn texture<'a>(c: &'a Critique, name: &str) -> &'a Texture {
        c.textures
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("no texture for {name}: {:?}", c.textures))
    }

    #[test]
    fn hues_have_names_and_distances() {
        assert_eq!(hue_name(0.0), "red");
        assert_eq!(hue_name(350.0), "red");
        assert_eq!(hue_name(120.0), "green");
        assert_eq!(hue_name(100.0), "yellow-green");
        assert_eq!(hue_distance(10.0, 350.0), 20.0);
        assert_eq!(hue_distance(0.0, 180.0), 180.0);
        assert!(
            in_key(300.0, &[("a".into(), 227.0), ("b".into(), 34.0)]),
            "between, the short way"
        );
        assert!(
            !in_key(120.0, &[("a".into(), 227.0), ("b".into(), 34.0)]),
            "the long way round"
        );
    }

    #[test]
    fn a_flat_grey_picture_is_flat_and_a_dark_disc_on_light_reads() {
        let image = Image::new(320, 200, [128, 128, 128, 255]);
        let flat = Critique::of(&subject(&image, None, vec![]));
        assert!(flat.values.flat);
        assert_eq!(flat.values.groups, [(2, 1.0)]);
        assert!(flat.lines().iter().any(|l| l.contains("looks flat")));
        assert_eq!(flat.colour.hues, 0);

        let mut image = Image::new(320, 200, [230, 230, 230, 255]);
        let mask = disc(&mut image, 200, 100, 40, [20, 20, 20, 255]);
        let mut s = subject(
            &image,
            Some((200.0, 100.0)),
            vec![part("disc", mask.clone())],
        );
        s.focal_part = Some(("disc".into(), mask.rect()));
        let c = Critique::of(&s);
        assert!(!c.values.flat);
        assert_eq!(c.values.groups.len(), 2, "{:?}", c.values.groups);
        let fc = c.values.focal_contrast.as_ref().unwrap();
        assert!(fc.near >= fc.elsewhere, "{fc:?}");
        assert!(
            c.lines()
                .iter()
                .any(|l| l.contains("the focal point has the strongest contrast"))
        );
        assert_eq!(c.focal.inside, Some(true));
        assert!(c.focal.distance.unwrap() < 60.0, "{:?}", c.focal);
        assert_eq!(c.focal.peak_part.as_deref(), Some("disc"));
        let read = &c.readability[0];
        assert!(!read.too_small && !read.merges, "{read:?}");
        let t = texture(&c, "disc");
        assert!(t.flat, "a solid disc is flat inside: {t:?}");
        assert!(!t.light, "a dark disc isn't a light");
        assert!(!t.light_fringe, "{t:?}");
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
    fn the_strongest_contrast_elsewhere_is_named() {
        // A faint focal disc and a hard black square far from it.
        let mut image = Image::new(400, 300, [200, 200, 200, 255]);
        let faint = disc(&mut image, 100, 150, 40, [170, 170, 170, 255]);
        let mut square = Mask::new(PixelRect::new(300, 100, 360, 160));
        for y in 100..160 {
            for x in 300..360 {
                image.set_pixel(x, y, [0, 0, 0, 255]);
                square.set(x, y, 255);
            }
        }
        let c = Critique::of(&subject(
            &image,
            Some((100.0, 150.0)),
            vec![part("faint", faint), part("square", square)],
        ));
        let fc = c.values.focal_contrast.as_ref().unwrap();
        assert!(fc.elsewhere > fc.near, "{fc:?}");
        assert_eq!(fc.elsewhere_part.as_deref(), Some("square"));
        assert!(
            c.lines()
                .iter()
                .any(|l| l.contains("the strongest contrast is elsewhere: square")),
            "{:?}",
            c.lines()
        );
    }

    #[test]
    fn a_green_blob_in_an_orange_key_is_out_of_key_and_noise_is_dirty() {
        let mut image = Image::new(256, 256, [230, 140, 60, 255]);
        let green = disc(&mut image, 64, 64, 30, [40, 200, 60, 255]);
        // A noisy disc: a checker of two greys, pixel by pixel.
        let noisy = disc(&mut image, 180, 180, 50, [120, 120, 120, 255]);
        for y in 130..=230 {
            for x in 130..=230 {
                if noisy.get(x, y) >= 128 {
                    let v = if (x + y) % 2 == 0 { 90 } else { 150 };
                    image.set_pixel(x, y, [v, v, v, 255]);
                }
            }
        }
        let mut s = subject(
            &image,
            Some((180.0, 180.0)),
            vec![part("leaf", green), part("stone", noisy)],
        );
        s.key_colors = vec![
            ("orange".into(), [230, 140, 60, 255]),
            ("ink".into(), [20, 20, 20, 255]),
        ];
        let c = Critique::of(&s);
        assert_eq!(c.colour.key_hues.len(), 1, "grey keys have no hue");
        assert_eq!(c.colour.out_of_key.len(), 1, "{:?}", c.colour.out_of_key);
        let blob = &c.colour.out_of_key[0];
        assert_eq!(hue_name(blob.hue), "green");
        assert_eq!(blob.part.as_deref(), Some("leaf"));
        assert!((blob.at.0 - 64.0).abs() < 20.0 && (blob.at.1 - 64.0).abs() < 20.0);
        let stone = texture(&c, "stone");
        assert!(stone.dirty, "{stone:?}");
        let leaf = texture(&c, "leaf");
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
    fn sparkles_are_marks_and_a_light_may_be_flat() {
        let mut image = Image::new(300, 300, [60, 60, 60, 255]);
        // A grey disc with a few tiny bright sparkles on it.
        let sparkly = disc(&mut image, 90, 150, 70, [110, 110, 110, 255]);
        for y in (90..210).step_by(20) {
            for x in (30..150).step_by(20) {
                if sparkly.get(x, y) >= 128 {
                    image.set_pixel(x, y, [255, 255, 255, 255]);
                    image.set_pixel(x + 1, y, [255, 255, 255, 255]);
                }
            }
        }
        // A solid bright disc on the dark ground: a light.
        let sun = disc(&mut image, 220, 150, 50, [250, 245, 220, 255]);
        let c = Critique::of(&subject(
            &image,
            None,
            vec![part("stone", sparkly), part("sun", sun)],
        ));
        let stone = texture(&c, "stone");
        assert!(!stone.dirty, "sparkles aren't grain: {stone:?}");
        let sun = texture(&c, "sun");
        assert!(sun.light, "{sun:?}");
        assert!(
            !sun.flat && !sun.low_structure,
            "a light may be flat: {sun:?}"
        );
        assert!(
            sun.flat_share > FLAT_SHARE,
            "it is flat inside, just not a finding"
        );
        assert!(
            c.detail_lines()
                .iter()
                .any(|l| l.starts_with("sun:") && l.ends_with("a light"))
        );
    }

    #[test]
    fn strokes_have_a_direction_and_blotches_dont() {
        let mut image = Image::new(400, 200, [128, 128, 128, 255]);
        // Vertical stripes: marks with one direction.
        let striped = disc(&mut image, 100, 100, 80, [128, 128, 128, 255]);
        for y in 20..=180 {
            for x in 20..=180 {
                if striped.get(x, y) >= 128 {
                    let v = if (x / 12) % 2 == 0 { 100 } else { 156 };
                    image.set_pixel(x, y, [v, v, v, 255]);
                }
            }
        }
        // Blotches: discs of 5 px at pseudo-random places, no direction.
        let blotchy = disc(&mut image, 300, 100, 80, [128, 128, 128, 255]);
        let mut seed = 12345u32;
        for _ in 0..700 {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let x = 220 + (seed >> 16) as i32 % 160;
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let y = 20 + (seed >> 16) as i32 % 160;
            // Soft blobs: a value that fades out over 6 px, like a noise fill.
            let up = (seed >> 8).is_multiple_of(2);
            for dy in -6..=6 {
                for dx in -6..=6 {
                    let d = ((dx * dx + dy * dy) as f32).sqrt();
                    if d <= 6.0 && blotchy.get(x + dx, y + dy) >= 128 {
                        let was = image.pixel((x + dx) as u32, (y + dy) as u32)[0] as f32;
                        let push = 28.0 * (1.0 - d / 6.0);
                        let v = (if up { was + push } else { was - push }).clamp(0.0, 255.0) as u8;
                        image.set_pixel(x + dx, y + dy, [v, v, v, 255]);
                    }
                }
            }
        }
        let c = Critique::of(&subject(
            &image,
            None,
            vec![part("stripes", striped), part("blotches", blotchy)],
        ));
        let stripes = texture(&c, "stripes");
        let blotches = texture(&c, "blotches");
        assert!(stripes.coherence.unwrap() > 0.7, "{stripes:?}");
        assert!(stripes.stills[0].unwrap() > BLOTCH_STILL, "{stripes:?}");
        assert!(!stripes.blotchy, "{stripes:?}");
        assert!(
            blotches.coherence.unwrap() < BLOTCH_COHERENCE,
            "{blotches:?}"
        );
        assert!(blotches.still.unwrap() < BLOTCH_STILL, "{blotches:?}");
        assert!(blotches.blotchy, "{blotches:?}");
        assert!(!stripes.dirty && !blotches.dirty, "neither is fine grain");
        assert!(
            c.lines()
                .iter()
                .any(|l| l.contains("blotches is blotchy inside"))
        );
    }

    #[test]
    fn a_soft_edge_with_a_lighter_background_showing_through_is_a_halo() {
        // A dark hill on a light sky, its own paint fading out over the
        // last 6 px of its silhouette (a blur on the silhouette).
        let (w, h) = (300u32, 300u32);
        let mut image = Image::new(w, h, [220, 220, 220, 255]);
        let mut own = Image::new(w, h, [0, 0, 0, 0]);
        let mut mask = Mask::new(PixelRect::new(0, 0, w as i32, h as i32));
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let d = (((x - 150i32).pow(2) + (y - 150i32).pow(2)) as f32).sqrt();
                if d > 100.0 {
                    continue;
                }
                let a = ((100.0 - d) / 8.0).clamp(0.0, 1.0);
                let alpha = (a * 255.0).round() as u8;
                own.set_pixel(x, y, [60, 60, 60, alpha]);
                let v = (60.0 * a + 220.0 * (1.0 - a)).round() as u8;
                image.set_pixel(x, y, [v, v, v, 255]);
                if alpha >= 128 {
                    mask.set(x, y, 255);
                }
            }
        }
        let hill = Part {
            name: "hill".into(),
            mask: mask.clone(),
            area: mask.rect(),
            own: Some(own),
        };
        let c = Critique::of(&subject(&image, None, vec![hill]));
        let t = texture(&c, "hill");
        assert!(t.soft_edge.unwrap() >= SOFT_EDGE, "{t:?}");
        assert!(t.light_fringe, "{t:?}");
        assert!(
            c.lines()
                .iter()
                .any(|l| l.contains("hill's") && l.contains("edge is soft"))
        );
        // Crisp: the same hill with a hard edge, and a rim light painted
        // in its own paint on one side: no halo.
        let mut image = Image::new(w, h, [220, 220, 220, 255]);
        let crisp = disc(&mut image, 150, 150, 100, [60, 60, 60, 255]);
        for y in 50..=250 {
            for x in 150..=250 {
                let d = (((x - 150i32).pow(2) + (y - 150i32).pow(2)) as f32).sqrt();
                if crisp.get(x, y) >= 128 && d > 93.0 {
                    image.set_pixel(x, y, [160, 160, 160, 255]);
                }
            }
        }
        let own = image.clone();
        let rim = Part {
            name: "hill".into(),
            mask: crisp.clone(),
            area: crisp.rect(),
            own: Some(own),
        };
        let c = Critique::of(&subject(&image, None, vec![rim]));
        let t = texture(&c, "hill");
        assert!(t.soft_edge.unwrap() < SOFT_EDGE, "{t:?}");
        assert!(!t.light_fringe, "{t:?}");
    }

    #[test]
    fn a_layers_own_paint_is_judged_without_what_is_behind_it() {
        // A sky gradient, and a cloud of one colour whose alpha fades:
        // on the picture it spans the gradient's values; on its own it
        // has no light and dark side.
        let mut image = Image::new(320, 200, [0, 0, 0, 255]);
        for y in 0..200 {
            let v = (30 + y) as u8;
            for x in 0..320 {
                image.set_pixel(x, y, [v, v, v, 255]);
            }
        }
        let mut own = Image::new(320, 200, [0, 0, 0, 0]);
        let mut mask = Mask::new(PixelRect::new(0, 0, 320, 200));
        for y in 20..180 {
            for x in 20..300 {
                // Opaque inside, fading over the last 20 px of its edge.
                let edge = (y - 20).min(179 - y).min(x - 20).min(299 - x);
                let a = (255 - (20 - edge.min(20)) * 12).clamp(0, 255) as u8;
                own.set_pixel(x, y, [200, 200, 200, a]);
                let [r, g, b, _] = image.pixel(x as u32, y as u32);
                let mix = |c: u8| {
                    ((u32::from(c) * (255 - u32::from(a)) + 200 * u32::from(a)) / 255) as u8
                };
                image.set_pixel(x, y, [mix(r), mix(g), mix(b), 255]);
                mask.set(x, y, 255);
            }
        }
        let cloud = Part {
            name: "layer \"Clouds\"".into(),
            mask: mask.clone(),
            area: mask.rect(),
            own: Some(own),
        };
        let mut s = subject(&image, None, vec![]);
        s.layers = vec![cloud];
        let c = Critique::of(&s);
        let t = texture(&c, "layer \"Clouds\"");
        assert!(t.own);
        assert!(t.low_structure, "{t:?}");
        assert!(
            c.lines()
                .iter()
                .any(|l| l.contains("layer \"Clouds\" has no light and dark side"))
        );
        // On the picture, the gradient behind it would have hidden that.
        let seen = Critique::of(&subject(&image, None, vec![part("cloud", mask)]));
        assert!(!texture(&seen, "cloud").low_structure);
    }

    #[test]
    fn a_part_that_merges_or_is_tiny_is_reported() {
        let mut image = Image::new(400, 300, [128, 128, 128, 255]);
        let faint = disc(&mut image, 100, 150, 40, [140, 140, 140, 255]);
        let tiny = disc(&mut image, 300, 150, 2, [0, 0, 0, 255]);
        let mut s = subject(
            &image,
            Some((200.0, 150.0)),
            vec![part("faint", faint), part("tiny", tiny.clone())],
        );
        s.focal_part = Some(("tiny".into(), tiny.rect()));
        let c = Critique::of(&s);
        let faint = c.readability.iter().find(|r| r.name == "faint").unwrap();
        assert!(faint.merges, "{faint:?}");
        let tiny = c.readability.iter().find(|r| r.name == "tiny").unwrap();
        assert!(tiny.too_small, "{tiny:?}");
        assert!(c.composition.dead_centre);
        let same = Critique::of(&s);
        assert_eq!(c, same, "deterministic");
    }
}

/// The senses against reality: the reference pictures (good paintings
/// and photographs) should mostly pass.
#[cfg(test)]
mod reference_tests {
    use super::*;

    thread_local! {
        /// Each reference picture's texture as one whole part.
        static WHOLE: std::cell::RefCell<Vec<(String, Texture)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

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
            layers: vec![],
        };
        let c = Critique::of(&subject);
        assert_eq!(c, Critique::of(&subject), "{file}: deterministic");
        // The whole picture as one part: its texture numbers.
        let mut mask = Mask::new(PixelRect::new(
            0,
            0,
            image.width() as i32,
            image.height() as i32,
        ));
        for y in 0..image.height() as i32 {
            for x in 0..image.width() as i32 {
                mask.set(x, y, 255);
            }
        }
        let whole = Subject {
            parts: vec![Part {
                name: "whole".into(),
                area: mask.rect(),
                mask,
                own: None,
            }],
            ..subject.clone()
        };
        let t = Critique::of(&whole);
        for line in t.detail_lines().iter().take(1) {
            eprintln!("{file}: {line}");
        }
        WHOLE.with(|w| {
            w.borrow_mut()
                .push((file.to_owned(), t.textures[0].clone()))
        });
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
        // Texture, the whole picture as one part: no painting or
        // photograph here is noisy, and the paintings of smooth form
        // (Vermeer, Friedrich) and the rocket at dusk aren't blotchy. A
        // whole picture is not a part, and at the biggest band (24 to
        // 96 px) Van Gogh's curving strokes, Hokusai's lines everywhere
        // and the photographs of soft things (a face, fur, a cup) read
        // as soft blobs with no one direction: a known limit, recorded
        // in decisions.md, that the painter overrules.
        WHOLE.with(|w| {
            for (file, t) in w.borrow().iter() {
                assert!(!t.dirty, "{file} is called noisy: {t:?}");
                if [
                    "vermeer-girl-with-a-pearl-earring.jpg",
                    "friedrich-wanderer.jpg",
                    "rocket.jpg",
                ]
                .contains(&file.as_str())
                {
                    assert!(!t.blotchy, "{file} is called blotchy: {t:?}");
                }
                if file == "van-gogh-starry-night.jpg" {
                    assert!(
                        t.blotchy,
                        "{file}: the known limit no longer shows; update decisions.md"
                    );
                }
            }
        });
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
