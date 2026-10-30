//! The painting tools (brush and eraser): turn pen and mouse input into
//! strokes.
//!
//! The pen comes from Windows Ink (`pen_input.rs`), with pressure. The
//! mouse comes from egui and always paints at full pressure. Fingers on a
//! touchscreen don't paint; they zoom and pan the canvas instead.
//!
//! Turning the pen round to its eraser end switches to the eraser, and
//! turning it back switches back to the tool you had before.

use std::time::{Duration, Instant};

use art_engine::brush::{BrushMode, BrushSettings};
use art_engine::document::Document;
use art_engine::history::{History, TileCommand};
use art_engine::pen::InputKind;
use art_engine::smoothing::Smoother;
use art_engine::stroke::{Stroke, StrokePoint};
use art_engine::view::{Point, View};
use eframe::egui::{self, PointerButton, Pos2, Rect, pos2};

use crate::pen_input::InkEvent;

/// After the pen or a finger was used, ignore the mouse for this long.
/// (Windows also moves the mouse pointer when you use the pen or a finger.)
const INK_TAKES_PRIORITY_FOR: Duration = Duration::from_millis(300);

/// At 100% smoothing the line is pulled on a string this long, in screen
/// points. It's measured on screen so smoothing feels the same at any zoom.
const LONGEST_SMOOTHING_STRING: f32 = 60.0;

/// The tools you can paint with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Brush,
    Eraser,
}

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
    stroke: Stroke,
    smoother: Smoother,
    source: Source,
    /// The latest point from the pen or mouse (before smoothing).
    last_point: StrokePoint,
}

impl ActiveStroke {
    fn add(&mut self, document: &mut Document, point: StrokePoint) {
        self.last_point = point;
        if let Some(smoothed) = self.smoother.add(point) {
            self.stroke.add_point(&mut document.layer, smoothed);
        }
    }

    fn finish(mut self, document: &mut Document, history: &mut History) {
        // Draw the last bit, so the line ends where the pen was lifted.
        if let Some(last) = self.smoother.finish(self.last_point) {
            self.stroke.add_point(&mut document.layer, last);
        }
        // Record the change so it can be undone (unless nothing changed,
        // e.g. erasing an empty area).
        let tiles_before = self.stroke.finish(&mut document.layer);
        if !tiles_before.is_empty() {
            history.push(Box::new(TileCommand::new(self.name, tiles_before)));
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
    /// While the pen is turned round to its eraser end: the tool that was
    /// selected before, to go back to when it's turned back.
    tool_before_pen_flip: Option<Tool>,
    stroke: Option<ActiveStroke>,
    last_ink_event: Option<Instant>,
    /// Where the pen hovers, in screen points, while it is near the tablet.
    pen_position: Option<Pos2>,
}

impl Default for PaintTool {
    fn default() -> Self {
        Self {
            tool: Tool::Brush,
            brush: BrushSettings::default(),
            eraser: BrushSettings::eraser(),
            smoothing: 0.25,
            tool_before_pen_flip: None,
            stroke: None,
            last_ink_event: None,
            pen_position: None,
        }
    }
}

impl PaintTool {
    /// The selected tool's settings.
    pub fn settings(&self) -> &BrushSettings {
        match self.tool {
            Tool::Brush => &self.brush,
            Tool::Eraser => &self.eraser,
        }
    }

    pub fn settings_mut(&mut self) -> &mut BrushSettings {
        match self.tool {
            Tool::Brush => &mut self.brush,
            Tool::Eraser => &mut self.eraser,
        }
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
            let point = to_canvas(pos, event.sample.pressure);

            match &mut self.stroke {
                Some(active) if active.source == Source::Pen => {
                    if event.in_contact {
                        active.add(document, point);
                    } else {
                        self.finish_stroke(document, history); // the pen was lifted
                    }
                }
                None if event.in_contact && !navigating && on_canvas(pos) => {
                    self.start_stroke(Source::Pen, view, document, point);
                }
                _ => {}
            }
        }

        // 2. The mouse, unless the pen or a finger was just used.
        let ink_is_recent = self
            .last_ink_event
            .is_some_and(|time| now - time < INK_TAKES_PRIORITY_FOR);
        let pen_is_painting = matches!(&self.stroke, Some(active) if active.source == Source::Pen);
        if ink_is_recent || pen_is_painting {
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
                    if self.stroke.is_none() && !navigating && on_canvas(pos) {
                        self.start_stroke(Source::Mouse, view, document, to_canvas(pos, 1.0));
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    if let Some(active) = &mut self.stroke {
                        active.add(document, to_canvas(pos, 1.0));
                    }
                }
                egui::Event::PointerButton {
                    button: PointerButton::Primary,
                    pressed: false,
                    ..
                }
                | egui::Event::PointerGone => self.finish_stroke(document, history),
                _ => {}
            }
        }
        // In case the button was let go outside the window.
        if !primary_down {
            self.finish_stroke(document, history);
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
        let mut active = ActiveStroke {
            name: match settings.mode {
                BrushMode::Paint => "Brush stroke",
                BrushMode::Erase => "Eraser stroke",
            },
            stroke: Stroke::new(settings, document.bounds()),
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

    /// True while a stroke is being painted (undo waits until it's done).
    pub fn is_painting(&self) -> bool {
        self.stroke.is_some()
    }
}
