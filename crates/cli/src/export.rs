//! `quill export`: an animated script as a sprite sheet with its JSON,
//! numbered frames, a GIF and an MP4 (see `docs/sprite-sheet-format.md`).
//!
//! Every frame is painted once, at full size with a see-through
//! background, then put into whichever files were asked for. `--scale`
//! enlarges everything with sharp pixels (pixel art stays crisp).

use std::path::{Path, PathBuf};
use std::process::Command;

use quill_engine::document::Background;
use quill_engine::gif::save_gif;
use quill_engine::image::{Color, Image, unpremultiply};
use quill_engine::script::{PlayOptions, Sequence, parse_hex};
use quill_engine::sprites::{SheetAnimation, sprite_sheet};

use crate::{Problem, failed, load};

/// Enlarging more than this makes files nobody wants.
const MAX_SCALE: u32 = 32;

pub struct ExportOptions {
    script: PathBuf,
    out: PathBuf,
    /// Only this animation (the sheet too).
    animation: Option<String>,
    sheet: bool,
    frames: bool,
    gif: bool,
    mp4: bool,
    scale: u32,
    /// Behind see-through parts of GIFs and MP4s.
    bg: Option<Color>,
}

pub fn parse_export(args: &[String]) -> Result<ExportOptions, Problem> {
    let mut options = ExportOptions {
        script: PathBuf::new(),
        out: PathBuf::from("out"),
        animation: None,
        sheet: false,
        frames: false,
        gif: false,
        mp4: false,
        scale: 1,
        bg: None,
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
            "--sheet" => options.sheet = true,
            "--frames" => options.frames = true,
            "--gif" => options.gif = true,
            "--mp4" => options.mp4 = true,
            "--scale" => {
                let text = value("--scale")?;
                options.scale = text
                    .parse::<u32>()
                    .ok()
                    .filter(|s| (1..=MAX_SCALE).contains(s))
                    .ok_or_else(|| {
                        Problem::Usage(format!(
                            "--scale is a whole number from 1 to {MAX_SCALE} (times bigger, \
                             sharp pixels), not {text}"
                        ))
                    })?;
            }
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
    options.script = script.ok_or_else(|| Problem::Usage("export needs a script file".into()))?;
    if !(options.sheet || options.frames || options.gif || options.mp4) {
        options.sheet = true; // the usual thing wanted
    }
    Ok(options)
}

/// One animation, painted: its frames at full size, see-through.
struct Painted {
    name: String,
    frames: Vec<Image>,
    fps: f32,
    looping: bool,
}

pub fn export(options: &ExportOptions) -> Result<(), Problem> {
    let script = load(&options.script)?;
    let problem = |e: &dyn std::fmt::Display| failed(format!("{}: {e}", options.script.display()));
    if script.animations.is_empty() {
        return Err(failed(
            "this script has no animations: `export` is for scripts with an \"animations\" \
             section (use `render` for a picture)",
        ));
    }
    if let Some(name) = &options.animation
        && !script.animations.contains_key(name)
    {
        let names: Vec<&str> = script.animations.keys().map(String::as_str).collect();
        return Err(failed(format!(
            "there's no animation called \"{name}\" (animations: {})",
            names.join(", ")
        )));
    }
    let stem = options.script.file_stem().map_or_else(
        || "animation".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let folder = options.script.parent().map(Path::to_path_buf);
    std::fs::create_dir_all(&options.out).map_err(|e| {
        failed(format!(
            "can't make the folder {}: {e}",
            options.out.display()
        ))
    })?;

    // Paint every frame of every (chosen) animation.
    let mut painted = Vec::new();
    let mut paper = [255, 255, 255, 255];
    let mut canvas = (0, 0);
    let mut notes: Vec<String> = Vec::new();
    for (name, animation) in &script.animations {
        if options
            .animation
            .as_ref()
            .is_some_and(|wanted| wanted != name)
        {
            continue;
        }
        let mut frames = Vec::with_capacity(animation.frames as usize);
        // The frames in order: static layers are painted once.
        let mut sequence = Sequence::new(
            script.clone(),
            PlayOptions {
                folder: folder.clone(),
                animation: Some(name.clone()),
                ..PlayOptions::default()
            },
        );
        for frame in 0..animation.frames {
            let (document, _) = sequence.paint(frame, |_, _| {}).map_err(|e| problem(&e))?;
            if frame + 1 == animation.frames
                && animation.frames > 1
                && let Some(summary) = sequence.summary(&document)
            {
                notes.push(format!("animation \"{name}\": {summary}"));
            }
            paper = unpremultiply(document.paper());
            canvas = (document.width(), document.height());
            frames.push(Image::of_document(&document, Background::Transparent));
        }
        painted.push(Painted {
            name: name.clone(),
            frames,
            fps: animation.fps,
            looping: animation.looping,
        });
    }
    let frame_count: usize = painted.iter().map(|a| a.frames.len()).sum();
    let pivot = script
        .pivot
        .unwrap_or([canvas.0 as f32 / 2.0, canvas.1 as f32]);
    let scale = options.scale;
    let enlarged = |image: &Image| {
        if scale > 1 {
            image.magnify(scale)
        } else {
            image.clone()
        }
    };
    let on_bg = |image: &Image, default: Option<Color>| match options.bg.or(default) {
        Some(color) => image.on_color(color),
        None => image.clone(),
    };
    let mut saved: Vec<String> = Vec::new();
    let save_png = |image: &Image, path: &Path| -> Result<(), Problem> {
        image
            .save_png(path, true)
            .map_err(|e| failed(format!("couldn't save {}: {e}", path.display())))
    };

    if options.sheet {
        let image_name = format!("{stem}.png");
        let animations: Vec<SheetAnimation> = painted
            .iter()
            .map(|a| SheetAnimation {
                name: a.name.clone(),
                frames: a.frames.clone(),
                fps: a.fps,
                looping: a.looping,
            })
            .collect();
        let (sheet, json) =
            sprite_sheet(&animations, &image_name, pivot, scale).map_err(|e| problem(&e))?;
        let png = options.out.join(&image_name);
        save_png(&sheet, &png)?;
        let json_path = options.out.join(format!("{stem}.json"));
        std::fs::write(&json_path, json)
            .map_err(|e| failed(format!("couldn't save {}: {e}", json_path.display())))?;
        saved.push(format!(
            "{} and {} (sheet {} x {}, {} animation{}, {frame_count} frames)",
            png.display(),
            json_path.display(),
            sheet.width(),
            sheet.height(),
            painted.len(),
            if painted.len() == 1 { "" } else { "s" }
        ));
    }
    if options.frames {
        let mut count = 0;
        for animation in &painted {
            for (n, frame) in animation.frames.iter().enumerate() {
                let path = options
                    .out
                    .join(format!("{stem}-{}-{n:03}.png", animation.name));
                save_png(&enlarged(frame), &path)?;
                count += 1;
            }
        }
        saved.push(format!("{count} frames as {stem}-<animation>-<frame>.png"));
    }
    if options.gif {
        for animation in &painted {
            let frames: Vec<Image> = animation
                .frames
                .iter()
                .map(|f| enlarged(&on_bg(f, None)))
                .collect();
            let path = options.out.join(format!("{stem}-{}.gif", animation.name));
            save_gif(&frames, animation.fps, animation.looping, &path)
                .map_err(|e| failed(format!("couldn't save {}: {e}", path.display())))?;
            saved.push(path.display().to_string());
        }
    }
    if options.mp4 {
        let have_ffmpeg = Command::new("ffmpeg").arg("-version").output().is_ok();
        for animation in &painted {
            // A video has no see-through background: the paper, unless --bg.
            let frames_folder = options
                .out
                .join(format!("{stem}-{}-mp4-frames", animation.name));
            std::fs::create_dir_all(&frames_folder).map_err(|e| {
                failed(format!(
                    "can't make the folder {}: {e}",
                    frames_folder.display()
                ))
            })?;
            for (n, frame) in animation.frames.iter().enumerate() {
                let path = frames_folder.join(format!("{n:03}.png"));
                save_png(&enlarged(&on_bg(frame, Some(paper))), &path)?;
            }
            let mp4 = options.out.join(format!("{stem}-{}.mp4", animation.name));
            let command = ffmpeg_command(&frames_folder, animation.fps, &mp4);
            let done = have_ffmpeg
                && Command::new(&command[0])
                    .args(&command[1..])
                    .status()
                    .is_ok_and(|status| status.success());
            if done {
                let _ = std::fs::remove_dir_all(&frames_folder);
                saved.push(mp4.display().to_string());
            } else {
                saved.push(format!(
                    "{} (frames for the video)",
                    frames_folder.display()
                ));
                notes.push(format!(
                    "{}: run this to make the video:\n    {}",
                    if have_ffmpeg {
                        "ffmpeg didn't manage"
                    } else {
                        "ffmpeg isn't installed"
                    },
                    shell_words(&command)
                ));
            }
        }
    }

    say!(
        "Exported {}: {}",
        options.script.display(),
        painted
            .iter()
            .map(|a| format!("{} ({} frames, {} fps)", a.name, a.frames.len(), a.fps))
            .collect::<Vec<_>>()
            .join(", ")
    );
    say!(
        "  frame {} x {}{}, pivot ({}, {})",
        canvas.0 * scale,
        canvas.1 * scale,
        if scale > 1 {
            format!(" ({scale} times bigger, sharp pixels)")
        } else {
            String::new()
        },
        pivot[0] * scale as f32,
        pivot[1] * scale as f32
    );
    for line in &saved {
        say!("  {line}");
    }
    if let Some(wanted) = &options.animation {
        let skipped: Vec<&str> = script
            .animations
            .keys()
            .filter(|name| *name != wanted)
            .map(String::as_str)
            .collect();
        if !skipped.is_empty() {
            say!(
                "  skipped (not --animation {wanted}): {}",
                skipped.join(", ")
            );
        }
    }
    for note in &notes {
        say!("  {note}");
    }
    Ok(())
}

/// The ffmpeg command that turns the numbered frames into an MP4 that
/// plays everywhere (H.264, even sizes, fast start).
fn ffmpeg_command(frames_folder: &Path, fps: f32, mp4: &Path) -> Vec<String> {
    [
        "ffmpeg",
        "-y",
        "-loglevel",
        "error",
        "-framerate",
        &format!("{fps}"),
        "-i",
        &frames_folder.join("%03d.png").display().to_string(),
        "-vf",
        "pad=ceil(iw/2)*2:ceil(ih/2)*2",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
        &mp4.display().to_string(),
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// A command as one line to paste into a terminal.
fn shell_words(command: &[String]) -> String {
    command
        .iter()
        .map(|word| {
            if word.chars().any(|c| c.is_whitespace() || "()*".contains(c)) {
                format!("\"{word}\"")
            } else {
                word.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn export_options_are_read_and_the_sheet_is_the_default() {
        let options = parse_export(&args("a.json")).ok().unwrap();
        assert!(options.sheet && !options.gif && !options.frames && !options.mp4);
        let options = parse_export(&args(
            "a.json --gif --scale 8 --animation walk --bg #102030",
        ))
        .ok()
        .unwrap();
        assert!(options.gif && !options.sheet);
        assert_eq!(options.scale, 8);
        assert_eq!(options.animation.as_deref(), Some("walk"));
        assert_eq!(options.bg, Some([0x10, 0x20, 0x30, 255]));
        assert!(matches!(
            parse_export(&args("a.json --scale 0")),
            Err(Problem::Usage(_))
        ));
        assert!(matches!(
            parse_export(&args("--gif")),
            Err(Problem::Usage(_))
        ));
    }

    #[test]
    fn the_ffmpeg_command_is_one_pasteable_line() {
        let command = ffmpeg_command(
            Path::new("out/x-walk-mp4-frames"),
            12.0,
            Path::new("out/x-walk.mp4"),
        );
        let line = shell_words(&command);
        assert!(
            line.starts_with("ffmpeg -y -loglevel error -framerate 12 -i "),
            "{line}"
        );
        assert!(
            line.contains("-vf \"pad=ceil(iw/2)*2:ceil(ih/2)*2\" -c:v libx264"),
            "{line}"
        );
        assert!(line.ends_with("out/x-walk.mp4"), "{line}");
    }
}
