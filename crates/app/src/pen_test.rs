//! The pen test screen: shows what your pen, mouse or finger reports.
//!
//! Pen and touch readings come from Windows Ink (see `pen_input.rs`).
//! The mouse comes from egui. The real canvas will read input the same way.

use std::time::{Duration, Instant};

use art_engine::pen::{InputKind, PenSample};
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, Sense, Stroke, pos2, vec2};

use crate::pen_input::{InkEvent, PenInput};

/// After a pen or touch event, ignore egui's mouse pointer for this long.
/// (Windows also moves the mouse pointer when you use the pen or a finger.)
const INK_TAKES_PRIORITY_FOR: Duration = Duration::from_millis(300);

/// What the screen currently shows.
#[derive(Default)]
struct Reading {
    /// Position is in egui points, relative to the window.
    sample: PenSample,
    hovering: bool,
    pressed: bool,
    barrel_button: bool,
    has_pressure: bool,
    has_tilt: bool,
    device_name: String,
}

pub struct PenTestScreen {
    pen_input: PenInput,
    reading: Option<Reading>,
    last_ink_event: Option<Instant>,
    rate: RateCounter,
}

impl PenTestScreen {
    pub fn new(pen_input: PenInput) -> Self {
        Self {
            pen_input,
            reading: None,
            last_ink_event: None,
            rate: RateCounter::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.read_input(ui.ctx());

        egui::Panel::right("pen_readings")
            .exact_size(300.0)
            .show(ui, |ui| self.readings_panel(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| self.test_area(ui));

        // Redraw a few times a second, so "events per second" drops back to
        // 0 when nothing is moving.
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }

    fn read_input(&mut self, ctx: &egui::Context) {
        let now = Instant::now();

        // 1. Pen and touch from Windows Ink.
        let points_per_pixel = 1.0 / ctx.pixels_per_point();
        for event in self.pen_input.take_events() {
            self.last_ink_event = Some(now);
            self.rate.add(event.reading_count);
            self.reading = Some(Reading::from_ink(event, points_per_pixel));
        }

        // 2. The mouse, unless the pen or a finger was just used.
        let ink_is_recent = self
            .last_ink_event
            .is_some_and(|time| now - time < INK_TAKES_PRIORITY_FOR);
        if !ink_is_recent {
            self.read_mouse(ctx);
        }

        self.rate.update(now);
    }

    fn read_mouse(&mut self, ctx: &egui::Context) {
        let (moves, button_changed, position, pressed, in_window, time) = ctx.input(|i| {
            let moves = i
                .events
                .iter()
                .filter(|e| matches!(e, egui::Event::PointerMoved(_)))
                .count();
            (
                moves as u32,
                i.pointer.any_pressed() || i.pointer.any_released(),
                i.pointer.latest_pos(),
                i.pointer.primary_down(),
                i.pointer.has_pointer(),
                i.time,
            )
        });

        // The mouse left the window: stop showing the ring.
        if !in_window {
            if let Some(reading) = &mut self.reading
                && reading.sample.kind == InputKind::Mouse
            {
                reading.hovering = false;
            }
            return;
        }

        let Some(position) = position else { return };
        if moves == 0 && !button_changed {
            return;
        }

        self.rate.add(moves);
        self.reading = Some(Reading {
            sample: PenSample {
                kind: InputKind::Mouse,
                x: position.x,
                y: position.y,
                pressure: if pressed { 1.0 } else { 0.0 },
                time_ms: (time * 1000.0) as u64,
                ..PenSample::default()
            },
            hovering: true,
            pressed,
            device_name: "Mouse".to_owned(),
            ..Reading::default()
        });
    }

    /// The big grey area. A ring follows the pen, grows when you press
    /// harder, and fills in while the pen is touching.
    fn test_area(&self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        ui.allocate_rect(rect, Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, Color32::from_gray(64));

        let Some(reading) = &self.reading else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Move your pen, mouse or finger here",
                FontId::proportional(20.0),
                Color32::from_gray(160),
            );
            return;
        };

        let position = pos2(reading.sample.x, reading.sample.y);
        if reading.hovering && rect.contains(position) {
            let radius = 6.0 + reading.sample.pressure * 30.0;
            if reading.pressed {
                painter.circle_filled(
                    position,
                    radius,
                    Color32::from_rgba_unmultiplied(61, 126, 255, 128),
                );
            }
            painter.circle_stroke(position, radius, Stroke::new(2.0, Color32::WHITE));
        }
    }

    /// The side panel with all the numbers.
    fn readings_panel(&self, ui: &mut egui::Ui) {
        let empty = Reading::default();
        let reading = self.reading.as_ref().unwrap_or(&empty);
        let sample = &reading.sample;
        let angles = sample.angles();
        let yes_no = |flag: bool| if flag { "Yes" } else { "No" };

        ui.add_space(8.0);
        ui.heading("Pen test");
        ui.add_space(4.0);

        let input_name = match (&self.reading, sample.kind) {
            (None, _) => "-",
            (Some(_), InputKind::Pen) => "Pen tip",
            (Some(_), InputKind::PenEraser) => "Pen eraser end",
            (Some(_), InputKind::Mouse) => "Mouse",
            (Some(_), InputKind::Touch) => "Touch",
        };
        row(ui, "Input", input_name);
        row(ui, "Touching / pressed", yes_no(reading.pressed));
        row(ui, "Side button", yes_no(reading.barrel_button));
        row(ui, "Events per second", &self.rate.per_second.to_string());
        ui.separator();

        row(ui, "Pressure", &format!("{:.3}", sample.pressure));
        ui.add(egui::ProgressBar::new(sample.pressure));
        ui.separator();

        row(ui, "Tilt X", &format!("{:.1}°", sample.tilt_x));
        row(ui, "Tilt Y", &format!("{:.1}°", sample.tilt_y));
        row(
            ui,
            "Altitude (90° = upright)",
            &format!("{:.1}°", angles.altitude),
        );
        row(ui, "Lean direction", &format!("{:.0}°", angles.azimuth));
        row(ui, "Barrel rotation", &format!("{:.1}°", sample.rotation));
        ui.vertical_centered(|ui| tilt_picture(ui, angles.altitude, angles.azimuth));
        ui.separator();

        row(ui, "Device reports pressure", yes_no(reading.has_pressure));
        row(ui, "Device reports tilt", yes_no(reading.has_tilt));
        let device = if reading.device_name.is_empty() {
            "-"
        } else {
            &reading.device_name
        };
        ui.weak(format!("Device: {device}"));

        // Tip at the bottom of the panel.
        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(8.0);
            ui.weak(
                "Try: press lightly then hard, tilt the pen in each direction, \
                 and flip it to the eraser end if it has one.",
            );
        });
    }
}

impl Reading {
    fn from_ink(event: InkEvent, points_per_pixel: f32) -> Self {
        let mut sample = event.sample;
        sample.x *= points_per_pixel;
        sample.y *= points_per_pixel;
        Self {
            sample,
            hovering: event.in_range,
            pressed: event.in_contact,
            barrel_button: event.barrel_button,
            has_pressure: event.has_pressure,
            has_tilt: event.has_tilt,
            device_name: event.device_name,
        }
    }
}

/// One "name ... value" line.
fn row(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.weak(name);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.strong(value));
    });
}

/// The pen's tilt seen from above: the line points the way the pen leans
/// and gets longer the further it leans.
fn tilt_picture(ui: &mut egui::Ui, altitude: f32, azimuth: f32) {
    let size = 120.0;
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let painter = ui.painter();
    let visuals = ui.visuals();
    let centre = rect.center();
    let radius = size / 2.0;

    painter.circle(
        centre,
        radius - 1.0,
        visuals.extreme_bg_color,
        visuals.widgets.noninteractive.bg_stroke,
    );

    let lean = (90.0 - altitude) / 90.0; // 0 = upright, 1 = lying flat
    let direction = egui::Vec2::angled(azimuth.to_radians());
    painter.line_segment(
        [centre, centre + direction * lean * radius],
        Stroke::new(3.0, visuals.selection.bg_fill),
    );
    painter.circle_filled(centre, 4.0, visuals.text_color());
}

/// Counts events and reports how many arrived during the last full second.
struct RateCounter {
    count: u32,
    since: Instant,
    per_second: u32,
}

impl RateCounter {
    fn new() -> Self {
        Self {
            count: 0,
            since: Instant::now(),
            per_second: 0,
        }
    }

    fn add(&mut self, events: u32) {
        self.count += events;
    }

    fn update(&mut self, now: Instant) {
        if now - self.since >= Duration::from_secs(1) {
            self.per_second = self.count;
            self.count = 0;
            self.since = now;
        }
    }
}
