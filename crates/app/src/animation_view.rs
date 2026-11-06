//! The Animation panel: a script's animations, frame by frame.
//!
//! When a script with animations is opened, every frame of every
//! animation is painted once (they're small, and it takes well under a
//! second), and the canvas shows one of them. The panel at the bottom
//! lists the frames, plays and pauses, steps one frame back or forward,
//! sets the fps and switches an onion skin on: the frame before in red
//! and the one after in blue, faint, over the current frame.
//!
//! Drawing on the frames by hand comes in phase 3; for now the canvas is
//! locked while the panel is open. Closing the panel keeps the frame
//! shown as the picture.

use std::path::{Path, PathBuf};

use art_engine::document::{Background, Document};
use art_engine::gif::save_gif;
use art_engine::image::Image;
use art_engine::script::{PlayOptions, Script, Sequence};
use art_engine::sprites::{SheetAnimation, sprite_sheet};
use eframe::egui::{self, Button, Slider, vec2};

/// Onion skins are this strong (0 to 1).
const ONION_STRENGTH: f32 = 0.35;
const ONION_BEFORE: [u8; 3] = [230, 60, 60];
const ONION_AFTER: [u8; 3] = [60, 110, 230];

/// One animation, painted.
pub struct FrameSet {
    pub name: String,
    pub fps: f32,
    pub looping: bool,
    pub frames: Vec<Document>,
}

/// What the panel asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelAction {
    None,
    /// Close the panel and keep the frame shown.
    Close,
}

pub struct AnimationView {
    /// The script file's name.
    pub name: String,
    sets: Vec<FrameSet>,
    /// Which animation and frame is shown.
    set: usize,
    frame: usize,
    playing: bool,
    /// Frames per second while playing (starts as the animation's).
    pub fps: f32,
    pub onion: bool,
    /// Time since the frame last changed, in seconds.
    elapsed: f32,
    pivot: [f32; 2],
}

impl AnimationView {
    /// Paints every frame of every animation in `script` (`folder` is
    /// the script's, for `open` paths). `Err` says what went wrong.
    pub fn new(script: &Script, folder: Option<PathBuf>, name: String) -> Result<Self, String> {
        let mut sets = Vec::new();
        let mut canvas = (0, 0);
        for (animation_name, animation) in &script.animations {
            let mut frames = Vec::with_capacity(animation.frames as usize);
            // The frames in order: static layers are painted once.
            let mut sequence = Sequence::new(
                script.clone(),
                PlayOptions {
                    folder: folder.clone(),
                    animation: Some(animation_name.clone()),
                    ..PlayOptions::default()
                },
            );
            for frame in 0..animation.frames {
                let (document, _) = sequence
                    .paint(frame, |_, _| {})
                    .map_err(|e| e.to_string())?;
                canvas = (document.width(), document.height());
                frames.push(document);
            }
            sets.push(FrameSet {
                name: animation_name.clone(),
                fps: animation.fps,
                looping: animation.looping,
                frames,
            });
        }
        if sets.is_empty() {
            return Err("this script has no animations".into());
        }
        let fps = sets[0].fps;
        Ok(Self {
            name,
            sets,
            set: 0,
            frame: 0,
            playing: true,
            fps,
            onion: false,
            elapsed: 0.0,
            pivot: script
                .pivot
                .unwrap_or([canvas.0 as f32 / 2.0, canvas.1 as f32]),
        })
    }

    /// The animation being shown.
    pub fn current(&self) -> &FrameSet {
        &self.sets[self.set]
    }

    /// The frame being shown, from 0.
    pub fn frame(&self) -> usize {
        self.frame
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// The frame being shown, as a picture to put on the canvas.
    pub fn document(&self) -> &Document {
        &self.current().frames[self.frame]
    }

    /// Shows another animation, from its first frame.
    pub fn choose(&mut self, set: usize) {
        if set < self.sets.len() && set != self.set {
            self.set = set;
            self.frame = 0;
            self.fps = self.sets[set].fps;
            self.elapsed = 0.0;
        }
    }

    /// Moves `by` frames (negative: back), round and round.
    pub fn step(&mut self, by: i64) {
        let count = self.current().frames.len() as i64;
        self.frame = (self.frame as i64 + by).rem_euclid(count) as usize;
        self.elapsed = 0.0;
    }

    /// Shows frame `frame` (if there is one).
    pub fn go_to(&mut self, frame: usize) {
        if frame < self.current().frames.len() {
            self.frame = frame;
            self.elapsed = 0.0;
        }
    }

    /// Lets `seconds` pass while playing. Returns true if the frame shown
    /// changed. An animation that doesn't loop stops on its last frame.
    pub fn tick(&mut self, seconds: f32) -> bool {
        if !self.playing || self.fps <= 0.0 {
            return false;
        }
        self.elapsed += seconds.max(0.0);
        let each = 1.0 / self.fps;
        let mut moved = false;
        while self.elapsed >= each {
            self.elapsed -= each;
            let last = self.current().frames.len() - 1;
            if self.frame < last {
                self.frame += 1;
                moved = true;
            } else if self.current().looping {
                self.frame = 0;
                moved = true;
            } else {
                self.playing = false;
                self.elapsed = 0.0;
                break;
            }
        }
        moved
    }

    /// The onion skin for the frame shown: the frame before in red and
    /// the one after in blue, faint, in one see-through picture. `None`
    /// when the onion skin is off or there's only one frame.
    pub fn onion_skin(&self) -> Option<Image> {
        let frames = &self.current().frames;
        if !self.onion || frames.len() < 2 {
            return None;
        }
        let looping = self.current().looping;
        let neighbour = |offset: i64| -> Option<&Document> {
            let n = self.frame as i64 + offset;
            if (0..frames.len() as i64).contains(&n) {
                Some(&frames[n as usize])
            } else if looping {
                Some(&frames[n.rem_euclid(frames.len() as i64) as usize])
            } else {
                None
            }
        };
        let (width, height) = (self.document().width(), self.document().height());
        let mut skin = Image::new(width, height, [0, 0, 0, 0]);
        if let Some(before) = neighbour(-1) {
            let image = Image::of_document(before, Background::Transparent);
            skin.paste(&image.tinted(ONION_BEFORE, ONION_STRENGTH), 0, 0);
        }
        if let Some(after) = neighbour(1) {
            let image = Image::of_document(after, Background::Transparent);
            skin.paste(&image.tinted(ONION_AFTER, ONION_STRENGTH), 0, 0);
        }
        Some(skin)
    }

    // ---- Exports -----------------------------------------------------------------

    /// Writes the sprite sheet (every animation) as `png_path` and its
    /// JSON next to it (same name, `.json`).
    pub fn export_sheet(&self, png_path: &Path) -> Result<PathBuf, String> {
        let animations: Vec<SheetAnimation> = self
            .sets
            .iter()
            .map(|set| SheetAnimation {
                name: set.name.clone(),
                frames: set
                    .frames
                    .iter()
                    .map(|d| Image::of_document(d, Background::Transparent))
                    .collect(),
                fps: set.fps,
                looping: set.looping,
            })
            .collect();
        let image_name = png_path.file_name().map_or_else(
            || "sheet.png".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let (sheet, json) = sprite_sheet(&animations, &image_name, self.pivot, 1)?;
        sheet
            .save_png(png_path, true)
            .map_err(|e| format!("{}: {e}", png_path.display()))?;
        let json_path = png_path.with_extension("json");
        std::fs::write(&json_path, json).map_err(|e| format!("{}: {e}", json_path.display()))?;
        Ok(json_path)
    }

    /// Writes every frame of the animation shown into `folder`, as
    /// `<animation>-000.png`, ... Returns how many.
    pub fn export_frames(&self, folder: &Path) -> Result<usize, String> {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let set = self.current();
        for (n, document) in set.frames.iter().enumerate() {
            let path = folder.join(format!("{}-{n:03}.png", set.name));
            Image::of_document(document, Background::Transparent)
                .save_png(&path, true)
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(set.frames.len())
    }

    /// Writes the animation shown as a GIF, at the fps set in the panel.
    pub fn export_gif(&self, path: &Path) -> Result<(), String> {
        let set = self.current();
        let frames: Vec<Image> = set
            .frames
            .iter()
            .map(|d| Image::of_document(d, Background::Transparent))
            .collect();
        save_gif(&frames, self.fps, set.looping, path)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    // ---- The panel -------------------------------------------------------------

    /// The panel at the bottom of the window.
    pub fn panel(&mut self, ui: &mut egui::Ui) -> PanelAction {
        let mut action = PanelAction::None;
        ui.horizontal(|ui| {
            ui.strong(format!("Animation: {}", self.name));
            if self.sets.len() > 1 {
                let mut chosen = self.set;
                egui::ComboBox::from_id_salt("animation_set")
                    .selected_text(&self.sets[self.set].name)
                    .show_ui(ui, |ui| {
                        for (i, set) in self.sets.iter().enumerate() {
                            ui.selectable_value(&mut chosen, i, &set.name);
                        }
                    });
                self.choose(chosen);
            } else {
                ui.label(&self.sets[0].name);
            }
            ui.separator();
            let size = vec2(60.0, 26.0);
            if ui
                .add(Button::new("|<").min_size(size))
                .on_hover_text("One frame back")
                .clicked()
            {
                self.playing = false;
                self.step(-1);
            }
            let label = if self.playing { "Pause" } else { "Play" };
            if ui.add(Button::new(label).min_size(size)).clicked() {
                self.playing = !self.playing;
                self.elapsed = 0.0;
            }
            if ui
                .add(Button::new(">|").min_size(size))
                .on_hover_text("One frame forward")
                .clicked()
            {
                self.playing = false;
                self.step(1);
            }
            ui.add(
                Slider::new(&mut self.fps, 1.0..=60.0)
                    .text("fps")
                    .custom_formatter(|v, _| format!("{v:.0}")),
            );
            ui.checkbox(&mut self.onion, "Onion skin")
                .on_hover_text("The frame before in red and the one after in blue, faint");
            ui.separator();
            if ui
                .add(Button::new("Close").min_size(size))
                .on_hover_text("Keep this frame as the picture")
                .clicked()
            {
                action = PanelAction::Close;
            }
        });
        // The frames: a row of numbered buttons, the shown one selected.
        ui.horizontal_wrapped(|ui| {
            let count = self.current().frames.len();
            for n in 0..count {
                let selected = n == self.frame;
                if ui.selectable_label(selected, format!("{n}")).clicked() {
                    self.playing = false;
                    self.go_to(n);
                }
            }
            ui.weak(format!(
                "{count} frames, {} fps in the script{}",
                self.current().fps,
                if self.current().looping {
                    ", loops"
                } else {
                    ""
                }
            ));
        });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = r##"{ "palette": { "ink": "#202020" }, "pivot": [10, 18], "commands": [
        { "cmd": "canvas", "width": 20, "height": 20 },
        { "cmd": "define", "name": "dot", "commands": [ { "cmd": "rect", "x": 0, "y": 0, "width": 4, "height": 4, "fill": "ink" } ] }
      ], "animations": {
        "idle": { "frames": 3, "fps": 10, "loop": true, "commands": [
            { "cmd": "use", "name": "dot", "translate": { "keys": [[0, [2, 2]], [2, [12, 2]]] } } ] },
        "once": { "frames": 2, "fps": 2, "loop": false, "commands": [ { "cmd": "use", "name": "dot" } ] }
      } }"##;

    fn view() -> AnimationView {
        AnimationView::new(&Script::parse(SCRIPT).unwrap(), None, "test.json".into()).unwrap()
    }

    fn painted_left(document: &Document) -> i32 {
        document.active_layer().surface.painted_area().unwrap().left
    }

    #[test]
    fn every_frame_is_painted_and_shown_in_turn() {
        let mut view = view();
        assert_eq!(view.current().name, "idle");
        assert_eq!(view.current().frames.len(), 3);
        assert_eq!(painted_left(view.document()), 2);
        view.step(1);
        assert_eq!(painted_left(view.document()), 7, "halfway");
        view.step(1);
        assert_eq!(painted_left(view.document()), 12);
        view.step(1);
        assert_eq!(view.frame(), 0, "round and round");
        view.step(-1);
        assert_eq!(view.frame(), 2);
        view.choose(1);
        assert_eq!(
            (view.current().name.as_str(), view.frame(), view.fps),
            ("once", 0, 2.0)
        );
    }

    #[test]
    fn playing_follows_the_fps_and_stops_at_the_end_unless_it_loops() {
        let mut view = view();
        assert!(view.is_playing());
        assert!(!view.tick(0.05), "not yet: a frame is 0.1 s at 10 fps");
        assert!(view.tick(0.06));
        assert_eq!(view.frame(), 1);
        view.fps = 20.0;
        assert!(view.tick(0.1), "two frames at once");
        assert_eq!(view.frame(), 0, "1 + 2 = 3, round to 0");
        view.choose(1);
        view.playing = true;
        view.tick(0.5);
        assert_eq!(view.frame(), 1);
        view.tick(0.5);
        assert_eq!(view.frame(), 1, "stays on the last frame");
        assert!(!view.is_playing());
    }

    #[test]
    fn the_onion_skin_shows_the_neighbours_in_two_tints() {
        let mut view = view();
        assert!(view.onion_skin().is_none(), "off");
        view.onion = true;
        view.go_to(1);
        let skin = view.onion_skin().unwrap();
        // Frame 0's dot (x 2..6) in red, frame 2's (x 12..16) in blue, faint.
        let red = skin.pixel(3, 3);
        let blue = skin.pixel(13, 3);
        assert_eq!((red[0], red[1], red[2]), (230, 60, 60));
        assert_eq!((blue[0], blue[1], blue[2]), (60, 110, 230));
        assert!(red[3] > 60 && red[3] < 120, "faint: {}", red[3]);
        assert_eq!(skin.pixel(8, 3)[3], 0, "the frame itself isn't in the skin");
        // The last frame of a loop has frame 0 after it.
        view.go_to(2);
        assert_eq!(
            view.onion_skin().unwrap().pixel(3, 3)[2],
            230,
            "blue: frame 0 comes next"
        );
    }

    #[test]
    fn exports_write_the_sheet_frames_and_gif() {
        let view = view();
        let folder = std::env::temp_dir().join(format!("art-app-animation-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let json = view.export_sheet(&folder.join("dots.png")).unwrap();
        let text = std::fs::read_to_string(&json).unwrap();
        assert!(text.contains("\"image\": \"dots.png\""), "{text}");
        assert!(text.contains("\"pivot\": [10.0,18.0]"), "{text}");
        assert!(
            text.contains("\"once\": {\"row\":1,\"frames\":[3,4]"),
            "{text}"
        );
        assert!(folder.join("dots.png").exists());
        assert_eq!(view.export_frames(&folder.join("frames")).unwrap(), 3);
        assert!(folder.join("frames/idle-002.png").exists());
        view.export_gif(&folder.join("idle.gif")).unwrap();
        assert!(
            std::fs::read(folder.join("idle.gif"))
                .unwrap()
                .starts_with(b"GIF89a")
        );
        std::fs::remove_dir_all(folder).ok();
    }
}
