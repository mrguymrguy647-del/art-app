//! The Art App painting engine.
//!
//! Everything about pictures lives here: pixels, brushes, layers, undo.
//! It has no window or UI code, so it can be tested on its own with
//! `cargo test`, and it doesn't care which interface sits on top.

pub mod brush;
pub mod color;
pub mod document;
pub mod file;
pub mod fill;
pub mod history;
pub mod layer;
pub mod layer_commands;
pub mod pen;
pub mod smoothing;
pub mod stroke;
pub mod surface;
pub mod tile;
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
