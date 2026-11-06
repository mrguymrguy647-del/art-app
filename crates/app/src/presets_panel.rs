//! The Presets panel: a grid of brush presets, each with a little
//! preview stroke. Click one to paint with it. The built-in presets come
//! with the program and can't be changed (duplicate one to change it);
//! your own are JSON files in the settings folder's `presets` folder,
//! with `order.json` remembering the order of the grid.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, ColorImage, Key, Sense, TextureHandle, TextureOptions};
use quill_engine::brush::{BrushMode, BrushSettings};
use quill_engine::presets::{self, Preset};
use quill_engine::stroke::{Stroke, StrokePoint};
use quill_engine::surface::{PixelRect, Surface};

use crate::paint_tool::{PaintTool, Tool};

/// The file in the presets folder that keeps the grid's order.
const ORDER_FILE: &str = "order.json";

/// One preset in the grid.
pub struct Entry {
    pub preset: Preset,
    /// The brush it gives, worked out once.
    pub settings: BrushSettings,
    pub built_in: bool,
    /// Where it's saved (your own presets only).
    file: Option<PathBuf>,
}

impl Entry {
    fn new(preset: Preset, built_in: bool, file: Option<PathBuf>) -> Result<Entry, String> {
        let settings = preset.settings()?;
        Ok(Entry {
            preset,
            settings,
            built_in,
            file,
        })
    }

    pub fn name(&self) -> &str {
        &self.preset.name
    }
}

/// Every preset, built-in and your own, in the grid's order.
pub struct PresetLibrary {
    /// Where your own presets are saved (`None`: nowhere, so they only
    /// last until the program closes).
    folder: Option<PathBuf>,
    entries: Vec<Entry>,
    /// Files that couldn't be read when the presets were loaded.
    pub problems: Vec<String>,
}

impl PresetLibrary {
    /// The built-in presets, then everything in `folder`, in the order
    /// saved there (new ones at the end).
    pub fn load(folder: Option<PathBuf>) -> PresetLibrary {
        let mut entries: Vec<Entry> = presets::built_in()
            .iter()
            .map(|p| Entry::new(p.clone(), true, None).expect("built-in presets are checked"))
            .collect();
        let mut problems = Vec::new();
        let mut order: Vec<String> = Vec::new();
        if let Some(folder) = &folder
            && let Ok(listing) = std::fs::read_dir(folder)
        {
            let mut files: Vec<PathBuf> = listing
                .filter_map(|item| item.ok().map(|item| item.path()))
                .filter(|path| {
                    path.extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case(presets::FILE_EXTENSION))
                })
                .collect();
            files.sort();
            for path in files {
                if path.file_name().is_some_and(|name| name == ORDER_FILE) {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        order = serde_json::from_str(&text).unwrap_or_default();
                    }
                    continue;
                }
                let read = std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|text| Preset::read(&text))
                    .and_then(|preset| Entry::new(preset, false, Some(path.clone())));
                match read {
                    Ok(entry) if entries.iter().any(|e| same_name(e.name(), entry.name())) => {
                        problems.push(format!(
                            "{}: another preset is already called \"{}\"",
                            path.display(),
                            entry.name()
                        ));
                    }
                    Ok(entry) => entries.push(entry),
                    Err(problem) => problems.push(format!("{}: {problem}", path.display())),
                }
            }
        }
        // The saved order first; anything it doesn't mention after, as found.
        let position = |entry: &Entry| {
            order
                .iter()
                .position(|name| same_name(name, entry.name()))
                .unwrap_or(usize::MAX)
        };
        entries.sort_by_key(position); // (stable: unmentioned ones keep their order)
        PresetLibrary {
            folder,
            entries,
            problems,
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| same_name(e.name(), name))
    }

    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.index(name).map(|i| &self.entries[i])
    }

    /// `wanted`, or "wanted 2", "wanted 3"... whichever isn't taken.
    pub fn free_name(&self, wanted: &str) -> String {
        let wanted = wanted.trim();
        let wanted = if wanted.is_empty() {
            "My brush"
        } else {
            wanted
        };
        if self.index(wanted).is_none() {
            return wanted.to_owned();
        }
        (2..)
            .map(|n| format!("{wanted} {n}"))
            .find(|name| self.index(name).is_none())
            .expect("some number is free")
    }

    /// Adds a preset at the end (renamed if its name is taken) and saves
    /// it. Returns the name it got.
    pub fn add(&mut self, mut preset: Preset) -> Result<String, String> {
        preset.name = self.free_name(&preset.name);
        let name = preset.name.clone();
        let mut entry = Entry::new(preset, false, None)?;
        entry.file = self.save_file(&entry.preset, None)?;
        self.entries.push(entry);
        self.save_order()?;
        Ok(name)
    }

    /// A copy of a preset ("Ink pen copy") right after it, to change.
    pub fn duplicate(&mut self, name: &str) -> Result<String, String> {
        let from = self.index(name).ok_or("no such preset")?;
        let mut copy = self.entries[from].preset.clone();
        copy.name = format!("{} copy", copy.name);
        let new_name = self.add(copy)?;
        let added = self.entries.len() - 1;
        self.move_to(added, from + 1)?;
        Ok(new_name)
    }

    pub fn rename(&mut self, name: &str, to: &str) -> Result<(), String> {
        let index = self.index(name).ok_or("no such preset")?;
        let to = to.trim();
        if self.entries[index].built_in {
            return Err("A built-in preset can't be renamed: duplicate it first.".into());
        }
        if to.is_empty() {
            return Err("A preset needs a name.".into());
        }
        if self.index(to).is_some_and(|other| other != index) {
            return Err(format!("There's already a preset called \"{to}\"."));
        }
        let mut renamed = self.entries[index].preset.clone();
        renamed.name = to.to_owned();
        let old_file = self.entries[index].file.clone();
        let new_file = self.save_file(&renamed, old_file.as_deref())?;
        self.entries[index].preset = renamed;
        self.entries[index].file = new_file;
        self.save_order()
    }

    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let index = self.index(name).ok_or("no such preset")?;
        if self.entries[index].built_in {
            return Err("A built-in preset can't be deleted.".into());
        }
        let entry = self.entries.remove(index);
        if let Some(file) = entry.file {
            std::fs::remove_file(&file)
                .or_else(|e| match e.kind() {
                    std::io::ErrorKind::NotFound => Ok(()),
                    _ => Err(e),
                })
                .map_err(|e| format!("Couldn't delete {}: {e}", file.display()))?;
        }
        self.save_order()
    }

    /// Moves the preset at `from` so it ends up at `to`.
    pub fn move_to(&mut self, from: usize, to: usize) -> Result<(), String> {
        if from >= self.entries.len() {
            return Ok(());
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to.min(self.entries.len()), entry);
        self.save_order()
    }

    /// Moves the preset at `from` into the gap before the preset at
    /// `slot` (as the grid was before the move; the end is `len`).
    pub fn move_to_slot(&mut self, from: usize, slot: usize) -> Result<(), String> {
        // The slot counts the moved preset itself if it was before it.
        let to = if from < slot { slot - 1 } else { slot };
        self.move_to(from, to)
    }

    /// Adds a preset from a file someone gave you.
    pub fn import(&mut self, path: &Path) -> Result<String, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
        self.add(Preset::read(&text)?)
    }

    pub fn export(&self, name: &str, path: &Path) -> Result<(), String> {
        let entry = self.get(name).ok_or("no such preset")?;
        std::fs::write(path, entry.preset.to_json())
            .map_err(|e| format!("Couldn't save {}: {e}", path.display()))
    }

    /// Saves a preset in the folder (replacing `old`, if given) and
    /// returns its file. Without a folder there's nothing to do.
    fn save_file(&self, preset: &Preset, old: Option<&Path>) -> Result<Option<PathBuf>, String> {
        let Some(folder) = &self.folder else {
            return Ok(None);
        };
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("Couldn't make the folder {}: {e}", folder.display()))?;
        // A file name from the preset's name, numbered if it's taken by
        // another preset's file ("a/b" and "a_b" both become "a_b").
        let stem = file_stem(&preset.name);
        let path = (1..)
            .map(|n| {
                let name = if n == 1 {
                    format!("{stem}.{}", presets::FILE_EXTENSION)
                } else {
                    format!("{stem} {n}.{}", presets::FILE_EXTENSION)
                };
                folder.join(name)
            })
            .find(|path| {
                Some(path.as_path()) == old
                    || (!path.exists()
                        && !self.entries.iter().any(|e| e.file.as_ref() == Some(path)))
            })
            .expect("some number is free");
        std::fs::write(&path, preset.to_json())
            .map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
        if let Some(old) = old
            && old != path
        {
            let _ = std::fs::remove_file(old);
        }
        Ok(Some(path))
    }

    fn save_order(&self) -> Result<(), String> {
        let Some(folder) = &self.folder else {
            return Ok(());
        };
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("Couldn't make the folder {}: {e}", folder.display()))?;
        let names: Vec<&str> = self.entries.iter().map(Entry::name).collect();
        let text = serde_json::to_string_pretty(&names).expect("names can be written");
        let path = folder.join(ORDER_FILE);
        std::fs::write(&path, text).map_err(|e| format!("Couldn't save {}: {e}", path.display()))
    }
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// A name that's safe as a file name on Windows (and everywhere else).
fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim().trim_end_matches(['.', ' ']).to_owned();
    // Names Windows keeps for itself, like "con" and "lpt1".
    let reserved = ["con", "prn", "aux", "nul"].contains(&stem.to_ascii_lowercase().as_str())
        || (stem.len() == 4
            && stem.is_ascii()
            && ["com", "lpt"].contains(&stem[..3].to_ascii_lowercase().as_str())
            && stem.as_bytes()[3].is_ascii_digit());
    if stem.is_empty() || reserved || stem.eq_ignore_ascii_case("order") {
        format!("preset {stem}").trim().to_owned()
    } else {
        stem
    }
}

// ---- Previews -------------------------------------------------------------------

/// A preview's size in pixels: drawn twice the size it's shown, so it's crisp.
const PREVIEW: [usize; 2] = [144, 64];
const PAPER: [u8; 3] = [244, 240, 230];
const INK: [u8; 3] = [32, 32, 44];
/// The eraser rubs out a band of this grey.
const ERASED: [u8; 3] = [120, 120, 140];

/// A little S-shaped stroke with this brush, pressing lightly, then hard,
/// then lightly again, on paper. Brush sizes are squeezed to fit.
pub fn preview(settings: &BrushSettings) -> ColorImage {
    let [w, h] = PREVIEW;
    let mut surface = Surface::new();
    let limits = PixelRect::new(0, 0, w as i32, h as i32);
    let eraser = settings.mode == BrushMode::Erase;
    if eraser {
        let [r, g, b] = ERASED;
        for y in 10..h as i32 - 10 {
            for x in 0..w as i32 {
                surface.set_pixel(x, y, quill_engine::tile::Rgba8 { r, g, b, a: 255 });
            }
        }
    }
    let mut brush = *settings;
    brush.size = settings.size.clamp(2.5, 16.0) * 2.0;
    brush.color = INK;
    brush.source = None;
    let mut stroke = Stroke::new(brush, limits).seeded(7);
    let steps = 48;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = 14.0 + t * (w as f32 - 28.0);
        let y = h as f32 / 2.0 - (t * std::f32::consts::TAU).sin() * h as f32 * 0.22;
        let pressure = 0.15 + 0.85 * (t * std::f32::consts::PI).sin();
        stroke.add_point(&mut surface, StrokePoint::new(x, y, pressure));
    }
    stroke.finish(&mut surface);
    let pixels = (0..h as i32)
        .flat_map(|y| (0..w as i32).map(move |x| (x, y)))
        .map(|(x, y)| {
            // Premultiplied paint over the paper.
            let p = surface.pixel(x, y);
            let under = 255 - u32::from(p.a);
            let mix =
                |paint: u8, paper: u8| (u32::from(paint) + u32::from(paper) * under / 255) as u8;
            Color32::from_rgb(mix(p.r, PAPER[0]), mix(p.g, PAPER[1]), mix(p.b, PAPER[2]))
        })
        .collect();
    ColorImage::new(PREVIEW, pixels)
}

// ---- The panel ------------------------------------------------------------------

/// A small window being typed into.
enum Asking {
    /// Save the current brush (or eraser) as a preset with this name.
    SaveAs(String),
    /// Rename this preset to that.
    Rename(String, String),
}

pub struct PresetsPanel {
    /// The section is open (remembered between runs).
    pub open: bool,
    /// Loaded the first time the panel is shown.
    library: Option<PresetLibrary>,
    /// Each preset's preview, with the settings it was drawn for.
    previews: HashMap<String, (BrushSettings, TextureHandle)>,
    asking: Option<Asking>,
    /// The last thing that went wrong (shown under the grid).
    problem: Option<String>,
}

impl Default for PresetsPanel {
    fn default() -> Self {
        Self {
            open: true,
            library: None,
            previews: HashMap::new(),
            asking: None,
            problem: None,
        }
    }
}

/// How a preview tile is shown, in points.
const TILE: egui::Vec2 = egui::vec2(66.0, 30.0);

impl PresetsPanel {
    fn library(&mut self) -> &mut PresetLibrary {
        if self.library.is_none() {
            let folder = crate::settings_folder::settings_folder().map(|f| f.join("presets"));
            let library = PresetLibrary::load(folder);
            if !library.problems.is_empty() {
                self.problem = Some(format!(
                    "Some presets couldn't be read:\n{}",
                    library.problems.join("\n")
                ));
            }
            self.library = Some(library);
        }
        self.library.as_mut().expect("just loaded")
    }

    /// True while a small window (Save as preset, Rename) is open.
    pub fn dialog_open(&self) -> bool {
        self.asking.is_some()
    }

    /// The grid. Clicking a preset changes `tools`.
    pub fn ui(&mut self, ui: &mut egui::Ui, tools: &mut PaintTool) {
        let shown = egui::CollapsingHeader::new("Presets")
            .default_open(self.open)
            .show(ui, |ui| self.grid(ui, tools));
        self.open = shown.openness > 0.5;
    }

    fn grid(&mut self, ui: &mut egui::Ui, tools: &mut PaintTool) {
        // Which preset the current brush (or eraser) is, if any.
        let current = *tools.settings();
        let names: Vec<String> = self
            .library()
            .entries()
            .iter()
            .map(|e| e.name().to_owned())
            .collect();
        let mut dropped: Option<(String, usize)> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for (index, name) in names.iter().enumerate() {
                let Some(entry) = self.library().get(name) else {
                    continue;
                };
                let (settings, built_in) = (entry.settings, entry.built_in);
                let description = entry.preset.description.clone();
                let smoothing = entry.preset.smoothing;
                let texture = self.preview_texture(ui.ctx(), name, &settings);
                let in_use = is_in_use(&settings, &current);
                let response = tile(ui, name, &texture, in_use);
                if response.drag_started() {
                    response.dnd_set_drag_payload(name.clone());
                }
                // Dropped on a tile's left half: before it; right half: after.
                if response.dnd_hover_payload::<String>().is_some()
                    && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
                {
                    let after = pointer.x > response.rect.center().x;
                    let x = if after {
                        response.rect.right() + 2.0
                    } else {
                        response.rect.left() - 2.0
                    };
                    let line = egui::Stroke::new(2.5, ui.visuals().selection.bg_fill);
                    ui.painter().vline(x, response.rect.y_range(), line);
                    if let Some(dragged) = response.dnd_release_payload::<String>() {
                        dropped = Some(((*dragged).clone(), index + usize::from(after)));
                    }
                }
                let hover = if description.is_empty() {
                    name.clone()
                } else {
                    format!("{name}\n{description}")
                };
                let response = response.on_hover_text(format!(
                    "{hover}\n\nClick to paint with it. Right-click for more; drag to move it."
                ));
                if response.clicked() {
                    use_preset(tools, &settings, smoothing);
                }
                response
                    .context_menu(|ui| self.menu(ui, tools, name, built_in, &settings, smoothing));
            }
        });
        if let Some((name, to)) = dropped
            && let Some(from) = self.library().index(&name)
        {
            let result = self.library().move_to_slot(from, to);
            self.report(result);
        }
        ui.horizontal(|ui| {
            if ui
                .button("Save as preset…")
                .on_hover_text("Keep the current brush (or eraser) settings as a new preset")
                .clicked()
            {
                let wanted = if tools.tool == Tool::Eraser {
                    "My eraser"
                } else {
                    "My brush"
                };
                self.asking = Some(Asking::SaveAs(self.library().free_name(wanted)));
            }
            if ui
                .button("Import…")
                .on_hover_text("Add a preset from a .json file")
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Brush preset", &[presets::FILE_EXTENSION])
                    .pick_file()
            {
                let result = self.library().import(&path).map(|_| ());
                self.report(result);
            }
        });
        if let Some(problem) = &self.problem {
            ui.colored_label(ui.visuals().warn_fg_color, problem);
            if ui.small_button("OK").clicked() {
                self.problem = None;
            }
        }
    }

    fn menu(
        &mut self,
        ui: &mut egui::Ui,
        tools: &mut PaintTool,
        name: &str,
        built_in: bool,
        settings: &BrushSettings,
        smoothing: f32,
    ) {
        if ui.button("Use").clicked() {
            use_preset(tools, settings, smoothing);
            ui.close();
        }
        if ui.button("Duplicate").clicked() {
            let result = self.library().duplicate(name).map(|_| ());
            self.report(result);
            ui.close();
        }
        if built_in {
            ui.weak("Built in: duplicate it to change it.");
        } else {
            if ui.button("Rename…").clicked() {
                self.asking = Some(Asking::Rename(name.to_owned(), name.to_owned()));
                ui.close();
            }
            if ui
                .button("Update from current brush")
                .on_hover_text(
                    "Replace its settings with the brush (or eraser) you're painting with now",
                )
                .clicked()
            {
                let current = Preset::from_settings(name, tools.settings(), tools.smoothing);
                let result = self.replace(name, current);
                self.report(result);
                ui.close();
            }
            if ui.button("Delete").clicked() {
                let result = self.library().delete(name);
                self.report(result);
                ui.close();
            }
        }
        if ui.button("Export…").clicked() {
            ui.close();
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Brush preset", &[presets::FILE_EXTENSION])
                .set_file_name(format!("{}.{}", file_stem(name), presets::FILE_EXTENSION))
                .save_file()
            {
                let result = self.library().export(name, &path);
                self.report(result);
            }
        }
    }

    /// Your own preset `name` gets new settings (keeping its description).
    fn replace(&mut self, name: &str, mut preset: Preset) -> Result<(), String> {
        let library = self.library();
        let index = library.index(name).ok_or("no such preset")?;
        preset.description = library.entries[index].preset.description.clone();
        let old = library.entries[index].file.clone();
        let file = library.save_file(&preset, old.as_deref())?;
        let mut entry = Entry::new(preset, false, file)?;
        std::mem::swap(&mut library.entries[index], &mut entry);
        Ok(())
    }

    fn report(&mut self, result: Result<(), String>) {
        if let Err(problem) = result {
            self.problem = Some(problem);
        }
    }

    fn preview_texture(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        settings: &BrushSettings,
    ) -> TextureHandle {
        match self.previews.get(name) {
            Some((drawn, texture)) if drawn == settings => texture.clone(),
            _ => {
                let texture = ctx.load_texture(
                    format!("preset-{name}"),
                    preview(settings),
                    TextureOptions::LINEAR,
                );
                self.previews
                    .insert(name.to_owned(), (*settings, texture.clone()));
                texture
            }
        }
    }

    /// The Save as preset and Rename windows (shown whatever the tool).
    pub fn windows(&mut self, ctx: &egui::Context, tools: &mut PaintTool) {
        let Some(asking) = &mut self.asking else {
            return;
        };
        let (title, text) = match asking {
            Asking::SaveAs(name) => ("Save as preset", name),
            Asking::Rename(_, to) => ("Rename preset", to),
        };
        let (mut ok, mut cancel) = (false, false);
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Name:");
                let field = ui.text_edit_singleline(text);
                field.request_focus();
                ui.horizontal(|ui| {
                    ok = ui.add(crate::theme::primary_button("OK")).clicked()
                        || ui.input(|i| i.key_pressed(Key::Enter));
                    cancel =
                        ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape));
                });
            });
        if cancel {
            self.asking = None;
        } else if ok {
            let result = match self.asking.take() {
                Some(Asking::SaveAs(name)) => {
                    let preset = Preset::from_settings(&name, tools.settings(), tools.smoothing);
                    if name.trim().is_empty() {
                        Err("A preset needs a name.".to_owned())
                    } else {
                        self.library().add(preset).map(|_| ())
                    }
                }
                Some(Asking::Rename(from, to)) => {
                    let result = self.library().rename(&from, &to);
                    if result.is_ok() {
                        self.previews.remove(&from);
                    }
                    result
                }
                None => Ok(()),
            };
            self.report(result);
        }
    }
}

/// Paints with a preset: the brush keeps its colour; an eraser preset
/// becomes the eraser.
fn use_preset(tools: &mut PaintTool, settings: &BrushSettings, smoothing: f32) {
    if settings.mode == BrushMode::Erase {
        tools.eraser = *settings;
        tools.tool = Tool::Eraser;
    } else {
        let mut brush = *settings;
        brush.color = tools.brush.color;
        tools.brush = brush;
        tools.tool = Tool::Brush;
    }
    tools.smoothing = smoothing;
}

/// Whether the brush in use is this preset (any colour).
fn is_in_use(preset: &BrushSettings, current: &BrushSettings) -> bool {
    let mut current = *current;
    current.color = preset.color;
    current.source = preset.source;
    current == *preset
}

/// One preview with its name under it.
fn tile(ui: &mut egui::Ui, name: &str, texture: &TextureHandle, in_use: bool) -> egui::Response {
    let size = egui::vec2(TILE.x, TILE.y + 14.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let picture = egui::Rect::from_min_size(rect.min, TILE);
    painter.image(
        texture.id(),
        picture,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    let visuals = ui.visuals();
    let outline = if in_use {
        egui::Stroke::new(2.5, visuals.selection.bg_fill)
    } else if response.hovered() {
        visuals.widgets.hovered.bg_stroke
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    painter.rect_stroke(picture, 3.0, outline, egui::StrokeKind::Inside);
    // One line, cut short with "…" if it's long (the tooltip has it all).
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.to_owned(),
        egui::FontId::proportional(10.0),
        visuals.text_color(),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(TILE.x);
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            picture.bottom() + 1.0,
        ),
        galley,
        visuals.text_color(),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("quill-presets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    fn names(library: &PresetLibrary) -> Vec<&str> {
        library.entries().iter().map(Entry::name).collect()
    }

    #[test]
    fn your_presets_are_saved_renamed_moved_and_deleted_and_stay_that_way() {
        let folder = folder("keep");
        let mut library = PresetLibrary::load(Some(folder.clone()));
        assert_eq!(library.entries().len(), presets::built_in().len());
        let settings = BrushSettings {
            size: 33.0,
            ..BrushSettings::default()
        };
        let mine = library
            .add(Preset::from_settings("Fat brush", &settings, 0.4))
            .unwrap();
        assert_eq!(mine, "Fat brush");
        // The same name again gets a number.
        assert_eq!(
            library
                .add(Preset::from_settings("fat brush", &settings, 0.4))
                .unwrap(),
            "fat brush 2"
        );
        library.rename("fat brush 2", "Thin/odd: name?").unwrap();
        let copy = library.duplicate("Ink pen").unwrap();
        assert_eq!(copy, "Ink pen copy");
        assert_eq!(
            names(&library)[2],
            "Ink pen copy",
            "right after the one it copies"
        );
        // Dragging: into the gap before the third, then to the very end,
        // then the last one to the front.
        library.move_to_slot(0, 2).unwrap();
        assert_eq!(
            names(&library)[..3],
            ["Ink pen", "Pencil (graphite)", "Ink pen copy"]
        );
        let end = library.entries().len();
        library.move_to_slot(1, end).unwrap();
        assert_eq!(*names(&library).last().unwrap(), "Pencil (graphite)");
        let last = library.entries().len() - 1;
        library.move_to(last - 1, 0).unwrap();
        // Loading again gives the same presets in the same order.
        let again = PresetLibrary::load(Some(folder.clone()));
        assert!(again.problems.is_empty(), "{:?}", again.problems);
        assert_eq!(names(&again), names(&library));
        assert_eq!(names(&again)[0], "Thin/odd: name?");
        assert_eq!(again.get("Fat brush").unwrap().settings.size, 33.0);
        assert_eq!(again.get("Fat brush").unwrap().preset.smoothing, 0.4);
        // Deleting takes the file away too.
        library.delete("Fat brush").unwrap();
        library.delete("Thin/odd: name?").unwrap();
        let again = PresetLibrary::load(Some(folder.clone()));
        assert!(again.get("Fat brush").is_none());
        assert_eq!(again.entries().len(), presets::built_in().len() + 1);
        let files = std::fs::read_dir(&folder).unwrap().count();
        assert_eq!(files, 2, "the copy and order.json");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn built_in_presets_cant_be_changed_and_names_must_be_free() {
        let mut library = PresetLibrary::load(None);
        assert!(library.rename("Pencil (graphite)", "Mine").is_err());
        assert!(library.delete("Pencil (graphite)").is_err());
        library
            .add(Preset::from_settings(
                "Mine",
                &BrushSettings::default(),
                0.0,
            ))
            .unwrap();
        assert!(
            library
                .rename("Mine", "ink PEN")
                .unwrap_err()
                .contains("already")
        );
        assert!(library.rename("Mine", "  ").is_err());
        library.rename("Mine", "MINE").unwrap(); // only the capitals change
        assert!(library.get("mine").is_some());
        // An eraser preset makes the eraser.
        let mut tools = PaintTool::default();
        let eraser = library.get("Soft eraser").unwrap();
        use_preset(&mut tools, &eraser.settings, eraser.preset.smoothing);
        assert_eq!(tools.tool, Tool::Eraser);
        assert!(is_in_use(&eraser.settings, tools.settings()));
        // A brush preset keeps the colour.
        tools.brush.color = [200, 0, 0];
        let pencil = library.get("Pencil (graphite)").unwrap();
        use_preset(&mut tools, &pencil.settings, pencil.preset.smoothing);
        assert_eq!(tools.tool, Tool::Brush);
        assert_eq!(tools.brush.color, [200, 0, 0]);
        assert_eq!(tools.brush.grain, pencil.settings.grain);
        assert!(is_in_use(&pencil.settings, tools.settings()));
        tools.brush.size += 1.0;
        assert!(!is_in_use(&pencil.settings, tools.settings()));
    }

    #[test]
    fn import_and_export_and_bad_files() {
        let folder = folder("files");
        let mut library = PresetLibrary::load(Some(folder.join("presets")));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("shared.json");
        library.export("Chalk", &file).unwrap();
        // Importing a preset with a built-in's name renames it.
        assert_eq!(library.import(&file).unwrap(), "Chalk 2");
        assert_eq!(
            library.get("Chalk 2").unwrap().settings,
            library.get("Chalk").unwrap().settings
        );
        std::fs::write(&file, "{ not json").unwrap();
        assert!(
            library
                .import(&file)
                .unwrap_err()
                .contains("not a brush preset")
        );
        // A broken file in the folder is reported, the rest still load.
        std::fs::write(folder.join("presets/broken.json"), "[]").unwrap();
        let again = PresetLibrary::load(Some(folder.join("presets")));
        assert!(again.get("Chalk 2").is_some());
        assert_eq!(again.problems.len(), 1, "{:?}", again.problems);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn file_names_are_safe_on_windows() {
        assert_eq!(file_stem("Ink pen"), "Ink pen");
        assert_eq!(file_stem("a/b\\c:d*?"), "a_b_c_d__");
        assert_eq!(file_stem("dots..."), "dots");
        assert_eq!(file_stem("CON"), "preset CON");
        assert_eq!(file_stem("lpt1"), "preset lpt1");
        assert_eq!(file_stem("order"), "preset order");
        assert_eq!(file_stem("   "), "preset");
    }

    #[test]
    fn previews_show_the_stroke() {
        let ink = preview(&presets::find("Ink pen").unwrap().settings().unwrap());
        assert_eq!(ink.size, PREVIEW);
        let dark = ink.pixels.iter().filter(|p| p.r() < 100).count();
        assert!(dark > 100, "{dark}");
        // The eraser rubs paper back into a grey band.
        let eraser = preview(&presets::find("Soft eraser").unwrap().settings().unwrap());
        let middle = eraser.pixels[PREVIEW[0] * (PREVIEW[1] / 2) + PREVIEW[0] / 2];
        let edge = eraser.pixels[PREVIEW[0] * 12 + 2];
        assert!(middle.r() > edge.r() + 50, "{middle:?} vs {edge:?}");
        // Every built-in one paints something.
        for preset in presets::built_in() {
            let image = preview(&preset.settings().unwrap());
            assert!(image.pixels.iter().any(|p| p.r() < 200), "{}", preset.name);
        }
    }
}
