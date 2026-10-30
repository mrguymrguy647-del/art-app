//! Saving and opening pictures, and exporting PNG images.
//!
//! An `.artapp` file is a zip archive (like Krita's `.kra` or the
//! OpenRaster `.ora` format). Inside:
//! - `document.json`: the canvas size, paper colour and each layer's
//!   settings, as readable text;
//! - `layers/<n>/<x>_<y>.png`: every painted tile of layer `n` (0 = bottom)
//!   as a 256x256 PNG image.
//!
//! Only painted tiles are stored, so saving is quick and files stay small.
//! Tiles are stored exactly as the app keeps them in memory (8-bit,
//! premultiplied), so opening a file gives back exactly the same pixels.
//! The file says which format and version it is, so later versions of the
//! app (for example with 16-bit colour) can still open old files.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::document::{Document, MAX_CANVAS_SIDE};
use crate::image::Image;
use crate::layer::{BlendMode, Layer, LayerId, LayerProps};
use crate::surface::{Surface, TileCoord};
use crate::tile::{Rgba8, TILE_SIZE, Tile};

/// The usual ending of our file names.
pub const FILE_EXTENSION: &str = "artapp";

/// Written into every file, so we can tell our files from others.
const FORMAT_NAME: &str = "artapp";
/// The file version this app writes, and the newest it can open. When the
/// format changes, this goes up, and older versions must keep opening.
pub const FORMAT_VERSION: u32 = 1;
/// How tile pixels are stored in version 1.
const PIXEL_FORMAT: &str = "rgba8-premultiplied";
/// The description inside the zip archive.
const DESCRIPTION_FILE: &str = "document.json";

/// `document.json`: everything about the picture except the pixels.
#[derive(Serialize, Deserialize)]
struct Description {
    format: String,
    version: u32,
    width: u32,
    height: u32,
    paper: [u8; 3],
    pixels: String,
    active_layer: usize,
    next_layer_number: u64,
    /// Bottom layer first.
    layers: Vec<LayerDescription>,
}

#[derive(Serialize, Deserialize)]
struct LayerDescription {
    name: String,
    visible: bool,
    opacity: f32,
    blend: String,
}

/// Something went wrong while saving, opening or exporting.
#[derive(Debug)]
pub enum FileError {
    /// Reading or writing failed: disk full, no permission, missing file...
    Io(io::Error),
    /// The file isn't an Art App picture.
    NotArtApp,
    /// The file was made by a newer version of Art App.
    TooNew(u32),
    /// It's an Art App picture, but part of it is damaged.
    Damaged(String),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::Io(error) => write!(f, "The file couldn't be read or written: {error}"),
            FileError::NotArtApp => write!(f, "This isn't an Art App picture (.{FILE_EXTENSION})."),
            FileError::TooNew(version) => write!(
                f,
                "This picture was saved by a newer version of Art App (file version {version}). \
                 Please update Art App to open it."
            ),
            FileError::Damaged(detail) => write!(f, "This picture file is damaged: {detail}"),
        }
    }
}

impl std::error::Error for FileError {}

impl From<io::Error> for FileError {
    fn from(error: io::Error) -> Self {
        FileError::Io(error)
    }
}

/// Turns any other kind of error that happens while *writing* into an I/O error.
pub(crate) fn write_error(error: impl fmt::Display) -> FileError {
    FileError::Io(io::Error::other(error.to_string()))
}

// ---- Saving -----------------------------------------------------------------

/// Saves the picture to `path`.
///
/// It first writes a temporary file next to it and only then replaces the
/// real file, so if saving fails halfway (for example, the disk is full),
/// the previous version of the file is still intact.
pub fn save(document: &Document, path: &Path) -> Result<(), FileError> {
    write_safely(path, |file| write_document(document, file).map(|_| ()))
}

/// Writes the picture as an `.artapp` archive to any writer.
pub fn write_document<W: Write + Seek>(document: &Document, writer: W) -> Result<W, FileError> {
    let mut zip = ZipWriter::new(writer);
    // PNG images are already compressed; the description is plain text.
    let as_is = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let compressed = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let layers = document.layers();
    let description = Description {
        format: FORMAT_NAME.to_owned(),
        version: FORMAT_VERSION,
        width: document.width(),
        height: document.height(),
        paper: [document.paper().r, document.paper().g, document.paper().b],
        pixels: PIXEL_FORMAT.to_owned(),
        active_layer: document.layer_index(document.active_id()).unwrap_or(0),
        next_layer_number: document.next_layer_number(),
        layers: layers
            .iter()
            .map(|layer| LayerDescription {
                name: layer.name().to_owned(),
                visible: layer.visible(),
                opacity: layer.opacity(),
                blend: layer.blend().name().to_owned(),
            })
            .collect(),
    };
    zip.start_file(DESCRIPTION_FILE, compressed)
        .map_err(write_error)?;
    serde_json::to_writer_pretty(&mut zip, &description).map_err(write_error)?;

    for (index, layer) in layers.iter().enumerate() {
        for (coord, tile) in layer.surface.tiles() {
            let name = format!("layers/{index}/{}_{}.png", coord.x, coord.y);
            zip.start_file(name, as_is).map_err(write_error)?;
            encode_tile(tile, &mut zip)?;
        }
    }
    zip.finish().map_err(write_error)
}

fn encode_tile(tile: &Tile, writer: impl Write) -> Result<(), FileError> {
    let bytes: Vec<u8> = tile
        .pixels()
        .iter()
        .flat_map(|pixel| [pixel.r, pixel.g, pixel.b, pixel.a])
        .collect();
    let size = TILE_SIZE as u32;
    let mut encoder = png::Encoder::new(writer, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut png_writer = encoder.write_header().map_err(write_error)?;
    png_writer.write_image_data(&bytes).map_err(write_error)?;
    png_writer.finish().map_err(write_error)
}

// ---- Opening ----------------------------------------------------------------

/// Opens an `.artapp` picture.
pub fn open(path: &Path) -> Result<Document, FileError> {
    let file = File::open(path)?;
    read_document(io::BufReader::new(file))
}

/// Reads an `.artapp` archive from any reader.
pub fn read_document<R: Read + Seek>(reader: R) -> Result<Document, FileError> {
    let mut archive = ZipArchive::new(reader).map_err(|_| FileError::NotArtApp)?;

    // 1. The description. Check what kind of file it is before anything else.
    let text = {
        let mut entry = archive
            .by_name(DESCRIPTION_FILE)
            .map_err(|_| FileError::NotArtApp)?;
        let mut text = String::new();
        entry
            .read_to_string(&mut text)
            .map_err(|_| FileError::Damaged("the description can't be read".into()))?;
        text
    };
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| FileError::NotArtApp)?;
    if value.get("format").and_then(|f| f.as_str()) != Some(FORMAT_NAME) {
        return Err(FileError::NotArtApp);
    }
    let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if version > FORMAT_VERSION {
        return Err(FileError::TooNew(version));
    }
    let description: Description = serde_json::from_value(value)
        .map_err(|error| FileError::Damaged(format!("the description is incomplete ({error})")))?;
    check_description(&description)?;

    // 2. The layers, empty for now.
    let mut surfaces: Vec<Surface> = description.layers.iter().map(|_| Surface::new()).collect();

    // 3. The tiles. Any files we don't recognise are left alone, so later
    //    versions can add things without breaking older ones.
    let names: Vec<String> = archive
        .file_names()
        .map(|name| name.map(|name| name.into_owned()))
        .collect::<Result<_, _>>()
        .map_err(|_| FileError::Damaged("the list of contents can't be read".into()))?;
    for name in names {
        let Some((layer_index, coord)) = parse_tile_name(&name) else {
            continue;
        };
        let surface = surfaces.get_mut(layer_index).ok_or_else(|| {
            FileError::Damaged(format!("{name} belongs to a layer that doesn't exist"))
        })?;
        let mut bytes = Vec::new();
        archive
            .by_name(&name)
            .and_then(|mut entry| entry.read_to_end(&mut bytes).map_err(Into::into))
            .map_err(|_| FileError::Damaged(format!("{name} can't be read")))?;
        let tile = decode_tile(&bytes)
            .map_err(|detail| FileError::Damaged(format!("{name}: {detail}")))?;
        surface.replace_tile(coord, Some(tile));
    }

    // 4. Put it all together.
    let layers = description
        .layers
        .into_iter()
        .zip(surfaces)
        .enumerate()
        .map(|(index, (saved, surface))| {
            let props = LayerProps {
                name: saved.name,
                visible: saved.visible,
                opacity: saved.opacity,
                // Checked by `check_description`.
                blend: BlendMode::from_name(&saved.blend).unwrap_or_default(),
            };
            Layer::from_saved(LayerId(index as u64 + 1), props, surface)
        })
        .collect::<Vec<_>>();
    let [r, g, b] = description.paper;
    // Layer ids are given out fresh (1, 2, 3, ...), so the next one must
    // come after them, whatever the file says.
    let next_number = description.next_layer_number.max(layers.len() as u64 + 1);
    Ok(Document::from_parts(
        description.width,
        description.height,
        Rgba8::new(r, g, b, 255),
        layers,
        description.active_layer,
        next_number,
    ))
}

/// Makes sure the description makes sense before we use it.
fn check_description(description: &Description) -> Result<(), FileError> {
    let damaged = |detail: &str| Err(FileError::Damaged(detail.to_owned()));
    if description.width == 0
        || description.height == 0
        || description.width > MAX_CANVAS_SIDE
        || description.height > MAX_CANVAS_SIDE
    {
        return damaged("the canvas size is impossible");
    }
    if description.pixels != PIXEL_FORMAT {
        return damaged("the pixels are stored in a way this version doesn't know");
    }
    if description.layers.is_empty() {
        return damaged("it has no layers");
    }
    if description
        .layers
        .iter()
        .any(|layer| BlendMode::from_name(&layer.blend).is_none())
    {
        return damaged("a layer uses a blend mode this version doesn't know");
    }
    Ok(())
}

/// "layers/2/5_7.png" -> layer 2, tile (5, 7).
fn parse_tile_name(name: &str) -> Option<(usize, TileCoord)> {
    let rest = name.strip_prefix("layers/")?.strip_suffix(".png")?;
    let (layer, tile) = rest.split_once('/')?;
    let (x, y) = tile.split_once('_')?;
    Some((
        layer.parse().ok()?,
        TileCoord::new(x.parse().ok()?, y.parse().ok()?),
    ))
}

fn decode_tile(bytes: &[u8]) -> Result<Tile, String> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let size = TILE_SIZE as u32;
    let info = reader.info();
    if info.width != size || info.height != size {
        return Err("the tile has the wrong size".into());
    }
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err("the tile has the wrong kind of pixels".into());
    }
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    reader
        .next_frame(&mut buffer)
        .map_err(|error| error.to_string())?;
    let pixels: Vec<Rgba8> = buffer
        .chunks_exact(4)
        .map(|p| Rgba8::new(p[0], p[1], p[2], p[3]))
        .collect();
    Tile::from_pixels(pixels).ok_or_else(|| "the tile has the wrong number of pixels".into())
}

// ---- Exporting PNG ----------------------------------------------------------

/// What goes behind the picture in an exported PNG.
pub use crate::document::Background;

/// Exports all visible layers, combined, as one PNG image the size of the
/// canvas. Like saving, it writes a temporary file first.
pub fn export_png(
    document: &Document,
    path: &Path,
    background: Background,
) -> Result<(), FileError> {
    write_safely(path, |file| write_png(document, background, file))
}

/// Writes the combined picture as a PNG image to any writer.
pub fn write_png(
    document: &Document,
    background: Background,
    writer: impl Write,
) -> Result<(), FileError> {
    Image::of_document(document, background)
        .write_png(writer, background == Background::Transparent)
}

// ---- Shared -----------------------------------------------------------------

/// Writes a file via a temporary file next to it, then swaps it in.
pub(crate) fn write_safely(
    path: &Path,
    write: impl FnOnce(BufWriter<File>) -> Result<(), FileError>,
) -> Result<(), FileError> {
    let temporary = temporary_path(path);
    let result = File::create(&temporary)
        .map_err(FileError::from)
        .and_then(|file| write(BufWriter::new(file)))
        .and_then(|()| fs::rename(&temporary, path).map_err(FileError::from));
    if result.is_err() {
        let _ = fs::remove_file(&temporary); // tidy up; the old file is untouched
    }
    result
}

/// "picture.artapp" -> "picture.artapp.saving"
fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".saving");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer_commands::LayerPresenceCommand;

    /// A picture with three layers, different settings and some paint,
    /// including half-transparent pixels.
    fn sample_document() -> Document {
        let mut doc = Document::new(700, 500);
        doc.set_paper(Rgba8::new(250, 240, 220, 255));
        doc.active_layer_mut()
            .surface
            .set_pixel(10, 10, Rgba8::new(255, 0, 0, 255));
        doc.active_layer_mut()
            .surface
            .set_pixel(600, 400, Rgba8::new(0, 64, 0, 128));

        LayerPresenceCommand::add_layer(&mut doc);
        doc.active_layer_mut()
            .surface
            .set_pixel(300, 300, Rgba8::new(0, 0, 200, 200));
        doc.active_layer_mut().set_opacity(0.4);

        LayerPresenceCommand::add_layer(&mut doc);
        doc.active_layer_mut().set_visible(false);
        let renamed = LayerProps {
            name: "Sketch ✏".into(),
            blend: BlendMode::Multiply,
            ..doc.active_layer().props()
        };
        doc.active_layer_mut().set_props(renamed);
        // Make the middle layer the active one.
        let middle = doc.layers()[1].id();
        doc.set_active(middle);
        doc
    }

    fn save_to_memory(document: &Document) -> Vec<u8> {
        write_document(document, Cursor::new(Vec::new()))
            .unwrap()
            .into_inner()
    }

    #[test]
    fn saving_and_opening_gives_back_the_same_picture() {
        let original = sample_document();
        let opened = read_document(Cursor::new(save_to_memory(&original))).expect("opens");

        assert_eq!((opened.width(), opened.height()), (700, 500));
        assert_eq!(opened.paper(), original.paper());
        assert_eq!(opened.layers().len(), 3);
        assert_eq!(opened.layer_index(opened.active_id()), Some(1));
        for (before, after) in original.layers().iter().zip(opened.layers()) {
            assert_eq!(before.props(), after.props());
            assert_eq!(before.surface.tile_count(), after.surface.tile_count());
            for (coord, tile) in before.surface.tiles() {
                assert!(
                    after.surface.tile(coord) == Some(tile),
                    "tile {coord:?} differs"
                );
            }
        }
        // New layers keep counting up instead of repeating names.
        let mut opened = opened;
        assert_eq!(opened.new_layer().name(), "Layer 4");
    }

    #[test]
    fn saving_and_opening_a_real_file() {
        let path = std::env::temp_dir().join(format!("art-app-test-{}.artapp", std::process::id()));
        save(&sample_document(), &path).expect("saves");
        assert!(
            !temporary_path(&path).exists(),
            "the temporary file is gone"
        );
        let opened = open(&path).expect("opens");
        assert_eq!(opened.layers().len(), 3);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn other_files_are_rejected_politely() {
        // Not a zip file at all.
        let error = read_document(Cursor::new(b"hello, this is text".to_vec()))
            .err()
            .unwrap();
        assert!(matches!(error, FileError::NotArtApp));

        // A zip file without our description.
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("readme.txt", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"hi").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(matches!(
            read_document(Cursor::new(bytes)),
            Err(FileError::NotArtApp)
        ));
    }

    /// An archive with the given description text and nothing else.
    fn archive_with_description(text: &str) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(DESCRIPTION_FILE, SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn files_from_a_newer_version_say_so() {
        let bytes = archive_with_description(
            r#"{ "format": "artapp", "version": 99, "something": "new" }"#,
        );
        assert!(matches!(
            read_document(Cursor::new(bytes)),
            Err(FileError::TooNew(99))
        ));
    }

    #[test]
    fn damaged_files_give_a_message_instead_of_crashing() {
        // A description with an impossible canvas size.
        let bytes = archive_with_description(
            r#"{ "format": "artapp", "version": 1, "width": 0, "height": 10, "paper": [255,255,255],
                 "pixels": "rgba8-premultiplied", "active_layer": 0, "next_layer_number": 2,
                 "layers": [ { "name": "Layer 1", "visible": true, "opacity": 1.0, "blend": "normal" } ] }"#,
        );
        assert!(matches!(
            read_document(Cursor::new(bytes)),
            Err(FileError::Damaged(_))
        ));

        // A good file whose tile image has been scribbled over.
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let doc = sample_document();
        let original = read_document(Cursor::new(save_to_memory(&doc))).unwrap();
        let mut description_text = String::new();
        ZipArchive::new(Cursor::new(save_to_memory(&original)))
            .unwrap()
            .by_name(DESCRIPTION_FILE)
            .unwrap()
            .read_to_string(&mut description_text)
            .unwrap();
        zip.start_file(DESCRIPTION_FILE, SimpleFileOptions::default())
            .unwrap();
        zip.write_all(description_text.as_bytes()).unwrap();
        zip.start_file("layers/0/0_0.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"not really a png").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let error = read_document(Cursor::new(bytes)).err().unwrap();
        assert!(matches!(error, FileError::Damaged(_)), "{error}");
        assert!(error.to_string().contains("layers/0/0_0.png"));
    }

    /// Decodes a PNG into (width, height, channels, pixels).
    fn decode_png(bytes: &[u8]) -> (u32, u32, usize, Vec<u8>) {
        let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        let channels = info.color_type.samples();
        (info.width, info.height, channels, buffer)
    }

    #[test]
    fn exported_png_shows_the_visible_picture_on_paper() {
        let doc = sample_document();
        let mut bytes = Vec::new();
        write_png(&doc, Background::Paper, &mut bytes).unwrap();
        let (width, height, channels, pixels) = decode_png(&bytes);
        assert_eq!((width, height, channels), (700, 500, 3));
        let at = |x: usize, y: usize| {
            let i = (y * 700 + x) * 3;
            [pixels[i], pixels[i + 1], pixels[i + 2]]
        };
        assert_eq!(at(10, 10), [255, 0, 0]); // red paint
        assert_eq!(at(50, 50), [250, 240, 220]); // the paper colour
        // Every pixel matches what the eyedropper sees.
        assert_eq!(at(300, 300), doc.sample_color(300.0, 300.0).unwrap());
        assert_eq!(at(600, 400), doc.sample_color(600.0, 400.0).unwrap());
    }

    #[test]
    fn exported_png_can_have_a_transparent_background() {
        let mut doc = Document::new(300, 200);
        doc.active_layer_mut()
            .surface
            .set_pixel(5, 5, Rgba8::new(0, 64, 0, 128));
        let mut bytes = Vec::new();
        write_png(&doc, Background::Transparent, &mut bytes).unwrap();
        let (width, height, channels, pixels) = decode_png(&bytes);
        assert_eq!((width, height, channels), (300, 200, 4));
        let at = |x: usize, y: usize| {
            let i = (y * 300 + x) * 4;
            [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
        };
        assert_eq!(at(0, 0), [0, 0, 0, 0]); // nothing painted: transparent
        // Half-transparent green, stored the normal (straight) way: 64 * 255 / 128 = 128.
        assert_eq!(at(5, 5), [0, 128, 0, 128]);
    }

    #[test]
    fn tile_file_names() {
        assert_eq!(
            parse_tile_name("layers/2/5_7.png"),
            Some((2, TileCoord::new(5, 7)))
        );
        assert_eq!(parse_tile_name("layers/x/5_7.png"), None);
        assert_eq!(parse_tile_name("document.json"), None);
        assert_eq!(parse_tile_name("layers/0/5-7.png"), None);
    }
}
