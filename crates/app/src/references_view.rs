//! Reference pictures, each in its own small window over the canvas.
//! Scroll to zoom, drag to move the picture about, the slider fades it,
//! Alt-click (or a click with the Pick tool) takes its colour for the
//! brush, and the window's × takes it away (that can be undone).

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, ColorImage, CursorIcon, Id, Pos2, Rect, Sense, TextureHandle, TextureOptions,
    vec2,
};
use quill_engine::image::Image;
use quill_engine::reference::{Reference, ReferenceView};

use crate::brush_panel::percent_slider;

/// Pictures bigger than this (across or down) are shown shrunk: graphics
/// cards can't take very big textures. Colours are still picked from the
/// full picture.
const MAX_TEXTURE_SIDE: u32 = 4096;
/// Zoom limits, in screen points per picture pixel.
const MIN_ZOOM: f32 = 0.02;
const MAX_ZOOM: f32 = 32.0;

/// What the user did in a reference window this frame.
#[derive(Debug, PartialEq)]
pub enum ReferenceAction {
    /// Took this colour for the brush.
    PickColor([u8; 3]),
    /// Closed reference number `n`.
    Remove(usize),
}

#[derive(Default)]
pub struct ReferencesView {
    /// One texture per picture, by the picture's address in memory.
    textures: HashMap<usize, TextureHandle>,
    /// View → Hide references.
    pub hidden: bool,
}

impl ReferencesView {
    /// Shows every reference's window. `picking`: the Pick tool is on, so
    /// a plain click picks a colour too.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        references: &mut [Reference],
        canvas_rect: Rect,
        picking: bool,
    ) -> Vec<ReferenceAction> {
        // Forget the textures of pictures that are gone.
        let alive: Vec<usize> = references.iter().map(|r| key(r.image())).collect();
        self.textures.retain(|k, _| alive.contains(k));
        let mut actions = Vec::new();
        if self.hidden {
            return actions;
        }
        for (index, reference) in references.iter_mut().enumerate() {
            // A cheap copy (the picture is shared) for the window to read.
            let picture = reference.clone();
            let image = Arc::clone(picture.image());
            let texture = self
                .textures
                .entry(key(&image))
                .or_insert_with(|| {
                    ctx.load_texture(
                        format!("reference-{}", reference.name),
                        texture_image(&image),
                        TextureOptions::LINEAR,
                    )
                })
                .clone();
            let view = &mut reference.view;
            let mut open = true;
            let mut window = egui::Window::new(reference.name.as_str())
                .id(Id::unique(("reference", key(&image))))
                .open(&mut open)
                .resizable(true)
                .collapsible(true)
                .min_size(vec2(140.0, 100.0));
            window = if view.window[2] > 0.0 {
                let [x, y, w, h] = view.window;
                window.default_rect(Rect::from_min_size(egui::pos2(x, y), vec2(w, h)))
            } else {
                // New: near the canvas's top right, each a little lower.
                let at = canvas_rect.right_top() + vec2(-340.0, 10.0 + 30.0 * index as f32);
                window.default_pos(at).default_size(vec2(320.0, 260.0))
            };
            let shown = window.show(ctx, |ui| {
                picture_ui(ui, &texture, &picture, view, picking, &mut actions);
            });
            if let Some(shown) = shown {
                let rect = shown.response.rect;
                view.window = [rect.left(), rect.top(), rect.width(), rect.height()];
            }
            if !open {
                actions.push(ReferenceAction::Remove(index));
            }
        }
        actions
    }
}

fn key(image: &Arc<Image>) -> usize {
    Arc::as_ptr(image) as usize
}

/// The window's inside: a row of controls, then the picture.
fn picture_ui(
    ui: &mut egui::Ui,
    texture: &TextureHandle,
    picture: &Reference,
    view: &mut ReferenceView,
    picking: bool,
    actions: &mut Vec<ReferenceAction>,
) {
    let image = picture.image();
    let mut fit_now = false;
    let mut zoom_by = 1.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().slider_width = 70.0;
        ui.add(percent_slider(&mut view.opacity, 0.05..=1.0, ""))
            .on_hover_text("How see-through it is");
        if ui.small_button("−").on_hover_text("Zoom out").clicked() {
            zoom_by = 1.0 / 1.25;
        }
        if ui.small_button("+").on_hover_text("Zoom in").clicked() {
            zoom_by = 1.25;
        }
        fit_now = ui
            .small_button("Fit")
            .on_hover_text("Show all of it")
            .clicked();
    });
    let size = ui.available_size().max(vec2(120.0, 60.0));
    let (area, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    if fit_now {
        fit(view, area, image);
    }
    if zoom_by != 1.0 {
        zoom_about(view, area, area.center(), zoom_by);
    }

    let (alt, scroll, pointer) = ui.input(|i| {
        (
            i.modifiers.alt,
            i.smooth_scroll_delta.y,
            i.pointer.hover_pos(),
        )
    });
    let pick_mode = alt || picking;
    if response.hovered() {
        // The wheel zooms where the pointer is.
        if scroll != 0.0
            && let Some(pos) = pointer
        {
            zoom_about(view, area, pos, (scroll * 0.002).exp());
        }
        ui.ctx().set_cursor_icon(if pick_mode {
            CursorIcon::Crosshair
        } else {
            CursorIcon::Grab
        });
    }
    if pick_mode {
        if (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
        {
            let [x, y] = to_picture(view, area, pos);
            if let Some(color) = picture.color_at(x, y) {
                actions.push(ReferenceAction::PickColor(color));
            }
        }
    } else if response.dragged() {
        let delta = response.drag_delta() / view.zoom;
        view.centre[0] -= delta.x;
        view.centre[1] -= delta.y;
    }

    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, ui.visuals().extreme_bg_color);
    let top_left = to_screen(view, area, [0.0, 0.0]);
    let bottom_right = to_screen(view, area, [image.width() as f32, image.height() as f32]);
    painter.image(
        texture.id(),
        Rect::from_min_max(top_left, bottom_right),
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE.gamma_multiply(view.opacity.clamp(0.0, 1.0)),
    );
}

/// The picture as a texture, shrunk if it's very big.
fn texture_image(image: &Image) -> ColorImage {
    let (w, h) = (image.width().max(1), image.height().max(1));
    let step = w.max(h).div_ceil(MAX_TEXTURE_SIDE).max(1);
    let (tw, th) = (w.div_ceil(step), h.div_ceil(step));
    let mut pixels = Vec::with_capacity((tw * th) as usize);
    for ty in 0..th {
        for tx in 0..tw {
            let [r, g, b, a] = image.pixel((tx * step).min(w - 1), (ty * step).min(h - 1));
            pixels.push(Color32::from_rgba_unmultiplied(r, g, b, a));
        }
    }
    ColorImage::new([tw as usize, th as usize], pixels)
}

/// Where picture pixel `p` is on screen, for a picture shown in `area`.
fn to_screen(view: &ReferenceView, area: Rect, p: [f32; 2]) -> Pos2 {
    area.center() + vec2(p[0] - view.centre[0], p[1] - view.centre[1]) * view.zoom
}

/// The picture pixel at screen position `pos`.
fn to_picture(view: &ReferenceView, area: Rect, pos: Pos2) -> [f32; 2] {
    let d = (pos - area.center()) / view.zoom;
    [view.centre[0] + d.x, view.centre[1] + d.y]
}

/// Zooms by `factor`, keeping the picture pixel under `pos` where it is.
fn zoom_about(view: &mut ReferenceView, area: Rect, pos: Pos2, factor: f32) {
    let before = to_picture(view, area, pos);
    view.zoom = (view.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    let after = to_picture(view, area, pos);
    view.centre[0] += before[0] - after[0];
    view.centre[1] += before[1] - after[1];
}

/// The whole picture, as big as fits in `area`.
fn fit(view: &mut ReferenceView, area: Rect, image: &Image) {
    let (w, h) = (image.width().max(1) as f32, image.height().max(1) as f32);
    view.zoom = (area.width() / w)
        .min(area.height() / h)
        .clamp(MIN_ZOOM, MAX_ZOOM);
    view.centre = [w / 2.0, h / 2.0];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> Reference {
        let mut image = Image::new(400, 200, [255, 0, 0, 255]);
        for y in 0..200 {
            for x in 200..400 {
                image.set_pixel(x, y, [0, 0, 255, 255]);
            }
        }
        let mut bytes = Vec::new();
        image.write_png(&mut bytes, true).unwrap();
        Reference::from_bytes("two.png", bytes).unwrap()
    }

    #[test]
    fn zooming_keeps_the_pixel_under_the_pointer_and_fit_shows_it_all() {
        let reference = picture();
        let mut view = reference.view;
        let area = Rect::from_min_size(egui::pos2(100.0, 50.0), vec2(300.0, 200.0));
        let pointer = egui::pos2(150.0, 90.0);
        let under = to_picture(&view, area, pointer);
        zoom_about(&mut view, area, pointer, 3.0);
        let still = to_picture(&view, area, pointer);
        assert!((under[0] - still[0]).abs() < 1e-3 && (under[1] - still[1]).abs() < 1e-3);
        zoom_about(&mut view, area, pointer, 1e9);
        assert_eq!(view.zoom, MAX_ZOOM);
        fit(&mut view, area, reference.image());
        assert_eq!(view.zoom, 0.75, "400 pixels across 300 points");
        assert_eq!(to_screen(&view, area, [0.0, 100.0]).x, 100.0);
        assert_eq!(to_screen(&view, area, [400.0, 100.0]).x, 400.0);
    }

    #[test]
    fn a_huge_picture_is_shrunk_for_the_screen() {
        // A huge picture's texture is shrunk to fit a graphics card.
        let big = Image::new(9000, 300, [1, 2, 3, 255]);
        let texture = texture_image(&big);
        assert!(
            texture.size[0] <= MAX_TEXTURE_SIDE as usize,
            "{:?}",
            texture.size
        );
        assert_eq!(texture.size, [3000, 100]);
    }
}
