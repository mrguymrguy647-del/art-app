//! A surface: all the pixels of one layer, stored as tiles.
//!
//! A tile only exists once something has been painted on it. Reading a
//! pixel where there is no tile gives "transparent". This is how a huge,
//! mostly empty canvas stays small in memory.
//!
//! The surface also remembers which tiles changed ("dirty" tiles), so the
//! screen only needs to redraw those.

use std::collections::{HashMap, HashSet};

use crate::tile::{Pixel, Rgba8, TILE_SIZE, Tile};

/// Which tile: its column and row. Tile (1, 0) covers pixels 256..511 across
/// and 0..255 down. Negative numbers are allowed (left of / above the canvas).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileCoord {
    pub x: i32,
    pub y: i32,
}

impl TileCoord {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// The tile that contains pixel (x, y).
    pub fn containing(x: i32, y: i32) -> Self {
        // div_euclid rounds down, also for negatives: -1 / 256 -> tile -1.
        let size = TILE_SIZE as i32;
        Self::new(x.div_euclid(size), y.div_euclid(size))
    }

    /// The pixel position of this tile's top-left corner.
    pub fn origin(self) -> (i32, i32) {
        let size = TILE_SIZE as i32;
        (self.x * size, self.y * size)
    }
}

/// A rectangle of pixels. `left` and `top` are inside it; `right` and
/// `bottom` are the first pixels *outside* it. So width = right - left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PixelRect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.left >= self.right || self.top >= self.bottom
    }

    /// The part both rectangles have in common.
    pub fn intersect(&self, other: &PixelRect) -> PixelRect {
        PixelRect::new(
            self.left.max(other.left),
            self.top.max(other.top),
            self.right.min(other.right),
            self.bottom.min(other.bottom),
        )
    }

    /// Every tile this rectangle touches.
    pub fn tiles(&self) -> impl Iterator<Item = TileCoord> + use<> {
        let (first, last) = if self.is_empty() {
            (TileCoord::new(0, 0), TileCoord::new(-1, -1)) // nothing
        } else {
            (
                TileCoord::containing(self.left, self.top),
                TileCoord::containing(self.right - 1, self.bottom - 1),
            )
        };
        (first.y..=last.y).flat_map(move |y| (first.x..=last.x).map(move |x| TileCoord::new(x, y)))
    }
}

/// All the pixels of one layer.
#[derive(Debug, Clone, Default)]
pub struct Surface<P: Pixel = Rgba8> {
    tiles: HashMap<TileCoord, Box<Tile<P>>>,
    dirty: HashSet<TileCoord>,
}

impl<P: Pixel> Surface<P> {
    /// An empty surface: no tiles, every pixel transparent.
    pub fn new() -> Self {
        Self {
            tiles: HashMap::new(),
            dirty: HashSet::new(),
        }
    }

    /// The pixel at (x, y). Transparent where nothing has been painted.
    pub fn pixel(&self, x: i32, y: i32) -> P {
        let coord = TileCoord::containing(x, y);
        match self.tiles.get(&coord) {
            Some(tile) => {
                let (left, top) = coord.origin();
                tile.pixel((x - left) as usize, (y - top) as usize)
            }
            None => P::TRANSPARENT,
        }
    }

    /// Sets one pixel. For many pixels, `paint_pixels` is much faster.
    pub fn set_pixel(&mut self, x: i32, y: i32, pixel: P) {
        let rect = PixelRect::new(x, y, x + 1, y + 1);
        self.paint_pixels(rect, |_, _| Some(pixel));
    }

    /// Changes pixels inside `rect`, one tile at a time.
    ///
    /// `new_pixel(x, y)` is asked about each pixel and returns the new
    /// value, or `None` to leave it alone. A tile is only created if at
    /// least one of its pixels gets a value. Changed tiles are marked dirty.
    pub fn paint_pixels(
        &mut self,
        rect: PixelRect,
        mut new_pixel: impl FnMut(i32, i32) -> Option<P>,
    ) {
        for coord in rect.tiles() {
            let (left, top) = coord.origin();
            let size = TILE_SIZE as i32;
            let area = rect.intersect(&PixelRect::new(left, top, left + size, top + size));

            // Take the tile out of the map while we work on it (or start
            // without one), and put it back afterwards.
            let mut tile = self.tiles.remove(&coord);
            let mut changed = false;
            for y in area.top..area.bottom {
                for x in area.left..area.right {
                    let Some(pixel) = new_pixel(x, y) else {
                        continue;
                    };
                    // Painting "transparent" where there is no tile changes nothing.
                    if tile.is_none() && pixel == P::TRANSPARENT {
                        continue;
                    }
                    let tile = tile.get_or_insert_with(|| Box::new(Tile::new()));
                    tile.set_pixel((x - left) as usize, (y - top) as usize, pixel);
                    changed = true;
                }
            }
            if let Some(tile) = tile {
                self.tiles.insert(coord, tile);
            }
            if changed {
                self.dirty.insert(coord);
            }
        }
    }

    /// The tile at `coord`, if anything has been painted there.
    pub fn tile(&self, coord: TileCoord) -> Option<&Tile<P>> {
        self.tiles.get(&coord).map(|tile| &**tile)
    }

    /// The tile at `coord` for changing, created (transparent) if it doesn't
    /// exist yet. It is marked dirty, since the caller is about to change it.
    pub fn tile_mut(&mut self, coord: TileCoord) -> &mut Tile<P> {
        self.dirty.insert(coord);
        self.tiles
            .entry(coord)
            .or_insert_with(|| Box::new(Tile::new()))
    }

    /// Removes one tile (its area becomes transparent) and returns it.
    pub fn remove_tile(&mut self, coord: TileCoord) -> Option<Tile<P>> {
        let tile = self.tiles.remove(&coord)?;
        self.dirty.insert(coord);
        Some(*tile)
    }

    /// Puts `tile` at `coord` (or, if `tile` is `None`, makes that area
    /// empty), and returns the tile that was there. Used by undo and redo.
    pub fn replace_tile(&mut self, coord: TileCoord, tile: Option<Tile<P>>) -> Option<Tile<P>> {
        self.dirty.insert(coord);
        let old = match tile {
            Some(tile) => self.tiles.insert(coord, Box::new(tile)),
            None => self.tiles.remove(&coord),
        };
        old.map(|tile| *tile)
    }

    /// Removes every tile: the whole surface becomes transparent again.
    /// Returns the removed tiles (so clearing can be undone).
    pub fn clear(&mut self) -> HashMap<TileCoord, Tile<P>> {
        // The removed tiles count as changed, so the screen updates.
        self.dirty.extend(self.tiles.keys().copied());
        self.tiles
            .drain()
            .map(|(coord, tile)| (coord, *tile))
            .collect()
    }

    /// All the tiles that exist, in no particular order.
    pub fn tiles(&self) -> impl Iterator<Item = (TileCoord, &Tile<P>)> {
        self.tiles.iter().map(|(&coord, tile)| (coord, &**tile))
    }

    /// How many tiles exist (are using memory).
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Marks every tile as changed (e.g. when the whole layer is shown,
    /// hidden or moved, so the screen must update all of it).
    pub fn mark_all_dirty(&mut self) {
        self.dirty.extend(self.tiles.keys().copied());
    }

    /// The tiles that changed since the last call. Clears the list.
    pub fn take_dirty_tiles(&mut self) -> Vec<TileCoord> {
        self.dirty.drain().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba8 = Rgba8::new(255, 0, 0, 255);

    #[test]
    fn tile_coord_rounds_down_also_for_negative_pixels() {
        assert_eq!(TileCoord::containing(0, 0), TileCoord::new(0, 0));
        assert_eq!(TileCoord::containing(255, 255), TileCoord::new(0, 0));
        assert_eq!(TileCoord::containing(256, 0), TileCoord::new(1, 0));
        assert_eq!(TileCoord::containing(-1, -1), TileCoord::new(-1, -1));
        assert_eq!(TileCoord::containing(-256, -257), TileCoord::new(-1, -2));
        assert_eq!(TileCoord::new(2, -1).origin(), (512, -256));
    }

    #[test]
    fn empty_surface_uses_no_tiles_and_reads_transparent() {
        let surface: Surface = Surface::new();
        assert_eq!(surface.tile_count(), 0);
        assert_eq!(surface.pixel(1000, -5000), Rgba8::TRANSPARENT);
    }

    #[test]
    fn painting_one_pixel_creates_exactly_one_tile() {
        let mut surface: Surface = Surface::new();
        surface.set_pixel(300, 10, RED);

        assert_eq!(surface.tile_count(), 1);
        assert!(surface.tile(TileCoord::new(1, 0)).is_some());
        assert_eq!(surface.pixel(300, 10), RED);
        assert_eq!(surface.pixel(301, 10), Rgba8::TRANSPARENT);
    }

    #[test]
    fn painting_transparent_on_empty_area_creates_nothing() {
        let mut surface: Surface = Surface::new();
        surface.set_pixel(5, 5, Rgba8::TRANSPARENT);
        assert_eq!(surface.tile_count(), 0);
        assert!(surface.take_dirty_tiles().is_empty());
    }

    #[test]
    fn rectangle_across_a_tile_corner_touches_four_tiles() {
        let mut surface: Surface = Surface::new();
        surface.paint_pixels(PixelRect::new(250, 250, 260, 260), |_, _| Some(RED));

        assert_eq!(surface.tile_count(), 4);
        let mut dirty = surface.take_dirty_tiles();
        dirty.sort(); // sorts by column (x) first, then row (y)
        assert_eq!(
            dirty,
            vec![
                TileCoord::new(0, 0),
                TileCoord::new(0, 1),
                TileCoord::new(1, 0),
                TileCoord::new(1, 1),
            ]
        );
        assert_eq!(surface.pixel(250, 250), RED);
        assert_eq!(surface.pixel(259, 259), RED);
        assert_eq!(surface.pixel(260, 260), Rgba8::TRANSPARENT);
    }

    #[test]
    fn tile_mut_creates_the_tile_and_marks_it_dirty() {
        let mut surface: Surface = Surface::new();
        surface.tile_mut(TileCoord::new(2, 3)).set_pixel(0, 0, RED);
        assert_eq!(surface.pixel(512, 768), RED);
        assert_eq!(surface.take_dirty_tiles(), vec![TileCoord::new(2, 3)]);
    }

    #[test]
    fn removing_a_tile_frees_it_and_marks_it_changed() {
        let mut surface: Surface = Surface::new();
        surface.set_pixel(300, 0, RED);
        surface.take_dirty_tiles();

        let removed = surface
            .remove_tile(TileCoord::new(1, 0))
            .expect("tile existed");
        assert_eq!(removed.pixel(44, 0), RED);
        assert_eq!(surface.tile_count(), 0);
        assert_eq!(surface.take_dirty_tiles(), vec![TileCoord::new(1, 0)]);
        assert!(surface.remove_tile(TileCoord::new(1, 0)).is_none());
    }

    #[test]
    fn clear_removes_all_tiles_and_reports_them_as_changed() {
        let mut surface: Surface = Surface::new();
        surface.set_pixel(0, 0, RED);
        surface.set_pixel(1000, 0, RED);
        surface.take_dirty_tiles();

        let removed = surface.clear();
        assert_eq!(removed.len(), 2);
        assert_eq!(surface.tile_count(), 0);
        assert_eq!(surface.pixel(0, 0), Rgba8::TRANSPARENT);
        assert_eq!(surface.take_dirty_tiles().len(), 2);
    }

    #[test]
    fn replace_tile_swaps_tiles_in_and_out() {
        let mut surface: Surface = Surface::new();
        let coord = TileCoord::new(0, 0);
        let mut red_tile = Tile::new();
        red_tile.set_pixel(0, 0, RED);

        // Put a tile where there was none.
        assert!(surface.replace_tile(coord, Some(red_tile)).is_none());
        assert_eq!(surface.pixel(0, 0), RED);
        // Take it out again: we get it back, and the area is empty.
        let back = surface.replace_tile(coord, None).expect("the red tile");
        assert_eq!(back.pixel(0, 0), RED);
        assert_eq!(surface.tile_count(), 0);
        assert_eq!(surface.take_dirty_tiles(), vec![coord]);
    }

    #[test]
    fn dirty_list_is_cleared_after_taking_it() {
        let mut surface: Surface = Surface::new();
        surface.set_pixel(0, 0, RED);
        assert_eq!(surface.take_dirty_tiles().len(), 1);
        assert!(surface.take_dirty_tiles().is_empty());
    }

    #[test]
    fn skipped_pixels_keep_their_value_and_create_no_tiles() {
        let mut surface: Surface = Surface::new();
        surface.set_pixel(1, 1, RED);
        surface.take_dirty_tiles();

        // Only touch pixels in the second tile across.
        surface.paint_pixels(PixelRect::new(0, 0, 512, 1), |x, _| {
            (x >= 256).then_some(Rgba8::WHITE)
        });

        assert_eq!(surface.pixel(1, 1), RED);
        assert_eq!(surface.pixel(300, 0), Rgba8::WHITE);
        assert_eq!(surface.take_dirty_tiles(), vec![TileCoord::new(1, 0)]);
    }

    #[test]
    fn rect_tiles_handles_empty_and_negative_rects() {
        assert_eq!(PixelRect::new(10, 10, 10, 20).tiles().count(), 0);
        let tiles: Vec<_> = PixelRect::new(-10, -10, 10, 10).tiles().collect();
        assert_eq!(tiles.len(), 4);
        assert!(tiles.contains(&TileCoord::new(-1, -1)));
    }
}
