//! `quill palette`, `values` and `compare`: learning from pictures.
//! They take PNG or JPEG files, or a painting script (its first frame),
//! and never need the picture to have been made with Duck Quill.

use std::path::{Path, PathBuf};

use quill_engine::critique::{BAND_NAMES, GROUP_BAND};
use quill_engine::document::Background;
use quill_engine::image::{Color, Image};
use quill_engine::script::PlayOptions;
use quill_engine::senses::Grey;
use quill_engine::study::{self, PALETTE_COLORS, VALUE_BANDS, hex_of};

use crate::font::{draw_text, text_height, text_width};
use crate::{Problem, failed, load, paint_frame};

/// Opens a picture file, or paints a script's first frame.
pub fn open_picture(path: &Path) -> Result<Image, Problem> {
    let is_script = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if is_script {
        let script = load(path)?;
        let options = PlayOptions {
            folder: path.parent().map(Path::to_path_buf),
            animation: script.animations.keys().next().cloned(),
            ..PlayOptions::default()
        };
        let (document, _, _) = paint_frame(path, &script, options)?;
        return Ok(Image::of_document(&document, Background::Paper));
    }
    Image::load(path).map_err(|e| failed(format!("{}: {e}", path.display())))
}

fn stem_of(path: &Path) -> String {
    path.file_stem().map_or_else(
        || "picture".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// `<picture> [--out folder]`, and for `palette` `--colors n`.
struct Args {
    picture: PathBuf,
    second: Option<PathBuf>,
    out: PathBuf,
    colors: usize,
}

fn parse(command: &str, args: &[String], two: bool) -> Result<Args, Problem> {
    let mut parsed = Args {
        picture: PathBuf::new(),
        second: None,
        out: PathBuf::from("out"),
        colors: PALETTE_COLORS,
    };
    let mut files: Vec<PathBuf> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| Problem::Usage(format!("{name} needs a value")))
        };
        match arg.as_str() {
            "--out" => parsed.out = PathBuf::from(value("--out")?),
            "--colors" | "--colours" if command == "palette" => {
                let text = value("--colors")?;
                parsed.colors = text
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=64).contains(n))
                    .ok_or_else(|| {
                        Problem::Usage(format!("--colors is a number from 1 to 64, not {text}"))
                    })?;
            }
            flag if flag.starts_with("--") => {
                return Err(Problem::Usage(format!("unknown option {flag}")));
            }
            file => files.push(PathBuf::from(file)),
        }
    }
    let wanted = if two { 2 } else { 1 };
    if files.len() != wanted {
        return Err(Problem::Usage(if two {
            format!("{command} needs two pictures (PNG, JPEG or a script): yours and the reference")
        } else {
            format!("{command} needs one picture (PNG, JPEG or a script)")
        }));
    }
    if two {
        parsed.second = files.pop();
    }
    parsed.picture = files.pop().expect("one file is left");
    Ok(parsed)
}

fn make_folder(folder: &Path) -> Result<(), Problem> {
    std::fs::create_dir_all(folder)
        .map_err(|e| failed(format!("can't make the folder {}: {e}", folder.display())))
}

fn save(image: &Image, file: &Path) -> Result<(), Problem> {
    image
        .save_png(file, false)
        .map_err(|e| failed(format!("can't save {}: {e}", file.display())))
}

const SWATCH: u32 = 120;
const SWATCH_HEIGHT: u32 = 72;
const TEXT_SCALE: u32 = 2;
const DARK: Color = [38, 38, 38, 255];
const LIGHT: Color = [230, 230, 230, 255];

/// `quill palette picture [--colors n] [--out folder]`.
pub fn palette(args: &[String]) -> Result<(), Problem> {
    let args = parse("palette", args, false)?;
    let image = open_picture(&args.picture)?;
    let colors = study::palette_of(&image, args.colors);
    // The swatches in a row, each with its hex and share under it.
    let label_height = 2 * text_height(TEXT_SCALE) + 12;
    let mut block = Image::new(
        SWATCH * colors.len().max(1) as u32,
        SWATCH_HEIGHT + label_height,
        DARK,
    );
    for (i, (color, share)) in colors.iter().enumerate() {
        let x = i as u32 * SWATCH;
        block.fill_rect(x as i32, 0, SWATCH, SWATCH_HEIGHT, *color);
        let hex = hex_of(*color);
        draw_text(
            &mut block,
            &hex,
            (x + (SWATCH - text_width(&hex, TEXT_SCALE)) / 2) as i32,
            (SWATCH_HEIGHT + 4) as i32,
            TEXT_SCALE,
            LIGHT,
        );
        let share = format!("{:.0}%", share * 100.0);
        draw_text(
            &mut block,
            &share,
            (x + (SWATCH - text_width(&share, TEXT_SCALE)) / 2) as i32,
            (SWATCH_HEIGHT + 8 + text_height(TEXT_SCALE)) as i32,
            TEXT_SCALE,
            LIGHT,
        );
    }
    make_folder(&args.out)?;
    let file = args
        .out
        .join(format!("{}-palette.png", stem_of(&args.picture)));
    save(&block, &file)?;
    say!(
        "palette of {} ({} colours, biggest share first):",
        args.picture.display(),
        colors.len()
    );
    let names: Vec<String> = colors
        .iter()
        .enumerate()
        .map(|(i, (color, _))| format!("\"c{}\": \"{}\"", i + 1, hex_of(*color)))
        .collect();
    say!("  \"palette\": {{ {} }}", names.join(", "));
    let shares: Vec<String> = colors
        .iter()
        .enumerate()
        .map(|(i, (_, share))| format!("c{} {:.0}%", i + 1, share * 100.0))
        .collect();
    say!("  shares: {}", shares.join(", "));
    say!("  saved {}", file.display());
    Ok(())
}

/// The value bands of a picture, in words.
fn band_words(shares: &[f32; VALUE_BANDS]) -> String {
    shares
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{} {:.0}%", BAND_NAMES[i], s * 100.0))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `quill values picture [--out folder]`.
pub fn values(args: &[String]) -> Result<(), Problem> {
    let args = parse("values", args, false)?;
    let image = open_picture(&args.picture)?;
    let (greys, shares) = study::values_of(&image);
    make_folder(&args.out)?;
    let file = args
        .out
        .join(format!("{}-values.png", stem_of(&args.picture)));
    save(&greys, &file)?;
    say!(
        "values of {}: {}",
        args.picture.display(),
        band_words(&shares)
    );
    // The value groups as the critique counts them: the squint's bands
    // with real area.
    let squint = Grey::of(&image).squint();
    let mut counts = [0usize; VALUE_BANDS];
    for &v in &squint.values {
        counts[study::band_of(v)] += 1;
    }
    let total = squint.values.len().max(1) as f32;
    let groups: Vec<String> = counts
        .iter()
        .enumerate()
        .filter(|(_, c)| **c as f32 / total >= GROUP_BAND)
        .map(|(i, &c)| format!("{} {:.0}%", BAND_NAMES[i], c as f32 / total * 100.0))
        .collect();
    say!(
        "  {} value group{} when squinted ({})",
        groups.len(),
        if groups.len() == 1 { "" } else { "s" },
        groups.join(", ")
    );
    say!("  saved {}", file.display());
    Ok(())
}

/// `quill compare mine ref [--out folder]`.
pub fn compare(args: &[String]) -> Result<(), Problem> {
    let args = parse("compare", args, true)?;
    let second = args.second.as_ref().expect("two pictures");
    let mine = open_picture(&args.picture)?;
    let other = open_picture(second)?;
    let comparison = study::compare(&mine, &other);
    // Side by side, twice the compare width, the five greys underneath.
    let scale = 2;
    let width = study::COMPARE_WIDTH * scale;
    let shrink = |image: &Image| {
        image
            .fit_within(study::COMPARE_WIDTH, u32::MAX)
            .magnify(scale)
    };
    let (a, b) = (shrink(&mine), shrink(&other));
    let (ga, gb) = (study::values_of(&a).0, study::values_of(&b).0);
    let gap = 16;
    let label = text_height(TEXT_SCALE) + 8;
    let top_height = a.height().max(b.height());
    let bottom_height = ga.height().max(gb.height());
    let mut sheet = Image::new(
        gap * 3 + width * 2,
        gap * 3 + label * 2 + top_height + bottom_height,
        DARK,
    );
    let names = [stem_of(&args.picture), stem_of(second)];
    for (i, (picture, greys)) in [(&a, &ga), (&b, &gb)].into_iter().enumerate() {
        let x = (gap + i as u32 * (width + gap)) as i32;
        draw_text(&mut sheet, &names[i], x, gap as i32, TEXT_SCALE, LIGHT);
        sheet.paste(picture, x, (gap + label) as i32);
        let y = gap * 2 + label + top_height;
        draw_text(&mut sheet, "values", x, y as i32, TEXT_SCALE, LIGHT);
        sheet.paste(greys, x, (y + label) as i32);
    }
    make_folder(&args.out)?;
    let file = args.out.join("compare.png");
    save(&sheet, &file)?;
    say!(
        "compare {} with {}: {}",
        args.picture.display(),
        second.display(),
        comparison.words()
    );
    say!(
        "  value structure {} of 100 (half the overlap of the five greys, half the agreement of \
         a 4 x 4 grid of mean values); colour {} of 100 (the overlap of the hue histograms)",
        comparison.values,
        comparison.colour
    );
    say!(
        "  warm share {:.0}% against {:.0}%; mean value {:.2} against {:.2}",
        comparison.warm.0 * 100.0,
        comparison.warm.1 * 100.0,
        comparison.mean_value.0,
        comparison.mean_value.1
    );
    say!("  saved {}", file.display());
    Ok(())
}
