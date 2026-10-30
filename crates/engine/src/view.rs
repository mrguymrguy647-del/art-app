//! The view: how the canvas is shown on screen (zoom, rotation, position).
//!
//! It converts between two kinds of positions:
//! - **canvas** positions, in canvas pixels (0,0 = top-left of the picture);
//! - **screen** positions, in the window's units (egui "points").
//!
//! The rule is: screen = origin + rotate(canvas * zoom).
//! `origin` is where canvas pixel (0, 0) appears on screen.

use std::f32::consts::{PI, TAU};
use std::ops::{Add, Mul, Sub};

/// Smallest and largest zoom: 2% and 6400%.
pub const MIN_ZOOM: f32 = 0.02;
pub const MAX_ZOOM: f32 = 64.0;

/// A position or a distance, in 2D.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    /// Turned by `angle` radians. On screen (where y points down) a
    /// positive angle turns clockwise.
    pub fn rotated(self, angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(self.x * cos - self.y * sin, self.x * sin + self.y * cos)
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, other: Point) -> Point {
        Point::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, other: Point) -> Point {
        Point::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f32> for Point {
    type Output = Point;
    fn mul(self, factor: f32) -> Point {
        Point::new(self.x * factor, self.y * factor)
    }
}

/// How the canvas is shown on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// 1.0 = 100%: one canvas pixel is one screen point.
    zoom: f32,
    /// Radians, clockwise on screen, always between -PI and PI.
    rotation: f32,
    /// Where canvas pixel (0, 0) is on screen.
    origin: Point,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            rotation: 0.0,
            origin: Point::default(),
        }
    }
}

impl View {
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Rotation in radians, between -PI and PI. Positive = clockwise.
    pub fn rotation(&self) -> f32 {
        self.rotation
    }

    /// Rotation in degrees, between -180 and 180. Positive = clockwise.
    pub fn rotation_degrees(&self) -> f32 {
        self.rotation.to_degrees()
    }

    pub fn canvas_to_screen(&self, canvas: Point) -> Point {
        self.origin + (canvas * self.zoom).rotated(self.rotation)
    }

    pub fn screen_to_canvas(&self, screen: Point) -> Point {
        (screen - self.origin).rotated(-self.rotation) * (1.0 / self.zoom)
    }

    /// Moves the canvas by `screen_delta` (e.g. how far the mouse moved).
    pub fn pan(&mut self, screen_delta: Point) {
        self.origin = self.origin + screen_delta;
    }

    /// Zooms by `factor` (2.0 = twice as big), keeping the canvas point
    /// under `screen_point` in place, so zooming follows the mouse.
    pub fn zoom_around(&mut self, screen_point: Point, factor: f32) {
        let canvas_point = self.screen_to_canvas(screen_point);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.keep_in_place(canvas_point, screen_point);
    }

    /// Turns the canvas by `angle` radians around `screen_point`.
    pub fn rotate_around(&mut self, screen_point: Point, angle: f32) {
        self.set_rotation_around(screen_point, self.rotation + angle);
    }

    /// Sets the rotation to `angle` radians, turning around `screen_point`.
    pub fn set_rotation_around(&mut self, screen_point: Point, angle: f32) {
        let canvas_point = self.screen_to_canvas(screen_point);
        // Keep the angle between -PI and PI so it doesn't grow forever.
        self.rotation = (angle + PI).rem_euclid(TAU) - PI;
        self.keep_in_place(canvas_point, screen_point);
    }

    /// Shows the whole canvas, unrotated and centred in the viewport, with
    /// a small margin around it.
    pub fn fit(
        &mut self,
        canvas_width: f32,
        canvas_height: f32,
        viewport_center: Point,
        viewport_size: Point,
    ) {
        let margin = 0.92; // use 92% of the space
        let zoom = (viewport_size.x / canvas_width).min(viewport_size.y / canvas_height) * margin;
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.rotation = 0.0;
        let canvas_center = Point::new(canvas_width / 2.0, canvas_height / 2.0);
        self.keep_in_place(canvas_center, viewport_center);
    }

    /// Moves the view so that `canvas_point` appears at `screen_point`.
    fn keep_in_place(&mut self, canvas_point: Point, screen_point: Point) {
        self.origin = screen_point - (canvas_point * self.zoom).rotated(self.rotation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: Point, expected: Point) {
        assert!(
            (actual - expected).length() < 0.01,
            "expected {expected:?}, got {actual:?}"
        );
    }

    /// A view that is zoomed, rotated and moved, to test the general case.
    fn busy_view() -> View {
        let mut view = View::default();
        view.pan(Point::new(120.0, -40.0));
        view.zoom_around(Point::new(300.0, 200.0), 2.5);
        view.rotate_around(Point::new(500.0, 400.0), 0.7);
        view
    }

    #[test]
    fn default_view_is_one_to_one() {
        let view = View::default();
        assert_close(
            view.canvas_to_screen(Point::new(10.0, 20.0)),
            Point::new(10.0, 20.0),
        );
    }

    #[test]
    fn screen_and_canvas_conversions_undo_each_other() {
        let view = busy_view();
        for point in [
            Point::new(0.0, 0.0),
            Point::new(123.0, 456.0),
            Point::new(-50.0, 3000.0),
        ] {
            assert_close(view.screen_to_canvas(view.canvas_to_screen(point)), point);
        }
    }

    #[test]
    fn zooming_keeps_the_point_under_the_mouse_still() {
        let mut view = busy_view();
        let mouse = Point::new(640.0, 360.0);
        let under_mouse = view.screen_to_canvas(mouse);
        view.zoom_around(mouse, 1.25);
        assert_close(view.canvas_to_screen(under_mouse), mouse);
        view.zoom_around(mouse, 0.5);
        assert_close(view.canvas_to_screen(under_mouse), mouse);
    }

    #[test]
    fn rotating_keeps_the_centre_point_still() {
        let mut view = busy_view();
        let centre = Point::new(400.0, 300.0);
        let under_centre = view.screen_to_canvas(centre);
        view.rotate_around(centre, 1.2);
        assert_close(view.canvas_to_screen(under_centre), centre);
    }

    #[test]
    fn positive_rotation_turns_clockwise_on_screen() {
        let mut view = View::default();
        view.rotate_around(Point::new(0.0, 0.0), 90_f32.to_radians());
        // Canvas "right" now points down the screen (y grows downwards).
        assert_close(
            view.canvas_to_screen(Point::new(100.0, 0.0)),
            Point::new(0.0, 100.0),
        );
    }

    #[test]
    fn rotation_wraps_around_to_stay_between_minus_and_plus_180() {
        let mut view = View::default();
        let centre = Point::new(0.0, 0.0);
        for _ in 0..30 {
            view.rotate_around(centre, 15_f32.to_radians()); // 450 degrees in total
        }
        assert!(
            (view.rotation_degrees() - 90.0).abs() < 0.01,
            "{}",
            view.rotation_degrees()
        );
    }

    #[test]
    fn zoom_is_kept_within_limits() {
        let mut view = View::default();
        view.zoom_around(Point::default(), 1_000_000.0);
        assert_eq!(view.zoom(), MAX_ZOOM);
        view.zoom_around(Point::default(), 0.000_001);
        assert_eq!(view.zoom(), MIN_ZOOM);
    }

    #[test]
    fn fit_centres_the_whole_canvas() {
        let mut view = busy_view();
        let viewport_center = Point::new(500.0, 400.0);
        view.fit(4000.0, 3000.0, viewport_center, Point::new(1000.0, 800.0));

        assert_eq!(view.rotation_degrees(), 0.0);
        // Width is the limit: 1000 / 4000 = 0.25, times the 92% margin.
        assert!((view.zoom() - 0.23).abs() < 0.001, "{}", view.zoom());
        assert_close(
            view.canvas_to_screen(Point::new(2000.0, 1500.0)),
            viewport_center,
        );
    }
}
