//! The selection tools: rectangle, ellipse, lasso and polygon lasso, and
//! the "marching ants" that show what's selected.
//!
//! Every change of selection is made as a `select` script command (the
//! same one an agent writes) and run with `selection::run`, so it's one
//! undo step and Export strokes replays it exactly.
//!
//! Shift adds to the selection, Alt takes away from it (only while a
//! selection tool is chosen: with the other tools Alt picks a colour).

use std::time::Instant;

use eframe::egui::{self, Color32, Pos2, Shape, Stroke};
use quill_engine::document::Document;
use quill_engine::history::History;
use quill_engine::script::{SelectCmd, record};
use quill_engine::selection::{self, Combine, Selection, SelectionCommand};
use quill_engine::view::{Point, View};
use serde_json::{Value, json};

/// The lasso adds a point when the pen has moved this many screen points.
const LASSO_STEP: f32 = 3.0;
/// A polygon corner this close (screen points) to the first one closes it.
const CLOSE_DISTANCE: f32 = 10.0;
/// Two clicks this quickly, close together, finish a polygon.
const DOUBLE_CLICK_SECONDS: f32 = 0.35;
/// A rectangle or ellipse smaller than this (screen points) is a click:
/// it drops the selection instead.
const CLICK_SIZE: f32 = 3.0;
/// Marching ants: dash length and how fast they march (screen points).
const DASH: f32 = 5.0;
const MARCH_PER_SECOND: f32 = 12.0;

/// Which shape the selection tool draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectShape {
    Rect,
    Ellipse,
    /// Freehand: the pen's path.
    Lasso,
    /// Click corner after corner; double-click, Enter, or a click on the
    /// first corner closes it.
    Polygon,
}

impl SelectShape {
    pub const ALL: [SelectShape; 4] = [
        SelectShape::Rect,
        SelectShape::Ellipse,
        SelectShape::Lasso,
        SelectShape::Polygon,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SelectShape::Rect => "Rectangle (M)",
            SelectShape::Ellipse => "Ellipse (M)",
            SelectShape::Lasso => "Lasso (L)",
            SelectShape::Polygon => "Polygon (L)",
        }
    }
}

/// A selection being drawn, in canvas pixels.
pub struct SelectDrag {
    pub shape: SelectShape,
    pub how: Combine,
    /// Rectangle and ellipse: where the drag started, then where it is.
    /// Lasso: the path. Polygon: the corners so far.
    points: Vec<Point>,
    /// Polygon: where the next corner would go (under the pen).
    hover: Option<Point>,
    /// Polygon: when the last corner was put down, for double-clicks.
    last_click: Option<Instant>,
}

impl SelectDrag {
    pub fn new(shape: SelectShape, how: Combine, at: Point) -> Self {
        let points = match shape {
            SelectShape::Rect | SelectShape::Ellipse => vec![at, at],
            SelectShape::Lasso | SelectShape::Polygon => vec![at],
        };
        Self {
            shape,
            how,
            points,
            hover: None,
            last_click: Some(Instant::now()),
        }
    }

    /// The pen moved to `at` (with the button down, or for a polygon also
    /// just hovering). `zoom` turns screen distances into canvas ones.
    pub fn drag_to(&mut self, at: Point, zoom: f32) {
        match self.shape {
            SelectShape::Rect | SelectShape::Ellipse => self.points[1] = at,
            SelectShape::Lasso => {
                let last = self.points[self.points.len() - 1];
                if (at.x - last.x).hypot(at.y - last.y) * zoom >= LASSO_STEP {
                    self.points.push(at);
                }
            }
            SelectShape::Polygon => self.hover = Some(at),
        }
    }

    /// A polygon click at `at`: adds a corner. True if the polygon is now
    /// closed (a double-click, or a click on the first corner).
    pub fn click(&mut self, at: Point, zoom: f32) -> bool {
        let near = |a: Point, b: Point| (a.x - b.x).hypot(a.y - b.y) * zoom < CLOSE_DISTANCE;
        let quick = self
            .last_click
            .is_some_and(|t| t.elapsed().as_secs_f32() < DOUBLE_CLICK_SECONDS);
        let last = self.points[self.points.len() - 1];
        self.last_click = Some(Instant::now());
        if self.points.len() >= 3 && near(at, self.points[0]) {
            return true;
        }
        if quick && near(at, last) {
            return self.points.len() >= 3;
        }
        self.points.push(at);
        false
    }

    /// Polygon: takes the last corner away (Backspace). False if none is left.
    pub fn undo_corner(&mut self) -> bool {
        self.points.pop();
        !self.points.is_empty()
    }

    /// True if the pen lifting finishes this shape (all but the polygon).
    pub fn ends_on_lift(&self) -> bool {
        self.shape != SelectShape::Polygon
    }

    /// The outline to show while drawing, in canvas pixels.
    pub fn preview(&self) -> Vec<Point> {
        match self.shape {
            SelectShape::Rect => {
                let [a, b] = [self.points[0], self.points[1]];
                vec![a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)]
            }
            SelectShape::Ellipse => {
                let (cx, cy, rx, ry) = self.ellipse();
                (0..64)
                    .map(|i| {
                        let t = i as f32 / 64.0 * std::f32::consts::TAU;
                        Point::new(cx + rx * t.cos(), cy + ry * t.sin())
                    })
                    .collect()
            }
            SelectShape::Lasso => self.points.clone(),
            SelectShape::Polygon => {
                let mut points = self.points.clone();
                points.extend(self.hover);
                points
            }
        }
    }

    fn ellipse(&self) -> (f32, f32, f32, f32) {
        let [a, b] = [self.points[0], self.points[1]];
        (
            (a.x + b.x) / 2.0,
            (a.y + b.y) / 2.0,
            (b.x - a.x).abs() / 2.0,
            (b.y - a.y).abs() / 2.0,
        )
    }

    /// The `select` command this shape is (numbers rounded to a tenth of a
    /// pixel, so Export strokes is easy to read). `None` for a click
    /// without a shape: it drops the selection when replacing it.
    pub fn command(&self, zoom: f32) -> Option<SelectCmd> {
        // Rounded as 64-bit numbers, so 20.3 is written as 20.3.
        let r = |v: f32| (f64::from(v) * 10.0).round() / 10.0;
        let mut command = SelectCmd::default();
        match self.shape {
            SelectShape::Rect | SelectShape::Ellipse => {
                let [a, b] = [self.points[0], self.points[1]];
                if (b.x - a.x).abs() * zoom < CLICK_SIZE || (b.y - a.y).abs() * zoom < CLICK_SIZE {
                    return None;
                }
                if self.shape == SelectShape::Rect {
                    let (left, top) = (a.x.min(b.x), a.y.min(b.y));
                    let (w, h) = ((b.x - a.x).abs(), (b.y - a.y).abs());
                    command.rect = Some(json!([r(left), r(top), r(w), r(h)]));
                } else {
                    let (cx, cy, rx, ry) = self.ellipse();
                    command.ellipse = Some(json!([r(cx), r(cy), r(rx), r(ry)]));
                }
            }
            SelectShape::Lasso | SelectShape::Polygon => {
                if self.points.len() < 3 {
                    return None;
                }
                let points: Vec<Value> = self
                    .points
                    .iter()
                    .map(|p| json!([r(p.x), r(p.y)]))
                    .collect();
                command.polygon = Some(Value::Array(points));
            }
        }
        command.mode = match self.how {
            Combine::Replace => None,
            Combine::Add => Some("add".into()),
            Combine::Subtract => Some("subtract".into()),
        };
        Some(command)
    }

    /// Done: the shape becomes (or changes) the selection. A click with no
    /// shape drops the selection (when replacing it).
    pub fn finish(self, zoom: f32, document: &mut Document, history: &mut History) {
        match self.command(zoom) {
            Some(command) => change(document, history, command, "Select"),
            None if self.how == Combine::Replace && document.selection().is_some() => {
                deselect(document, history);
            }
            None => {}
        }
    }
}

/// Runs `command` on the document's selection as one undo step, recorded
/// for Export strokes. Nothing happens if the selection stays the same.
pub fn change(document: &mut Document, history: &mut History, command: SelectCmd, name: &str) {
    let Ok(new) = selection::run(&command, document) else {
        return; // the app only makes valid commands
    };
    if new.as_ref() == document.selection() {
        return;
    }
    history.push_recorded(
        Box::new(SelectionCommand::set(document, new, name)),
        record::select(command),
    );
}

pub fn deselect(document: &mut Document, history: &mut History) {
    let command = SelectCmd {
        none: Some(true),
        ..SelectCmd::default()
    };
    change(document, history, command, "Deselect");
}

/// The selection's outline, worked out once per selection (it can be
/// thousands of pieces), and drawn as marching ants each frame.
#[derive(Default)]
pub struct Ants {
    /// The selection it was made for (by the mask's address), and its rings.
    made_for: Option<usize>,
    rings: Vec<Vec<(f32, f32)>>,
    start: Option<Instant>,
}

impl Ants {
    /// Draws `selection`'s edge (and `drawing`, a shape being drawn) as
    /// black and white dashes that move slowly: visible on any colour.
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        view: &View,
        selection: Option<&Selection>,
        drawing: Option<(Vec<Point>, bool)>,
    ) {
        let key = selection.map(|s| std::sync::Arc::as_ptr(s.mask()) as usize);
        if key != self.made_for {
            self.rings = selection.map(Selection::outline).unwrap_or_default();
            self.made_for = key;
        }
        if self.rings.is_empty() && drawing.is_none() {
            return;
        }
        let seconds = self
            .start
            .get_or_insert_with(Instant::now)
            .elapsed()
            .as_secs_f32();
        let offset = (seconds * MARCH_PER_SECOND) % (2.0 * DASH);
        let to_screen = |p: Point| {
            let s = view.canvas_to_screen(p);
            Pos2::new(s.x, s.y)
        };
        let mut shapes = Vec::new();
        for ring in &self.rings {
            // On screen, closed, leaving out points too close together to
            // matter (a ring can have thousands of pixel steps).
            let mut points: Vec<Pos2> = Vec::with_capacity(ring.len() + 1);
            for &(x, y) in ring.iter().chain(ring.first()) {
                let p = to_screen(Point::new(x, y));
                if points.last().is_none_or(|last| last.distance(p) >= 1.0) {
                    points.push(p);
                }
            }
            if points.len() >= 2 {
                dashes(&mut shapes, &points, offset);
            }
        }
        if let Some((points, closed)) = drawing {
            let mut screen: Vec<Pos2> = points.into_iter().map(to_screen).collect();
            if closed && let Some(&first) = screen.first() {
                screen.push(first);
            }
            dashes(&mut shapes, &screen, offset);
        }
        painter.extend(shapes);
        // The ants march: draw again soon.
        ctx.request_repaint_after(std::time::Duration::from_millis(80));
    }
}

/// A white line with black dashes over it, the dashes shifted by `offset`.
fn dashes(shapes: &mut Vec<Shape>, points: &[Pos2], offset: f32) {
    shapes.push(Shape::line(
        points.to_vec(),
        Stroke::new(1.0, Color32::WHITE),
    ));
    shapes.extend(Shape::dashed_line_with_offset(
        points,
        Stroke::new(1.0, Color32::BLACK),
        &[DASH],
        &[DASH],
        offset,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dragged_rectangle_becomes_a_select_command() {
        let mut drag = SelectDrag::new(SelectShape::Rect, Combine::Add, Point::new(30.25, 40.0));
        drag.drag_to(Point::new(10.0, 10.04), 1.0);
        let command = drag.command(1.0).unwrap();
        assert_eq!(command.rect, Some(json!([10.0, 10.0, 20.3, 30.0])));
        assert_eq!(command.mode.as_deref(), Some("add"));
        // A click without dragging is no shape.
        let click = SelectDrag::new(SelectShape::Ellipse, Combine::Replace, Point::new(5.0, 5.0));
        assert!(click.command(1.0).is_none());
    }

    #[test]
    fn a_polygon_closes_on_its_first_corner_and_undoes_corners() {
        let mut drag =
            SelectDrag::new(SelectShape::Polygon, Combine::Replace, Point::new(0.0, 0.0));
        drag.last_click = None; // not a double-click
        assert!(!drag.click(Point::new(100.0, 0.0), 1.0));
        drag.last_click = None;
        assert!(!drag.click(Point::new(100.0, 100.0), 1.0));
        drag.last_click = None;
        assert!(
            drag.click(Point::new(2.0, 1.0), 1.0),
            "near the first corner"
        );
        assert_eq!(drag.points.len(), 3);
        assert!(drag.undo_corner());
        assert_eq!(drag.points.len(), 2);
    }

    #[test]
    fn selecting_is_one_undo_step_and_is_recorded() {
        let mut document = Document::new(200, 100);
        let mut history = History::default();
        let mut drag = SelectDrag::new(SelectShape::Rect, Combine::Replace, Point::new(10.0, 10.0));
        drag.drag_to(Point::new(50.0, 40.0), 1.0);
        drag.finish(1.0, &mut document, &mut history);
        assert_eq!(document.selection().unwrap().bounds().left, 10);
        // A click elsewhere drops it.
        SelectDrag::new(SelectShape::Rect, Combine::Replace, Point::new(90.0, 90.0)).finish(
            1.0,
            &mut document,
            &mut history,
        );
        assert!(document.selection().is_none());
        history.undo(&mut document);
        assert!(document.selection().is_some());
        assert_eq!(history.undo_name(), Some("Select"));
    }
}
