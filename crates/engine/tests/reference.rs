//! Old scripts must keep painting exactly the same pixels.
//!
//! The pictures in `tests/reference/` were rendered before groups,
//! symbols and text were added. Each test plays a script and compares
//! every pixel with its reference picture.
//!
//! If a change is *meant* to change pictures, make new references with
//! `ARTAPP_UPDATE_REFERENCES=1 cargo test -p art_engine --test reference`
//! and look at them before committing.

use std::path::{Path, PathBuf};

use art_engine::document::Background;
use art_engine::image::Image;
use art_engine::script::{PlayOptions, Player, Script};

/// The repository's top folder.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Plays a script at full size and returns the picture.
fn render(script: &Path, background: Background) -> Image {
    let text = std::fs::read_to_string(script).unwrap();
    let options = PlayOptions {
        folder: script.parent().map(Path::to_path_buf),
        ..PlayOptions::default()
    };
    let mut player = Player::new(Script::parse(&text).unwrap(), options).unwrap();
    let mut document = player.create_document().unwrap();
    player.run(&mut document, |_, _| {}).unwrap();
    Image::of_document(&document, background)
}

/// Reads a PNG as RGBA pixels: (width, height, pixels).
fn read_png(path: &Path) -> (u32, u32, Vec<[u8; 4]>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buffer).unwrap();
    let channels = info.color_type.samples();
    let pixels = buffer[..info.buffer_size()]
        .chunks(channels)
        .map(|p| match p {
            [r, g, b] => [*r, *g, *b, 255],
            [r, g, b, a] => [*r, *g, *b, *a],
            _ => panic!("unexpected PNG layout"),
        })
        .collect();
    (info.width, info.height, pixels)
}

/// Compares a fresh render of `script` with `reference` (or writes the
/// reference when updating).
fn compare(script: &str, reference: &str, background: Background) {
    let script = root().join(script);
    let reference = root().join("crates/engine/tests/reference").join(reference);
    let image = render(&script, background);
    let with_alpha = background == Background::Transparent;
    if std::env::var_os("ARTAPP_UPDATE_REFERENCES").is_some() {
        image.save_png(&reference, with_alpha).unwrap();
        return;
    }

    let (width, height, expected) = read_png(&reference);
    assert_eq!((image.width(), image.height()), (width, height), "{}", script.display());
    let mut different = 0;
    let mut first = None;
    for y in 0..height {
        for x in 0..width {
            let mut actual = image.pixel(x, y);
            if !with_alpha {
                actual[3] = 255;
            }
            if actual != expected[(y * width + x) as usize] {
                different += 1;
                first.get_or_insert((x, y, actual, expected[(y * width + x) as usize]));
            }
        }
    }
    if different > 0 {
        let saved = std::env::temp_dir().join(reference.file_name().unwrap());
        image.save_png(&saved, with_alpha).unwrap();
        let (x, y, actual, wanted) = first.unwrap();
        panic!(
            "{}: {different} pixels differ from {} (first at {x},{y}: {actual:?} instead of {wanted:?}). \
             The new picture is at {}",
            script.display(),
            reference.display(),
            saved.display()
        );
    }
}

#[test]
fn the_sunset_example_keeps_its_pixels() {
    compare("examples/sunset.json", "sunset.png", Background::Paper);
}

#[test]
fn the_night_hills_example_keeps_its_pixels() {
    compare("examples/night-hills.json", "night-hills.png", Background::Paper);
}

#[test]
fn a_script_with_every_command_keeps_its_pixels() {
    compare(
        "crates/engine/tests/scripts/all-commands.json",
        "all-commands.png",
        Background::Paper,
    );
    compare(
        "crates/engine/tests/scripts/all-commands.json",
        "all-commands-transparent.png",
        Background::Transparent,
    );
}
