//! The canvas on screen: draws the document and handles zoom, pan and rotate.
//!
//! Each tile is sent to the graphics card as its own texture. Moving the
//! view only changes *where* those textures are drawn, so zooming, panning
//! and rotating stay smooth even on huge canvases. When a tile's pixels
//! change, only that tile is sent again.
//!
//! When zoomed out, tiles are sent as smaller previews (see
//! `Tile::display_bytes`): it looks smoother and uses less memory.
//!
//! Painting itself is handled by the painting tools (`paint_tool.rs`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui::epaint::Vertex;
use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, Key, Mesh, MouseWheelUnit, PointerButton, Pos2, Rect,
    Sense, Shape, Stroke, TextureFilter, TextureHandle, TextureOptions, Vec2, pos2, vec2,
};
use quill_engine::brush::{MAX_SIZE, MIN_SIZE};
use quill_engine::document::{Background, Document};
use quill_engine::history::{History, TileCommand};
use quill_engine::image::Image;
use quill_engine::layer::{BlendMode, blend_over};
use quill_engine::script::record;
use quill_engine::surface::{PixelRect, TileCoord};
use quill_engine::tile::{MAX_PREVIEW_LEVEL, TILE_SIZE};
use quill_engine::tile::{Rgba8, Tile};
use quill_engine::view::{Point, View};

use crate::brush_panel::brush_panel;
use crate::color_panel::ColorPicker;
use crate::layers_panel::LayersPanel;
use crate::paint_tool::{CanvasPlacement, PaintTool, Tool};
use crate::pen_input::InkEvent;
use crate::transform_tool::{LayerAction, LayerTransform, flip};

/// Each zoom step (mouse wheel notch or +/- key) zooms by 25%.
const ZOOM_STEP: f32 = 1.25;
/// The 4 and 6 keys rotate by this much.
const ROTATE_STEP_DEGREES: f32 = 15.0;
/// The [ and ] keys change the brush size by this factor.
const BRUSH_SIZE_STEP: f32 = 1.2;
/// From this zoom up, show canvas pixels as sharp squares instead of
/// smoothing them, so you can see exactly what you painted.
const SHARP_PIXELS_FROM_ZOOM: f32 = 2.0;
/// After zooming, re-send at most this many tiles per frame. The old
/// textures keep showing meanwhile, so there is no stutter.
const PREVIEW_UPDATES_PER_FRAME: usize = 64;
/// The grey around the canvas.
const BACKGROUND: Color32 = Color32::from_gray(48);
/// How long a short message (like "nothing on this layer") stays up.
const NOTICE_SHOWS_FOR: Duration = Duration::from_secs(3);

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
    /// While a script is playing: no painting, and the panels are greyed
    /// out (zooming and moving around still work).
    pub locked: bool,
    view: View,
    textures: HashMap<TileCoord, TileTexture>,
    /// Fit the canvas into the window on the first frame.
    needs_fit: bool,
    /// The view right after the last automatic fit. While the view is
    /// still exactly this (you haven't zoomed, moved or turned it), the
    /// canvas is fitted again whenever the window changes size.
    fitted_view: Option<View>,
    /// Where the canvas area was last frame, to notice the window resizing.
    last_rect: Option<Rect>,
    drag: Option<Drag>,
    /// The canvas pixel under the mouse, for the status bar.
    pointer_on_canvas: Option<Point>,
    tools: PaintTool,
    colors: ColorPicker,
    layers: LayersPanel,
    /// Layer → Transform… in progress.
    transform: Option<LayerTransform>,
    /// A short message at the top of the canvas, and since when.
    notice: Option<(&'static str, Instant)>,
    /// Where the tool panel was last frame: clicking there while typing
    /// text (to change its colour or size) doesn't finish the text.
    brush_panel_rect: Option<Rect>,
    /// A see-through picture (an onion skin) shown on the paper, under the
    /// picture's layers: blended into every tile's texture.
    overlay: Option<Image>,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            locked: false,
            view: View::default(),
            textures: HashMap::new(),
            needs_fit: true,
            fitted_view: None,
            last_rect: None,
            drag: None,
            pointer_on_canvas: None,
            tools: PaintTool::default(),
            colors: ColorPicker::default(),
            layers: LayersPanel::default(),
            transform: None,
            notice: None,
            brush_panel_rect: None,
            overlay: None,
        }
    }
}

impl CanvasView {
    /// Shows the canvas with its panels. Changes are recorded in
    /// `history`. `ink_events` are the pen/touch events since the last frame.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut Document,
        history: &mut History,
        ink_events: Vec<InkEvent>,
    ) {
        // Text being typed is finished by a click anywhere but on the
        // canvas (the text tool sees those) or the tool panel.
        if self.tools.is_typing() {
            let pressed = ui.input(|i| {
                i.pointer
                    .any_pressed()
                    .then(|| i.pointer.press_origin())
                    .flatten()
            });
            let keeps_typing = |pos: Pos2| {
                self.last_rect.is_some_and(|r| r.contains(pos))
                    || self.brush_panel_rect.is_some_and(|r| r.contains(pos))
            };
            if let Some(pos) = pressed
                && !keeps_typing(pos)
            {
                self.tools.finish_typing(document, history);
            }
        }
        egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui, document, history));
        if self.transform.is_some() {
            egui::Panel::bottom("transform_bar").show(ui, |ui| {
                self.transform_bar(ui, document, history);
            });
        }
        // While transforming, the panels wait (the layer is hidden meanwhile).
        let panels_enabled = !self.locked && self.transform.is_none();
        let brush_panel = egui::Panel::right("brush_panel")
            .exact_size(250.0)
            .show(ui, |ui| {
                let actions = ui
                    .add_enabled_ui(panels_enabled, |ui| {
                        brush_panel(ui, &mut self.tools, &mut self.colors)
                    })
                    .inner;
                if actions.clear_layer {
                    self.tools.finish_typing(document, history);
                    let layer = document.active_id();
                    let name = document.active_layer().name().to_owned();
                    let tiles_before = document.clear_active_layer();
                    if !tiles_before.is_empty() {
                        history.push_recorded(
                            Box::new(TileCommand::new("Clear layer", layer, tiles_before)),
                            record::clear_layer(&name),
                        );
                    }
                }
            });
        self.brush_panel_rect = Some(brush_panel.response.rect);
        egui::Panel::left("layers_panel")
            .exact_size(210.0)
            .show(ui, |ui| {
                ui.add_enabled_ui(panels_enabled, |ui| {
                    self.layers.ui(ui, document, history);
                });
            });
        egui::CentralPanel::no_frame()
            .show(ui, |ui| self.canvas_area(ui, document, history, ink_events));
    }

    /// Forgets the previous picture's textures and view, for when a new
    /// picture is started or opened. Tools and colours stay as they are.
    /// The Text tool's size for a canvas this high: about a twelfth of
    /// it, so a word reads well on a small sprite and a big painting.
    pub fn fit_text_size(&mut self, canvas_height: u32) {
        self.tools.text.size = (canvas_height as f32 / 12.0).round().clamp(8.0, 400.0);
    }

    pub fn reset(&mut self) {
        self.textures.clear();
        self.needs_fit = true;
        self.fitted_view = None;
        self.drag = None;
        self.pointer_on_canvas = None;
        self.layers = LayersPanel::default();
    }

    /// True while a stroke is being painted.
    pub fn is_painting(&self) -> bool {
        self.tools.is_painting()
    }

    /// True while something must be finished first (a transform): the
    /// menus, undo and redo wait meanwhile.
    pub fn is_editing(&self) -> bool {
        self.transform.is_some()
    }

    /// Ends whatever is in progress, for when the window closes: text
    /// being typed is kept (one undo step); a transform that wasn't
    /// applied is cancelled.
    pub fn finish_editing(&mut self, document: &mut Document, history: &mut History) {
        self.tools.finish_typing(document, history);
        if let Some(transform) = self.transform.take() {
            transform.cancel(document);
        }
    }

    /// Finishes the text being typed, if any (before undo, a menu, ...).
    pub fn finish_typing(&mut self, document: &mut Document, history: &mut History) {
        self.tools.finish_typing(document, history);
    }

    /// Draws `image` (the canvas's size, see-through) over the canvas
    /// from now on; `None` takes it away. Setting it again replaces it.
    pub fn set_overlay(&mut self, image: Option<Image>) {
        self.overlay = image;
        // It's blended into every tile's picture, so they're all redone.
        for texture in self.textures.values_mut() {
            texture.outdated = true;
        }
    }

    /// A tile of paper with the overlay (onion skin) blended on it, for
    /// the layers to go over.
    fn paper_with_overlay(&self, document: &Document, coord: TileCoord) -> Tile {
        let mut base = document.paper_tile(coord);
        let Some(image) = &self.overlay else {
            return base;
        };
        let (left, top) = coord.origin();
        let size = TILE_SIZE as i32;
        let area = document
            .bounds()
            .intersect(&PixelRect::new(left, top, left + size, top + size));
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                let [r, g, b, a] = image.pixel(x as u32, y as u32);
                if a == 0 {
                    continue;
                }
                let i = (y - top) as usize * TILE_SIZE + (x - left) as usize;
                let pixels = base.pixels_mut();
                pixels[i] = blend_over(
                    pixels[i],
                    Rgba8::from_straight(r, g, b, a),
                    1.0,
                    BlendMode::Normal,
                );
            }
        }
        base
    }

    /// Does what was chosen in the Layer menu.
    pub fn layer_action(
        &mut self,
        action: LayerAction,
        document: &mut Document,
        history: &mut History,
    ) {
        match action {
            LayerAction::FlipHorizontal | LayerAction::FlipVertical => {
                let horizontal = action == LayerAction::FlipHorizontal;
                if !flip(document, history, horizontal) {
                    self.show_notice("There's nothing on this layer to flip.");
                }
            }
            LayerAction::Transform => match LayerTransform::start(document) {
                Ok(transform) => self.transform = Some(transform),
                Err(problem) => self.show_notice(problem),
            },
        }
    }

    pub fn show_notice(&mut self, text: &'static str) {
        self.notice = Some((text, Instant::now()));
    }

    /// The bar under the canvas while transforming: what to do, Apply and Cancel.
    fn transform_bar(&mut self, ui: &mut egui::Ui, document: &mut Document, history: &mut History) {
        let mut apply = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            let size = vec2(110.0, 30.0); // easy to tap with the pen
            apply = ui
                .add(egui::Button::new("Apply (Enter)").min_size(size))
                .clicked();
            cancel = ui
                .add(egui::Button::new("Cancel (Esc)").min_size(size))
                .clicked();
            ui.separator();
            ui.label(
                "Transform: drag inside the box to move it, a corner to resize \
                 (Shift keeps the proportions), the round handle to turn it.",
            );
        });
        if apply && let Some(transform) = self.transform.take() {
            transform.apply(document, history);
        } else if cancel && let Some(transform) = self.transform.take() {
            transform.cancel(document);
        }
    }

    fn canvas_area(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut Document,
        history: &mut History,
        ink_events: Vec<InkEvent>,
    ) {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());

        self.follow_window_size(document, rect);
        let navigating = self.handle_input(ui, &response, rect, document);
        if let Some(transform) = &mut self.transform {
            transform.handle_input(ui, rect, &self.view, ink_events, navigating);
            let (enter, escape) =
                ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if enter && let Some(transform) = self.transform.take() {
                transform.apply(document, history);
            } else if escape && let Some(transform) = self.transform.take() {
                transform.cancel(document);
            }
        } else if !self.locked {
            let placement = CanvasPlacement {
                rect,
                view: &self.view,
                navigating,
            };
            self.tools
                .handle_input(ui, placement, document, history, ink_events);
        }

        let visible = self.visible_tiles(document, rect);
        self.update_textures(ui.ctx(), document, &visible);
        let painter = ui.painter_at(rect);
        self.paint(&painter, document, rect, &visible);

        if let Some(transform) = &mut self.transform {
            transform.draw(ui.ctx(), &painter, &self.view);
            if let Some((text, since)) = self.notice
                && since.elapsed() < NOTICE_SHOWS_FOR
            {
                draw_notice(&painter, rect, text);
            }
            return; // no brush outline while transforming
        }

        // Text being typed: a box around it, and a line where the next letter goes.
        if let Some((top_left, bottom_right, caret_top, caret_bottom)) = self.tools.text_frame() {
            let corners = [
                top_left,
                Point::new(bottom_right.x, top_left.y),
                bottom_right,
                Point::new(top_left.x, bottom_right.y),
            ]
            .map(|p| to_pos2(self.view.canvas_to_screen(p)));
            let caret = [caret_top, caret_bottom].map(|p| to_pos2(self.view.canvas_to_screen(p)));
            for (width, color) in [(3.0, Color32::from_black_alpha(150)), (1.0, Color32::WHITE)] {
                painter.add(Shape::closed_line(
                    corners.to_vec(),
                    Stroke::new(width, color),
                ));
                painter.line_segment(caret, Stroke::new(width + 1.0, color));
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
        }

        // The brush outline (or, when picking a colour, a preview of the
        // colour underneath) replaces the mouse pointer over the canvas.
        if let Some(position) = self.tools.outline_position(ui, rect) {
            if self.tools.eyedropper_active(ui) {
                let on_canvas = self.view.screen_to_canvas(to_point(position));
                let colour = document.sample_color(on_canvas.x, on_canvas.y);
                draw_eyedropper(&painter, position, colour);
            } else if self.tools.uses_crosshair() {
                draw_brush_outline(&painter, position, 0.0); // a small cross
            } else {
                let radius = self.tools.settings().size / 2.0 * self.view.zoom();
                draw_brush_outline(&painter, position, radius);
            }
            if !navigating {
                ui.ctx().set_cursor_icon(CursorIcon::None);
            }
        }

        // The gradient being dragged out: a line from start to end.
        if let Some((start, end)) = self.tools.gradient_line() {
            let start = to_pos2(self.view.canvas_to_screen(start));
            let end = to_pos2(self.view.canvas_to_screen(end));
            for (width, color) in [(4.0, Color32::from_black_alpha(150)), (1.5, Color32::WHITE)] {
                painter.line_segment([start, end], Stroke::new(width, color));
                painter.circle_stroke(start, 5.0, Stroke::new(width, color));
                painter.circle_stroke(end, 5.0, Stroke::new(width, color));
            }
        }

        let notice = match self.notice {
            Some((text, since)) if since.elapsed() < NOTICE_SHOWS_FOR => Some(text),
            _ if self.tools.show_hidden_layer_notice() => {
                Some("This layer is hidden. Show it in the Layers panel to paint on it.")
            }
            _ => None,
        };
        if let Some(text) = notice {
            draw_notice(&painter, rect, text);
            // Redraw soon, so the notice goes away on time.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    // ---- Input ------------------------------------------------------------

    /// Zoom, pan, rotate and keyboard shortcuts. Returns true while the
    /// user is panning or rotating (or holding Space to start doing so).
    fn handle_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        document: &Document,
    ) -> bool {
        let ctx = ui.ctx();
        // A small window (like "New picture") is open on top of the canvas:
        // the wheel and keys belong to it.
        if !ctx.memory(|m| m.allows_interaction(ui.layer_id())) {
            return false;
        }
        let centre = to_point(rect.center());
        let line = ctx.options(|o| o.input_options.line_scroll_speed);
        // While typing text, the keys are for the text.
        let typing = self.tools.is_typing();
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
                i.key_down(Key::Space) && !typing,
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

        // Keyboard shortcuts (unless you're typing in a text box). While
        // transforming, only the ones that move the view.
        if !ctx.egui_wants_keyboard_input() && !typing {
            let editing = self.is_editing();
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
            let size_steps = presses(Key::CloseBracket) - presses(Key::OpenBracket);
            if size_steps != 0 {
                let settings = self.tools.settings_mut();
                settings.size =
                    (settings.size * BRUSH_SIZE_STEP.powi(size_steps)).clamp(MIN_SIZE, MAX_SIZE);
            }
            for (key, tool) in [
                (Key::B, Tool::Brush),
                (Key::E, Tool::Eraser),
                (Key::I, Tool::Eyedropper),
                (Key::F, Tool::Fill),
                (Key::G, Tool::Gradient),
                (Key::T, Tool::Text),
            ] {
                if !editing && presses(key) > 0 {
                    self.tools.tool = tool;
                }
            }
        }

        self.pointer_on_canvas = hover.map(|pos| self.view.screen_to_canvas(to_point(pos)));
        space || self.drag.is_some()
    }

    /// Fits the whole canvas into `rect`.
    fn fit(&mut self, document: &Document, rect: Rect) {
        self.view.fit(
            document.width() as f32,
            document.height() as f32,
            to_point(rect.center()),
            Point::new(rect.width(), rect.height()),
        );
        self.fitted_view = Some(self.view);
    }

    /// Keeps the canvas well placed when the window changes size (for
    /// example when you maximize it). If you haven't zoomed or moved since
    /// the canvas was fitted, it's fitted again, so it fills the new space.
    /// Otherwise the same part of the picture stays in the middle, at the
    /// same zoom.
    fn follow_window_size(&mut self, document: &Document, rect: Rect) {
        if rect.width() < 1.0 || rect.height() < 1.0 {
            return; // minimized: wait until there's room again
        }
        if self.needs_fit {
            self.fit(document, rect);
            self.needs_fit = false;
        } else if let Some(old) = self.last_rect
            && old != rect
        {
            if self.fitted_view == Some(self.view) {
                self.fit(document, rect);
            } else {
                self.view.pan(vec_to_point(rect.center() - old.center()));
            }
        }
        self.last_rect = Some(rect);
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

        // Tiles whose combined picture changed: their textures are out of date.
        let overlaid = self.overlay.is_some();
        for coord in document.take_dirty_tiles() {
            if !document.has_visible_tile(coord) && !overlaid {
                self.textures.remove(&coord);
            } else if let Some(texture) = self.textures.get_mut(&coord) {
                texture.outdated = true;
            }
        }

        let mut preview_updates_left = PREVIEW_UPDATES_PER_FRAME;
        for &coord in visible {
            if !document.has_visible_tile(coord) && !overlaid {
                continue;
            }
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

            // All visible layers combined into one tile (but not a layer
            // being transformed: a picture of it is drawn instead).
            let left_out = self.transform.as_ref().map(LayerTransform::layer);
            let shown = |layer: &quill_engine::layer::Layer| {
                layer.visible() && Some(layer.id()) != left_out
            };
            let tile = if overlaid {
                let base = self.paper_with_overlay(document, coord);
                document.composite_tile_over(coord, base, shown)
            } else {
                match document.composite_tile_showing(coord, Background::Paper, shown) {
                    Some(tile) => tile,
                    None => {
                        self.textures.remove(&coord);
                        continue;
                    }
                }
            };
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
        let colour = document.paper();
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

    fn status_bar(&self, ui: &mut egui::Ui, document: &Document, history: &History) {
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
            ui.label(format!("Tiles in memory: {}", document.tile_count()));
            ui.separator();
            let megabytes = history.memory_used() as f64 / (1024.0 * 1024.0);
            ui.label(format!("Undo steps: {} ({megabytes:.0} MB)", history.undo_steps()));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak("Wheel: zoom · Space+drag: pan · Shift+Space+drag: rotate · [ ]: size · Alt: pick colour · 0: fit");
            });
        });
    }
}

/// Draws the brush outline: a circle the size of the brush, in black and
/// white so it shows up on any colour. Tiny brushes get a small cross.
/// A short message in a red box at the top of the canvas.
fn draw_notice(painter: &egui::Painter, canvas: Rect, text: &str) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(15.0),
        Color32::WHITE,
    );
    let padding = vec2(14.0, 8.0);
    let size = galley.size() + 2.0 * padding;
    let top_left = pos2(canvas.center().x - size.x / 2.0, canvas.top() + 16.0);
    let box_rect = Rect::from_min_size(top_left, size);
    painter.rect_filled(
        box_rect,
        6.0,
        Color32::from_rgba_unmultiplied(170, 45, 45, 235),
    );
    painter.galley(top_left + padding, galley, Color32::WHITE);
}

/// Draws the eyedropper: a thick ring showing the colour under the
/// pointer (grey off the canvas), with a small cross in the middle.
fn draw_eyedropper(painter: &egui::Painter, centre: Pos2, colour: Option<[u8; 3]>) {
    let fill = colour.map_or(Color32::from_gray(128), |[r, g, b]| {
        Color32::from_rgb(r, g, b)
    });
    let radius = 18.0;
    painter.circle_stroke(
        centre,
        radius,
        Stroke::new(10.0, Color32::from_black_alpha(160)),
    );
    painter.circle_stroke(centre, radius, Stroke::new(8.0, fill));
    painter.circle_stroke(
        centre,
        radius - 4.5,
        Stroke::new(1.0, Color32::from_white_alpha(200)),
    );
    for stroke in [
        Stroke::new(3.0, Color32::from_black_alpha(140)),
        Stroke::new(1.0, Color32::from_white_alpha(230)),
    ] {
        painter.line_segment([centre - vec2(4.0, 0.0), centre + vec2(4.0, 0.0)], stroke);
        painter.line_segment([centre - vec2(0.0, 4.0), centre + vec2(0.0, 4.0)], stroke);
    }
}

fn draw_brush_outline(painter: &egui::Painter, centre: Pos2, radius: f32) {
    let dark = Stroke::new(3.0, Color32::from_black_alpha(140));
    let light = Stroke::new(1.0, Color32::from_white_alpha(230));
    if radius >= 3.0 {
        painter.circle_stroke(centre, radius, dark);
        painter.circle_stroke(centre, radius, light);
    } else {
        for stroke in [dark, light] {
            painter.line_segment([centre - vec2(6.0, 0.0), centre + vec2(6.0, 0.0)], stroke);
            painter.line_segment([centre - vec2(0.0, 6.0), centre + vec2(0.0, 6.0)], stroke);
        }
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
        assert_eq!(preview_level(0.0001), quill_engine::tile::MAX_PREVIEW_LEVEL);
    }
}
