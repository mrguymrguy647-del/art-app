//! The Duck Quill painting engine.
//!
//! Everything about pictures lives here: pixels, brushes, layers, undo.
//! It has no window or UI code, so it can be tested on its own with
//! `cargo test`, and it doesn't care which interface sits on top.

pub mod blend;
pub mod brush;
pub mod color;
pub mod critique;
pub mod document;
pub mod file;
pub mod fill;
pub mod filters;
pub mod gif;
pub mod history;
pub mod image;
pub mod layer;
pub mod layer_commands;
pub mod layer_transform;
pub mod noise;
pub mod pen;
pub mod pixels;
pub mod presets;
pub mod random;
pub mod reference;
pub mod script;
pub mod selection;
pub mod senses;
pub mod smoothing;
pub mod sprites;
pub mod stroke;
pub mod study;
pub mod surface;
pub mod text;
pub mod tile;
pub mod transform;
pub mod view;

/// The engine's version, for example "0.1.0" (taken from Cargo.toml).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_looks_like_major_minor_patch() {
        let parts: Vec<&str> = super::VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "version was {:?}", super::VERSION);
        for part in parts {
            assert!(part.parse::<u32>().is_ok(), "not a number: {part:?}");
        }
    }
}
