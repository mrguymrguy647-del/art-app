//! What Duck Quill remembers between runs: the window's size, the brush
//! and eraser, colours, the theme, a couple of panels and the recent
//! files. Kept as `settings.json` in the settings folder (see
//! `settings_folder.rs`), readable text. A missing or broken file just
//! means starting with the defaults.

use std::path::{Path, PathBuf};

use quill_engine::presets::Preset;
use serde::{Deserialize, Serialize};

/// The file in the settings folder.
const FILE_NAME: &str = "settings.json";
/// How many recent files File → Open recent lists.
pub const MAX_RECENT_FILES: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub window: Option<WindowPlace>,
    /// Newest first.
    pub recent_files: Vec<PathBuf>,
    /// The brush and eraser as they were left (a preset holds every
    /// setting but the colour, which is kept apart).
    pub brush: Option<Preset>,
    pub eraser: Option<Preset>,
    pub color: Option<[u8; 3]>,
    pub recent_colors: Vec<[u8; 3]>,
    /// "dark", "light" or "system".
    pub theme: Option<String>,
    /// The Presets section is open.
    pub presets_open: Option<bool>,
    pub references_hidden: bool,
}

/// The window's size (in screen points) when it isn't maximized, and
/// whether it was maximized.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlace {
    pub size: [f32; 2],
    pub maximized: bool,
}

impl Settings {
    /// The settings saved in `folder`, or the defaults.
    pub fn load(folder: Option<&Path>) -> Settings {
        folder
            .and_then(|folder| std::fs::read_to_string(folder.join(FILE_NAME)).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Saves them in `folder` (made if needed). Written to a temporary
    /// file first, so a crash halfway never leaves a broken file.
    pub fn save(&self, folder: &Path) -> Result<(), String> {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let temporary = folder.join(format!("{FILE_NAME}.tmp"));
        std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, folder.join(FILE_NAME)).map_err(|e| e.to_string())
    }
}

/// Puts `path` at the front of `recent` (once), keeping at most
/// `MAX_RECENT_FILES`.
pub fn add_recent(recent: &mut Vec<PathBuf>, path: &Path) {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    recent.retain(|p| p != &path);
    recent.insert(0, path);
    recent.truncate(MAX_RECENT_FILES);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_engine::brush::BrushSettings;

    #[test]
    fn settings_are_saved_and_read_back_and_a_bad_file_gives_defaults() {
        let folder = std::env::temp_dir().join(format!("quill-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(
            Settings::load(Some(&folder)),
            Settings::default(),
            "no file yet"
        );
        let brush = BrushSettings {
            size: 42.0,
            ..BrushSettings::default()
        };
        let mut settings = Settings {
            window: Some(WindowPlace {
                size: [1200.0, 700.0],
                maximized: false,
            }),
            brush: Some(Preset::from_settings("Brush", &brush, 0.3)),
            color: Some([10, 20, 30]),
            recent_colors: vec![[1, 2, 3]],
            theme: Some("light".into()),
            presets_open: Some(false),
            references_hidden: true,
            ..Settings::default()
        };
        add_recent(&mut settings.recent_files, Path::new("/pictures/a.quill"));
        settings.save(&folder).unwrap();
        let back = Settings::load(Some(&folder));
        assert_eq!(back, settings);
        assert_eq!(back.brush.unwrap().settings().unwrap().size, 42.0);
        // A broken file, or one from a newer version with more in it.
        std::fs::write(folder.join(FILE_NAME), "{ broken").unwrap();
        assert_eq!(Settings::load(Some(&folder)), Settings::default());
        std::fs::write(
            folder.join(FILE_NAME),
            r#"{"color": [1, 1, 1], "later": 5}"#,
        )
        .unwrap();
        assert_eq!(Settings::load(Some(&folder)).color, Some([1, 1, 1]));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn recent_files_are_newest_first_without_repeats() {
        let mut recent = Vec::new();
        for i in 0..12 {
            add_recent(&mut recent, Path::new(&format!("/p/{i}.quill")));
        }
        add_recent(&mut recent, Path::new("/p/5.quill"));
        assert_eq!(recent.len(), MAX_RECENT_FILES);
        assert_eq!(recent[0], Path::new("/p/5.quill"));
        assert_eq!(recent[1], Path::new("/p/11.quill"));
        assert_eq!(recent.iter().filter(|p| p.ends_with("5.quill")).count(), 1);
    }
}
