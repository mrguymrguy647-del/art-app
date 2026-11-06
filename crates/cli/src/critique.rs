//! `art-cli critique`: the artist's eye in numbers. Paints the script,
//! then reports about fifteen findings: values, the focal point, colour
//! and key, texture, readability at thumbnail size and composition.
//! Every finding has a place, the number behind it and a hint; the
//! numbers are warnings to look at, never goals.

use std::path::{Path, PathBuf};
use std::time::Instant;

use art_engine::critique::{Critique, Subject};
use art_engine::document::Background;
use art_engine::image::Image;
use art_engine::script::{Focal, PlayOptions, Report, Script, focal_point, parse_hex};
use art_engine::surface::PixelRect;

use crate::views::{named_parts, part_name};
use crate::{Problem, load, paint_frame, shown_animation};

/// What `critique` was asked for.
pub struct CritiqueOptions {
    pub script: PathBuf,
    pub animation: Option<String>,
    /// One frame, or a range of frames (one line each).
    pub frames: (u32, Option<u32>),
    /// Also every part's numbers.
    pub detail: bool,
}

pub fn parse_critique(args: &[String]) -> Result<CritiqueOptions, Problem> {
    let mut options = CritiqueOptions {
        script: PathBuf::new(),
        animation: None,
        frames: (0, None),
        detail: false,
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
            "--animation" => options.animation = Some(value("--animation")?),
            "--detail" => options.detail = true,
            "--frame" => {
                let text = value("--frame")?;
                let n = text.parse::<u32>().map_err(|_| {
                    Problem::Usage(format!("--frame is a frame number from 0, not {text}"))
                })?;
                options.frames = (n, None);
            }
            "--frames" => {
                let text = value("--frames")?;
                let range = text
                    .split_once('-')
                    .and_then(|(a, b)| {
                        Some((a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?))
                    })
                    .filter(|(a, b)| a <= b)
                    .ok_or_else(|| {
                        Problem::Usage(format!(
                            "--frames is a range like 0-7 (first and last frame, from 0), not {text}"
                        ))
                    })?;
                options.frames = (range.0, Some(range.1));
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
    options.script = script.ok_or_else(|| {
        Problem::Usage(
            "critique needs one script file (and maybe --detail, --frame n, --frames a-b, \
             --animation)"
                .into(),
        )
    })?;
    Ok(options)
}

/// A frame's picture with everything the critique needs to know about it.
pub struct Painted {
    pub image: Image,
    pub focal: Option<(f32, f32)>,
    pub focal_part: Option<(String, PixelRect)>,
    pub key_colors: Vec<(String, [u8; 4])>,
    pub parts: Vec<art_engine::critique::Part>,
    /// Problems with the intent (a focal part that isn't there).
    pub notes: Vec<String>,
}

/// Paints one frame plainly (the picture) and once more with the
/// silhouettes kept, and gathers the intent's focal point, focal part
/// and key colours.
pub fn paint_subject(
    path: &Path,
    script: &Script,
    animation: Option<String>,
    frame: u32,
) -> Result<Painted, Problem> {
    let folder = path.parent().map(Path::to_path_buf);
    let play = |silhouettes: bool| PlayOptions {
        folder: folder.clone(),
        animation: animation.clone(),
        frame,
        silhouettes,
        ..PlayOptions::default()
    };
    let (document, report, _) = paint_frame(path, script, play(false))?;
    let image = Image::of_document(&document, Background::Paper);
    let canvas = (document.width(), document.height());
    let (second, with_parts, _) = paint_frame(path, script, play(true))?;
    let painted = (second.width(), second.height());
    let parts = named_parts(&with_parts, painted, canvas);
    let mut notes = Vec::new();
    let mut focal = None;
    let mut focal_part = None;
    let mut key_colors = Vec::new();
    if let Some(intent) = &script.intent {
        match focal_point(intent, &report, canvas) {
            Ok(point) => focal = point,
            Err(problem) => notes.push(problem),
        }
        if let Ok(Some(Focal::Part(name))) = intent.focal() {
            focal_part = focal_part_box(&report, &name, painted, canvas);
        }
        for name in &intent.key_colors {
            if let Some(hex) = script.palette.get(name)
                && let Ok(color) = parse_hex(hex)
            {
                key_colors.push((name.clone(), color));
            }
        }
    }
    Ok(Painted {
        image,
        focal,
        focal_part,
        key_colors,
        parts,
        notes,
    })
}

/// The box of the part the focal point names (the first group or use
/// with that name, outermost first), in canvas pixels.
fn focal_part_box(
    report: &Report,
    name: &str,
    painted: (u32, u32),
    canvas: (u32, u32),
) -> Option<(String, PixelRect)> {
    let part = report.parts.iter().find(|p| part_name(&p.path) == name)?;
    let a = part.area?;
    let sx = canvas.0 as f32 / painted.0.max(1) as f32;
    let sy = canvas.1 as f32 / painted.1.max(1) as f32;
    Some((
        name.to_owned(),
        PixelRect::new(
            (a.left as f32 * sx).floor() as i32,
            (a.top as f32 * sy).floor() as i32,
            (a.right as f32 * sx).ceil() as i32,
            (a.bottom as f32 * sy).ceil() as i32,
        ),
    ))
}

impl Painted {
    pub fn critique(&self) -> Critique {
        Critique::of(&Subject {
            image: &self.image,
            focal: self.focal,
            focal_part: self.focal_part.clone(),
            key_colors: self.key_colors.clone(),
            parts: self.parts.clone(),
        })
    }
}

pub fn critique(options: &CritiqueOptions) -> Result<(), Problem> {
    let started = Instant::now();
    let path = options.script.as_path();
    let script = load(path)?;
    let (first, last) = options.frames;
    let shown = shown_animation(&script, options.animation.as_deref(), last.unwrap_or(first))?;
    match last {
        None => {
            let painted = paint_subject(path, &script, shown.clone(), first)?;
            let critique = painted.critique();
            say!(
                "critique: {} (painted in {:.2} s{})",
                path.display(),
                started.elapsed().as_secs_f32(),
                match &shown {
                    Some(name) => format!(", animation \"{name}\" frame {first}"),
                    None => String::new(),
                }
            );
            say!(
                "  canvas {} x {}{}",
                painted.image.width(),
                painted.image.height(),
                match script.intent.as_ref().and_then(|i| i.subject.clone()) {
                    Some(subject) => format!("; intent: {subject}"),
                    None =>
                        "; no intent block (write one: it's what the picture is judged against)"
                            .to_owned(),
                }
            );
            for note in &painted.notes {
                say!("  warning: {note}");
            }
            for line in critique.lines() {
                say!("  {line}");
            }
            if options.detail {
                say!("  every part's numbers:");
                for line in critique.detail_lines() {
                    say!("    {line}");
                }
            }
        }
        Some(last) => {
            let name = shown.clone().expect("a range needs an animation");
            say!(
                "critique: {} animation \"{name}\", frames {first} to {last}",
                path.display()
            );
            let mut worst: Option<(u32, usize, f32)> = None;
            for frame in first..=last {
                let painted = paint_subject(path, &script, shown.clone(), frame)?;
                let c = painted.critique();
                let warnings = c.warnings();
                let distance = c.focal.distance.unwrap_or(0.0);
                let out_of_key: f32 = c.colour.out_of_key.iter().map(|b| b.share).sum();
                say!(
                    "  frame {frame}: {} value group{}; eye {}; out of key {:.1}%; {warnings} warning{}",
                    c.values.groups.len(),
                    if c.values.groups.len() == 1 { "" } else { "s" },
                    match c.focal.distance {
                        Some(d) => format!("{d:.0} px from the focal point"),
                        None => format!("at x {:.0}, y {:.0}", c.focal.peak.0, c.focal.peak.1),
                    },
                    out_of_key * 100.0,
                    if warnings == 1 { "" } else { "s" }
                );
                let score = (warnings, distance);
                if worst.is_none_or(|(_, w, d)| score.0 > w || (score.0 == w && score.1 > d)) {
                    worst = Some((frame, warnings, distance));
                }
            }
            if let Some((frame, warnings, _)) = worst {
                say!(
                    "  worst frame: {frame} ({warnings} warning{}); critique it alone with --frame {frame}",
                    if warnings == 1 { "" } else { "s" }
                );
            }
            say!(
                "  Numbers can't judge beauty: look at the views, run the blind-read test, and ask \
                 for a rating. (painted in {:.2} s)",
                started.elapsed().as_secs_f32()
            );
        }
    }
    Ok(())
}
