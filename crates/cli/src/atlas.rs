//! `quill atlas`: several tiles or sprites (one script each) packed into
//! one PNG with a JSON of where each one is, for a game to load
//! (`docs/sprite-sheet-format.md`, "Tilesets").
//!
//! Tiles of one size go in a grid, so a game can also cut the picture by
//! the tile size; tiles of different sizes go in rows, left to right.

use std::path::{Path, PathBuf};

use quill_engine::document::Background;
use quill_engine::image::Image;
use quill_engine::script::{PlayOptions, Player};
use serde_json::{Map, Value, json};

use crate::{Problem, failed, load};

/// Enlarging more than this makes files nobody wants.
const MAX_SCALE: u32 = 32;

pub struct AtlasOptions {
    scripts: Vec<PathBuf>,
    out: PathBuf,
    /// The PNG and JSON are `<name>.png` and `<name>.json`.
    name: String,
    /// Tiles per row (default: about a square).
    columns: Option<u32>,
    /// See-through pixels between tiles (and round the edge).
    padding: u32,
    scale: u32,
}

pub fn parse_atlas(args: &[String]) -> Result<AtlasOptions, Problem> {
    let mut options = AtlasOptions {
        scripts: Vec::new(),
        out: PathBuf::from("out"),
        name: "atlas".to_owned(),
        columns: None,
        padding: 0,
        scale: 1,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| Problem::Usage(format!("{name} needs a value")))
        };
        let whole = |name: &str, text: String, low: u32, high: u32| {
            text.parse::<u32>()
                .ok()
                .filter(|n| (low..=high).contains(n))
                .ok_or_else(|| {
                    Problem::Usage(format!(
                        "{name} must be a whole number from {low} to {high}"
                    ))
                })
        };
        match arg.as_str() {
            "--out" => options.out = PathBuf::from(value("--out")?),
            "--name" => options.name = value("--name")?,
            "--columns" => options.columns = Some(whole("--columns", value("--columns")?, 1, 256)?),
            "--padding" => options.padding = whole("--padding", value("--padding")?, 0, 64)?,
            "--scale" => options.scale = whole("--scale", value("--scale")?, 1, MAX_SCALE)?,
            flag if flag.starts_with("--") => {
                return Err(Problem::Usage(format!("unknown option {flag}")));
            }
            script => options.scripts.push(PathBuf::from(script)),
        }
    }
    if options.scripts.is_empty() {
        return Err(Problem::Usage(
            "atlas needs the scripts to pack, one per tile or sprite".into(),
        ));
    }
    Ok(options)
}

/// One tile, painted.
struct Tile {
    name: String,
    image: Image,
}

/// Where each tile goes: `(x, y)`, and the whole picture's size.
fn pack(sizes: &[(u32, u32)], columns: Option<u32>, padding: u32) -> (Vec<(u32, u32)>, (u32, u32)) {
    let count = sizes.len() as u32;
    let columns = columns
        .unwrap_or_else(|| (count as f32).sqrt().ceil() as u32)
        .clamp(1, count.max(1));
    let mut places = Vec::with_capacity(sizes.len());
    let (mut x, mut y, mut row_height, mut width) = (padding, padding, 0, 0);
    for (i, &(w, h)) in sizes.iter().enumerate() {
        if (i as u32).is_multiple_of(columns) && i > 0 {
            x = padding;
            y += row_height + padding;
            row_height = 0;
        }
        places.push((x, y));
        x += w + padding;
        width = width.max(x);
        row_height = row_height.max(h);
    }
    (places, (width, y + row_height + padding))
}

pub fn atlas(options: &AtlasOptions) -> Result<(), Problem> {
    // Paint each tile, at full size on a see-through background.
    let mut tiles: Vec<Tile> = Vec::new();
    let mut notes = Vec::new();
    for path in &options.scripts {
        let name = path
            .file_stem()
            .map_or_else(|| "tile".to_owned(), |s| s.to_string_lossy().into_owned());
        if tiles.iter().any(|t| t.name == name) {
            return Err(failed(format!(
                "two tiles are called \"{name}\": tiles are named after their script files, so \
                 give them different names"
            )));
        }
        let script = load(path)?;
        if !script.animations.is_empty() {
            notes.push(format!(
                "{name} has animations: its first frame is used (`quill export` makes a sprite \
                 sheet of all its frames)"
            ));
        }
        let problem = |e: &dyn std::fmt::Display| failed(format!("{}: {e}", path.display()));
        let play = PlayOptions {
            folder: path.parent().map(Path::to_path_buf),
            ..PlayOptions::default()
        };
        let mut player = Player::new(script, play).map_err(|e| problem(&e))?;
        let mut document = player.create_document().map_err(|e| problem(&e))?;
        player
            .run(&mut document, |_, _| {})
            .map_err(|e| problem(&e))?;
        let mut image = Image::of_document(&document, Background::Transparent);
        if options.scale > 1 {
            image = image.magnify(options.scale);
        }
        tiles.push(Tile { name, image });
    }

    // Pack them.
    let sizes: Vec<(u32, u32)> = tiles
        .iter()
        .map(|t| (t.image.width(), t.image.height()))
        .collect();
    let same_size = sizes.windows(2).all(|pair| pair[0] == pair[1]);
    let (places, (width, height)) = pack(&sizes, options.columns, options.padding);
    let mut sheet = Image::new(width, height, [0, 0, 0, 0]);
    for (tile, &(left, top)) in tiles.iter().zip(&places) {
        for y in 0..tile.image.height() {
            for x in 0..tile.image.width() {
                sheet.set_pixel((left + x) as i32, (top + y) as i32, tile.image.pixel(x, y));
            }
        }
    }

    // Save the picture and its JSON.
    std::fs::create_dir_all(&options.out).map_err(|e| {
        failed(format!(
            "can't make the folder {}: {e}",
            options.out.display()
        ))
    })?;
    let png = options.out.join(format!("{}.png", options.name));
    sheet
        .save_png(&png, true)
        .map_err(|e| failed(format!("couldn't save {}: {e}", png.display())))?;
    let mut entries = Map::new();
    for (index, (tile, &(x, y))) in tiles.iter().zip(&places).enumerate() {
        entries.insert(
            tile.name.clone(),
            json!({
                "index": index,
                "x": x,
                "y": y,
                "width": tile.image.width(),
                "height": tile.image.height(),
            }),
        );
    }
    let mut description = json!({
        "image": format!("{}.png", options.name),
        "scale": options.scale,
        "padding": options.padding,
    });
    if same_size && let Some(&(w, h)) = sizes.first() {
        // One size: also a plain grid, cut by the tile size.
        let columns = places.iter().filter(|(_, y)| *y == places[0].1).count();
        description["tile"] = json!({ "width": w, "height": h });
        description["columns"] = json!(columns);
        description["rows"] = json!(tiles.len().div_ceil(columns));
    }
    description["tiles"] = Value::Object(entries);
    let json_path = options.out.join(format!("{}.json", options.name));
    let text = serde_json::to_string_pretty(&description).unwrap_or_default() + "\n";
    std::fs::write(&json_path, text)
        .map_err(|e| failed(format!("couldn't save {}: {e}", json_path.display())))?;

    say!(
        "Packed {} tiles into {} x {} ({}).",
        tiles.len(),
        width,
        height,
        if same_size {
            "one size: a grid"
        } else {
            "different sizes: in rows"
        }
    );
    say!("  saved: {}, {}", png.display(), json_path.display());
    for note in notes {
        say!("  ({note})");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_sized_tiles_make_a_grid_with_padding() {
        let (places, size) = pack(&[(16, 16); 5], None, 1);
        // Five tiles: three across (about a square), two rows.
        assert_eq!(places[0], (1, 1));
        assert_eq!(places[1], (18, 1));
        assert_eq!(places[3], (1, 18));
        assert_eq!(size, (52, 35));
        let (places, size) = pack(&[(8, 8); 4], Some(4), 0);
        assert_eq!(places[3], (24, 0));
        assert_eq!(size, (32, 8));
    }

    #[test]
    fn different_sizes_go_in_rows_as_tall_as_their_tallest() {
        let (places, size) = pack(&[(10, 20), (30, 5), (8, 8)], Some(2), 0);
        assert_eq!(places, vec![(0, 0), (10, 0), (0, 20)]);
        assert_eq!(size, (40, 28));
    }
}
