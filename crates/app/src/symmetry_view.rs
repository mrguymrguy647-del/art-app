//! Symmetry for the brush and eraser: what's painted on one side of a
//! mirror line is painted on the other side too. The lines are drawn on
//! the canvas; drag the round handle at a line's end to move it (it
//! snaps to the middle of the canvas).

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, pos2};
use quill_engine::stroke::Symmetry;
use quill_engine::view::{Point, View};

use crate::pen_input::InkEvent;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SymmetryMode {
    #[default]
    Off,
    /// Left and right mirror each other (a line going down).
    LeftRight,
    /// Top and bottom mirror each other (a line going across).
    TopBottom,
    Both,
}

impl SymmetryMode {
    pub const ALL: [SymmetryMode; 4] = [
        SymmetryMode::Off,
        SymmetryMode::LeftRight,
        SymmetryMode::TopBottom,
        SymmetryMode::Both,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SymmetryMode::Off => "Off",
            SymmetryMode::LeftRight => "Left–right",
            SymmetryMode::TopBottom => "Top–bottom",
            SymmetryMode::Both => "Both",
        }
    }

    fn vertical(self) -> bool {
        matches!(self, SymmetryMode::LeftRight | SymmetryMode::Both)
    }

    fn horizontal(self) -> bool {
        matches!(self, SymmetryMode::TopBottom | SymmetryMode::Both)
    }
}

/// The mode, and where the lines cross as a fraction of the canvas (so
/// they stay in place on a new or resized picture).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SymmetrySettings {
    pub mode: SymmetryMode,
    pub centre: (f32, f32),
}

impl Default for SymmetrySettings {
    fn default() -> Self {
        Self {
            mode: SymmetryMode::Off,
            centre: (0.5, 0.5),
        }
    }
}

impl SymmetrySettings {
    /// The mirror lines on a canvas this size, if symmetry is on.
    pub fn on_canvas(&self, width: u32, height: u32) -> Option<Symmetry> {
        (self.mode != SymmetryMode::Off).then(|| Symmetry {
            centre: (self.centre.0 * width as f32, self.centre.1 * height as f32),
            vertical: self.mode.vertical(),
            horizontal: self.mode.horizontal(),
            angle: 0.0,
        })
    }

    /// Puts the lines back in the middle.
    pub fn centre_lines(&mut self) {
        self.centre = (0.5, 0.5);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Line {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, PartialEq)]
enum By {
    Pen,
    Mouse,
}

/// The handles that move the lines.
#[derive(Default)]
pub struct MirrorHandles {
    dragging: Option<(Line, By)>,
}

/// How near (screen points) the pointer must be to grab a handle.
const GRAB: f32 = 12.0;
/// A line dragged this near (screen points) the canvas middle snaps to it.
const SNAP: f32 = 8.0;

impl MirrorHandles {
    /// Moves a line if a handle is dragged. Uses (and removes) the pen
    /// events that do; returns true while dragging, when the painting
    /// tools must leave the pointer alone.
    #[allow(clippy::too_many_arguments)]
    pub fn handle_input(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        view: &View,
        canvas: (u32, u32),
        settings: &mut SymmetrySettings,
        ink_events: &mut Vec<InkEvent>,
        can_start: bool,
    ) -> bool {
        let Some(symmetry) = settings.on_canvas(canvas.0, canvas.1) else {
            self.dragging = None;
            return false;
        };
        let handles = handles(&symmetry, rect, view, canvas);
        let points_per_pixel = 1.0 / ui.ctx().pixels_per_point();
        // The pen.
        ink_events.retain(|event| {
            let pos = pos2(
                event.sample.x * points_per_pixel,
                event.sample.y * points_per_pixel,
            );
            match self.dragging {
                Some((line, By::Pen)) => {
                    drag_to(settings, line, pos, view, canvas);
                    if !event.in_contact {
                        self.dragging = None;
                    }
                    false
                }
                Some((_, By::Mouse)) => true,
                None if can_start && event.in_contact => match grabbed(&handles, pos) {
                    Some(line) => {
                        self.dragging = Some((line, By::Pen));
                        false
                    }
                    None => true,
                },
                None => true,
            }
        });
        // The mouse.
        // Grab where the button went down: the pointer may already have
        // moved on by the time the frame sees the press.
        let (pressed, down, pos, origin) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.interact_pos(),
                i.pointer.press_origin(),
            )
        });
        match (self.dragging, pos) {
            (Some((line, By::Mouse)), Some(pos)) => {
                drag_to(settings, line, pos, view, canvas);
                if !down {
                    self.dragging = None;
                }
            }
            (Some((_, By::Mouse)), None) => self.dragging = None,
            (None, Some(pos)) if pressed && can_start => {
                if let Some(line) = grabbed(&handles, origin.unwrap_or(pos)) {
                    self.dragging = Some((line, By::Mouse));
                }
            }
            _ => {}
        }
        if self.dragging.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if pos.is_some_and(|pos| grabbed(&handles, pos).is_some()) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        self.dragging.is_some()
    }

    /// The lines, dashed, with their handles.
    pub fn draw(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        view: &View,
        canvas: (u32, u32),
        settings: &SymmetrySettings,
        accent: Color32,
    ) {
        let Some(symmetry) = settings.on_canvas(canvas.0, canvas.1) else {
            return;
        };
        let (cx, cy) = symmetry.centre;
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let mut lines = Vec::new();
        if symmetry.vertical {
            lines.push((Line::Vertical, [Point::new(cx, 0.0), Point::new(cx, h)]));
        }
        if symmetry.horizontal {
            lines.push((Line::Horizontal, [Point::new(0.0, cy), Point::new(w, cy)]));
        }
        let handles = handles(&symmetry, rect, view, canvas);
        for (line, ends) in lines {
            let [a, b] = ends.map(|p| {
                let p = view.canvas_to_screen(p);
                pos2(p.x, p.y)
            });
            painter.line_segment([a, b], Stroke::new(3.0, Color32::from_black_alpha(90)));
            painter.add(egui::Shape::dashed_line(
                &[a, b],
                Stroke::new(1.5, accent),
                8.0,
                5.0,
            ));
            let dragged = self.dragging.is_some_and(|(l, _)| l == line);
            if let Some((_, at)) = handles.iter().find(|(l, _)| *l == line) {
                let radius = if dragged { 8.0 } else { 6.5 };
                painter.circle_filled(*at, radius + 1.5, Color32::from_black_alpha(140));
                painter.circle_filled(*at, radius, accent);
                painter.circle_filled(*at, 2.5, Color32::WHITE);
            }
        }
    }
}

/// Each line's handle on screen: at the line's top (or left) end, or
/// where it comes into view if that end is off screen.
fn handles(symmetry: &Symmetry, rect: Rect, view: &View, canvas: (u32, u32)) -> Vec<(Line, Pos2)> {
    let (cx, cy) = symmetry.centre;
    let (w, h) = (canvas.0 as f32, canvas.1 as f32);
    let screen = |p: Point| {
        let p = view.canvas_to_screen(p);
        pos2(p.x, p.y)
    };
    let inside = rect.shrink(GRAB);
    let mut out = Vec::new();
    if symmetry.vertical
        && let Some(at) = first_inside(
            screen(Point::new(cx, 0.0)),
            screen(Point::new(cx, h)),
            inside,
        )
    {
        out.push((Line::Vertical, at));
    }
    if symmetry.horizontal
        && let Some(at) = first_inside(
            screen(Point::new(0.0, cy)),
            screen(Point::new(w, cy)),
            inside,
        )
    {
        out.push((Line::Horizontal, at));
    }
    out
}

/// The first point of the segment a→b that's inside `rect`.
fn first_inside(a: Pos2, b: Pos2, rect: Rect) -> Option<Pos2> {
    if rect.contains(a) {
        return Some(a);
    }
    // Where the segment enters the rectangle (Liang–Barsky).
    let d = b - a;
    let (mut enter, mut leave) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-d.x, a.x - rect.left()),
        (d.x, rect.right() - a.x),
        (-d.y, a.y - rect.top()),
        (d.y, rect.bottom() - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                enter = enter.max(t);
            } else {
                leave = leave.min(t);
            }
        }
    }
    (enter <= leave).then(|| a + d * enter)
}

fn grabbed(handles: &[(Line, Pos2)], pos: Pos2) -> Option<Line> {
    handles
        .iter()
        .filter(|(_, at)| at.distance(pos) <= GRAB)
        .min_by(|a, b| a.1.distance(pos).total_cmp(&b.1.distance(pos)))
        .map(|(line, _)| *line)
}

/// Moves `line` to where the pointer is, on the canvas (snapping to its
/// middle when near it).
fn drag_to(
    settings: &mut SymmetrySettings,
    line: Line,
    pos: Pos2,
    view: &View,
    canvas: (u32, u32),
) {
    let on_canvas = view.screen_to_canvas(Point::new(pos.x, pos.y));
    let snap = SNAP / view.zoom();
    let fraction = |at: f32, size: u32| {
        let size = size as f32;
        let at = if (at - size / 2.0).abs() <= snap {
            size / 2.0
        } else {
            at.round()
        };
        (at / size).clamp(0.0, 1.0)
    };
    match line {
        Line::Vertical => settings.centre.0 = fraction(on_canvas.x, canvas.0),
        Line::Horizontal => settings.centre.1 = fraction(on_canvas.y, canvas.1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_give_lines_on_the_canvas() {
        let mut settings = SymmetrySettings::default();
        assert!(settings.on_canvas(400, 300).is_none(), "off");
        settings.mode = SymmetryMode::Both;
        settings.centre = (0.25, 0.5);
        let symmetry = settings.on_canvas(400, 300).unwrap();
        assert_eq!(symmetry.centre, (100.0, 150.0));
        assert!(symmetry.vertical && symmetry.horizontal);
        settings.mode = SymmetryMode::LeftRight;
        let symmetry = settings.on_canvas(400, 300).unwrap();
        assert!(symmetry.vertical && !symmetry.horizontal);
        settings.centre_lines();
        assert_eq!(settings.on_canvas(400, 300).unwrap().centre, (200.0, 150.0));
    }

    #[test]
    fn dragging_moves_the_line_and_snaps_to_the_middle() {
        let view = View::default(); // canvas pixels are screen points
        let mut settings = SymmetrySettings {
            mode: SymmetryMode::Both,
            ..SymmetrySettings::default()
        };
        drag_to(
            &mut settings,
            Line::Vertical,
            pos2(100.0, 50.0),
            &view,
            (400, 300),
        );
        assert_eq!(settings.centre.0, 0.25);
        drag_to(
            &mut settings,
            Line::Vertical,
            pos2(204.0, 50.0),
            &view,
            (400, 300),
        );
        assert_eq!(settings.centre.0, 0.5, "snapped to the middle");
        drag_to(
            &mut settings,
            Line::Horizontal,
            pos2(10.0, 900.0),
            &view,
            (400, 300),
        );
        assert_eq!(settings.centre.1, 1.0, "kept on the canvas");
    }

    #[test]
    fn a_handle_sits_where_its_line_comes_into_view() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 100.0));
        // The top end is in view: the handle is there.
        assert_eq!(
            first_inside(pos2(50.0, 10.0), pos2(50.0, 300.0), rect),
            Some(pos2(50.0, 10.0))
        );
        // It starts above the view: the handle is at the view's top edge.
        assert_eq!(
            first_inside(pos2(50.0, -200.0), pos2(50.0, 300.0), rect),
            Some(pos2(50.0, 0.0))
        );
        // Never in view: no handle.
        assert_eq!(
            first_inside(pos2(150.0, -200.0), pos2(150.0, 300.0), rect),
            None
        );
    }
}
