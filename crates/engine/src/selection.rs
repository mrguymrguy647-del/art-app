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
use crate::script::geometry;
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

    /// The selection's edge as line pieces in canvas pixels, for drawing
    /// "marching ants". A pixel counts as selected from half selected up,
    /// and the edge runs along pixel sides; pieces in a row along the same
    /// line are joined, so even a big selection gives few pieces.
    pub fn outline(&self) -> Vec<[(f32, f32); 2]> {
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
        let pieces = selection.outline();
        // A rectangle: two long horizontal and two short vertical pieces.
        assert_eq!(pieces.len(), 4, "{pieces:?}");
        let length: f32 = pieces
            .iter()
            .map(|[a, b]| (b.0 - a.0).abs() + (b.1 - a.1).abs())
            .sum();
        assert_eq!(length, 2.0 * (20.0 + 5.0));
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
}
