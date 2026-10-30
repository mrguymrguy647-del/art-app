//! The brush tool: turns pen and mouse input into brush strokes.
//!
//! The pen comes from Windows Ink (`pen_input.rs`), with pressure. The
//! mouse comes from egui and always paints at full pressure. Fingers on a
//! touchscreen don't paint; they zoom and pan the canvas instead.

use std::time::{Duration, Instant};

use art_engine::brush::BrushSettings;
use art_engine::document::Document;
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

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Pen,
    Mouse,
}

/// A stroke in progress.
struct ActiveStroke {
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

    fn finish(mut self, document: &mut Document) {
        // Draw the last bit, so the line ends where the pen was lifted.
        if let Some(last) = self.smoother.finish(self.last_point) {
            self.stroke.add_point(&mut document.layer, last);
        }
        // The tiles as they were before this stroke. Undo (Step 5) will
        // keep these; for now we let them go.
        let _tiles_before = self.stroke.finish(&mut document.layer);
    }
}

pub struct BrushTool {
    pub brush: BrushSettings,
    /// How much lines are steadied: 0 = not at all, 1 = the most.
    pub smoothing: f32,
    stroke: Option<ActiveStroke>,
    last_ink_event: Option<Instant>,
    /// Where the pen hovers, in screen points, while it is near the tablet.
    pen_position: Option<Pos2>,
}

impl Default for BrushTool {
    fn default() -> Self {
        Self {
            brush: BrushSettings::default(),
            smoothing: 0.25,
            stroke: None,
            last_ink_event: None,
            pen_position: None,
        }
    }
}

impl BrushTool {
    /// Reads this frame's pen and mouse input and paints.
    ///
    /// `rect` is the canvas area on screen. `navigating` means the user is
    /// panning or rotating, so no new stroke should start.
    pub fn handle_input(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        view: &View,
        document: &mut Document,
        ink_events: Vec<InkEvent>,
        navigating: bool,
    ) {
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
            if event.sample.kind == InputKind::Touch {
                continue; // fingers zoom and pan, they don't paint
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
                        self.finish_stroke(document); // the pen was lifted
                    }
                }
                None if event.in_contact
                    && event.sample.kind == InputKind::Pen // the eraser end comes in Step 4
                    && !navigating
                    && on_canvas(pos) =>
                {
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
                | egui::Event::PointerGone => self.finish_stroke(document),
                _ => {}
            }
        }
        // In case the button was let go outside the window.
        if !primary_down {
            self.finish_stroke(document);
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
        let mut active = ActiveStroke {
            stroke: Stroke::new(self.brush, document.bounds()),
            smoother: Smoother::new(string_length),
            source,
            last_point: point,
        };
        active.add(document, point);
        self.stroke = Some(active);
    }

    fn finish_stroke(&mut self, document: &mut Document) {
        if let Some(active) = self.stroke.take() {
            active.finish(document);
        }
    }
}
