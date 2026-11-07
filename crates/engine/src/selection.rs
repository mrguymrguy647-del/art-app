//! Selections: the part of the picture that painting is limited to.
//!
//! A selection is a [`Mask`]: for each pixel, how much of it is selected
//! (0 to 255), so its edges can be smooth or feathered. While there is a
//! selection, every brush and eraser stroke, fill, gradient and filter
//! changes only the pixels inside it, as much as each is selected: the
//! same mask a script's group `mask` uses, so the app and scripts can't
//! behave differently.
//!
//! Selections belong to the document but aren't saved in the file (like
//! in other painting apps). Changing one is an undo step.

use std::sync::Arc;

use crate::document::Document;
use crate::fill::Mask;
use crate::history::Command;
use crate::image::Image;
use crate::layer::LayerId;
use crate::script::SelectCmd;
use crate::script::geometry::{self, Pt};
use crate::stroke::TilesBefore;
use crate::surface::{PixelRect, Surface};
use crate::tile::{Rgba8, TILE_SIZE};

/// How a new shape changes the selection there is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Combine {
    /// The shape becomes the selection.
    #[default]
    Replace,
    /// The shape is added to it (Shift in the app).
    Add,
    /// The shape is taken away from it (Alt in the app).
    Subtract,
}

/// What's selected. Always selects at least one pixel: an empty selection
/// is no selection (`None`), which means everything can be painted.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    /// Cropped to what's selected, so a small selection on a big canvas
    /// stays small. Shared (`Arc`) so handing it to a stroke is free.
    mask: Arc<Mask>,
}

impl Selection {
    /// A selection from `mask`, or `None` if it selects nothing.
    pub fn new(mask: Mask) -> Option<Selection> {
        let bounds = mask.content_bounds();
        if bounds.is_empty() {
            return None;
        }
        let mask = if bounds == mask.rect() {
            mask
        } else {
            mask.cropped(bounds)
        };
        Some(Selection {
            mask: Arc::new(mask),
        })
    }

    /// The mask painting is clipped to.
    pub fn mask(&self) -> &Arc<Mask> {
        &self.mask
    }

    /// The box around what's selected.
    pub fn bounds(&self) -> PixelRect {
        self.mask.rect()
    }

    /// Combines `shape` with `current` (the selection there is, if any).
    pub fn combine(current: Option<&Selection>, shape: Mask, how: Combine) -> Option<Selection> {
        match (how, current) {
            (Combine::Replace, _) | (Combine::Add, None) => Selection::new(shape),
            (Combine::Add, Some(current)) => Selection::new(current.mask.union(&shape)),
            // Taking away from nothing leaves nothing.
            (Combine::Subtract, None) => None,
            (Combine::Subtract, Some(current)) => Selection::new(current.mask.subtract(&shape)),
        }
    }

    /// Everything on the canvas that isn't selected. Inverting nothing
    /// selects everything.
    pub fn inverted(current: Option<&Selection>, limits: PixelRect) -> Option<Selection> {
        match current {
            None => Selection::new(all(limits)),
            Some(current) => Selection::new(current.mask.inverted(limits)),
        }
    }

    /// The same selection with its edge softened over `radius` pixels.
    pub fn feathered(&self, radius: f32, limits: PixelRect) -> Option<Selection> {
        Selection::new(self.mask.feathered(radius, limits))
    }

    /// The selection's edge as closed paths (corner points in canvas
    /// pixels), for drawing "marching ants": one path per ring, so the
    /// dashes run on smoothly around curves. A pixel counts as selected
    /// from half selected up, and the edge runs along pixel sides.
    pub fn outline(&self) -> Vec<Vec<(f32, f32)>> {
        // Join the straight pieces end to end into rings. Corners are on
        // whole pixels, so they can be matched exactly.
        use std::collections::HashMap;
        let pieces = self.edge_pieces();
        let key = |p: (f32, f32)| (p.0 as i32, p.1 as i32);
        let mut at: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, [a, b]) in pieces.iter().enumerate() {
            at.entry(key(*a)).or_default().push(i);
            at.entry(key(*b)).or_default().push(i);
        }
        let mut used = vec![false; pieces.len()];
        let mut rings = Vec::new();
        for start in 0..pieces.len() {
            if used[start] {
                continue;
            }
            used[start] = true;
            let [first, mut end] = pieces[start];
            let mut ring = vec![first, end];
            // Follow unused pieces from the end until the ring closes.
            while let Some(&next) = at
                .get(&key(end))
                .and_then(|list| list.iter().find(|&&i| !used[i]))
            {
                used[next] = true;
                let [a, b] = pieces[next];
                end = if key(a) == key(end) { b } else { a };
                if key(end) == key(first) {
                    break;
                }
                ring.push(end);
            }
            rings.push(ring);
        }
        rings
    }

    /// The edge as straight pieces along pixel sides; pieces in a row along
    /// the same line are joined.
    fn edge_pieces(&self) -> Vec<[(f32, f32); 2]> {
        let mask = &self.mask;
        let r = mask.rect();
        let inside = |x: i32, y: i32| mask.get(x, y) >= 128;
        let mut pieces = Vec::new();
        // Horizontal edges: between row y-1 and row y.
        for y in r.top..=r.bottom {
            let mut start: Option<i32> = None;
            for x in r.left..=r.right {
                let edge = x < r.right && inside(x, y - 1) != inside(x, y);
                match (edge, start) {
                    (true, None) => start = Some(x),
                    (false, Some(from)) => {
                        pieces.push([(from as f32, y as f32), (x as f32, y as f32)]);
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        // Vertical edges: between column x-1 and column x.
        for x in r.left..=r.right {
            let mut start: Option<i32> = None;
            for y in r.top..=r.bottom {
                let edge = y < r.bottom && inside(x - 1, y) != inside(x, y);
                match (edge, start) {
                    (true, None) => start = Some(y),
                    (false, Some(from)) => {
                        pieces.push([(x as f32, from as f32), (x as f32, y as f32)]);
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        pieces
    }
}

// ---- The select command ----------------------------------------------------

/// What a `select` command asks for (checked), in the app or a script.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub what: What,
    pub how: Combine,
    /// Pixels the edge fades over (0: a sharp edge).
    pub feather: f32,
}

/// The one thing a `select` names.
#[derive(Clone, Debug, PartialEq)]
pub enum What {
    /// "rect", "ellipse", "circle" or "polygon", with its numbers as written.
    Shape(&'static str, serde_json::Value),
    Layer(String),
    All,
    None,
    Invert,
    /// `feather` on its own: softens the selection there is.
    Feather,
}

impl Request {
    /// Checks a `select` command: one thing at a time, a known `mode`, a
    /// feather of 0 or more. The messages say what to do instead.
    pub fn read(command: &SelectCmd) -> Result<Request, String> {
        let how = match command.mode.as_deref() {
            None | Some("replace") => Combine::Replace,
            Some("add") => Combine::Add,
            Some("subtract") => Combine::Subtract,
            Some(other) => {
                return Err(format!(
                    "`mode` is \"replace\", \"add\" or \"subtract\", not \"{other}\""
                ));
            }
        };
        let shapes = [
            ("rect", &command.rect),
            ("ellipse", &command.ellipse),
            ("circle", &command.circle),
            ("polygon", &command.polygon),
        ];
        let mut found: Vec<(&str, What)> = shapes
            .iter()
            .filter_map(|(kind, value)| {
                value
                    .as_ref()
                    .map(|v| (*kind, What::Shape(kind, v.clone())))
            })
            .collect();
        if let Some(name) = &command.layer {
            found.push(("layer", What::Layer(name.clone())));
        }
        for (flag, name, what) in [
            (command.all, "all", What::All),
            (command.none, "none", What::None),
            (command.invert, "invert", What::Invert),
        ] {
            if flag == Some(true) {
                found.push((name, what));
            }
        }
        if found.len() > 1 {
            let names: Vec<&str> = found.iter().map(|(n, _)| *n).collect();
            return Err(format!(
                "one at a time: `{}` (use two `select` commands, with \"mode\": \"add\")",
                names.join("`, `")
            ));
        }
        let feather = match command.feather {
            Some(f) if f < 0.0 || !f.is_finite() => {
                return Err("`feather` is a number of pixels, 0 or more".into());
            }
            Some(f) => f,
            None => 0.0,
        };
        let what = match found.pop() {
            Some((_, what)) => what,
            None if command.feather.is_some() => What::Feather,
            None => {
                return Err(
                    "`select` needs a shape (`rect`, `ellipse`, `circle`, `polygon`), \
                            `layer`, `all`, `none`, `invert` or `feather`"
                        .into(),
                );
            }
        };
        match what {
            What::None if command.mode.is_some() || command.feather.is_some() => {
                Err("`none` drops the selection: leave out `mode` and `feather`".into())
            }
            What::Invert if command.mode.is_some() => Err("`invert` takes no `mode`".into()),
            what => Ok(Request { what, how, feather }),
        }
    }
}

/// The outline of a shape written in canvas pixels, like an area:
/// "rect" [x, y, w, h], "ellipse" [cx, cy, rx, ry], "circle" [cx, cy, r],
/// "polygon" [[x, y], ...]. The script player makes its own (it also
/// takes parameters and places shapes inside groups) with the same
/// geometry, so the app and a script get the same pixels.
pub fn shape_outline(kind: &str, numbers: &serde_json::Value) -> Result<Vec<Pt>, String> {
    let help = "a shape is \"rect\": [x, y, width, height], \"ellipse\": [cx, cy, rx, ry], \
                \"circle\": [cx, cy, r] or \"polygon\": [[x, y], ...]";
    let flat = |n: usize| -> Result<Vec<f32>, String> {
        let list = numbers.as_array().filter(|l| l.len() == n).ok_or(help)?;
        list.iter()
            .map(|v| v.as_f64().map(|f| f as f32).ok_or_else(|| help.to_owned()))
            .collect()
    };
    let outline = match kind {
        "rect" => {
            let v = flat(4)?;
            geometry::rect_outline(v[0], v[1], v[2], v[3], 0.0, 0.0, 1.0)
        }
        "ellipse" => {
            let v = flat(4)?;
            geometry::ellipse_outline((v[0], v[1]), v[2], v[3], 0.0, 1.0)
        }
        "circle" => {
            let v = flat(3)?;
            geometry::ellipse_outline((v[0], v[1]), v[2], v[2], 0.0, 1.0)
        }
        "polygon" => {
            let points: Vec<[f32; 2]> =
                serde_json::from_value(numbers.clone()).map_err(|_| help.to_owned())?;
            points.into_iter().map(|[x, y]| (x, y)).collect()
        }
        _ => return Err(help.into()),
    };
    if outline.len() < 3 {
        return Err("a shape needs at least 3 points".into());
    }
    Ok(outline)
}

/// The selection after a request, given the selection there is and the
/// request's shape (or layer, or the whole canvas) as a mask. `mask` is
/// ignored for `None`, `Invert` and `Feather`.
pub fn apply(
    current: Option<&Selection>,
    request: &Request,
    mask: Option<Mask>,
    limits: PixelRect,
) -> Option<Selection> {
    let feather = request.feather;
    let soften = |mask: Mask| {
        if feather > 0.0 {
            mask.feathered(feather, limits)
        } else {
            mask
        }
    };
    match &request.what {
        What::None => None,
        What::Invert => {
            let inverted = Selection::inverted(current, limits)?;
            if feather > 0.0 {
                inverted.feathered(feather, limits)
            } else {
                Some(inverted)
            }
        }
        What::Feather => current.and_then(|c| c.feathered(feather, limits)),
        What::All | What::Layer(_) | What::Shape(..) => {
            let mask = mask.unwrap_or_else(|| Mask::new(PixelRect::new(0, 0, 0, 0)));
            Selection::combine(current, soften(mask), request.how)
        }
    }
}

/// Runs a `select` command written in canvas pixels (as the app writes
/// them) on `document`: the selection it leaves. The app does every
/// selection change this way, so Export strokes replays exactly what was
/// selected.
pub fn run(command: &SelectCmd, document: &Document) -> Result<Option<Selection>, String> {
    let request = Request::read(command)?;
    let limits = document.bounds();
    let mask = match &request.what {
        What::Shape(kind, numbers) => Some(polygon(&shape_outline(kind, numbers)?, limits)),
        What::Layer(name) => {
            let layer = document
                .layers()
                .iter()
                .find(|l| l.name() == name)
                .ok_or_else(|| format!("there's no layer called \"{name}\""))?;
            Some(layer_contents(&layer.surface, limits))
        }
        What::All => Some(all(limits)),
        What::None | What::Invert | What::Feather => None,
    };
    if request.what == What::Feather && document.selection().is_none() {
        return Err("`feather` alone softens the selection, but there is none yet".into());
    }
    Ok(apply(document.selection(), &request, mask, limits))
}

// ---- Shapes ----------------------------------------------------------------

/// The whole canvas.
pub fn all(limits: PixelRect) -> Mask {
    let mut mask = Mask::new(limits);
    for y in limits.top..limits.bottom {
        for x in limits.left..limits.right {
            mask.set(x, y, 255);
        }
    }
    mask
}

/// A rectangle from its top-left corner and size, in canvas pixels.
pub fn rect(left: f32, top: f32, width: f32, height: f32, limits: PixelRect) -> Mask {
    let (left, width) = if width < 0.0 {
        (left + width, -width)
    } else {
        (left, width)
    };
    let (top, height) = if height < 0.0 {
        (top + height, -height)
    } else {
        (top, height)
    };
    polygon(
        &[
            (left, top),
            (left + width, top),
            (left + width, top + height),
            (left, top + height),
        ],
        limits,
    )
}

/// An ellipse from its centre and its two radii.
pub fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32, limits: PixelRect) -> Mask {
    // Detail 1: points close enough together that the edge looks round
    // at full size.
    polygon(
        &geometry::ellipse_outline((cx, cy), rx.abs(), ry.abs(), 0.0, 1.0),
        limits,
    )
}

/// A polygon (a lasso): its corners in order. Anti-aliased.
pub fn polygon(points: &[(f32, f32)], limits: PixelRect) -> Mask {
    if points.len() < 3 {
        return Mask::new(PixelRect::new(0, 0, 0, 0));
    }
    geometry::polygons_mask(&[points], limits)
}

/// Where a layer has paint, as much as each pixel is opaque.
pub fn layer_contents(surface: &Surface, limits: PixelRect) -> Mask {
    match surface.painted_area() {
        Some(area) => Mask::from_alpha(surface, area.intersect(&limits)),
        None => Mask::new(PixelRect::new(0, 0, 0, 0)),
    }
}

// ---- Using the selection ---------------------------------------------------

/// What Copy takes: the selected part of a layer, as a picture, and where
/// its top-left corner was on the canvas (so Paste puts it back there).
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub image: Image,
    pub left: i32,
    pub top: i32,
}

/// The selected part of `surface`: each pixel as much as it's selected.
/// `None` if nothing selected is painted.
pub fn copy(surface: &Surface, selection: &Selection) -> Option<Clip> {
    let mask = selection.mask();
    let area = surface.painted_area()?.intersect(&mask.rect());
    if area.is_empty() {
        return None;
    }
    let mut image = Image::new(
        (area.right - area.left) as u32,
        (area.bottom - area.top) as u32,
        [0, 0, 0, 0],
    );
    let mut anything = false;
    for coord in area.tiles() {
        let Some(tile) = surface.tile(coord) else {
            continue;
        };
        let (left, top) = coord.origin();
        let size = TILE_SIZE as i32;
        let part = area.intersect(&PixelRect::new(left, top, left + size, top + size));
        for y in part.top..part.bottom {
            for x in part.left..part.right {
                let amount = u32::from(mask.get(x, y));
                if amount == 0 {
                    continue;
                }
                let p = tile.pixel((x - left) as usize, (y - top) as usize);
                // Premultiplied, so every channel scales the same way.
                let scale = |c: u8| ((u32::from(c) * amount + 127) / 255) as u8;
                let pixel = [scale(p.r), scale(p.g), scale(p.b), scale(p.a)];
                if pixel[3] > 0 {
                    anything = true;
                }
                image.set_pixel(x - area.left, y - area.top, pixel);
            }
        }
    }
    anything.then_some(Clip {
        image,
        left: area.left,
        top: area.top,
    })
}

/// Lays `clip` onto an empty `surface` at its place (for Paste, onto a new
/// layer). Pixels off the canvas (`limits`) are left out.
pub fn paste_into(surface: &mut Surface, clip: &Clip, limits: PixelRect) -> TilesBefore {
    let mut before = TilesBefore::new();
    let area = PixelRect::new(
        clip.left,
        clip.top,
        clip.left + clip.image.width() as i32,
        clip.top + clip.image.height() as i32,
    )
    .intersect(&limits);
    for coord in area.tiles() {
        let (left, top) = coord.origin();
        let size = TILE_SIZE as i32;
        let part = area.intersect(&PixelRect::new(left, top, left + size, top + size));
        let mut tile_changed = false;
        for y in part.top..part.bottom {
            for x in part.left..part.right {
                let [r, g, b, a] = clip
                    .image
                    .pixel((x - clip.left) as u32, (y - clip.top) as u32);
                if a == 0 {
                    continue;
                }
                if !tile_changed {
                    before
                        .entry(coord)
                        .or_insert_with(|| surface.tile(coord).cloned());
                    tile_changed = true;
                }
                surface.tile_mut(coord).set_pixel(
                    (x - left) as usize,
                    (y - top) as usize,
                    Rgba8 { r, g, b, a },
                );
            }
        }
    }
    before
}

/// Lays everything on layer `from` over layer `onto` (normal blending,
/// full strength), as a selection that was moved is put down. Returns
/// `onto`'s tiles as they were. `from` is left as it is.
pub fn drop_onto(document: &mut Document, from: LayerId, onto: LayerId) -> TilesBefore {
    use crate::layer::{BlendMode, blend_over};
    let mut before = TilesBefore::new();
    let Some(source) = document.layer(from).map(|l| l.surface.clone()) else {
        return before;
    };
    let Some(target) = document.layer_mut(onto) else {
        return before;
    };
    let Some(area) = source.painted_area() else {
        return before;
    };
    for coord in area.tiles() {
        let Some(tile) = source.tile(coord) else {
            continue;
        };
        before
            .entry(coord)
            .or_insert_with(|| target.surface.tile(coord).cloned());
        let dest = target.surface.tile_mut(coord);
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let top = tile.pixel(x, y);
                if top.a == 0 {
                    continue;
                }
                let under = dest.pixel(x, y);
                dest.set_pixel(x, y, blend_over(under, top, 1.0, BlendMode::Normal));
            }
        }
    }
    before
}

/// `selection` moved by `transform` (as the transform tool moves a
/// layer), on a canvas `limits` big. `None` if it ends up off the canvas.
pub fn transformed(
    selection: &Selection,
    transform: crate::transform::Transform,
    limits: PixelRect,
) -> Option<Selection> {
    // The mask as a layer's opacity, moved with the same code as pixels.
    let mut scratch = Document::new(limits.right as u32, limits.bottom as u32);
    let id = scratch.active_id();
    let mask = selection.mask();
    let r = mask.rect();
    {
        let surface = &mut scratch.active_layer_mut().surface;
        for y in r.top..r.bottom {
            for x in r.left..r.right {
                let v = mask.get(x, y);
                if v > 0 {
                    surface.set_pixel(
                        x,
                        y,
                        Rgba8 {
                            r: v,
                            g: v,
                            b: v,
                            a: v,
                        },
                    );
                }
            }
        }
    }
    crate::layer_transform::transform_layer(&mut scratch, id, transform);
    Selection::new(layer_contents(&scratch.active_layer().surface, limits))
}

// ---- Undo --------------------------------------------------------------------

/// A change of selection, for undo: it keeps the other selection, like
/// `TileCommand` keeps the other tiles.
pub struct SelectionCommand {
    name: String,
    other: Option<Selection>,
}

impl SelectionCommand {
    /// Makes `new` the document's selection and returns the command that
    /// undoes it. `name` is what Undo calls it ("Select", "Deselect"...).
    pub fn set(document: &mut Document, new: Option<Selection>, name: &str) -> SelectionCommand {
        let other = document.set_selection(new);
        SelectionCommand {
            name: name.to_owned(),
            other,
        }
    }

    fn swap(&mut self, document: &mut Document) {
        let other = self.other.take();
        self.other = document.set_selection(other);
    }
}

impl Command for SelectionCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn undo(&mut self, document: &mut Document) {
        self.swap(document);
    }

    fn redo(&mut self, document: &mut Document) {
        self.swap(document);
    }

    fn memory_size(&self) -> usize {
        self.other.as_ref().map_or(0, |s| {
            let r = s.bounds();
            ((r.right - r.left) * (r.bottom - r.top)) as usize
        })
    }
}

/// The layer `id`'s pixels inside the selection, wiped (Delete).
/// Returns the tiles as they were, for undo.
pub fn delete_inside(document: &mut Document, id: LayerId, selection: &Selection) -> TilesBefore {
    use crate::brush::BrushMode;
    use crate::fill::{Paint, paint_mask};
    let limits = document.bounds();
    match document.layer_mut(id) {
        Some(layer) => paint_mask(
            &mut layer.surface,
            selection.mask(),
            &Paint::Color([0, 0, 0, 255]),
            1.0,
            BrushMode::Erase,
            limits,
        ),
        None => TilesBefore::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::{Paint, paint_mask};
    use crate::history::History;

    fn canvas() -> PixelRect {
        PixelRect::new(0, 0, 200, 100)
    }

    #[test]
    fn shapes_select_their_inside_with_soft_edges() {
        let selection = Selection::new(rect(10.0, 10.0, 50.0, 30.0, canvas())).unwrap();
        assert_eq!(selection.bounds(), PixelRect::new(10, 10, 60, 40));
        assert_eq!(selection.mask().get(30, 20), 255);
        assert_eq!(selection.mask().get(5, 5), 0);
        let round = ellipse(100.0, 50.0, 30.4, 20.4, canvas());
        let edge = (0..200)
            .map(|x| round.get(x, 50))
            .filter(|&v| v > 0 && v < 255)
            .count();
        assert!(edge >= 1, "an ellipse's edge is smooth");
        // A rectangle dragged backwards is the same rectangle.
        assert_eq!(
            rect(60.0, 40.0, -50.0, -30.0, canvas()),
            rect(10.0, 10.0, 50.0, 30.0, canvas())
        );
    }

    #[test]
    fn add_subtract_and_invert() {
        let a = Selection::new(rect(0.0, 0.0, 50.0, 50.0, canvas()));
        let both = Selection::combine(
            a.as_ref(),
            rect(100.0, 0.0, 50.0, 50.0, canvas()),
            Combine::Add,
        )
        .unwrap();
        assert_eq!(both.mask().get(25, 25), 255);
        assert_eq!(both.mask().get(125, 25), 255);
        assert_eq!(both.mask().get(75, 25), 0);
        let holed = Selection::combine(
            Some(&both),
            rect(10.0, 10.0, 10.0, 10.0, canvas()),
            Combine::Subtract,
        )
        .unwrap();
        assert_eq!(holed.mask().get(15, 15), 0);
        assert_eq!(holed.mask().get(5, 5), 255);
        // Subtracting everything leaves no selection.
        assert!(Selection::combine(Some(&both), all(canvas()), Combine::Subtract).is_none());
        let inverted = Selection::inverted(Some(&both), canvas()).unwrap();
        assert_eq!(inverted.mask().get(75, 25), 255);
        assert_eq!(inverted.mask().get(25, 25), 0);
        assert_eq!(
            Selection::inverted(None, canvas()).unwrap().bounds(),
            canvas()
        );
    }

    #[test]
    fn feathering_softens_the_edge_and_keeps_the_middle() {
        let hard = Selection::new(rect(50.0, 20.0, 100.0, 60.0, canvas())).unwrap();
        let soft = hard.feathered(9.0, canvas()).unwrap();
        assert_eq!(soft.mask().get(100, 50), 255, "the middle stays selected");
        let edge = soft.mask().get(50, 50);
        assert!(
            edge > 60 && edge < 200,
            "half way on the old edge, got {edge}"
        );
        assert!(soft.mask().get(44, 50) > 0, "the feather reaches outside");
        assert!(soft.bounds().left < 50);
    }

    #[test]
    fn the_outline_runs_around_the_selection() {
        let selection = Selection::new(rect(10.0, 10.0, 20.0, 5.0, canvas())).unwrap();
        let rings = selection.outline();
        // A rectangle: one ring with its four corners.
        assert_eq!(rings.len(), 1, "{rings:?}");
        assert_eq!(rings[0].len(), 4, "{rings:?}");
        // A ring with a hole: two rings. An ellipse: one long ring.
        let holed = Selection::combine(
            Some(&selection),
            rect(15.0, 11.0, 5.0, 2.0, canvas()),
            Combine::Subtract,
        )
        .unwrap();
        assert_eq!(holed.outline().len(), 2);
        let round = Selection::new(ellipse(100.0, 50.0, 30.0, 20.0, canvas())).unwrap();
        let rings = round.outline();
        assert_eq!(rings.len(), 1);
        assert!(rings[0].len() > 40, "follows the curve step by step");
    }

    #[test]
    fn copy_takes_the_selected_pixels_and_paste_puts_them_back() {
        let mut surface = Surface::new();
        let red = Paint::Color([255, 0, 0, 255]);
        let brush = crate::brush::BrushMode::Paint;
        paint_mask(&mut surface, &all(canvas()), &red, 1.0, brush, canvas());
        let selection = Selection::new(rect(20.0, 30.0, 10.0, 5.0, canvas())).unwrap();
        let clip = copy(&surface, &selection).unwrap();
        assert_eq!((clip.left, clip.top), (20, 30));
        assert_eq!((clip.image.width(), clip.image.height()), (10, 5));
        assert_eq!(clip.image.pixel(0, 0), [255, 0, 0, 255]);
        let mut empty = Surface::new();
        let before = paste_into(&mut empty, &clip, canvas());
        assert!(!before.is_empty());
        assert_eq!(empty.pixel(25, 32).a, 255);
        assert_eq!(empty.pixel(19, 32).a, 0);
        assert_eq!(empty.painted_area(), Some(PixelRect::new(20, 30, 30, 35)));
    }

    #[test]
    fn selection_changes_undo_and_redo() {
        let mut document = Document::new(200, 100);
        let mut history = History::default();
        let first = Selection::new(rect(0.0, 0.0, 10.0, 10.0, canvas()));
        history.push(Box::new(SelectionCommand::set(
            &mut document,
            first.clone(),
            "Select",
        )));
        history.push(Box::new(SelectionCommand::set(
            &mut document,
            None,
            "Deselect",
        )));
        assert!(document.selection().is_none());
        history.undo(&mut document);
        assert_eq!(document.selection(), first.as_ref());
        history.undo(&mut document);
        assert!(document.selection().is_none());
        history.redo(&mut document);
        assert_eq!(document.selection(), first.as_ref());
    }

    #[test]
    fn delete_wipes_only_inside_the_selection() {
        let mut document = Document::new(200, 100);
        let id = document.active_id();
        let red = Paint::Color([255, 0, 0, 255]);
        let brush = crate::brush::BrushMode::Paint;
        paint_mask(
            &mut document.active_layer_mut().surface,
            &all(canvas()),
            &red,
            1.0,
            brush,
            canvas(),
        );
        let selection = Selection::new(rect(50.0, 50.0, 20.0, 20.0, canvas())).unwrap();
        let before = delete_inside(&mut document, id, &selection);
        assert!(!before.is_empty());
        let surface = &document.active_layer().surface;
        assert_eq!(surface.pixel(60, 60).a, 0);
        assert_eq!(surface.pixel(40, 60).a, 255);
    }

    #[test]
    fn a_moved_selection_is_dropped_back_and_the_selection_moves_with_it() {
        let mut document = Document::new(200, 100);
        let base = document.active_id();
        let red = Paint::Color([255, 0, 0, 255]);
        let brush = crate::brush::BrushMode::Paint;
        paint_mask(
            &mut document.active_layer_mut().surface,
            &rect(0.0, 0.0, 10.0, 10.0, canvas()),
            &red,
            1.0,
            brush,
            canvas(),
        );
        let floating = crate::layer_commands::LayerPresenceCommand::add_layer(&mut document);
        let _ = floating;
        let top = document.active_id();
        paint_mask(
            &mut document.active_layer_mut().surface,
            &rect(50.0, 50.0, 10.0, 10.0, canvas()),
            &red,
            1.0,
            brush,
            canvas(),
        );
        let before = drop_onto(&mut document, top, base);
        assert!(!before.is_empty());
        let surface = &document.layer(base).unwrap().surface;
        assert_eq!(surface.pixel(55, 55).a, 255);
        assert_eq!(surface.pixel(5, 5).a, 255, "what was there stays");
        let selection = Selection::new(rect(0.0, 0.0, 10.0, 10.0, canvas())).unwrap();
        let moved = transformed(
            &selection,
            crate::transform::Transform::translate(50.0, 20.0),
            canvas(),
        )
        .unwrap();
        assert_eq!(moved.bounds(), PixelRect::new(50, 20, 60, 30));
        assert_eq!(moved.mask().get(55, 25), 255);
    }
}
