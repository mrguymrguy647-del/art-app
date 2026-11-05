//! art-cli: Art App on the command line.
//!
//! Renders painting scripts to PNG and .artapp files, checks scripts,
//! and describes .artapp files. It only uses the engine, so it runs
//! anywhere Rust does, with no window, screen or graphics card: for
//! example on a server, where an AI assistant can paint with it.
//!
//! Run `art-cli help` for how to use it.

/// Prints a line, like `println!`, but never crashes: see [`write_out`].
macro_rules! say {
    ($($text:tt)*) => {
        $crate::write_out(format_args!($($text)*))
    };
}

/// Prints a line to the error output, like `eprintln!`, but never crashes.
macro_rules! complain {
    ($($text:tt)*) => {
        $crate::write_err(format_args!($($text)*))
    };
}

mod font;
mod grid;
mod sheet;

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use art_engine::document::{Background, Document};
use art_engine::file::{self, FILE_EXTENSION};
use art_engine::image::{Color, Image, unpremultiply};
use art_engine::layer::Layer;
use art_engine::script::{Event, PlayOptions, Player, Report, Script, parse_hex};
use art_engine::surface::PixelRect;

use grid::{View, draw_grid};
use sheet::{Entry, THUMB_HEIGHT, THUMB_WIDTH, contact_sheet};

const HELP: &str = "\
art-cli: render Art App painting scripts, without a window.

USAGE
  art-cli render <script.json> [options]   paint the script, save the picture
  art-cli check <script.json> [--detail]   paint without saving: where everything
                                           went, and what looks like a mistake
                                           (--detail: every group and use, also
                                           the ones inside others)
  art-cli info <picture.artapp>            describe a saved picture: its layers
  art-cli info <script.json>               the same for the picture a script
                                           paints, with the commands that
                                           painted on each layer
  art-cli help                             show this

RENDER OPTIONS
  --out <folder>     where to save (default: out)
  --snapshots        also save a picture at every `snapshot` command and
                     whenever the script moves on from a layer it painted
  --sheet            save one image with all those pictures side by side
  --preview          paint smaller, for a quick look (longest side 800 px)
  --scale <s>        paint at this size, e.g. 0.5 for half (instead of --preview)
  --crop x,y,w,h     save only this part, at full size, to check details
                     (it may go past the edges: outside shows as grey squares)
  --zoom <n>         enlarge the saved picture n times (pixels become squares)
  --transparent      leave the paper out of the PNG (see-through background)
  --layers \"A,B\"     show only these layers (the .artapp keeps them all)
  --hide \"A,B\"       show all layers but these
  --bg <#RRGGBB>     with --transparent: this colour behind the see-through
                     parts of previews, crops, snapshots and sheets
  --grid <n>         a light grid every n canvas pixels, with coordinates,
                     on previews, crops and snapshots
  --detail           after rendering, list everything `check --detail` does
                     instead of the short summary
  --frame <n>        for a script with animations: paint frame n (from 0)
  --frames <a>-<b>   paint frames a to b; --all-frames paints every frame
  --animation <name> which animation, if the script has more than one
                     (frames are saved as <name>-<animation>-<frame>.png)

  Pictures for looking at (previews, crops, snapshots, sheets) get --bg and
  --grid; <name>.png and <name>.artapp never do. At full size, the picture
  with them is saved as <name>-view.png.

EXAMPLES
  art-cli render examples/sunset.json --preview --sheet
  art-cli render examples/sunset.json --out render --snapshots
  art-cli render examples/sunset.json --crop 700,450,200,150 --zoom 3 --grid 50
  art-cli render examples/sunset.json --preview --layers \"Sky,Sun\"

The script format is described in docs/script-format.md.";

/// The longest side of a `--preview` picture.
const PREVIEW_SIZE: f32 = 800.0;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("render") => parse_render(&args[1..]).and_then(|options| render(&options)),
        Some("check") => match &args[1..] {
            [script] if !script.starts_with("--") => check(Path::new(script), false),
            [script, flag] | [flag, script] if flag == "--detail" && !script.starts_with("--") => {
                check(Path::new(script), true)
            }
            _ => Err(Problem::Usage(
                "check needs one script file (and maybe --detail)".into(),
            )),
        },
        Some("info") => match &args[1..] {
            [picture] => info(Path::new(picture)),
            _ => Err(Problem::Usage(
                "info needs one .artapp file or painting script".into(),
            )),
        },
        None | Some("help" | "--help" | "-h") => {
            say!("{HELP}");
            Ok(())
        }
        Some(other) => Err(Problem::Usage(format!("unknown command \"{other}\""))),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Problem::Usage(message)) => {
            complain!("error: {message}\n\nRun `art-cli help` to see how to use it.");
            ExitCode::from(2)
        }
        Err(Problem::Failed(message)) => {
            complain!("error: {message}");
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

/// Writes one line of normal output.
///
/// When the output goes into another program that stops reading early
/// (for example `art-cli info picture.artapp | head -1`), writing fails
/// with a "broken pipe". That isn't a problem: whatever was asked for is
/// already done (pictures are saved before anything is printed), and
/// nobody is reading any more. So art-cli just stops, quietly and
/// successfully, like other command-line tools do.
fn write_out(text: fmt::Arguments) {
    if let Err(error) = writeln!(io::stdout().lock(), "{text}") {
        if error.kind() == io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        write_err(format_args!("error: can't write the output: {error}"));
        std::process::exit(1);
    }
}

/// Writes one line of error output. If even that can't be written,
/// there's nowhere left to say so: the exit code still tells.
fn write_err(text: fmt::Arguments) {
    let _ = writeln!(io::stderr().lock(), "{text}");
}

// ---- render ---------------------------------------------------------------------

struct RenderOptions {
    script: PathBuf,
    out: PathBuf,
    snapshots: bool,
    sheet: bool,
    preview: bool,
    scale: Option<f32>,
    /// x, y, width, height (x and y may be below 0: off the canvas).
    crop: Option<[i32; 4]>,
    zoom: u32,
    transparent: bool,
    /// `--layers`: show only these.
    layers: Option<Vec<String>>,
    /// `--hide`: don't show these.
    hide: Vec<String>,
    bg: Option<Color>,
    grid: Option<u32>,
    /// `--detail`: the full report instead of the short summary.
    detail: bool,
    /// `--animation`: which animation (default: the first).
    animation: Option<String>,
    /// Which frames of it to paint.
    frames: Frames,
}

/// Which frames of an animation `render` paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frames {
    /// Nothing asked: frame 0 (and a note saying so).
    Default,
    One(u32),
    /// From one frame to another, both included.
    Range(u32, u32),
    All,
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
        layers: None,
        hide: Vec::new(),
        bg: None,
        grid: None,
        detail: false,
        animation: None,
        frames: Frames::Default,
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
            "--detail" => options.detail = true,
            "--all-frames" => options.frames = Frames::All,
            "--animation" => options.animation = Some(value("--animation")?),
            "--frame" => {
                let text = value("--frame")?;
                let frame = text.parse::<u32>().map_err(|_| {
                    Problem::Usage(format!("--frame is a frame number from 0, not {text}"))
                })?;
                options.frames = Frames::One(frame);
            }
            "--frames" => {
                let text = value("--frames")?;
                let range = text
                    .split_once('-')
                    .and_then(|(a, b)| Some((a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?)))
                    .filter(|(a, b)| a <= b)
                    .ok_or_else(|| {
                        Problem::Usage(format!(
                            "--frames is a range like 0-7 (first and last frame, from 0), not {text}"
                        ))
                    })?;
                options.frames = Frames::Range(range.0, range.1);
            }
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
            "--layers" => options.layers = Some(names("--layers", &value("--layers")?)?),
            "--hide" => options.hide = names("--hide", &value("--hide")?)?,
            "--bg" => {
                let text = value("--bg")?;
                options.bg = Some(
                    parse_hex(&text)
                        .ok()
                        .filter(|color| color[3] == 255)
                        .ok_or_else(|| {
                            Problem::Usage(format!(
                                "--bg is a colour like \"#4080A0\" (not see-through), not {text}"
                            ))
                        })?,
                );
            }
            "--grid" => {
                let text = value("--grid")?;
                options.grid = Some(
                    text.parse::<u32>()
                        .ok()
                        .filter(|n| (2..=100_000).contains(n))
                        .ok_or_else(|| {
                            Problem::Usage(format!(
                                "--grid is the distance between lines in canvas pixels, like 100 (not {text})"
                            ))
                        })?,
                );
            }
            "--crop" => {
                let text = value("--crop")?;
                let numbers: Result<Vec<i32>, _> =
                    text.split(',').map(|n| n.trim().parse::<i32>()).collect();
                match numbers.as_deref() {
                    Ok(&[x, y, w, h]) if w > 0 && h > 0 => options.crop = Some([x, y, w, h]),
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
    if options.layers.is_some() && !options.hide.is_empty() {
        return Err(Problem::Usage("use --layers or --hide, not both".into()));
    }
    if options.frames != Frames::Default
        && !matches!(options.frames, Frames::One(_))
        && (options.snapshots || options.sheet)
    {
        return Err(Problem::Usage(
            "--snapshots and --sheet show the stages of one picture: use them with --frame N"
                .into(),
        ));
    }
    if options.bg.is_some() && !options.transparent {
        return Err(Problem::Usage(
            "--bg is the colour behind see-through parts, so it goes with --transparent \
             (to change the paper, change the script's `paper`)"
                .into(),
        ));
    }
    Ok(options)
}

/// Layer names from "Sky, Duck" (commas between them).
fn names(option: &str, text: &str) -> Result<Vec<String>, Problem> {
    let names: Vec<String> = text
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    if names.is_empty() {
        return Err(Problem::Usage(format!(
            "{option} needs layer names with commas between them, like \"Sky,Duck\""
        )));
    }
    Ok(names)
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
    let player = Player::new(script.clone(), PlayOptions::default())
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
    // Which frames to paint: one picture, or frames of an animation.
    let (frames, mut notes) = frames_to_paint(&script, options)?;

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

    // Which layers the pictures show.
    let shown = |layer: &Layer| match &options.layers {
        Some(names) => names.iter().any(|name| name == layer.name()),
        None => layer.visible() && !options.hide.iter().any(|name| name == layer.name()),
    };
    let background = background_for(options.transparent);
    let picture = |document: &Document, area: PixelRect| {
        Image::from_document_showing(document, area, background, shown)
    };
    // A picture for looking at: on the --bg colour, with the --grid.
    let for_looking = |image: Image, view: View| {
        let mut image = match options.bg {
            Some(color) => image.on_color(color),
            None => image,
        };
        if let Some(spacing) = options.grid {
            draw_grid(&mut image, spacing, view);
        }
        image
    };
    let keeps_alpha = options.transparent && options.bg.is_none();
    let whole = View {
        left: 0.0,
        top: 0.0,
        zoom: scale,
    };

    let save = |image: &Image,
                path: PathBuf,
                with_alpha: bool,
                saved: &mut Vec<PathBuf>|
     -> Result<(), Problem> {
        image
            .save_png(&path, with_alpha)
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
    let zoom = options.zoom as f32;
    let mut saved: Vec<PathBuf> = Vec::new();
    let mut snapshot_count = 0;
    let mut last = None;
    for frame in &frames {
        let play = PlayOptions {
            scale,
            folder: folder.clone(),
            animation: frame.as_ref().map(|(name, _)| name.clone()),
            frame: frame.as_ref().map_or(0, |(_, n)| *n),
            ..PlayOptions::default()
        };
        let mut player = Player::new(script.clone(), play)
            .map_err(|e| failed(format!("{}: {e}", options.script.display())))?;
        let mut document = player
            .create_document()
            .map_err(|e| failed(format!("{}: {e}", options.script.display())))?;
        // One picture is <stem>.png; a frame is <stem>-<animation>-<frame>.png.
        let name = match frame {
            Some((animation, n)) => format!("{stem}-{animation}-{n:03}"),
            None => stem.clone(),
        };

        // Play, saving a picture at each event if asked.
        let mut sheet_entries: Vec<Entry> = Vec::new();
        let mut save_error: Option<String> = None;
        let mut event_count = 0;
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
                let image = picture(document, document.bounds());
                if options.snapshots {
                    let path =
                        snapshot_folder.join(format!("{event_count:02}-{}.png", slug(&label)));
                    match for_looking(image.clone(), whole).save_png(&path, keeps_alpha) {
                        Ok(()) => snapshot_count += 1,
                        Err(e) => {
                            save_error = save_error
                                .take()
                                .or(Some(format!("{}: {e}", path.display())))
                        }
                    }
                }
                if options.sheet {
                    let image = match options.bg {
                        Some(color) => image.on_color(color),
                        None => image,
                    };
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
        check_layer_names(options, &document)?;

        // The finished picture.
        if let Some([x, y, w, h]) = options.crop {
            // The part asked for, even where it goes past the canvas's edges:
            // what's outside is shown as grey squares.
            let area = PixelRect::new(x, y, x.saturating_add(w), y.saturating_add(h));
            let canvas = document.bounds();
            if area.intersect(&canvas).is_empty() {
                return Err(failed(format!(
                    "--crop {x},{y},{w},{h} is entirely outside the {} x {} canvas",
                    document.width(),
                    document.height()
                )));
            }
            let mut image = zoomed(picture(&document, area));
            if area.intersect(&canvas) != area {
                outside_canvas(&mut image, area, canvas, options.zoom);
                let note = "part of the crop is outside the canvas: it's shown as grey squares";
                if !notes.contains(&note) {
                    notes.push(note);
                }
            }
            let view = View {
                left: x as f32,
                top: y as f32,
                zoom,
            };
            let image = for_looking(image, view);
            let file = format!("{name}-crop-{x}-{y}-{w}-{h}.png");
            save(&image, options.out.join(file), keeps_alpha, &mut saved)?;
        } else {
            let image = picture(&document, document.bounds());
            if options.sheet {
                let image = match options.bg {
                    Some(color) => image.on_color(color),
                    None => image.clone(),
                };
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
            let view = View {
                zoom: scale * zoom,
                ..whole
            };
            if scale == 1.0 {
                let image = zoomed(image);
                save(
                    &image,
                    options.out.join(format!("{name}.png")),
                    options.transparent,
                    &mut saved,
                )?;
                // The layers, for the app (one picture only: frames are flat).
                if frame.is_none() {
                    let path = options.out.join(format!("{name}.{FILE_EXTENSION}"));
                    file::save(&document, &path)
                        .map_err(|e| failed(format!("couldn't save {}: {e}", path.display())))?;
                    saved.push(path);
                }
                if options.bg.is_some() || options.grid.is_some() {
                    let path = options.out.join(format!("{name}-view.png"));
                    save(&for_looking(image, view), path, keeps_alpha, &mut saved)?;
                }
            } else {
                let image = for_looking(zoomed(image), view);
                let path = options.out.join(format!("{name}-preview.png"));
                save(&image, path, keeps_alpha, &mut saved)?;
            }
        }
        if options.sheet {
            let file = options
                .script
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let title = format!("{file} - {} x {}", document.width(), document.height());
            let sheet = contact_sheet(&title, &sheet_entries);
            save(
                &sheet,
                options.out.join(format!("{name}-sheet.png")),
                false,
                &mut saved,
            )?;
        }
        last = Some((player, document));
    }
    let (player, document) = last.expect("at least one frame is painted");

    // A short summary (or everything): what's where, and what looks wrong.
    let canvas = canvas_size(&document, scale);
    say!(
        "Rendered {} in {:.2} s{}",
        options.script.display(),
        started.elapsed().as_secs_f32(),
        if scale == 1.0 {
            String::new()
        } else {
            format!(" (at {:.0}% size)", scale * 100.0)
        }
    );
    say!(
        "  canvas {} x {}, {command_count} commands",
        canvas.0,
        canvas.1
    );
    if let Some((animation, _)) = &frames[0] {
        let numbers: Vec<u32> = frames
            .iter()
            .filter_map(|f| f.as_ref().map(|(_, n)| *n))
            .collect();
        let total = script.animations[animation].frames;
        say!(
            "  animation \"{animation}\": {} of {total} frames painted ({}); the summary below is frame {}",
            numbers.len(),
            frame_list(&numbers),
            numbers[numbers.len() - 1]
        );
    }
    let report = player.report();
    let layers = layer_boxes(&document, scale, shown, Some(&report));
    let warnings = warnings(&report, &layers);
    if options.detail {
        print_findings(&layers, &report, &warnings, true);
    } else {
        say!("  layers, bottom to top:");
        let entries: Vec<String> = layers
            .iter()
            .map(|layer| format!("{}: {}", layer.title(), describe_box(layer.area)))
            .collect();
        for line in packed(&entries, SUMMARY_WIDTH, SUMMARY_LAYER_LINES) {
            say!("    {line}");
        }
        if !warnings.is_empty() {
            say!("  warnings ({}):", warnings.len());
            for warning in warnings.iter().take(SUMMARY_WARNINGS) {
                say!("    {warning}");
            }
            if warnings.len() > SUMMARY_WARNINGS {
                say!(
                    "    ...and {} more: `art-cli check` lists them all",
                    warnings.len() - SUMMARY_WARNINGS
                );
            }
        }
    }
    let mut files: Vec<String> = if saved.len() > 4 {
        let folder = options.out.display();
        vec![format!("{} files in {folder}", saved.len())]
    } else {
        saved.iter().map(|p| p.display().to_string()).collect()
    };
    if snapshot_count > 0 {
        files.push(format!(
            "{snapshot_count} snapshots in {}",
            snapshot_folder.display()
        ));
    }
    say!("  saved: {}", files.join(", "));
    for note in notes {
        say!("  ({note})");
    }
    Ok(())
}

/// One picture to paint: `None` for a single picture, otherwise which
/// animation and which frame.
type FrameToPaint = Option<(String, u32)>;

/// The frames `render` paints, and notes for the summary.
fn frames_to_paint(
    script: &Script,
    options: &RenderOptions,
) -> Result<(Vec<FrameToPaint>, Vec<&'static str>), Problem> {
    if script.animations.is_empty() {
        if options.frames != Frames::Default || options.animation.is_some() {
            return Err(failed(
                "this script has no animations (--frame, --frames, --all-frames and --animation \
                 are for scripts with an \"animations\" section)",
            ));
        }
        return Ok((vec![None], Vec::new()));
    }
    let name = match &options.animation {
        Some(name) => name.clone(),
        None => script.animations.keys().next().cloned().expect("not empty"),
    };
    let Some(animation) = script.animations.get(&name) else {
        let names: Vec<&str> = script.animations.keys().map(String::as_str).collect();
        return Err(failed(format!(
            "there's no animation called \"{name}\" (animations: {})",
            names.join(", ")
        )));
    };
    let last = animation.frames - 1;
    let too_far = |n: u32| {
        failed(format!(
            "animation \"{name}\" has {} frames (0 to {last}), so there's no frame {n}",
            animation.frames
        ))
    };
    let mut notes = Vec::new();
    let numbers: Vec<u32> = match options.frames {
        Frames::Default => {
            notes.push(
                "this script has animations: frame 0 was painted (--frame N, --frames A-B or \
                 --all-frames choose frames; --animation chooses which animation)",
            );
            vec![0]
        }
        Frames::One(n) if n > last => return Err(too_far(n)),
        Frames::One(n) => vec![n],
        Frames::Range(_, b) if b > last => return Err(too_far(b)),
        Frames::Range(a, b) => (a..=b).collect(),
        Frames::All => (0..=last).collect(),
    };
    Ok((
        numbers
            .into_iter()
            .map(|n| Some((name.clone(), n)))
            .collect(),
        notes,
    ))
}

/// "0-7" or "3" or "0, 2, 4" for the summary.
fn frame_list(numbers: &[u32]) -> String {
    match numbers {
        [] => String::new(),
        [one] => one.to_string(),
        [first, .., last] if numbers.len() as u32 == last - first + 1 => format!("{first}-{last}"),
        _ => numbers
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Grey squares on the part of a crop that's off the canvas. `area` is
/// the crop, in canvas pixels; `image` is it enlarged `zoom` times.
fn outside_canvas(image: &mut Image, area: PixelRect, canvas: PixelRect, zoom: u32) {
    const SQUARE: u32 = 8;
    for py in 0..image.height() {
        for px in 0..image.width() {
            let x = area.left + (px / zoom) as i32;
            let y = area.top + (py / zoom) as i32;
            let outside =
                x < canvas.left || x >= canvas.right || y < canvas.top || y >= canvas.bottom;
            if outside {
                let light = (px / SQUARE + py / SQUARE).is_multiple_of(2);
                let grey = if light { 200 } else { 160 };
                image.set_pixel(px as i32, py as i32, [grey, grey, grey, 255]);
            }
        }
    }
}

/// The summary after `render` shows this many warnings at most...
const SUMMARY_WARNINGS: usize = 3;
/// ...and its layers on this many lines, about this wide.
const SUMMARY_LAYER_LINES: usize = 4;
const SUMMARY_WIDTH: usize = 100;

/// `--layers` and `--hide` must name layers that exist.
fn check_layer_names(options: &RenderOptions, document: &Document) -> Result<(), Problem> {
    let asked = options.layers.iter().flatten().chain(&options.hide);
    for name in asked {
        if !document.layers().iter().any(|layer| layer.name() == name) {
            return Err(failed(format!(
                "there's no layer called \"{name}\" (layers: {})",
                layer_names(document)
            )));
        }
    }
    Ok(())
}

/// The canvas size in canvas pixels (the document is smaller in previews).
fn canvas_size(document: &Document, scale: f32) -> (u32, u32) {
    (
        (document.width() as f32 / scale).round() as u32,
        (document.height() as f32 / scale).round() as u32,
    )
}

/// One layer, for summaries: its name, whether it's shown, the box
/// around what's painted on it (in canvas pixels), and the script's
/// commands that painted on it.
struct LayerBox {
    name: String,
    shown: bool,
    area: Option<PixelRect>,
    commands: Vec<usize>,
}

impl LayerBox {
    fn title(&self) -> String {
        if self.shown {
            self.name.clone()
        } else {
            format!("{} (hidden)", self.name)
        }
    }
}

fn layer_boxes(
    document: &Document,
    scale: f32,
    shown: impl Fn(&Layer) -> bool,
    report: Option<&Report>,
) -> Vec<LayerBox> {
    let (width, height) = canvas_size(document, scale);
    document
        .layers()
        .iter()
        .map(|layer| LayerBox {
            name: layer.name().to_owned(),
            shown: shown(layer),
            commands: report
                .and_then(|report| report.painted_by.get(&layer.id()))
                .cloned()
                .unwrap_or_default(),
            area: layer.surface.painted_area().map(|area| {
                // In canvas pixels, and on the canvas.
                PixelRect::new(
                    ((area.left as f32 / scale).floor() as i32).max(0),
                    ((area.top as f32 / scale).floor() as i32).max(0),
                    ((area.right as f32 / scale).ceil() as i32).min(width as i32),
                    ((area.bottom as f32 / scale).ceil() as i32).min(height as i32),
                )
            }),
        })
        .collect()
}

/// "commands 3-5, 12" or "command 4" (runs of numbers shortened).
fn command_list(numbers: &[usize]) -> String {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &n in numbers {
        match runs.last_mut() {
            Some((_, end)) if *end + 1 == n => *end = n,
            _ => runs.push((n, n)),
        }
    }
    let parts: Vec<String> = runs
        .iter()
        .map(|&(start, end)| match end - start {
            0 => start.to_string(),
            1 => format!("{start}, {end}"),
            _ => format!("{start}-{end}"),
        })
        .collect();
    let word = if numbers.len() == 1 {
        "command"
    } else {
        "commands"
    };
    format!("{word} {}", parts.join(", "))
}

/// "x 10..118, y 20..120", or "empty".
fn describe_box(area: Option<PixelRect>) -> String {
    match area {
        Some(a) => format!("x {}..{}, y {}..{}", a.left, a.right, a.top, a.bottom),
        None => "empty".to_owned(),
    }
}

/// The player's warnings, and empty layers.
fn warnings(report: &Report, layers: &[LayerBox]) -> Vec<String> {
    let mut warnings = report.warnings.clone();
    for layer in layers.iter().filter(|layer| layer.area.is_none()) {
        warnings.push(format!("layer \"{}\" is empty", layer.name));
    }
    warnings
}

/// `entries` joined with " | " into lines about `width` long; at most
/// `max_lines` of them, the last saying how many more there are.
fn packed(entries: &[String], width: usize, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let fits = lines
            .last()
            .is_some_and(|line| line.len() + 3 + entry.len() <= width);
        if fits {
            let line = lines.last_mut().expect("checked above");
            line.push_str(" | ");
            line.push_str(entry);
        } else if lines.len() < max_lines {
            lines.push(entry.clone());
        } else {
            let more = entries.len() - i;
            let last = lines.last_mut().expect("max_lines is above 0");
            last.push_str(&format!(" | ...and {more} more"));
            break;
        }
    }
    lines
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

/// Plays a script at full size, painting but saving nothing. Returns
/// the picture, the player's report and the snapshots' names. For a
/// script with animations: every frame of every animation is played (so
/// a mistake anywhere is found), and the picture and report are frame 0
/// of the first animation.
fn paint(path: &Path) -> Result<(Document, Report, Vec<String>), Problem> {
    let script = load(path)?;
    let folder = path.parent().map(Path::to_path_buf);
    for (name, animation) in &script.animations {
        for frame in 1..animation.frames {
            let options = PlayOptions {
                folder: folder.clone(),
                animation: Some(name.clone()),
                frame,
                ..PlayOptions::default()
            };
            paint_frame(path, &script, options)?;
        }
    }
    for name in script.animations.keys().skip(1) {
        let options = PlayOptions {
            folder: folder.clone(),
            animation: Some(name.clone()),
            ..PlayOptions::default()
        };
        paint_frame(path, &script, options)?;
    }
    let options = PlayOptions {
        folder,
        ..PlayOptions::default()
    };
    paint_frame(path, &script, options)
}

/// One picture (or frame) of `script`, played at full size.
fn paint_frame(
    path: &Path,
    script: &Script,
    options: PlayOptions,
) -> Result<(Document, Report, Vec<String>), Problem> {
    let mut snapshots = Vec::new();
    let mut player = Player::new(script.clone(), options)
        .map_err(|e| failed(format!("{}: {e}", path.display())))?;
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
    Ok((document, player.report(), snapshots))
}

fn check(path: &Path, detail: bool) -> Result<(), Problem> {
    let started = Instant::now();
    let command_count = load(path)?.commands.len();
    // It really paints (at full size), so what it reports is exactly
    // what `render` would make.
    let script = load(path)?;
    let (document, report, snapshots) = paint(path)?;
    let layers = layer_boxes(&document, 1.0, Layer::visible, Some(&report));
    let warnings = warnings(&report, &layers);

    say!(
        "OK: {} (painted in {:.2} s{})",
        path.display(),
        started.elapsed().as_secs_f32(),
        match warnings.len() {
            0 => String::new(),
            1 => ", 1 warning".to_owned(),
            n => format!(", {n} warnings"),
        }
    );
    say!(
        "  canvas {} x {}, {command_count} commands",
        document.width(),
        document.height()
    );
    if !snapshots.is_empty() {
        say!("  snapshots: {}", snapshots.join(", "));
    }
    if !script.animations.is_empty() {
        let list: Vec<String> = script
            .animations
            .iter()
            .map(|(name, a)| {
                format!(
                    "{name} ({} frames, {} fps{})",
                    a.frames,
                    a.fps,
                    if a.looping { ", loops" } else { "" }
                )
            })
            .collect();
        say!(
            "  animations (every frame painted without mistakes): {}",
            list.join("; ")
        );
        let first = script.animations.keys().next().expect("not empty");
        say!("  (the layers and parts below are frame 0 of \"{first}\")");
    }
    print_findings(&layers, &report, &warnings, detail);
    Ok(())
}

/// Without `--detail`, at most this many groups and uses, and warnings.
const CHECK_PARTS: usize = 60;
const CHECK_WARNINGS: usize = 40;

/// Every layer with its box; the named groups and uses (only the
/// outermost ones, unless `detail`); and the warnings.
fn print_findings(layers: &[LayerBox], report: &Report, warnings: &[String], detail: bool) {
    say!("  layers, bottom to top:");
    for layer in layers {
        let by = if layer.commands.is_empty() {
            String::new()
        } else {
            format!(" ({})", command_list(&layer.commands))
        };
        say!("    {}: {}{by}", layer.title(), describe_box(layer.area));
    }
    let parts = &report.parts;
    if !parts.is_empty() {
        // How deep each one is, and how many others are inside it.
        let mut depth = vec![0; parts.len()];
        let mut inside = vec![0; parts.len()];
        for (i, part) in parts.iter().enumerate() {
            let mut around = part.inside;
            while let Some(outer) = around {
                depth[i] += 1;
                inside[outer] += 1;
                around = parts[outer].inside;
            }
        }
        let listed: Vec<usize> = (0..parts.len())
            .filter(|&i| detail || depth[i] == 0)
            .collect();
        let limit = if detail { usize::MAX } else { CHECK_PARTS };
        say!(
            "  named groups and uses{}:",
            if detail { "" } else { " (outermost)" }
        );
        for &i in listed.iter().take(limit) {
            let indent = "  ".repeat(depth[i]);
            let count = match (detail, inside[i]) {
                (false, n) if n > 0 => format!(" ({n} inside)"),
                _ => String::new(),
            };
            say!(
                "    {indent}{}: {}{count}",
                parts[i].path,
                describe_box(parts[i].area)
            );
        }
        if listed.len() > limit {
            say!("    ...and {} more", listed.len() - limit);
        }
        let hidden = parts.len() - listed.len();
        if hidden > 0 {
            say!("    (--detail lists the {hidden} inside these too)");
        }
    }
    if !warnings.is_empty() {
        say!("  warnings:");
        let limit = if detail { usize::MAX } else { CHECK_WARNINGS };
        for warning in warnings.iter().take(limit) {
            say!("    {warning}");
        }
        if warnings.len() > limit {
            say!("    ...and {} more", warnings.len() - limit);
        }
    }
}

// ---- info -----------------------------------------------------------------------

fn info(path: &Path) -> Result<(), Problem> {
    let is_script = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    let (document, report) = if is_script {
        let (document, report, _) = paint(path)?;
        (document, Some(report))
    } else {
        let document = file::open(path).map_err(|e| failed(format!("{}: {e}", path.display())))?;
        (document, None)
    };
    let [r, g, b, _] = unpremultiply(document.paper());
    say!("{}", path.display());
    say!(
        "  canvas {} x {}, paper #{r:02X}{g:02X}{b:02X}",
        document.width(),
        document.height()
    );
    say!("  {} layer(s), bottom to top:", document.layers().len());
    let active = document.active_id();
    for (i, layer) in document.layers().iter().enumerate() {
        let mut painted = match layer.surface.painted_area() {
            Some(area) => format!("painted {}", describe_box(Some(area))),
            None => "empty".to_owned(),
        };
        if let Some(numbers) = report.as_ref().and_then(|r| r.painted_by.get(&layer.id())) {
            painted += &format!(", by {}", command_list(numbers));
        }
        say!(
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
    fn seeing_options_are_read() {
        let options = parse_render(&args(
            "a.json --layers Sky,Duck --transparent --bg #4080A0 --grid 50",
        ))
        .ok()
        .unwrap();
        assert_eq!(options.layers, Some(vec!["Sky".into(), "Duck".into()]));
        assert_eq!(options.bg, Some([0x40, 0x80, 0xA0, 255]));
        assert_eq!(options.grid, Some(50));
        let options = parse_render(&args("a.json --hide Sketch,,Notes"))
            .ok()
            .unwrap();
        assert_eq!(options.hide, ["Sketch", "Notes"]);
    }

    #[test]
    fn bad_seeing_options_are_explained() {
        let message = |text: &str| match parse_render(&args(text)) {
            Err(Problem::Usage(message)) => message,
            _ => panic!("expected a usage problem for {text}"),
        };
        assert!(message("a.json --layers A --hide B").contains("not both"));
        assert!(message("a.json --bg #4080A0").contains("--transparent"));
        assert!(message("a.json --transparent --bg blue").contains("--bg is a colour"));
        assert!(message("a.json --transparent --bg #4080A080").contains("not see-through"));
        assert!(message("a.json --grid 1").contains("--grid"));
        assert!(message("a.json --layers ,").contains("layer names"));
    }

    #[test]
    fn summaries_pack_layers_into_a_few_lines() {
        let entries: Vec<String> = (1..=30).map(|i| format!("Layer {i}: empty")).collect();
        let lines = packed(&entries, 40, 3);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "Layer 1: empty | Layer 2: empty");
        assert!(lines[2].ends_with("...and 24 more"), "{}", lines[2]);
        assert_eq!(
            packed(&entries[..2], 100, 3),
            ["Layer 1: empty | Layer 2: empty"]
        );
    }

    #[test]
    fn command_numbers_are_shortened_into_runs() {
        assert_eq!(command_list(&[4]), "command 4");
        assert_eq!(command_list(&[3, 4, 5, 12]), "commands 3-5, 12");
        assert_eq!(
            command_list(&[2, 3, 7, 9, 10, 11]),
            "commands 2, 3, 7, 9-11"
        );
    }

    #[test]
    fn boxes_are_described_in_canvas_pixels() {
        assert_eq!(
            describe_box(Some(PixelRect::new(10, 20, 118, 120))),
            "x 10..118, y 20..120"
        );
        assert_eq!(describe_box(None), "empty");
    }

    #[test]
    fn snapshot_names_become_file_names() {
        assert_eq!(slug("After Sky (2)"), "after-sky-2");
        assert_eq!(slug("blocked in!"), "blocked-in");
        assert_eq!(slug("???"), "snapshot");
    }
}
