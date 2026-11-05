//! The Layer menu: flip the selected layer, or move, resize and turn it
//! with handles (Layer → Transform…).
//!
//! Both use the engine's `transform_layer`. The change is first written
//! as a `transform_layer` script command, and that command is what's
//! painted, so "Export strokes" replays exactly what you saw.
//!
//! While transforming, the layer itself is hidden and a picture of it is
//! drawn where it's going, on top of the canvas, inside a box with
//! handles: drag inside to move, a corner to resize (Shift keeps the
//! proportions), the round handle to turn. Enter (or Apply) paints it
//! there, as one undo step; Escape (or Cancel) leaves the layer as it was.

use std::time::{Duration, Instant};

use art_engine::document::{Background, Document};
use art_engine::history::{History, TileCommand};
use art_engine::image::Image;
use art_engine::layer::LayerId;
use art_engine::layer_transform::transform_layer;
use art_engine::pen::InputKind;
use art_engine::script::{TransformLayerCmd, record};
use art_engine::surface::PixelRect;
use art_engine::transform::Transform;
use art_engine::view::{Point, View};
use eframe::egui::epaint::Vertex;
use eframe::egui::{
    self, Button, Color32, ColorImage, CursorIcon, Mesh, PointerButton, Pos2, Rect, Shape, Stroke,
    TextureHandle, TextureOptions, pos2, vec2,
};

use crate::pen_input::InkEvent;

/// How close (in screen points) the pen must be to a handle to grab it.
const GRAB_DISTANCE: f32 = 16.0;
/// How far above the box the turning handle sits, in screen points.
const TURN_HANDLE_GAP: f32 = 34.0;
/// The size of the corner squares, in screen points.
const CORNER_SIZE: f32 = 10.0;
/// Resizing stays between these.
const MIN_SCALE: f32 = 0.01;
const MAX_SCALE: f32 = 100.0;
/// The picture shown while transforming is at most this big (a preview).
const PREVIEW_SIZE: u32 = 4096;
/// After the pen was used, ignore the mouse for this long (Windows moves
/// the mouse pointer along with the pen).
const INK_TAKES_PRIORITY_FOR: Duration = Duration::from_millis(300);

/// What the Layer menu asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerAction {
    FlipHorizontal,
    FlipVertical,
    Transform,
}

/// The Layer button in the top bar, with its menu. Greyed out while
/// `busy`. Returns what was chosen, if anything.
pub fn layer_menu(ui: &mut egui::Ui, busy: bool) -> Option<LayerAction> {
    let mut chosen = None;
    let entries = [
        (
            "Flip horizontal",
            LayerAction::FlipHorizontal,
            "Mirror the selected layer: left and right swap",
        ),
        (
            "Flip vertical",
            LayerAction::FlipVertical,
            "Mirror the selected layer: top and bottom swap",
        ),
        (
            "Transform…",
            LayerAction::Transform,
            "Move, resize and turn the selected layer with handles",
        ),
    ];
    let button = Button::new("Layer").min_size(vec2(56.0, 26.0));
    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        ui.set_min_width(200.0);
        for (label, action, tip) in entries {
            let entry = Button::new(label).min_size(vec2(0.0, 28.0)); // easy to tap
            if ui.add_enabled(!busy, entry).on_hover_text(tip).clicked() {
                chosen = Some(action);
            }
        }
    });
    chosen
}

/// Mirrors everything on the selected layer around the canvas's middle:
/// left and right swap (`horizontal`), or top and bottom. One undo step.
/// Returns false if there was nothing on the layer.
pub fn flip(document: &mut Document, history: &mut History, horizontal: bool) -> bool {
    let command = TransformLayerCmd {
        mirror: Some(if horizontal { "x" } else { "y" }.to_owned()),
        origin: Some([
            document.width() as f32 / 2.0,
            document.height() as f32 / 2.0,
        ]),
        ..TransformLayerCmd::default()
    };
    let name = if horizontal {
        "Flip horizontal"
    } else {
        "Flip vertical"
    };
    let layer = document.active_id();
    apply_command(document, history, layer, &command, name)
}

/// Paints `command` on `layer` and records it as one undo step (with the
/// command, for "Export strokes"). False if nothing changed.
fn apply_command(
    document: &mut Document,
    history: &mut History,
    layer: LayerId,
    command: &TransformLayerCmd,
    name: &str,
) -> bool {
    let Ok(transform) = command.transform() else {
        return false;
    };
    let Some(layer_name) = document.layer(layer).map(|l| l.name().to_owned()) else {
        return false;
    };
    let before = transform_layer(document, layer, transform);
    if before.is_empty() {
        return false;
    }
    history.push_recorded(
        Box::new(TileCommand::new(name, layer, before)),
        record::transform_layer(&layer_name, command),
    );
    true
}

/// How the layer is being moved: around the middle of what's on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub translate: Point,
    pub scale: (f32, f32),
    /// Degrees, clockwise.
    pub rotate: f32,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            translate: Point::new(0.0, 0.0),
            scale: (1.0, 1.0),
            rotate: 0.0,
        }
    }
}

/// What dragging does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// Inside the box: move.
    Move,
    /// A corner: resize.
    Corner,
    /// The round handle above the box: turn.
    Turn,
}

#[derive(Clone, Copy)]
struct Drag {
    handle: Handle,
    /// The placement when the drag started, and where it started.
    start: Placement,
    from: Point,
}

/// A transform in progress (Layer → Transform…).
pub struct LayerTransform {
    layer: LayerId,
    /// What's on the layer: the box that's moved around.
    area: PixelRect,
    pub placement: Placement,
    drag: Option<Drag>,
    /// The layer as a picture, until it's on the graphics card.
    picture: Option<Image>,
    texture: Option<TextureHandle>,
    /// The pen or mouse button is down.
    pointer_down: bool,
    last_ink: Option<Instant>,
}

impl LayerTransform {
    /// Starts transforming the selected layer: hides it, and keeps a
    /// picture of it to move around instead. An `Err` explains why not.
    pub fn start(document: &mut Document) -> Result<Self, &'static str> {
        let layer = document.active_layer();
        if !layer.visible() {
            return Err("This layer is hidden. Show it in the Layers panel to transform it.");
        }
        let Some(area) = layer.surface.painted_area() else {
            return Err("There's nothing on this layer to transform.");
        };
        let id = layer.id();
        let picture =
            Image::from_document_showing(document, area, Background::Transparent, |l| l.id() == id)
                .fit_within(PREVIEW_SIZE, PREVIEW_SIZE);
        if let Some(layer) = document.layer_mut(id) {
            layer.set_visible(false); // the picture stands in for it meanwhile
        }
        Ok(Self {
            layer: id,
            area,
            placement: Placement::default(),
            drag: None,
            picture: Some(picture),
            texture: None,
            pointer_down: false,
            last_ink: None,
        })
    }

    /// The middle of what's on the layer: turning and resizing are around it.
    fn origin(&self) -> Point {
        let a = self.area;
        Point::new(
            (a.left + a.right) as f32 / 2.0,
            (a.top + a.bottom) as f32 / 2.0,
        )
    }

    /// Half the box's width and height, before resizing.
    fn half(&self) -> (f32, f32) {
        let a = self.area;
        (
            ((a.right - a.left) as f32 / 2.0).max(0.5),
            ((a.bottom - a.top) as f32 / 2.0).max(0.5),
        )
    }

    /// The command this transform is, with its numbers rounded (so the
    /// exported script is easy to read and replays exactly).
    pub fn command(&self) -> TransformLayerCmd {
        let p = self.placement;
        let translate = [round(p.translate.x, 2), round(p.translate.y, 2)];
        let scale = (round_f64(p.scale.0, 4), round_f64(p.scale.1, 4));
        let rotate = round(p.rotate, 2);
        let origin = self.origin();
        TransformLayerCmd {
            name: None,
            translate: (translate != [0.0, 0.0]).then_some(translate),
            scale: record::scale(scale.0, scale.1),
            rotate: (rotate != 0.0).then_some(rotate),
            mirror: None,
            origin: Some([origin.x, origin.y]),
        }
    }

    fn transform(&self) -> Transform {
        self.command().transform().unwrap_or(Transform::IDENTITY)
    }

    /// The box's corners on the canvas, clockwise from the top left.
    pub fn corners(&self) -> [Point; 4] {
        let a = self.area;
        let transform = self.transform();
        [
            (a.left, a.top),
            (a.right, a.top),
            (a.right, a.bottom),
            (a.left, a.bottom),
        ]
        .map(|(x, y)| {
            let (x, y) = transform.apply((x as f32, y as f32));
            Point::new(x, y)
        })
    }

    /// Where the middle of the box is now.
    fn centre(&self, placement: Placement) -> Point {
        self.origin() + placement.translate
    }

    /// The round handle for turning: above the middle of the top edge.
    fn turn_handle(&self, zoom: f32) -> Point {
        let p = self.placement;
        let above = self.half().1 * p.scale.1 + TURN_HANDLE_GAP / zoom;
        self.centre(p) + turned(Point::new(0.0, -above), p.rotate)
    }

    /// The handle at `at` (canvas pixels), if any. `zoom` is screen points
    /// per canvas pixel.
    pub fn handle_at(&self, at: Point, zoom: f32) -> Option<Handle> {
        let reach = GRAB_DISTANCE / zoom;
        if distance(at, self.turn_handle(zoom)) <= reach {
            return Some(Handle::Turn);
        }
        let corners = self.corners();
        if corners.iter().any(|&c| distance(at, c) <= reach) {
            return Some(Handle::Corner);
        }
        inside(&corners, at).then_some(Handle::Move)
    }

    /// The pen or mouse went down at `at`: grab whatever handle is there.
    pub fn press(&mut self, at: Point, zoom: f32) {
        self.drag = self.handle_at(at, zoom).map(|handle| Drag {
            handle,
            start: self.placement,
            from: at,
        });
    }

    /// The pen or mouse moved to `at` while down.
    pub fn drag_to(&mut self, at: Point, keep_proportions: bool) {
        let Some(drag) = self.drag else {
            return;
        };
        let start = drag.start;
        let centre = self.centre(start);
        match drag.handle {
            Handle::Move => {
                self.placement.translate = start.translate + (at - drag.from);
            }
            Handle::Corner => {
                // The corner follows the pen, measured along the box's own
                // sides (it may be turned).
                let local = turned(at - centre, -start.rotate);
                let (hx, hy) = self.half();
                let scale = if keep_proportions {
                    let was = (hx * start.scale.0).hypot(hy * start.scale.1);
                    let factor = local.x.hypot(local.y) / was;
                    (start.scale.0 * factor, start.scale.1 * factor)
                } else {
                    (local.x.abs() / hx, local.y.abs() / hy)
                };
                self.placement.scale = (
                    scale.0.clamp(MIN_SCALE, MAX_SCALE),
                    scale.1.clamp(MIN_SCALE, MAX_SCALE),
                );
            }
            Handle::Turn => {
                let angle = |p: Point| (p.y - centre.y).atan2(p.x - centre.x).to_degrees();
                let mut rotate = start.rotate + angle(at) - angle(drag.from);
                rotate = (rotate + 180.0).rem_euclid(360.0) - 180.0;
                self.placement.rotate = rotate;
            }
        }
    }

    /// The pen or mouse was lifted.
    pub fn release(&mut self) {
        self.drag = None;
    }

    /// Paints the layer where it's been put, as one undo step (recorded
    /// for "Export strokes"), and shows the layer again.
    pub fn apply(self, document: &mut Document, history: &mut History) {
        let command = self.command();
        self.show_layer_again(document);
        apply_command(document, history, self.layer, &command, "Transform layer");
    }

    /// Leaves the layer as it was.
    pub fn cancel(self, document: &mut Document) {
        self.show_layer_again(document);
    }

    fn show_layer_again(&self, document: &mut Document) {
        if let Some(layer) = document.layer_mut(self.layer) {
            layer.set_visible(true);
        }
    }

    /// Reads this frame's pen and mouse input. `rect` is the canvas area.
    pub fn handle_input(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        view: &View,
        ink_events: Vec<InkEvent>,
        navigating: bool,
    ) {
        let ctx = ui.ctx();
        let now = Instant::now();
        let zoom = view.zoom();
        let points_per_pixel = 1.0 / ctx.pixels_per_point();
        let canvas_layer = ui.layer_id();
        let on_canvas =
            |pos: Pos2| rect.contains(pos) && ctx.layer_id_at(pos) == Some(canvas_layer);
        let to_canvas = |pos: Pos2| view.screen_to_canvas(Point::new(pos.x, pos.y));
        let shift = ui.input(|i| i.modifiers.shift);

        for event in ink_events {
            if event.sample.kind == InputKind::Touch {
                continue; // fingers zoom and pan
            }
            self.last_ink = Some(now);
            let pos = pos2(
                event.sample.x * points_per_pixel,
                event.sample.y * points_per_pixel,
            );
            match (event.in_contact, self.pointer_down) {
                (true, false) if on_canvas(pos) && !navigating => {
                    self.pointer_down = true;
                    self.press(to_canvas(pos), zoom);
                }
                (true, true) => self.drag_to(to_canvas(pos), shift),
                (false, true) => {
                    self.pointer_down = false;
                    self.release();
                }
                _ => {}
            }
        }

        // The mouse, unless the pen was just used.
        if self
            .last_ink
            .is_some_and(|time| now - time < INK_TAKES_PRIORITY_FOR)
        {
            return;
        }
        let (events, primary_down, hover) = ui.input(|i| {
            (
                i.events.clone(),
                i.pointer.primary_down(),
                i.pointer.hover_pos(),
            )
        });
        for event in events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    ..
                } if on_canvas(pos) && !navigating => {
                    self.pointer_down = true;
                    self.press(to_canvas(pos), zoom);
                }
                egui::Event::PointerMoved(pos) if self.pointer_down => {
                    self.drag_to(to_canvas(pos), shift);
                }
                egui::Event::PointerButton {
                    button: PointerButton::Primary,
                    pressed: false,
                    ..
                }
                | egui::Event::PointerGone => {
                    self.pointer_down = false;
                    self.release();
                }
                _ => {}
            }
        }
        if !primary_down && self.pointer_down {
            self.pointer_down = false;
            self.release();
        }

        // Show what dragging here would do.
        let handle = match self.drag {
            Some(drag) => Some(drag.handle),
            None => hover
                .filter(|&pos| on_canvas(pos))
                .and_then(|pos| self.handle_at(to_canvas(pos), zoom)),
        };
        if !navigating && let Some(handle) = handle {
            ctx.set_cursor_icon(match handle {
                Handle::Move => CursorIcon::Move,
                Handle::Corner => CursorIcon::ResizeNwSe,
                Handle::Turn => CursorIcon::Grab,
            });
        }
    }

    /// Draws the layer where it's going, with its box and handles.
    pub fn draw(&mut self, ctx: &egui::Context, painter: &egui::Painter, view: &View) {
        let screen = |p: Point| {
            let s = view.canvas_to_screen(p);
            pos2(s.x, s.y)
        };
        if self.texture.is_none()
            && let Some(picture) = self.picture.take()
        {
            let size = [picture.width() as usize, picture.height() as usize];
            let mut bytes = Vec::with_capacity(size[0] * size[1] * 4);
            for y in 0..picture.height() {
                for x in 0..picture.width() {
                    bytes.extend_from_slice(&picture.pixel(x, y));
                }
            }
            let image = ColorImage::from_rgba_unmultiplied(size, &bytes);
            self.texture = Some(ctx.load_texture("layer transform", image, TextureOptions::LINEAR));
        }
        let corners = self.corners().map(screen);
        if let Some(texture) = &self.texture {
            let mut mesh = Mesh::with_texture(texture.id());
            let uvs = [
                pos2(0.0, 0.0),
                pos2(1.0, 0.0),
                pos2(1.0, 1.0),
                pos2(0.0, 1.0),
            ];
            for (pos, uv) in corners.into_iter().zip(uvs) {
                mesh.vertices.push(Vertex {
                    pos,
                    uv,
                    color: Color32::WHITE,
                });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(Shape::mesh(mesh));
        }

        // The box, the turning handle on its stalk, and the corners: black
        // under white, so they show on any colour.
        let top_middle = corners[0] + (corners[1] - corners[0]) / 2.0;
        let turn = screen(self.turn_handle(view.zoom()));
        for (width, color) in [(3.0, Color32::from_black_alpha(160)), (1.0, Color32::WHITE)] {
            let stroke = Stroke::new(width, color);
            painter.add(Shape::closed_line(corners.to_vec(), stroke));
            painter.line_segment([top_middle, turn], stroke);
        }
        painter.circle(turn, 7.0, Color32::WHITE, Stroke::new(2.0, Color32::BLACK));
        for corner in corners {
            let square = Rect::from_center_size(corner, vec2(CORNER_SIZE, CORNER_SIZE));
            painter.rect(
                square,
                1.0,
                Color32::WHITE,
                Stroke::new(1.5, Color32::BLACK),
                egui::StrokeKind::Middle,
            );
        }
    }
}

/// `value` rounded to `places` decimals.
fn round(value: f32, places: i32) -> f32 {
    let factor = 10f32.powi(places);
    (value * factor).round() / factor
}

/// `value` rounded to `places` decimals, as the exported number will be.
fn round_f64(value: f32, places: i32) -> f64 {
    let factor = 10f64.powi(places);
    (f64::from(value) * factor).round() / factor
}

/// `p` turned around (0, 0) by `degrees`, clockwise on screen.
fn turned(p: Point, degrees: f32) -> Point {
    let (sin, cos) = degrees.to_radians().sin_cos();
    Point::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos)
}

fn distance(a: Point, b: Point) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// True if `p` is inside the four-cornered shape (corners in order).
fn inside(corners: &[Point; 4], p: Point) -> bool {
    let side = |a: Point, b: Point| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let sides: Vec<f32> = (0..4)
        .map(|i| side(corners[i], corners[(i + 1) % 4]))
        .collect();
    sides.iter().all(|&s| s >= 0.0) || sides.iter().all(|&s| s <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use art_engine::script::{PlayOptions, Player, Script};
    use art_engine::tile::Rgba8;

    const RED: Rgba8 = Rgba8::new(255, 0, 0, 255);

    /// A 400 x 300 picture with a 40 x 20 red block at (100, 50).
    fn picture() -> Document {
        let mut document = Document::new(400, 300);
        for y in 50..70 {
            for x in 100..140 {
                document.active_layer_mut().surface.set_pixel(x, y, RED);
            }
        }
        document
    }

    fn red_area(document: &Document) -> Option<PixelRect> {
        document.active_layer().surface.painted_area()
    }

    /// Replays what was recorded, as "Export strokes" would write it.
    fn replay(document: &Document, history: &History) -> Document {
        let script = record::script(
            document.width(),
            document.height(),
            document.paper(),
            history.recorded_script(),
            None,
            None,
        );
        let mut start = picture();
        let mut player = Player::new(
            Script::parse(&script.to_json()).unwrap(),
            PlayOptions::default(),
        )
        .unwrap();
        player.start_on(&mut start).unwrap();
        player.run(&mut start, |_, _| {}).unwrap();
        start
    }

    fn same_pixels(a: &Document, b: &Document) -> bool {
        (0..a.height() as i32).all(|y| {
            (0..a.width() as i32).all(|x| {
                a.active_layer().surface.pixel(x, y) == b.active_layer().surface.pixel(x, y)
            })
        })
    }

    #[test]
    fn flips_mirror_around_the_middle_as_one_undo_step() {
        let mut document = picture();
        let mut history = History::default();
        assert!(flip(&mut document, &mut history, true));
        // 400 across: x 100..140 becomes 260..300.
        assert_eq!(red_area(&document), Some(PixelRect::new(260, 50, 300, 70)));
        assert!(flip(&mut document, &mut history, false));
        assert_eq!(
            red_area(&document),
            Some(PixelRect::new(260, 230, 300, 250))
        );
        assert_eq!(history.undo_steps(), 2);
        assert_eq!(history.undo_name(), Some("Flip vertical"));
        assert!(same_pixels(&replay(&document, &history), &document));
        history.undo(&mut document);
        history.undo(&mut document);
        assert_eq!(red_area(&document), Some(PixelRect::new(100, 50, 140, 70)));
        // Nothing to flip: no undo step.
        let mut empty = Document::new(100, 100);
        assert!(!flip(&mut empty, &mut history, true));
    }

    #[test]
    fn handles_move_resize_and_turn() {
        let mut document = picture();
        let mut tool = LayerTransform::start(&mut document).unwrap();
        assert!(!document.active_layer().visible(), "hidden meanwhile");
        let zoom = 1.0;
        let at = |x, y| Point::new(x, y);

        // Inside the box: move.
        assert_eq!(tool.handle_at(at(120.0, 60.0), zoom), Some(Handle::Move));
        tool.press(at(120.0, 60.0), zoom);
        tool.drag_to(at(150.0, 80.0), false);
        tool.release();
        assert_eq!(tool.placement.translate, Point::new(30.0, 20.0));
        assert_eq!(tool.corners()[0], Point::new(130.0, 70.0));

        // A corner: resize around the middle (now at 150, 80).
        tool.press(at(170.0, 90.0), zoom);
        tool.drag_to(at(190.0, 100.0), false); // 40 x 20 from the middle: twice as big
        tool.release();
        assert_eq!(tool.placement.scale, (2.0, 2.0));
        tool.press(at(190.0, 100.0), zoom);
        tool.drag_to(at(190.0, 120.0), false); // only taller
        tool.release();
        assert_eq!(tool.placement.scale, (2.0, 4.0));
        tool.press(at(190.0, 120.0), zoom);
        tool.drag_to(at(170.0, 100.0), true); // Shift: proportions stay 2:4
        tool.release();
        let (sx, sy) = tool.placement.scale;
        assert!((sy / sx - 2.0).abs() < 1e-4, "{sx} {sy}");

        // The round handle: turn. It's above the top edge; dragging it to
        // the right of the middle turns a quarter clockwise.
        let handle = tool.turn_handle(zoom);
        assert_eq!(tool.handle_at(handle, zoom), Some(Handle::Turn));
        tool.press(handle, zoom);
        tool.drag_to(at(250.0, 80.0), false);
        tool.release();
        assert!(
            (tool.placement.rotate - 90.0).abs() < 1e-3,
            "{}",
            tool.placement.rotate
        );

        // Far outside: nothing.
        assert_eq!(tool.handle_at(at(5.0, 290.0), zoom), None);
    }

    #[test]
    fn apply_paints_one_undo_step_that_replays_exactly() {
        let mut document = picture();
        let mut history = History::default();
        let mut tool = LayerTransform::start(&mut document).unwrap();
        tool.placement = Placement {
            translate: Point::new(80.123, 40.0),
            scale: (1.5, 0.75),
            rotate: 30.0,
        };
        let command = tool.command();
        assert_eq!(command.translate, Some([80.12, 40.0]), "rounded");
        tool.apply(&mut document, &mut history);
        assert!(document.active_layer().visible());
        assert_eq!(history.undo_steps(), 1);
        assert_eq!(history.undo_name(), Some("Transform layer"));
        assert_ne!(red_area(&document), Some(PixelRect::new(100, 50, 140, 70)));
        assert!(same_pixels(&replay(&document, &history), &document));
    }

    #[test]
    fn cancel_leaves_the_layer_as_it_was() {
        let mut document = picture();
        let mut tool = LayerTransform::start(&mut document).unwrap();
        tool.placement.rotate = 45.0;
        tool.cancel(&mut document);
        assert!(document.active_layer().visible());
        assert_eq!(red_area(&document), Some(PixelRect::new(100, 50, 140, 70)));

        let mut empty = Document::new(50, 50);
        assert!(LayerTransform::start(&mut empty).is_err());
    }
}
