//! The painting tools: turn pen and mouse input into strokes (brush and
//! eraser), fills (paint bucket) and gradients.
//!
//! The pen comes from Windows Ink (`pen_input.rs`), with pressure. The
//! mouse comes from egui and always paints at full pressure. Fingers on a
//! touchscreen don't paint; they zoom and pan the canvas instead.
//!
//! Turning the pen round to its eraser end switches to the eraser, and
//! turning it back switches back to the tool you had before.
//!
//! The eyedropper picks a colour from the canvas instead of painting. You
//! can choose it as a tool, or hold Alt (or the pen's side button) to pick
//! a colour while using any tool.
//!
//! The paint bucket fills where you click. The gradient tool is dragged:
//! from where the gradient starts to where it ends.

use std::time::{Duration, Instant};

use art_engine::brush::{BrushMode, BrushSettings};
use art_engine::document::Document;
use art_engine::fill::{self, FloodFill, Gradient, GradientShape, Paint, SampleFrom, flood_fill};
use art_engine::history::{History, TileCommand};
use art_engine::layer::LayerId;
use art_engine::pen::InputKind;
use art_engine::script::record;
use art_engine::smoothing::Smoother;
use art_engine::stroke::{Stroke, StrokePoint};
use art_engine::view::{Point, View};
use eframe::egui::{self, PointerButton, Pos2, Rect, pos2};

use crate::color_panel::MAX_RECENT_COLORS;
use crate::pen_input::InkEvent;

/// After the pen or a finger was used, ignore the mouse for this long.
/// (Windows also moves the mouse pointer when you use the pen or a finger.)
const INK_TAKES_PRIORITY_FOR: Duration = Duration::from_millis(300);

/// How long the "this layer is hidden" notice stays up.
const NOTICE_SHOWS_FOR: Duration = Duration::from_secs(3);

/// At 100% smoothing the line is pulled on a string this long, in screen
/// points. It's measured on screen so smoothing feels the same at any zoom.
const LONGEST_SMOOTHING_STRING: f32 = 60.0;

/// The tools you can paint with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Brush,
    Eraser,
    /// Picks a colour from the canvas, then switches back to the brush.
    Eyedropper,
    /// The paint bucket.
    Fill,
    Gradient,
}

/// The paint bucket's settings.
pub struct FillSettings {
    /// How different a colour may be and still be filled (0..1).
    pub tolerance: f32,
    /// Look at all visible layers (true) or only the selected one (false)
    /// to decide where the fill stops.
    pub all_layers: bool,
    /// Grow the fill by this many pixels, under the edges of lines.
    pub grow: u32,
    pub opacity: f32,
}

impl Default for FillSettings {
    fn default() -> Self {
        Self {
            tolerance: 0.1,
            all_layers: true,
            grow: 1,
            opacity: 1.0,
        }
    }
}

/// What a gradient fades to, from the brush colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientEnd {
    Transparent,
    SecondColor,
}

/// The gradient tool's settings.
pub struct GradientSettings {
    pub shape: GradientShape,
    pub end: GradientEnd,
    pub second_color: [u8; 3],
    pub opacity: f32,
}

impl Default for GradientSettings {
    fn default() -> Self {
        Self {
            shape: GradientShape::Linear,
            end: GradientEnd::Transparent,
            second_color: [255, 255, 255],
            opacity: 1.0,
        }
    }
}

/// A gradient being dragged out: from `start` to `end`, in canvas pixels.
struct GradientDrag {
    source: Source,
    start: Point,
    end: Point,
}

/// Holding Shift while dragging a gradient keeps its angle to steps of this.
const GRADIENT_ANGLE_STEP_DEGREES: f32 = 15.0;

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Pen,
    Mouse,
}

/// Where the canvas is on screen, and what the user is doing with it.
pub struct CanvasPlacement<'a> {
    /// The canvas area on screen.
    pub rect: Rect,
    /// How the canvas is zoomed, panned and rotated.
    pub view: &'a View,
    /// The user is panning or rotating, so no new stroke should start.
    pub navigating: bool,
}

/// A stroke in progress.
struct ActiveStroke {
    /// What undo calls this stroke, e.g. "Brush stroke".
    name: &'static str,
    /// The layer being painted on.
    layer: LayerId,
    /// The brush it's painted with.
    settings: BrushSettings,
    stroke: Stroke,
    /// Every point that went into the stroke (after smoothing), so it can
    /// be written down for "Export strokes".
    fed: Vec<StrokePoint>,
    smoother: Smoother,
    source: Source,
    /// The latest point from the pen or mouse (before smoothing).
    last_point: StrokePoint,
}

impl ActiveStroke {
    fn add(&mut self, document: &mut Document, point: StrokePoint) {
        self.last_point = point;
        let Some(layer) = document.layer_mut(self.layer) else {
            return;
        };
        if let Some(smoothed) = self.smoother.add(point) {
            self.stroke.add_point(&mut layer.surface, smoothed);
            self.fed.push(smoothed);
        }
    }

    fn finish(mut self, document: &mut Document, history: &mut History) {
        let Some(layer) = document.layer_mut(self.layer) else {
            return;
        };
        // Draw the last bit, so the line ends where the pen was lifted.
        if let Some(last) = self.smoother.finish(self.last_point) {
            self.stroke.add_point(&mut layer.surface, last);
            self.fed.push(last);
        }
        // Record the change so it can be undone (unless nothing changed,
        // e.g. erasing an empty area).
        let name = layer.name().to_owned();
        let tiles_before = self.stroke.finish(&mut layer.surface);
        if !tiles_before.is_empty() {
            history.push_recorded(
                Box::new(TileCommand::new(self.name, self.layer, tiles_before)),
                record::stroke(&name, &self.settings, &self.fed),
            );
        }
    }
}

pub struct PaintTool {
    /// The selected tool.
    pub tool: Tool,
    /// Each tool remembers its own settings (size, softness, ...).
    pub brush: BrushSettings,
    pub eraser: BrushSettings,
    /// How much lines are steadied: 0 = not at all, 1 = the most.
    pub smoothing: f32,
    pub fill: FillSettings,
    pub gradient: GradientSettings,
    /// Colours recently painted with, newest first.
    pub recent_colors: Vec<[u8; 3]>,
    /// While the pen is turned round to its eraser end: the tool that was
    /// selected before, to go back to when it's turned back.
    tool_before_pen_flip: Option<Tool>,
    stroke: Option<ActiveStroke>,
    gradient_drag: Option<GradientDrag>,
    /// The pen or mouse button is still down after a click that already
    /// did its job (a fill): wait for it to be lifted.
    waiting_for_lift: Option<Source>,
    /// Set while a colour is being picked (the pen or mouse button is down).
    picking: Option<Source>,
    last_ink_event: Option<Instant>,
    /// Where the pen hovers, in screen points, while it is near the tablet.
    pen_position: Option<Pos2>,
    /// The pen's side button is held.
    pen_side_button: bool,
    /// When the user last tried to paint on a hidden layer.
    hidden_layer_attempt: Option<Instant>,
}

impl Default for PaintTool {
    fn default() -> Self {
        Self {
            tool: Tool::Brush,
            brush: BrushSettings::default(),
            eraser: BrushSettings::eraser(),
            smoothing: 0.25,
            fill: FillSettings::default(),
            gradient: GradientSettings::default(),
            recent_colors: Vec::new(),
            tool_before_pen_flip: None,
            stroke: None,
            gradient_drag: None,
            waiting_for_lift: None,
            picking: None,
            last_ink_event: None,
            pen_position: None,
            pen_side_button: false,
            hidden_layer_attempt: None,
        }
    }
}

impl PaintTool {
    /// The selected tool's brush settings (the eyedropper, paint bucket
    /// and gradient use the brush's colour).
    pub fn settings(&self) -> &BrushSettings {
        match self.tool {
            Tool::Eraser => &self.eraser,
            _ => &self.brush,
        }
    }

    pub fn settings_mut(&mut self) -> &mut BrushSettings {
        match self.tool {
            Tool::Eraser => &mut self.eraser,
            _ => &mut self.brush,
        }
    }

    /// True for the tools that work at one point (paint bucket, gradient)
    /// rather than with a round brush: they show a cross as the pointer.
    pub fn uses_crosshair(&self) -> bool {
        matches!(self.tool, Tool::Fill | Tool::Gradient)
    }

    /// The gradient being dragged out, in canvas pixels, to show on screen.
    pub fn gradient_line(&self) -> Option<(Point, Point)> {
        self.gradient_drag
            .as_ref()
            .map(|drag| (drag.start, drag.end))
    }

    /// True if pressing on the canvas now would pick a colour rather than
    /// paint: the eyedropper is selected, or Alt or the pen's side button is held.
    pub fn eyedropper_active(&self, ui: &egui::Ui) -> bool {
        self.tool == Tool::Eyedropper
            || self.picking.is_some()
            || ui.input(|i| i.modifiers.alt)
            || (self.pen_side_button && self.pen_position.is_some())
    }

    /// Reads this frame's pen and mouse input and paints. Finished strokes
    /// are recorded in `history`.
    pub fn handle_input(
        &mut self,
        ui: &egui::Ui,
        placement: CanvasPlacement,
        document: &mut Document,
        history: &mut History,
        ink_events: Vec<InkEvent>,
    ) {
        let CanvasPlacement {
            rect,
            view,
            navigating,
        } = placement;
        let ctx = ui.ctx();
        let now = Instant::now();
        let points_per_pixel = 1.0 / ctx.pixels_per_point();
        let canvas_layer = ui.layer_id();
        // On the canvas, and not under a pop-up (like the colour picker).
        let on_canvas =
            |pos: Pos2| rect.contains(pos) && ctx.layer_id_at(pos) == Some(canvas_layer);
        let to_canvas = |pos: Pos2, pressure: f32| {
            let point = view.screen_to_canvas(Point::new(pos.x, pos.y));
            StrokePoint::new(point.x, point.y, pressure)
        };
        let (alt_held, shift_held) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));

        // 1. The pen.
        for event in ink_events {
            self.last_ink_event = Some(now);
            match event.sample.kind {
                InputKind::Touch => continue, // fingers zoom and pan, they don't paint
                InputKind::PenEraser => self.pen_turned_to_eraser(),
                InputKind::Pen => self.pen_turned_to_tip(),
                InputKind::Mouse => {}
            }
            let pos = pos2(
                event.sample.x * points_per_pixel,
                event.sample.y * points_per_pixel,
            );
            self.pen_position = event.in_range.then_some(pos);
            self.pen_side_button = event.barrel_button;
            let point = to_canvas(pos, event.sample.pressure);

            let pen_is_painting =
                matches!(&self.stroke, Some(active) if active.source == Source::Pen);
            let pen_is_dragging =
                matches!(&self.gradient_drag, Some(drag) if drag.source == Source::Pen);
            if pen_is_dragging {
                if event.in_contact {
                    self.drag_gradient_to(point, shift_held);
                } else {
                    self.finish_gradient(document, history);
                }
            } else if self.waiting_for_lift == Some(Source::Pen) {
                if !event.in_contact {
                    self.waiting_for_lift = None;
                }
            } else if pen_is_painting {
                if event.in_contact {
                    if let Some(active) = &mut self.stroke {
                        active.add(document, point);
                    }
                } else {
                    self.finish_stroke(document, history); // the pen was lifted
                }
            } else if self.picking == Some(Source::Pen) {
                if event.in_contact {
                    self.pick(document, point);
                } else {
                    self.finish_picking();
                }
            } else if self.is_free() && event.in_contact && !navigating && on_canvas(pos) {
                // The pen touched the canvas: pick a colour, or start painting.
                if self.tool == Tool::Eyedropper || alt_held || event.barrel_button {
                    self.picking = Some(Source::Pen);
                    self.pick(document, point);
                } else {
                    self.press(Source::Pen, view, document, history, point);
                }
            }
        }

        // 2. The mouse, unless the pen or a finger was just used.
        let ink_is_recent = self
            .last_ink_event
            .is_some_and(|time| now - time < INK_TAKES_PRIORITY_FOR);
        let pen_is_busy = matches!(&self.stroke, Some(active) if active.source == Source::Pen)
            || matches!(&self.gradient_drag, Some(drag) if drag.source == Source::Pen)
            || self.waiting_for_lift == Some(Source::Pen)
            || self.picking == Some(Source::Pen);
        if ink_is_recent || pen_is_busy {
            return;
        }

        let (events, primary_down) = ui.input(|i| (i.events.clone(), i.pointer.primary_down()));
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    ..
                } => {
                    if self.is_free() && !navigating && on_canvas(pos) {
                        let point = to_canvas(pos, 1.0);
                        if self.tool == Tool::Eyedropper || alt_held {
                            self.picking = Some(Source::Mouse);
                            self.pick(document, point);
                        } else {
                            self.press(Source::Mouse, view, document, history, point);
                        }
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    if let Some(active) = &mut self.stroke {
                        active.add(document, to_canvas(pos, 1.0));
                    } else if self.gradient_drag.is_some() {
                        self.drag_gradient_to(to_canvas(pos, 1.0), shift_held);
                    } else if self.picking.is_some() {
                        self.pick(document, to_canvas(pos, 1.0));
                    }
                }
                egui::Event::PointerButton {
                    button: PointerButton::Primary,
                    pressed: false,
                    ..
                }
                | egui::Event::PointerGone => {
                    self.release_mouse(document, history);
                }
                _ => {}
            }
        }
        // In case the button was let go outside the window.
        if !primary_down {
            self.release_mouse(document, history);
        }
    }

    /// Nothing is going on: a new press can start something.
    fn is_free(&self) -> bool {
        self.stroke.is_none()
            && self.gradient_drag.is_none()
            && self.picking.is_none()
            && self.waiting_for_lift.is_none()
    }

    /// The mouse button was let go: finish whatever it was doing.
    fn release_mouse(&mut self, document: &mut Document, history: &mut History) {
        self.finish_stroke(document, history);
        self.finish_gradient(document, history);
        self.finish_picking();
        if self.waiting_for_lift == Some(Source::Mouse) {
            self.waiting_for_lift = None;
        }
    }

    /// The pen or mouse button went down on the canvas: start the
    /// selected tool there.
    fn press(
        &mut self,
        source: Source,
        view: &View,
        document: &mut Document,
        history: &mut History,
        point: StrokePoint,
    ) {
        // You can't see what you paint on a hidden layer, so don't.
        if !document.active_layer().visible() {
            self.hidden_layer_attempt = Some(Instant::now());
            self.waiting_for_lift = Some(source);
            return;
        }
        match self.tool {
            Tool::Fill => {
                self.fill_at(document, history, point);
                self.waiting_for_lift = Some(source);
            }
            Tool::Gradient => {
                let at = Point::new(point.x, point.y);
                self.gradient_drag = Some(GradientDrag {
                    source,
                    start: at,
                    end: at,
                });
            }
            _ => self.start_stroke(source, view, document, point),
        }
    }

    /// The paint bucket: fills around `point` with the brush colour.
    fn fill_at(&mut self, document: &mut Document, history: &mut History, point: StrokePoint) {
        let settings = &self.fill;
        let fill = FloodFill {
            x: point.x,
            y: point.y,
            tolerance: settings.tolerance,
            sample: if settings.all_layers {
                SampleFrom::Visible
            } else {
                SampleFrom::Layer
            },
            expand: settings.grow,
        };
        let [r, g, b] = self.brush.color;
        let layer = document.active_id();
        let name = document.active_layer().name().to_owned();
        let opacity = settings.opacity;
        let paint = Paint::Color([r, g, b, 255]);
        let tiles_before = flood_fill(document, layer, &fill, &paint, opacity);
        if !tiles_before.is_empty() {
            history.push_recorded(
                Box::new(TileCommand::new("Fill", layer, tiles_before)),
                record::bucket(&name, &fill, [r, g, b, 255], opacity),
            );
            self.remember_color(self.brush.color);
        }
    }

    /// Moves the end of the gradient being dragged. With Shift held, its
    /// angle snaps to steps of 15°, for straight horizontal or vertical
    /// gradients.
    fn drag_gradient_to(&mut self, point: StrokePoint, snap: bool) {
        let Some(drag) = &mut self.gradient_drag else {
            return;
        };
        let mut end = Point::new(point.x, point.y);
        if snap {
            let offset = end - drag.start;
            let length = offset.x.hypot(offset.y);
            let step = GRADIENT_ANGLE_STEP_DEGREES.to_radians();
            let angle = (offset.y.atan2(offset.x) / step).round() * step;
            end = drag.start + Point::new(angle.cos() * length, angle.sin() * length);
        }
        drag.end = end;
    }

    /// The gradient was let go: paint it on the selected layer.
    fn finish_gradient(&mut self, document: &mut Document, history: &mut History) {
        let Some(drag) = self.gradient_drag.take() else {
            return;
        };
        let offset = drag.end - drag.start;
        if offset.x.hypot(offset.y) < 1.0 {
            return; // a click without dragging: no direction to go in
        }
        let settings = &self.gradient;
        let [r, g, b] = self.brush.color;
        let end = match settings.end {
            GradientEnd::Transparent => [r, g, b, 0],
            GradientEnd::SecondColor => {
                let [r, g, b] = settings.second_color;
                [r, g, b, 255]
            }
        };
        let gradient = Gradient::new(
            settings.shape,
            (drag.start.x, drag.start.y),
            (drag.end.x, drag.end.y),
            &[[r, g, b, 255], end],
        );
        let layer = document.active_id();
        let name = document.active_layer().name().to_owned();
        let script = record::gradient(&name, &gradient, settings.opacity);
        let tiles_before = fill::fill_layer(
            document,
            layer,
            &Paint::Gradient(gradient),
            settings.opacity,
        );
        if !tiles_before.is_empty() {
            history.push_recorded(
                Box::new(TileCommand::new("Gradient", layer, tiles_before)),
                script,
            );
            self.remember_color(self.brush.color);
        }
    }

    /// Takes the colour under `point` as the brush colour.
    fn pick(&mut self, document: &Document, point: StrokePoint) {
        if let Some(color) = document.sample_color(point.x, point.y) {
            self.brush.color = color;
        }
    }

    /// The button was let go after picking. The eyedropper tool hands
    /// over to the brush, ready to paint with the new colour.
    fn finish_picking(&mut self) {
        if self.picking.take().is_some() && self.tool == Tool::Eyedropper {
            self.tool = Tool::Brush;
        }
    }

    /// The pen's eraser end is in use: switch to the eraser (once).
    fn pen_turned_to_eraser(&mut self) {
        if self.tool_before_pen_flip.is_none() {
            self.tool_before_pen_flip = Some(self.tool);
            self.tool = Tool::Eraser;
        }
    }

    /// The pen's tip is in use again: go back to the tool from before.
    fn pen_turned_to_tip(&mut self) {
        if let Some(tool) = self.tool_before_pen_flip.take() {
            self.tool = tool;
        }
    }

    /// Where to draw the brush outline (screen points), if anywhere.
    pub fn outline_position(&self, ui: &egui::Ui, rect: Rect) -> Option<Pos2> {
        // A hovering pen may not move the mouse pointer, so prefer the pen.
        let position = self
            .pen_position
            .or_else(|| ui.input(|i| i.pointer.hover_pos()))?;
        rect.contains(position).then_some(position)
    }

    fn start_stroke(
        &mut self,
        source: Source,
        view: &View,
        document: &mut Document,
        point: StrokePoint,
    ) {
        let string_length = self.smoothing * LONGEST_SMOOTHING_STRING / view.zoom();
        let settings = *self.settings();
        if settings.mode == BrushMode::Paint {
            self.remember_color(settings.color);
        }
        let mut active = ActiveStroke {
            name: match settings.mode {
                BrushMode::Paint => "Brush stroke",
                BrushMode::Erase => "Eraser stroke",
            },
            layer: document.active_id(),
            settings,
            stroke: Stroke::new(settings, document.bounds()),
            fed: Vec::new(),
            smoother: Smoother::new(string_length),
            source,
            last_point: point,
        };
        active.add(document, point);
        self.stroke = Some(active);
    }

    fn finish_stroke(&mut self, document: &mut Document, history: &mut History) {
        if let Some(active) = self.stroke.take() {
            active.finish(document, history);
        }
    }

    /// Puts `color` at the front of the recent colours.
    fn remember_color(&mut self, color: [u8; 3]) {
        self.recent_colors.retain(|&c| c != color);
        self.recent_colors.insert(0, color);
        self.recent_colors.truncate(MAX_RECENT_COLORS);
    }

    /// True for a few seconds after trying to paint on a hidden layer, so
    /// the canvas can explain why nothing happened.
    pub fn show_hidden_layer_notice(&self) -> bool {
        self.hidden_layer_attempt
            .is_some_and(|time| time.elapsed() < NOTICE_SHOWS_FOR)
    }

    /// True while a stroke is being painted or a gradient dragged (undo
    /// waits until it's done).
    pub fn is_painting(&self) -> bool {
        self.stroke.is_some() || self.gradient_drag.is_some()
    }
}
