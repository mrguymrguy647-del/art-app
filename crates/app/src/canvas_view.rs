//! The canvas on screen: draws the document and handles zoom, pan and rotate.
//!
//! Each tile is sent to the graphics card as its own texture. Moving the
//! view only changes *where* those textures are drawn, so zooming, panning
//! and rotating stay smooth even on huge canvases. When a tile's pixels
//! change, only that tile is sent again.
//!
//! When zoomed out, tiles are sent as smaller previews (see
//! `Tile::display_bytes`): it looks smoother and uses less memory.

use std::collections::HashMap;

use art_engine::document::Document;
use art_engine::surface::{PixelRect, TileCoord};
use art_engine::tile::{MAX_PREVIEW_LEVEL, TILE_SIZE};
use art_engine::view::{Point, View};
use eframe::egui::epaint::Vertex;
use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, Key, Mesh, MouseWheelUnit, PointerButton, Pos2, Rect,
    Sense, Shape, Stroke, TextureFilter, TextureHandle, TextureOptions, Vec2, pos2,
};

/// Each zoom step (mouse wheel notch or +/- key) zooms by 25%.
const ZOOM_STEP: f32 = 1.25;
/// The 4 and 6 keys rotate by this much.
const ROTATE_STEP_DEGREES: f32 = 15.0;
/// From this zoom up, show canvas pixels as sharp squares instead of
/// smoothing them, so you can see exactly what you painted.
const SHARP_PIXELS_FROM_ZOOM: f32 = 2.0;
/// After zooming, re-send at most this many tiles per frame. The old
/// textures keep showing meanwhile, so there is no stutter.
const PREVIEW_UPDATES_PER_FRAME: usize = 64;
/// The grey around the canvas.
const BACKGROUND: Color32 = Color32::from_gray(48);

/// A tile's texture on the graphics card, and how it was made.
struct TileTexture {
    handle: TextureHandle,
    /// Preview level: 0 = full size, 1 = half size, ...
    level: u32,
    sharp: bool,
    /// The tile's pixels changed since this texture was made.
    outdated: bool,
}

/// What dragging on the canvas currently does.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Pan,
    /// Remembers where the drag started, so the canvas turns by exactly
    /// the angle the mouse has moved around the centre since then.
    Rotate {
        start_mouse_angle: f32,
        start_rotation: f32,
    },
}

pub struct CanvasView {
    view: View,
    textures: HashMap<TileCoord, TileTexture>,
    /// Fit the canvas into the window on the first frame.
    needs_fit: bool,
    drag: Option<Drag>,
    /// The canvas pixel under the mouse, for the status bar.
    pointer_on_canvas: Option<Point>,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            view: View::default(),
            textures: HashMap::new(),
            needs_fit: true,
            drag: None,
            pointer_on_canvas: None,
        }
    }
}

impl CanvasView {
    pub fn ui(&mut self, ui: &mut egui::Ui, document: &mut Document) {
        egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui, document));
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas_area(ui, document));
    }

    fn canvas_area(&mut self, ui: &mut egui::Ui, document: &mut Document) {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());

        if self.needs_fit {
            self.fit(document, rect);
            self.needs_fit = false;
        }
        self.handle_input(ui, &response, rect, document);

        let visible = self.visible_tiles(document, rect);
        self.update_textures(ui.ctx(), document, &visible);
        self.paint(&ui.painter_at(rect), document, rect, &visible);
    }

    // ---- Input ------------------------------------------------------------

    fn handle_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        document: &Document,
    ) {
        let ctx = ui.ctx();
        let centre = to_point(rect.center());
        let line = ctx.options(|o| o.input_options.line_scroll_speed);
        let (space, shift, wheel_notches, zoom_delta, touch, hover) = ui.input(|i| {
            // Mouse wheel notches since the last frame (a touchpad sends
            // fractions). Ctrl + wheel is left out: egui reports it as
            // `zoom_delta` instead.
            let wheel_notches: f32 = i
                .events
                .iter()
                .map(|event| match event {
                    egui::Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } if !modifiers.command => match unit {
                        MouseWheelUnit::Line => delta.y,
                        MouseWheelUnit::Page => delta.y * 3.0,
                        MouseWheelUnit::Point => delta.y / line,
                    },
                    _ => 0.0,
                })
                .sum();
            (
                i.key_down(Key::Space),
                i.modifiers.shift,
                wheel_notches,
                i.zoom_delta(),
                i.multi_touch(),
                i.pointer.hover_pos().filter(|pos| rect.contains(*pos)),
            )
        });

        if let Some(touch) = touch.filter(|touch| rect.contains(touch.center_pos)) {
            // Two or more fingers: pinch to zoom, twist to rotate, move to pan.
            let fingers = to_point(touch.center_pos);
            self.view.pan(vec_to_point(touch.translation_delta));
            self.view.zoom_around(fingers, touch.zoom_delta);
            self.view.rotate_around(fingers, touch.rotation_delta);
        } else if let Some(mouse) = hover {
            // Mouse wheel zooms. A touchpad pinch arrives as Ctrl + wheel,
            // which egui reports as `zoom_delta`.
            let factor = ZOOM_STEP.powf(wheel_notches) * zoom_delta;
            if factor != 1.0 {
                self.view.zoom_around(to_point(mouse), factor);
            }
        }

        // Dragging: middle button or Space pans, Shift + Space rotates.
        if response.drag_started() {
            self.drag = if response.dragged_by(PointerButton::Middle) {
                Some(Drag::Pan)
            } else if space && shift {
                let start = ui
                    .input(|i| i.pointer.press_origin())
                    .unwrap_or(rect.center());
                Some(Drag::Rotate {
                    start_mouse_angle: angle_around(rect.center(), start),
                    start_rotation: self.view.rotation(),
                })
            } else if space {
                Some(Drag::Pan)
            } else {
                None // Step 3: painting
            };
        }
        if let Some(drag) = self.drag
            && response.dragged()
        {
            match drag {
                Drag::Pan => self.view.pan(vec_to_point(response.drag_delta())),
                Drag::Rotate {
                    start_mouse_angle,
                    start_rotation,
                } => {
                    if let Some(mouse) = response.interact_pointer_pos() {
                        let turned = angle_around(rect.center(), mouse) - start_mouse_angle;
                        self.view
                            .set_rotation_around(centre, start_rotation + turned);
                    }
                }
            }
        }
        if response.drag_stopped() {
            self.drag = None;
        }

        // Show what a drag would do.
        let cursor = match (self.drag, space) {
            (Some(Drag::Pan), _) => Some(CursorIcon::Grabbing),
            (Some(Drag::Rotate { .. }), _) => Some(CursorIcon::AllScroll),
            (None, true) if hover.is_some() => Some(CursorIcon::Grab),
            _ => None,
        };
        if let Some(cursor) = cursor {
            ctx.set_cursor_icon(cursor);
        }

        // Keyboard shortcuts (unless you're typing in a text box).
        if !ctx.egui_wants_keyboard_input() {
            // How many times each key was pressed since the last frame
            // (a slow frame can contain more than one press).
            let presses = |key| ui.input(|i| i.num_presses(key)) as i32;
            let zoom_steps = presses(Key::Plus) + presses(Key::Equals) - presses(Key::Minus);
            if zoom_steps != 0 {
                self.view.zoom_around(centre, ZOOM_STEP.powi(zoom_steps));
            }
            let rotate_steps = presses(Key::Num6) - presses(Key::Num4);
            if rotate_steps != 0 {
                let angle = (ROTATE_STEP_DEGREES * rotate_steps as f32).to_radians();
                self.view.rotate_around(centre, angle);
            }
            if presses(Key::Num5) > 0 {
                self.view.set_rotation_around(centre, 0.0);
            }
            if presses(Key::Num0) > 0 {
                self.fit(document, rect);
            }
        }

        self.pointer_on_canvas = hover.map(|pos| self.view.screen_to_canvas(to_point(pos)));
    }

    fn fit(&mut self, document: &Document, rect: Rect) {
        self.view.fit(
            document.width() as f32,
            document.height() as f32,
            to_point(rect.center()),
            Point::new(rect.width(), rect.height()),
        );
    }

    // ---- Textures ---------------------------------------------------------

    /// The canvas tiles that can be seen in `rect`.
    fn visible_tiles(&self, document: &Document, rect: Rect) -> Vec<TileCoord> {
        // Where the window's corners land on the canvas. The canvas may be
        // rotated, so take the box around all four.
        let corners = [
            rect.left_top(),
            rect.right_top(),
            rect.left_bottom(),
            rect.right_bottom(),
        ]
        .map(|corner| self.view.screen_to_canvas(to_point(corner)));
        let min_x = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let max_y = corners
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);

        let seen = PixelRect::new(
            min_x.floor() as i32,
            min_y.floor() as i32,
            max_x.ceil() as i32 + 1,
            max_y.ceil() as i32 + 1,
        );
        seen.intersect(&document.bounds()).tiles().collect()
    }

    /// Sends tiles to the graphics card where needed.
    fn update_textures(
        &mut self,
        ctx: &egui::Context,
        document: &mut Document,
        visible: &[TileCoord],
    ) {
        // How big canvas pixels are in real screen pixels (Windows display
        // scaling, e.g. 150%, makes one egui point 1.5 pixels).
        let real_zoom = self.view.zoom() * ctx.pixels_per_point();
        let level = preview_level(real_zoom);
        let sharp = real_zoom >= SHARP_PIXELS_FROM_ZOOM;

        // Tiles whose pixels changed: their textures are out of date.
        for coord in document.layer.take_dirty_tiles() {
            if document.layer.tile(coord).is_none() {
                self.textures.remove(&coord);
            } else if let Some(texture) = self.textures.get_mut(&coord) {
                texture.outdated = true;
            }
        }

        let mut preview_updates_left = PREVIEW_UPDATES_PER_FRAME;
        for &coord in visible {
            let Some(tile) = document.layer.tile(coord) else {
                continue;
            };
            let send = match self.textures.get(&coord) {
                // Never sent, or the pixels changed: send now.
                None => true,
                Some(texture) if texture.outdated => true,
                // Only the zoom changed: the old texture still looks fine
                // for a moment, so spread these over a few frames.
                Some(texture) if texture.level != level || texture.sharp != sharp => {
                    let send = preview_updates_left > 0;
                    preview_updates_left = preview_updates_left.saturating_sub(1);
                    send
                }
                Some(_) => false,
            };
            if !send {
                continue;
            }

            let size = TILE_SIZE >> level;
            let image =
                ColorImage::from_rgba_premultiplied([size, size], &tile.display_bytes(level));
            let options = TextureOptions {
                magnification: if sharp {
                    TextureFilter::Nearest
                } else {
                    TextureFilter::Linear
                },
                ..TextureOptions::LINEAR
            };
            match self.textures.get_mut(&coord) {
                Some(texture) => {
                    texture.handle.set(image, options);
                    texture.level = level;
                    texture.sharp = sharp;
                    texture.outdated = false;
                }
                None => {
                    let name = format!("tile {},{}", coord.x, coord.y);
                    let handle = ctx.load_texture(name, image, options);
                    self.textures.insert(
                        coord,
                        TileTexture {
                            handle,
                            level,
                            sharp,
                            outdated: false,
                        },
                    );
                }
            }
        }

        if preview_updates_left == 0 {
            ctx.request_repaint(); // more tiles to update next frame
        }
    }

    // ---- Drawing ----------------------------------------------------------

    fn paint(
        &self,
        painter: &egui::Painter,
        document: &Document,
        rect: Rect,
        visible: &[TileCoord],
    ) {
        painter.rect_filled(rect, 0.0, BACKGROUND);

        // The paper: a (possibly rotated) rectangle under the tiles.
        let (width, height) = (document.width() as f32, document.height() as f32);
        let paper = [(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)]
            .map(|(x, y)| to_pos2(self.view.canvas_to_screen(Point::new(x, y))));
        let colour = document.paper;
        painter.add(Shape::convex_polygon(
            paper.to_vec(),
            Color32::from_rgba_premultiplied(colour.r, colour.g, colour.b, colour.a),
            Stroke::new(1.0, Color32::from_gray(24)),
        ));

        // Each tile: a four-cornered shape showing that tile's texture.
        let tile_size = TILE_SIZE as f32;
        for coord in visible {
            let Some(texture) = self.textures.get(coord) else {
                continue;
            };
            let (left, top) = coord.origin();
            let (left, top) = (left as f32, top as f32);
            let corners = [
                (left, top),
                (left + tile_size, top),
                (left + tile_size, top + tile_size),
                (left, top + tile_size),
            ];
            // Which part of the texture goes at each corner (0..1 across and down).
            let texture_corners = [
                pos2(0.0, 0.0),
                pos2(1.0, 0.0),
                pos2(1.0, 1.0),
                pos2(0.0, 1.0),
            ];

            let mut mesh = Mesh::with_texture(texture.handle.id());
            for ((x, y), uv) in corners.into_iter().zip(texture_corners) {
                mesh.vertices.push(Vertex {
                    pos: to_pos2(self.view.canvas_to_screen(Point::new(x, y))),
                    uv,
                    color: Color32::WHITE,
                });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(Shape::mesh(mesh));
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui, document: &Document) {
        ui.horizontal(|ui| {
            // 100% = one canvas pixel per real screen pixel.
            let percent = self.view.zoom() * ui.ctx().pixels_per_point() * 100.0;
            let zoom = if percent < 10.0 { format!("{percent:.1}%") } else { format!("{percent:.0}%") };
            ui.label(format!("Zoom {zoom}"));
            ui.separator();
            ui.label(format!("Rotation {}°", self.view.rotation_degrees().round() as i32));
            ui.separator();
            match self.pointer_on_canvas {
                Some(p) => ui.label(format!("X {} Y {}", p.x.floor() as i32, p.y.floor() as i32)),
                None => ui.label("X - Y -"),
            };
            ui.separator();
            ui.label(format!("Canvas {} × {}", document.width(), document.height()));
            ui.separator();
            ui.label(format!("Tiles in memory: {}", document.layer.tile_count()));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak("Wheel: zoom · Space+drag: pan · Shift+Space+drag: rotate · 0: fit · 4/6: rotate · 5: unrotate");
            });
        });
    }
}

/// Which preview size to use: the smallest one that still has at least one
/// texture pixel per screen pixel. Full size (0) above 50% zoom, half size
/// (1) from 50% down to 25%, quarter size (2) from 25% to 12.5%, and so on.
fn preview_level(real_zoom: f32) -> u32 {
    if real_zoom >= 1.0 {
        0
    } else {
        ((1.0 / real_zoom).log2().floor() as u32).min(MAX_PREVIEW_LEVEL)
    }
}

/// The direction from `centre` to `point`, in radians.
fn angle_around(centre: Pos2, point: Pos2) -> f32 {
    let offset = point - centre;
    offset.y.atan2(offset.x)
}

fn to_point(pos: Pos2) -> Point {
    Point::new(pos.x, pos.y)
}

fn vec_to_point(vec: Vec2) -> Point {
    Point::new(vec.x, vec.y)
}

fn to_pos2(point: Point) -> Pos2 {
    pos2(point.x, point.y)
}

#[cfg(test)]
mod tests {
    use super::preview_level;

    #[test]
    fn preview_level_halves_the_size_for_each_halving_of_zoom() {
        assert_eq!(preview_level(4.0), 0);
        assert_eq!(preview_level(1.0), 0);
        assert_eq!(preview_level(0.99), 0);
        assert_eq!(preview_level(0.5), 1);
        assert_eq!(preview_level(0.3), 1);
        assert_eq!(preview_level(0.25), 2);
        assert_eq!(preview_level(0.1), 3);
        assert_eq!(preview_level(0.0001), art_engine::tile::MAX_PREVIEW_LEVEL);
    }
}
