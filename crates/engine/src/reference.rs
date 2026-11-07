//! Reference pictures pinned beside the painting: a photo or a drawing to
//! look at (and pick colours from) while painting. They aren't part of
//! the picture: they're never painted, exported or shown to scripts, but
//! they're saved in the `.quill` file with how each one was shown.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::file::FileError;
use crate::history::Command;
use crate::image::Image;

/// The biggest picture file that can be pinned, so a mistaken pick can't
/// fill the memory.
pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
/// And the biggest picture, in pixels across or down.
pub const MAX_SIDE: u32 = 16_384;

/// How a reference is shown.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReferenceView {
    /// Screen points per picture pixel.
    pub zoom: f32,
    /// The picture pixel in the middle of the window.
    pub centre: [f32; 2],
    /// 0 (invisible) to 1.
    pub opacity: f32,
    /// Where its window is: left, top, width, height in screen points
    /// (all 0 until it's first shown).
    pub window: [f32; 4],
}

impl ReferenceView {
    /// The whole picture, fitting a window about 320 points wide.
    fn fitting(image: &Image) -> ReferenceView {
        let (w, h) = (image.width().max(1) as f32, image.height().max(1) as f32);
        ReferenceView {
            zoom: (320.0 / w).min(320.0 / h),
            centre: [w / 2.0, h / 2.0],
            opacity: 1.0,
            window: [0.0; 4],
        }
    }
}

/// A pinned picture.
#[derive(Clone, Debug)]
pub struct Reference {
    /// The file it came from (without the folder), to show as its title.
    pub name: String,
    /// The file's bytes as they were (PNG or JPEG): saved like that.
    bytes: Arc<[u8]>,
    image: Arc<Image>,
    pub view: ReferenceView,
}

impl Reference {
    /// A reference from a picture file's bytes.
    pub fn from_bytes(name: &str, bytes: Vec<u8>) -> Result<Reference, FileError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(FileError::Damaged(format!(
                "the picture is too big to pin (over {} MB)",
                MAX_FILE_BYTES / (1024 * 1024)
            )));
        }
        let image = Image::decode(&bytes)?;
        if image.width() > MAX_SIDE || image.height() > MAX_SIDE {
            return Err(FileError::Damaged(format!(
                "the picture is too big to pin (over {MAX_SIDE} pixels across)"
            )));
        }
        Ok(Reference {
            name: name.to_owned(),
            view: ReferenceView::fitting(&image),
            bytes: bytes.into(),
            image: Arc::new(image),
        })
    }

    /// A reference read back from a file, shown as it was saved.
    pub(crate) fn saved(
        name: &str,
        bytes: Vec<u8>,
        view: ReferenceView,
    ) -> Result<Reference, FileError> {
        let mut reference = Reference::from_bytes(name, bytes)?;
        reference.view = view;
        Ok(reference)
    }

    pub fn image(&self) -> &Arc<Image> {
        &self.image
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The file ending its bytes deserve ("png" or "jpg").
    pub(crate) fn extension(&self) -> &'static str {
        if self.bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            "png"
        } else {
            "jpg"
        }
    }

    /// The colour at picture pixel (x, y), if that's on the picture.
    pub fn color_at(&self, x: f32, y: f32) -> Option<[u8; 3]> {
        let (w, h) = (self.image.width(), self.image.height());
        if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
            return None;
        }
        let [r, g, b, _] = self.image.pixel(x as u32, y as u32);
        Some([r, g, b])
    }
}

/// Undo for pinning or removing a reference.
pub struct ReferenceCommand {
    name: String,
    index: usize,
    /// The reference while it's not in the document.
    reference: Option<Reference>,
}

impl ReferenceCommand {
    /// Pins `reference` (at the end) and returns the undo step.
    pub fn add(document: &mut Document, reference: Reference) -> ReferenceCommand {
        let index = document.references().len();
        document.insert_reference(index, reference);
        ReferenceCommand {
            name: "Add reference".into(),
            index,
            reference: None,
        }
    }

    /// Takes the reference at `index` away, and returns the undo step.
    pub fn remove(document: &mut Document, index: usize) -> Option<ReferenceCommand> {
        let reference = document.remove_reference(index)?;
        Some(ReferenceCommand {
            name: "Remove reference".into(),
            index,
            reference: Some(reference),
        })
    }

    /// Puts it in, or takes it out: whichever it isn't.
    fn swap(&mut self, document: &mut Document) {
        match self.reference.take() {
            Some(reference) => document.insert_reference(self.index, reference),
            None => self.reference = document.remove_reference(self.index),
        }
    }
}

impl Command for ReferenceCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn undo(&mut self, document: &mut Document) {
        self.swap(document);
    }

    fn redo(&mut self, document: &mut Document) {
        self.swap(document);
    }

    fn memory_size(&self) -> usize {
        // The picture is shared with the document; only a removed one is
        // kept here alone.
        self.reference.as_ref().map_or(0, |r| r.bytes.len())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::document::Background;
    use crate::history::History;

    /// A small PNG: red on the left half, blue on the right.
    pub fn two_colour_png() -> Vec<u8> {
        let mut document = Document::new(40, 20);
        let surface = &mut document.active_layer_mut().surface;
        for y in 0..20 {
            for x in 0..40 {
                let colour = if x < 20 {
                    crate::tile::Rgba8::new(255, 0, 0, 255)
                } else {
                    crate::tile::Rgba8::new(0, 0, 255, 255)
                };
                surface.set_pixel(x, y, colour);
            }
        }
        let mut bytes = Vec::new();
        crate::file::write_png(&document, Background::Paper, &mut bytes).unwrap();
        bytes
    }

    #[test]
    fn a_reference_shows_its_picture_and_gives_its_colours() {
        let reference = Reference::from_bytes("cat.png", two_colour_png()).unwrap();
        assert_eq!(reference.image().width(), 40);
        assert_eq!(reference.color_at(5.0, 5.0), Some([255, 0, 0]));
        assert_eq!(reference.color_at(35.5, 19.9), Some([0, 0, 255]));
        assert_eq!(reference.color_at(40.0, 5.0), None, "off the picture");
        assert_eq!(reference.color_at(-0.5, 5.0), None);
        // It starts fitting a window 320 points wide, centred.
        assert_eq!(reference.view.zoom, 8.0);
        assert_eq!(reference.view.centre, [20.0, 10.0]);
        assert_eq!(reference.extension(), "png");
        assert!(Reference::from_bytes("notes.txt", b"hello".to_vec()).is_err());
    }

    #[test]
    fn pinning_and_removing_can_be_undone() {
        let mut document = Document::new(100, 100);
        let mut history = History::default();
        let reference = Reference::from_bytes("a.png", two_colour_png()).unwrap();
        history.push(Box::new(ReferenceCommand::add(
            &mut document,
            reference.clone(),
        )));
        let mut second = reference;
        second.name = "b.png".into();
        history.push(Box::new(ReferenceCommand::add(&mut document, second)));
        let step = ReferenceCommand::remove(&mut document, 0).unwrap();
        history.push(Box::new(step));
        let names = |d: &Document| {
            d.references()
                .iter()
                .map(|r| r.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&document), ["b.png"]);
        history.undo(&mut document);
        assert_eq!(names(&document), ["a.png", "b.png"], "back in its place");
        history.undo(&mut document);
        history.undo(&mut document);
        assert!(document.references().is_empty());
        history.redo(&mut document);
        assert_eq!(names(&document), ["a.png"]);
        assert!(
            history.recorded_script().is_empty(),
            "not part of the painting"
        );
        assert_eq!(history.unrecordable_steps(), 0);
        assert!(ReferenceCommand::remove(&mut document, 5).is_none());
    }
}
