//! The colour picker: a colour wheel, a hex code box and recent colours.
//!
//! The wheel is a ring of hues around a square. Around the ring you choose
//! the hue (red, yellow, green, ...); in the square you choose how
//! saturated (left to right) and how bright (bottom to top) it is. Both
//! are drawn on the graphics card as coloured shapes.

use std::f32::consts::TAU;

use art_engine::color::{Hsv, from_hex, to_hex};
use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Sense, Stroke, TextEdit, vec2};

/// The widest the wheel gets, in screen points.
const WHEEL_SIZE: f32 = 230.0;
/// How many recent colours to show.
pub const MAX_RECENT_COLORS: usize = 12;

/// Which part of the wheel is being dragged.
#[derive(Clone, Copy, PartialEq)]
enum WheelPart {
    Ring,
    Square,
}

pub struct ColorPicker {
    /// The colour as hue/saturation/value. We keep this ourselves (rather
    /// than working it out from RGB each time), so the hue isn't lost when
    /// you drag down to black or grey and back up.
    hsv: Hsv,
    /// What's typed in the hex box.
    hex_text: String,
    dragging: Option<WheelPart>,
}

impl Default for ColorPicker {
    fn default() -> Self {
        Self {
            hsv: Hsv::new(0.0, 0.0, 0.0),
            hex_text: String::new(),
            dragging: None,
        }
    }
}

impl ColorPicker {
    /// Shows the picker for `color`, changing it when the user picks a new
    /// one. `recent` are colours used recently, newest first.
    pub fn ui(&mut self, ui: &mut egui::Ui, color: &mut [u8; 3], recent: &[[u8; 3]]) {
        // The colour may have changed elsewhere (eyedropper, recent colours).
        if self.hsv.to_rgb() != *color {
            self.hsv = self.hsv_keeping_hue(*color);
        }

        if self.wheel(ui) {
            *color = self.hsv.to_rgb();
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            // The current colour, as a swatch.
            let (rect, _) = ui.allocate_exact_size(vec2(44.0, 22.0), Sense::hover());
            ui.painter().rect_filled(rect, 3.0, to_color32(*color));
            ui.painter().rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0, Color32::from_gray(90)),
                egui::StrokeKind::Inside,
            );

            // Hex code: type a code like #FF8800 and press Enter.
            let response = ui.add(TextEdit::singleline(&mut self.hex_text).desired_width(80.0));
            if response.lost_focus()
                && let Some(typed) = from_hex(&self.hex_text)
            {
                *color = typed;
            }
            if !response.has_focus() {
                self.hex_text = to_hex(*color);
            }
        });

        if !recent.is_empty() {
            ui.add_space(4.0);
            ui.weak("Recent colours");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for &recent_color in recent {
                    let (rect, response) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                    ui.painter()
                        .rect_filled(rect, 3.0, to_color32(recent_color));
                    if response.hovered() {
                        ui.painter().rect_stroke(
                            rect,
                            3.0,
                            Stroke::new(2.0, Color32::WHITE),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if response.on_hover_text(to_hex(recent_color)).clicked() {
                        *color = recent_color;
                    }
                }
            });
        }
    }

    /// `rgb` as HSV, but keeping the current hue for greys and black (and
    /// the current saturation for black), which have no hue of their own.
    fn hsv_keeping_hue(&self, rgb: [u8; 3]) -> Hsv {
        let mut hsv = Hsv::from_rgb(rgb);
        if hsv.s == 0.0 || hsv.v == 0.0 {
            hsv.h = self.hsv.h;
        }
        if hsv.v == 0.0 {
            hsv.s = self.hsv.s;
        }
        hsv
    }

    /// Draws the wheel and handles dragging on it. Returns true if the
    /// colour changed.
    fn wheel(&mut self, ui: &mut egui::Ui) -> bool {
        let size = ui.available_width().min(WHEEL_SIZE);
        let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
        let centre = rect.center();
        let outer = size / 2.0 - 2.0;
        let inner = outer * 0.8;
        // The square fits inside the ring with a small gap.
        let half = inner * 0.68;
        let square = Rect::from_center_size(centre, vec2(2.0 * half, 2.0 * half));

        // Which part did the drag start on? Keep dragging that part even
        // if the pointer wanders off it.
        let mut changed = false;
        // React while the button is held, and also to a quick click or tap
        // (pressed and let go within one frame).
        let pressed = response.is_pointer_button_down_on() || response.clicked();
        if pressed && let Some(pointer) = response.interact_pointer_pos() {
            if self.dragging.is_none() {
                let distance = (pointer - centre).length();
                self.dragging = if square.contains(pointer) {
                    Some(WheelPart::Square)
                } else if distance >= inner - 4.0 && distance <= outer + 4.0 {
                    Some(WheelPart::Ring)
                } else {
                    None
                };
            }
            match self.dragging {
                Some(WheelPart::Ring) => {
                    self.hsv.h = hue_at(centre, pointer);
                    changed = true;
                }
                Some(WheelPart::Square) => {
                    self.hsv.s = ((pointer.x - square.left()) / square.width()).clamp(0.0, 1.0);
                    self.hsv.v =
                        1.0 - ((pointer.y - square.top()) / square.height()).clamp(0.0, 1.0);
                    changed = true;
                }
                None => {}
            }
        }
        if !response.is_pointer_button_down_on() {
            self.dragging = None; // let go: the next press can choose a part again
        }

        let painter = ui.painter_at(rect);
        let background = ui.visuals().panel_fill;

        // 1. The hue ring: a band of thin slices, each coloured by its hue.
        let slices = 120;
        let mut ring = Mesh::default();
        for i in 0..=slices {
            let hue = i as f32 / slices as f32;
            let direction = direction_of(hue);
            let colour = to_color32(Hsv::new(hue, 1.0, 1.0).to_rgb());
            ring.colored_vertex(centre + direction * inner, colour);
            ring.colored_vertex(centre + direction * outer, colour);
            if i > 0 {
                let k = 2 * i as u32;
                ring.add_triangle(k - 2, k - 1, k);
                ring.add_triangle(k - 1, k + 1, k);
            }
        }
        painter.add(ring);
        // Thin circles in the background colour hide the ring's jagged edges.
        painter.circle_stroke(centre, outer, Stroke::new(1.5, background));
        painter.circle_stroke(centre, inner, Stroke::new(1.5, background));

        // 2. The square: a grid of small cells, each corner coloured by its
        //    saturation (across) and brightness (down).
        let cells = 16;
        let mut grid = Mesh::default();
        for row in 0..=cells {
            for column in 0..=cells {
                let (across, down) = (column as f32 / cells as f32, row as f32 / cells as f32);
                let colour = to_color32(Hsv::new(self.hsv.h, across, 1.0 - down).to_rgb());
                let position =
                    square.left_top() + vec2(across * square.width(), down * square.height());
                grid.colored_vertex(position, colour);
            }
        }
        let stride = cells as u32 + 1;
        for row in 0..cells as u32 {
            for column in 0..cells as u32 {
                let top_left = row * stride + column;
                grid.add_triangle(top_left, top_left + 1, top_left + stride);
                grid.add_triangle(top_left + 1, top_left + stride + 1, top_left + stride);
            }
        }
        painter.add(grid);

        // 3. Markers showing the current choice, in black and white so
        //    they show up on any colour.
        let ring_marker = centre + direction_of(self.hsv.h) * ((inner + outer) / 2.0);
        draw_marker(&painter, ring_marker, (outer - inner) / 2.0 + 1.0);
        let square_marker = Pos2::new(
            square.left() + self.hsv.s * square.width(),
            square.top() + (1.0 - self.hsv.v) * square.height(),
        );
        draw_marker(&painter, square_marker, 6.0);

        changed
    }
}

/// The direction from the centre for a hue: red to the right, then
/// yellow, green, ... going anticlockwise.
fn direction_of(hue: f32) -> egui::Vec2 {
    let angle = hue * TAU;
    // Screen y points down, so "up" is negative.
    vec2(angle.cos(), -angle.sin())
}

/// The hue at `pointer` on a ring around `centre` (the reverse of `direction_of`).
fn hue_at(centre: Pos2, pointer: Pos2) -> f32 {
    let offset = pointer - centre;
    (-offset.y).atan2(offset.x).rem_euclid(TAU) / TAU
}

fn draw_marker(painter: &egui::Painter, position: Pos2, radius: f32) {
    painter.circle_stroke(position, radius, Stroke::new(3.0, Color32::BLACK));
    painter.circle_stroke(position, radius, Stroke::new(1.5, Color32::WHITE));
}

fn to_color32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_and_direction_match_up() {
        let centre = Pos2::new(100.0, 100.0);
        for i in 0..12 {
            let hue = i as f32 / 12.0;
            let pointer = centre + direction_of(hue) * 50.0;
            assert!((hue_at(centre, pointer) - hue).abs() < 1e-4, "hue {hue}");
        }
        // Red is to the right, and a quarter turn anticlockwise (up) is hue 0.25.
        assert!(hue_at(centre, Pos2::new(150.0, 100.0)).abs() < 1e-6);
        assert!((hue_at(centre, Pos2::new(100.0, 50.0)) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn black_and_grey_keep_the_previous_hue() {
        let picker = ColorPicker {
            hsv: Hsv::new(0.6, 0.8, 0.9),
            ..ColorPicker::default()
        };
        let black = picker.hsv_keeping_hue([0, 0, 0]);
        assert_eq!((black.h, black.s, black.v), (0.6, 0.8, 0.0));
        let grey = picker.hsv_keeping_hue([128, 128, 128]);
        assert_eq!((grey.h, grey.s), (0.6, 0.0));
    }
}
