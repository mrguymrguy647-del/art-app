//! art-cli: Art App on the command line.
//!
//! Renders painting scripts to PNG and .artapp files, checks scripts,
//! and describes .artapp files. It only uses the engine, so it runs
//! anywhere Rust does, with no window, screen or graphics card: for
//! example on a server, where an AI assistant can paint with it.
//!
//! Run `art-cli help` for how to use it.

mod font;
mod sheet;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use art_engine::document::{Background, Document};
use art_engine::file::{self, FILE_EXTENSION};
use art_engine::image::{Image, unpremultiply};
use art_engine::script::{Event, PlayOptions, Player, Script};
use art_engine::surface::PixelRect;

use sheet::{Entry, THUMB_HEIGHT, THUMB_WIDTH, contact_sheet};

const HELP: &str = "\
art-cli: render Art App painting scripts, without a window.

USAGE
  art-cli render <script.json> [options]   paint the script, save the picture
  art-cli check <script.json>              look for mistakes without painting
  art-cli info <picture.artapp>            describe a saved picture
  art-cli help                             show this

RENDER OPTIONS
  --out <folder>     where to save (default: out)
  --snapshots        also save a picture at every `snapshot` command and
                     whenever the script moves on from a layer it painted
  --sheet            save one image with all those pictures side by side
  --preview          paint smaller, for a quick look (longest side 800 px)
  --scale <s>        paint at this size, e.g. 0.5 for half (instead of --preview)
  --crop x,y,w,h     save only this part, at full size, to check details
  --zoom <n>         enlarge the saved picture n times (pixels become squares)
  --transparent      leave the paper out of the PNG (see-through background)

EXAMPLES
  art-cli render examples/sunset.json --preview --sheet
  art-cli render examples/sunset.json --out render --snapshots
  art-cli render examples/sunset.json --crop 700,450,200,150 --zoom 3

The script format is described in docs/script-format.md.";

/// The longest side of a `--preview` picture.
const PREVIEW_SIZE: f32 = 800.0;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("render") => parse_render(&args[1..]).and_then(|options| render(&options)),
        Some("check") => match &args[1..] {
            [script] => check(Path::new(script)),
            _ => Err(Problem::Usage("check needs one script file".into())),
        },
        Some("info") => match &args[1..] {
            [picture] => info(Path::new(picture)),
            _ => Err(Problem::Usage("info needs one .artapp file".into())),
        },
        None | Some("help" | "--help" | "-h") => {
            println!("{HELP}");
            Ok(())
        }
        Some(other) => Err(Problem::Usage(format!("unknown command \"{other}\""))),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Problem::Usage(message)) => {
            eprintln!("error: {message}\n\nRun `art-cli help` to see how to use it.");
            ExitCode::from(2)
        }
        Err(Problem::Failed(message)) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

/// Why art-cli stopped.
enum Problem {
    /// The command line was wrong (exit code 2).
    Usage(String),
    /// The script, a file, or saving went wrong (exit code 1).
    Failed(String),
}

fn failed(message: impl Into<String>) -> Problem {
    Problem::Failed(message.into())
}

// ---- render ---------------------------------------------------------------------

struct RenderOptions {
    script: PathBuf,
    out: PathBuf,
    snapshots: bool,
    sheet: bool,
    preview: bool,
    scale: Option<f32>,
    crop: Option<[u32; 4]>,
    zoom: u32,
    transparent: bool,
}

fn parse_render(args: &[String]) -> Result<RenderOptions, Problem> {
    let mut options = RenderOptions {
        script: PathBuf::new(),
        out: PathBuf::from("out"),
        snapshots: false,
        sheet: false,
        preview: false,
        scale: None,
        crop: None,
        zoom: 1,
        transparent: false,
    };
    let mut script = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| Problem::Usage(format!("{name} needs a value")))
        };
        match arg.as_str() {
            "--out" => options.out = PathBuf::from(value("--out")?),
            "--snapshots" => options.snapshots = true,
            "--sheet" => options.sheet = true,
            "--preview" => options.preview = true,
            "--transparent" => options.transparent = true,
            "--scale" => {
                let text = value("--scale")?;
                let scale = text
                    .parse::<f32>()
                    .ok()
                    .filter(|s| *s > 0.0 && *s <= 4.0)
                    .ok_or_else(|| {
                        Problem::Usage(format!(
                            "--scale must be a number above 0 and up to 4, not {text}"
                        ))
                    })?;
                options.scale = Some(scale);
            }
            "--zoom" => {
                let text = value("--zoom")?;
                options.zoom = text
                    .parse::<u32>()
                    .ok()
                    .filter(|z| (1..=16).contains(z))
                    .ok_or_else(|| {
                        Problem::Usage(format!(
                            "--zoom must be a whole number from 1 to 16, not {text}"
                        ))
                    })?;
            }
            "--crop" => {
                let text = value("--crop")?;
                let numbers: Vec<u32> = text
                    .split(',')
                    .filter_map(|n| n.trim().parse().ok())
                    .collect();
                match numbers[..] {
                    [x, y, w, h] if w > 0 && h > 0 => options.crop = Some([x, y, w, h]),
                    _ => {
                        return Err(Problem::Usage(format!(
                            "--crop is x,y,width,height in whole pixels, like 100,200,300,150 (not {text})"
                        )));
                    }
                }
            }
            flag if flag.starts_with("--") => {
                return Err(Problem::Usage(format!("unknown option {flag}")));
            }
            file if script.is_none() => script = Some(PathBuf::from(file)),
            extra => {
                return Err(Problem::Usage(format!(
                    "only one script at a time (also got {extra})"
                )));
            }
        }
    }
    options.script = script.ok_or_else(|| Problem::Usage("render needs a script file".into()))?;
    if options.preview && options.scale.is_some() {
        return Err(Problem::Usage("use --preview or --scale, not both".into()));
    }
    if options.crop.is_some() && (options.preview || options.scale.is_some()) {
        return Err(Problem::Usage(
            "--crop always renders at full size: leave out --preview and --scale".into(),
        ));
    }
    Ok(options)
}

/// Reads and checks a script file.
fn load(path: &Path) -> Result<Script, Problem> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| failed(format!("can't read {}: {e}", path.display())))?;
    Script::parse(&text).map_err(|e| failed(format!("{}: {e}", path.display())))
}

fn render(options: &RenderOptions) -> Result<(), Problem> {
    let started = Instant::now();
    let script = load(&options.script)?;
    let command_count = script.commands.len();
    let stem = options.script.file_stem().map_or_else(
        || "picture".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let folder = options.script.parent().map(Path::to_path_buf);

    // How big to paint.
    let mut player = Player::new(script.clone(), PlayOptions::default())
        .map_err(|e| failed(format!("{}: {e}", options.script.display())))?;
    let scale = match (options.preview, options.scale) {
        (true, _) => {
            let (width, height) = player.canvas_size().ok_or_else(|| {
                failed(
                    "--preview needs a canvas with a width and height (this one opens a picture)",
                )
            })?;
            (PREVIEW_SIZE / width.max(height) as f32).min(1.0)
        }
        (false, Some(scale)) => scale,
        (false, None) => 1.0,
    };
    let play = PlayOptions {
        scale,
        folder,
        ..PlayOptions::default()
    };
    player = Player::new(script, play)
        .map_err(|e| failed(format!("{}: {e}", options.script.display())))?;
    let mut document = player
        .create_document()
        .map_err(|e| failed(format!("{}: {e}", options.script.display())))?;

    let snapshot_folder = options.out.join("snapshots");
    std::fs::create_dir_all(if options.snapshots {
        &snapshot_folder
    } else {
        &options.out
    })
    .map_err(|e| {
        failed(format!(
            "can't make the folder {}: {e}",
            options.out.display()
        ))
    })?;

    // Play, saving a picture at each event if asked.
    let mut saved: Vec<PathBuf> = Vec::new();
    let mut sheet_entries: Vec<Entry> = Vec::new();
    let mut save_error: Option<String> = None;
    let mut event_count = 0;
    let background = background_for(options.transparent);
    player
        .run(&mut document, |event, document| {
            if !options.snapshots && !options.sheet {
                return;
            }
            event_count += 1;
            let label = match event {
                Event::Snapshot(name) => name.clone(),
                Event::LayerDone(layer) => format!("after {layer}"),
            };
            let image = Image::of_document(document, background);
            if options.snapshots {
                let path = snapshot_folder.join(format!("{event_count:02}-{}.png", slug(&label)));
                match image.save_png(&path, options.transparent) {
                    Ok(()) => saved.push(path),
                    Err(e) => {
                        save_error = save_error
                            .take()
                            .or(Some(format!("{}: {e}", path.display())))
                    }
                }
            }
            if options.sheet {
                sheet_entries.push(Entry {
                    label,
                    image: image.fit_within(THUMB_WIDTH, THUMB_HEIGHT),
                });
            }
        })
        .map_err(|e| failed(format!("{}: {e}", options.script.display())))?;
    if let Some(error) = save_error {
        return Err(failed(format!("couldn't save a snapshot: {error}")));
    }

    // The finished picture.
    let save = |image: &Image, path: PathBuf, saved: &mut Vec<PathBuf>| -> Result<(), Problem> {
        image
            .save_png(&path, options.transparent)
            .map_err(|e| failed(format!("couldn't save {}: {e}", path.display())))?;
        saved.push(path);
        Ok(())
    };
    let zoomed = |image: Image| {
        if options.zoom > 1 {
            image.magnify(options.zoom)
        } else {
            image
        }
    };
    if let Some([x, y, w, h]) = options.crop {
        if x >= document.width() || y >= document.height() {
            return Err(failed(format!(
                "--crop starts at {x},{y}, outside the {} x {} canvas",
                document.width(),
                document.height()
            )));
        }
        let area = PixelRect::new(
            x as i32,
            y as i32,
            (x + w).min(document.width()) as i32,
            (y + h).min(document.height()) as i32,
        );
        let image = zoomed(Image::from_document(&document, area, background));
        let name = format!(
            "{stem}-crop-{x}-{y}-{}-{}.png",
            area.right - area.left,
            area.bottom - area.top
        );
        save(&image, options.out.join(name), &mut saved)?;
    } else {
        let image = Image::of_document(&document, background);
        if options.sheet {
            let thumbnail = image.fit_within(THUMB_WIDTH, THUMB_HEIGHT);
            // Unless the last snapshot already shows the finished picture.
            if sheet_entries
                .last()
                .is_none_or(|last| last.image != thumbnail)
            {
                sheet_entries.push(Entry {
                    label: "final".into(),
                    image: thumbnail,
                });
            }
        }
        let name = if scale == 1.0 {
            format!("{stem}.png")
        } else {
            format!("{stem}-preview.png")
        };
        save(&zoomed(image), options.out.join(name), &mut saved)?;
        if scale == 1.0 {
            let path = options.out.join(format!("{stem}.{FILE_EXTENSION}"));
            file::save(&document, &path)
                .map_err(|e| failed(format!("couldn't save {}: {e}", path.display())))?;
            saved.push(path);
        }
    }
    if options.sheet {
        let name = options
            .script
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let title = format!("{name} - {} x {}", document.width(), document.height());
        let sheet = contact_sheet(&title, &sheet_entries);
        save(
            &sheet,
            options.out.join(format!("{stem}-sheet.png")),
            &mut saved,
        )?;
    }

    println!(
        "Rendered {} in {:.2} s{}",
        options.script.display(),
        started.elapsed().as_secs_f32(),
        if scale == 1.0 {
            String::new()
        } else {
            format!(" (at {:.0}% size)", scale * 100.0)
        }
    );
    println!(
        "  canvas {} x {}, {command_count} commands, layers (bottom to top): {}",
        document.width(),
        document.height(),
        layer_names(&document)
    );
    for path in saved {
        println!("  {}", path.display());
    }
    Ok(())
}

fn background_for(transparent: bool) -> Background {
    if transparent {
        Background::Transparent
    } else {
        Background::Paper
    }
}

/// "After Sky (2)" -> "after-sky-2", for file names.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out: String = out.trim_end_matches('-').chars().take(40).collect();
    if out.is_empty() {
        "snapshot".into()
    } else {
        out
    }
}

fn layer_names(document: &Document) -> String {
    document
        .layers()
        .iter()
        .map(|l| l.name())
        .collect::<Vec<_>>()
        .join(", ")
}

// ---- check ----------------------------------------------------------------------

fn check(path: &Path) -> Result<(), Problem> {
    let script = load(path)?;
    let command_count = script.commands.len();
    let options = PlayOptions {
        folder: path.parent().map(Path::to_path_buf),
        dry_run: true,
        ..PlayOptions::default()
    };
    let mut snapshots = Vec::new();
    let mut player =
        Player::new(script, options).map_err(|e| failed(format!("{}: {e}", path.display())))?;
    let mut document = player
        .create_document()
        .map_err(|e| failed(format!("{}: {e}", path.display())))?;
    player
        .run(&mut document, |event, _| {
            if let Event::Snapshot(name) = event {
                snapshots.push(name.clone());
            }
        })
        .map_err(|e| failed(format!("{}: {e}", path.display())))?;
    println!("OK: {}", path.display());
    println!(
        "  canvas {} x {}, {command_count} commands",
        document.width(),
        document.height()
    );
    println!("  layers (bottom to top): {}", layer_names(&document));
    if !snapshots.is_empty() {
        println!("  snapshots: {}", snapshots.join(", "));
    }
    Ok(())
}

// ---- info -----------------------------------------------------------------------

fn info(path: &Path) -> Result<(), Problem> {
    let document = file::open(path).map_err(|e| failed(format!("{}: {e}", path.display())))?;
    let [r, g, b, _] = unpremultiply(document.paper());
    println!("{}", path.display());
    println!(
        "  canvas {} x {}, paper #{r:02X}{g:02X}{b:02X}",
        document.width(),
        document.height()
    );
    println!("  {} layer(s), bottom to top:", document.layers().len());
    let active = document.active_id();
    for (i, layer) in document.layers().iter().enumerate() {
        let painted = match painted_area(layer) {
            Some(area) => format!(
                "painted x {}..{}, y {}..{}",
                area.left, area.right, area.top, area.bottom
            ),
            None => "empty".to_owned(),
        };
        println!(
            "  {:>3}. {}{}  [{}, opacity {:.0}%, {}]  {painted}",
            i + 1,
            layer.name(),
            if layer.id() == active {
                " (selected)"
            } else {
                ""
            },
            if layer.visible() { "visible" } else { "hidden" },
            layer.opacity() * 100.0,
            layer.blend().name(),
        );
    }
    Ok(())
}

/// The smallest rectangle around everything painted on a layer.
fn painted_area(layer: &art_engine::layer::Layer) -> Option<PixelRect> {
    let mut area: Option<PixelRect> = None;
    for (coord, tile) in layer.surface.tiles() {
        let (left, top) = coord.origin();
        for (i, pixel) in tile.pixels().iter().enumerate() {
            if pixel.a == 0 {
                continue;
            }
            let size = art_engine::tile::TILE_SIZE;
            let (x, y) = (left + (i % size) as i32, top + (i / size) as i32);
            area = Some(match area {
                None => PixelRect::new(x, y, x + 1, y + 1),
                Some(a) => PixelRect::new(
                    a.left.min(x),
                    a.top.min(y),
                    a.right.max(x + 1),
                    a.bottom.max(y + 1),
                ),
            });
        }
    }
    area
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn render_options_are_read() {
        let options = parse_render(&args(
            "a.json --out x --snapshots --sheet --crop 1,2,30,40 --zoom 3",
        ))
        .ok()
        .unwrap();
        assert_eq!(options.script, PathBuf::from("a.json"));
        assert_eq!(options.out, PathBuf::from("x"));
        assert!(options.snapshots && options.sheet);
        assert_eq!(options.crop, Some([1, 2, 30, 40]));
        assert_eq!(options.zoom, 3);
    }

    #[test]
    fn bad_render_options_are_explained() {
        let message = |text: &str| match parse_render(&args(text)) {
            Err(Problem::Usage(message)) => message,
            _ => panic!("expected a usage problem for {text}"),
        };
        assert!(message("a.json --crop 1,2").contains("--crop is x,y,width,height"));
        assert!(message("a.json --preview --crop 1,2,3,4").contains("full size"));
        assert!(message("a.json --zoom 0").contains("--zoom"));
        assert!(message("a.json --colour red").contains("unknown option --colour"));
        assert!(message("--sheet").contains("needs a script"));
    }

    #[test]
    fn snapshot_names_become_file_names() {
        assert_eq!(slug("After Sky (2)"), "after-sky-2");
        assert_eq!(slug("blocked in!"), "blocked-in");
        assert_eq!(slug("???"), "snapshot");
    }
}
