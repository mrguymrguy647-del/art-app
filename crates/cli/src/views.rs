//! `art-cli views`: the artist's tricks as pictures. Paints the script
//! and saves a sheet of seven views (the picture, grey, squint, thumb,
//! flip, silhouettes, heat), or one of them alone. Looking never changes
//! the picture: the views are made from the painted pixels.

use std::path::{Path, PathBuf};
use std::time::Instant;

use art_engine::document::Background;
use art_engine::image::Image;
use art_engine::script::{PlayOptions, Report, focal_point};
use art_engine::senses::{self, GRID, Grey, Heat, SILHOUETTE_COLORS};

use crate::sheet::{Entry, THUMB_HEIGHT, THUMB_WIDTH, contact_sheet};
use crate::{Problem, failed, load, paint_frame, shown_animation};

/// The views, in the order they're on the sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Picture,
    Gray,
    Squint,
    Thumb,
    Flip,
    Silhouette,
    Heat,
}

impl View {
    const ALL: [View; 7] = [
        View::Picture,
        View::Gray,
        View::Squint,
        View::Thumb,
        View::Flip,
        View::Silhouette,
        View::Heat,
    ];

    /// The option that asks for this view alone, and its file name part.
    fn word(self) -> &'static str {
        match self {
            View::Picture => "picture",
            View::Gray => "gray",
            View::Squint => "squint",
            View::Thumb => "thumb",
            View::Flip => "flip",
            View::Silhouette => "silhouette",
            View::Heat => "heat",
        }
    }
}

/// What `views` was asked for.
pub struct ViewsOptions {
    pub script: PathBuf,
    pub out: PathBuf,
    pub animation: Option<String>,
    pub frame: u32,
    /// The views asked for alone; empty: the sheet with all of them.
    pub only: Vec<View>,
}

pub fn parse_views(args: &[String]) -> Result<ViewsOptions, Problem> {
    let mut options = ViewsOptions {
        script: PathBuf::new(),
        out: PathBuf::from("out"),
        animation: None,
        frame: 0,
        only: Vec::new(),
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
            "--animation" => options.animation = Some(value("--animation")?),
            "--frame" => {
                let text = value("--frame")?;
                options.frame = text.parse::<u32>().map_err(|_| {
                    Problem::Usage(format!("--frame is a frame number from 0, not {text}"))
                })?;
            }
            "--gray" | "--grey" => options.only.push(View::Gray),
            "--squint" => options.only.push(View::Squint),
            "--thumb" => options.only.push(View::Thumb),
            "--flip" => options.only.push(View::Flip),
            "--silhouette" | "--silhouettes" => options.only.push(View::Silhouette),
            "--heat" => options.only.push(View::Heat),
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
    options.only.dedup();
    options.script = script.ok_or_else(|| {
        Problem::Usage(
            "views needs one script file (and maybe --out, --animation, --frame, or one view: \
             --gray, --squint, --thumb, --flip, --silhouette, --heat)"
                .into(),
        )
    })?;
    Ok(options)
}

pub fn views(options: &ViewsOptions) -> Result<(), Problem> {
    let started = Instant::now();
    let path = options.script.as_path();
    let script = load(path)?;
    let shown = shown_animation(&script, options.animation.as_deref(), options.frame)?;
    let folder = path.parent().map(Path::to_path_buf);
    let play = |silhouettes: bool| PlayOptions {
        folder: folder.clone(),
        animation: shown.clone(),
        frame: options.frame,
        silhouettes,
        ..PlayOptions::default()
    };
    let (document, report, _) = paint_frame(path, &script, play(false))?;
    let image = Image::of_document(&document, Background::Paper);
    let canvas = (document.width(), document.height());
    let wanted: Vec<View> = if options.only.is_empty() {
        View::ALL.to_vec()
    } else {
        options.only.clone()
    };

    // The focal point from the intent (`check` warns if its part is missing).
    let focal = script
        .intent
        .as_ref()
        .and_then(|intent| focal_point(intent, &report, canvas).ok().flatten());
    let grey = Grey::of(&image);
    let squint = grey.squint();
    let heat = Heat::of_grey(&image, &grey, &squint);
    // The silhouettes come from a second painting with the parts kept
    // apart, so the picture above is exactly what `render` makes.
    let mut part_names: Vec<String> = Vec::new();
    let silhouettes = if wanted.contains(&View::Silhouette) {
        let (second, report, _) = paint_frame(path, &script, play(true))?;
        let (picture, names) = silhouettes_of(&report, second.width(), second.height());
        part_names = names;
        Some(if (picture.width(), picture.height()) == canvas {
            picture
        } else {
            picture.fit_within(canvas.0, canvas.1)
        })
    } else {
        None
    };

    std::fs::create_dir_all(&options.out).map_err(|e| {
        failed(format!(
            "can't make the folder {}: {e}",
            options.out.display()
        ))
    })?;
    let stem = path.file_stem().map_or_else(
        || "picture".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let name = match &shown {
        Some(animation) => format!("{stem}-{animation}-{}", options.frame),
        None => stem,
    };
    let save = |image: &Image, suffix: &str| -> Result<PathBuf, Problem> {
        let file = options.out.join(format!("{name}-{suffix}.png"));
        image
            .save_png(&file, false)
            .map_err(|e| failed(format!("can't save {}: {e}", file.display())))?;
        Ok(file)
    };

    let mut entries: Vec<Entry> = Vec::new();
    let mut saved: Vec<PathBuf> = Vec::new();
    for view in &wanted {
        let picture = match view {
            View::Picture => image.clone(),
            View::Gray => grey.to_image(),
            View::Squint => squint.to_image(),
            View::Thumb => senses::thumbnail(&image),
            View::Flip => senses::flipped(&image),
            View::Silhouette => silhouettes.clone().expect("painted when wanted"),
            View::Heat => heat.to_image(focal),
        };
        if options.only.is_empty() {
            let label = match view {
                View::Silhouette if part_names.is_empty() => {
                    "silhouettes: no named group or use".to_owned()
                }
                View::Silhouette => format!("silhouettes: {}", part_names.join(", ")),
                View::Thumb => format!("thumb ({} px wide)", senses::THUMB_WIDTH),
                View::Heat => format!(
                    "heat: the eye goes to {}{}",
                    describe_cell(&heat),
                    if focal.is_some() {
                        "; ring = focal"
                    } else {
                        ""
                    }
                ),
                other => other.word().to_owned(),
            };
            entries.push(Entry {
                label,
                image: fit_box(&picture),
            });
        } else {
            saved.push(save(&picture, view.word())?);
        }
    }
    if options.only.is_empty() {
        let title = format!(
            "{name}: {} x {}{}",
            canvas.0,
            canvas.1,
            match focal {
                Some((x, y)) => format!(", focal point ({x:.0}, {y:.0})"),
                None => String::new(),
            }
        );
        let sheet = contact_sheet(&title, &entries);
        saved.push(save(&sheet, "views")?);
    }

    say!(
        "OK: {} (painted in {:.2} s)",
        path.display(),
        started.elapsed().as_secs_f32()
    );
    for file in &saved {
        say!("  saved {}", file.display());
    }
    say!(
        "  the eye goes to {}{}",
        describe_cell(&heat),
        match focal {
            Some((x, y)) => format!("; the intent's focal point is ({x:.0}, {y:.0})"),
            None => "; the intent names no focal point".to_owned(),
        }
    );
    if wanted.contains(&View::Silhouette) {
        if part_names.is_empty() {
            say!("  silhouettes: no named group or use to show");
        } else {
            say!(
                "  silhouettes (outermost parts, in their colours): {}",
                part_names.join(", ")
            );
        }
    }
    say!("  the heat map is a model of attention, not the truth: look at the views");
    Ok(())
}

/// The picture in the sheet's box: shrunk to fit, or, when it's smaller
/// than the box (a sprite, the thumb), enlarged with sharp pixels.
fn fit_box(picture: &Image) -> Image {
    let times = (THUMB_WIDTH / picture.width().max(1)).min(THUMB_HEIGHT / picture.height().max(1));
    if times > 1 {
        picture.magnify(times)
    } else {
        picture.fit_within(THUMB_WIDTH, THUMB_HEIGHT)
    }
}

/// "cell (12, 9) of 32 x 32, around (600, 300)".
fn describe_cell(heat: &Heat) -> String {
    let (column, row) = heat.peak;
    let (x, y) = heat.centre_of(column, row);
    format!("cell ({column}, {row}) of {GRID} x {GRID}, around ({x:.0}, {y:.0})")
}

/// The outermost named parts' silhouettes, each in its own colour, and
/// their names in the same order (a part that drew nothing is left out).
fn silhouettes_of(report: &Report, width: u32, height: u32) -> (Image, Vec<String>) {
    let mut masks = Vec::new();
    let mut names = Vec::new();
    // In the order the parts started (the report lists them so), so the
    // colours follow the script.
    for (index, part) in report.parts.iter().enumerate() {
        if part.inside.is_some() {
            continue;
        }
        let Some((_, mask)) = report.silhouettes.iter().find(|(i, _)| *i == index) else {
            continue;
        };
        masks.push(mask.clone());
        names.push(part_name(&part.path));
    }
    let picture = senses::silhouettes(width, height, &masks);
    let mut names = counted(&names);
    if masks.len() > SILHOUETTE_COLORS.len() {
        names.push(format!(
            "(colours repeat after {})",
            SILHOUETTE_COLORS.len()
        ));
    }
    (picture, names)
}

/// `command 12 (group "duck")` -> `duck`; `... > 2 (use "pair")` -> `pair`;
/// an included symbol's `(use "reed" (from ../lib.json, command 1))` -> `reed`.
fn part_name(path: &str) -> String {
    let last = path.rsplit(" > ").next().unwrap_or(path);
    let mut quoted = last.split('"');
    match (quoted.next(), quoted.next()) {
        (Some(_), Some(name)) => name.to_owned(),
        _ => last.trim_end_matches(')').to_owned(),
    }
}

/// `duck, reed, reed, reed` -> `duck, reed (x3)`: runs of the same name
/// are counted, so the label stays short.
fn counted(names: &[String]) -> Vec<String> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for name in names {
        match out.last_mut() {
            Some((last, count)) if last == name => *count += 1,
            _ => out.push((name.clone(), 1)),
        }
    }
    out.into_iter()
        .map(|(name, count)| {
            if count > 1 {
                format!("{name} (x{count})")
            } else {
                name
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_names_are_the_quoted_names() {
        assert_eq!(part_name("command 12 (group \"duck\")"), "duck");
        assert_eq!(
            part_name("command 20 (use \"pair\") > 1 (use \"big duck\")"),
            "big duck"
        );
        assert_eq!(
            part_name("command 34 (use \"lilypad\" (from ../lib.json, command 1))"),
            "lilypad"
        );
        assert_eq!(part_name("command 3 (repeat)"), "command 3 (repeat");
        let names: Vec<String> = ["duck", "reed", "reed", "reed", "duck"]
            .iter()
            .map(|n| (*n).to_owned())
            .collect();
        assert_eq!(counted(&names), ["duck", "reed (x3)", "duck"]);
    }

    #[test]
    fn the_options_are_read() {
        let options = parse_views(&args(&["pic.json", "--heat", "--out", "o", "--frame", "2"]))
            .unwrap_or_else(|_| panic!("should parse"));
        assert_eq!(options.only, [View::Heat]);
        assert_eq!(options.out, PathBuf::from("o"));
        assert_eq!(options.frame, 2);
        assert!(parse_views(&args(&["--heat"])).is_err());
        assert!(parse_views(&args(&["pic.json", "--sheet"])).is_err());
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }
}
