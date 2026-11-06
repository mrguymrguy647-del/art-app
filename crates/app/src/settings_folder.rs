//! Where Duck Quill keeps its own files (presets now; more later):
//! `%APPDATA%\Duck Quill` on Windows, `~/.config/duck-quill` elsewhere.

use std::path::PathBuf;

/// The folder, or `None` if the system doesn't say where it should be.
/// It may not exist yet: whoever saves into it creates it.
pub fn settings_folder() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|folder| PathBuf::from(folder).join("Duck Quill"))
    }
    #[cfg(not(windows))]
    {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|folder| folder.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(config.join("duck-quill"))
    }
}
