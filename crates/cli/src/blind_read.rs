//! `art-cli blind-read`: the leak-proof blind-read test. A packet holds
//! only the picture under a neutral name and the three questions, so a
//! reader who was not told the intent can say what they see; then the
//! answers and the verdict are recorded next to the script.

use std::path::{Path, PathBuf};

use art_engine::document::Background;
use art_engine::image::Image;
use art_engine::script::PlayOptions;

use crate::gallery::BLIND_READ_QUESTIONS;
use crate::{Problem, failed, load, paint_frame};

/// What `blind-read` was asked for.
struct Options {
    script: PathBuf,
    out: PathBuf,
    /// `--record answers.txt`: the reader's answers to write down.
    record: Option<PathBuf>,
    /// `--verdict "reads"` or `"doesn't read: ..."`.
    verdict: Option<String>,
}

fn parse(args: &[String]) -> Result<Options, Problem> {
    let mut options = Options {
        script: PathBuf::new(),
        out: PathBuf::from("out"),
        record: None,
        verdict: None,
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
            "--record" => options.record = Some(PathBuf::from(value("--record")?)),
            "--verdict" => options.verdict = Some(value("--verdict")?),
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
            "blind-read needs a script: `art-cli blind-read picture.json [--out folder]` makes the \
             packet; `--record answers.txt --verdict \"reads\"` records the answers"
                .into(),
        )
    })?;
    if options.record.is_some() != options.verdict.is_some() {
        return Err(Problem::Usage(
            "--record and --verdict go together: the answers file, and \"reads\" or \"doesn't read: \
             why\""
                .into(),
        ));
    }
    Ok(options)
}

/// Four hex characters from the picture's pixels (FNV-1a), so the same
/// picture gets the same neutral name and nothing in the name says what
/// it is.
pub fn pixel_code(image: &Image) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for n in [image.width(), image.height()] {
        for byte in n.to_le_bytes() {
            mix(byte);
        }
    }
    for y in 0..image.height() {
        for x in 0..image.width() {
            for byte in image.pixel(x, y) {
                mix(byte);
            }
        }
    }
    format!("{:04x}", (hash ^ (hash >> 32) ^ (hash >> 16)) & 0xffff)
}

pub fn blind_read(args: &[String]) -> Result<(), Problem> {
    let options = parse(args)?;
    let path = options.script.as_path();
    let script = load(path)?;
    let play = PlayOptions {
        folder: path.parent().map(Path::to_path_buf),
        animation: script.animations.keys().next().cloned(),
        ..PlayOptions::default()
    };
    let (document, _, _) = paint_frame(path, &script, play)?;
    let image = Image::of_document(&document, Background::Paper);
    let code = pixel_code(&image);

    if let (Some(answers), Some(verdict)) = (&options.record, &options.verdict) {
        return record(path, &script, &code, answers, verdict);
    }

    // The packet: a folder of its own, the picture under a neutral name,
    // the questions, and nothing else.
    let packet = options.out.join(format!("blind-read-{code}"));
    std::fs::create_dir_all(&packet)
        .map_err(|e| failed(format!("can't make {}: {e}", packet.display())))?;
    let picture = packet.join(format!("piece-{code}.png"));
    image
        .save_png(&picture, false)
        .map_err(|e| failed(format!("can't save {}: {e}", picture.display())))?;
    let mut questions = String::from(
        "Look at the picture in this folder and answer in a few sentences each. You have not been \
         told what it is meant to be; say what you see.\n\n",
    );
    for (i, question) in BLIND_READ_QUESTIONS.iter().enumerate() {
        questions.push_str(&format!("{}. {question}\n", i + 1));
    }
    let questions_file = packet.join("questions.txt");
    std::fs::write(&questions_file, questions)
        .map_err(|e| failed(format!("can't write {}: {e}", questions_file.display())))?;
    say!("blind-read packet: {}", packet.display());
    say!("  {}", picture.display());
    say!("  {}", questions_file.display());
    say!(
        "  give a separate agent or a fresh session ONLY this folder: not the script, not the \
         notes, not the folder it came from, not a file named after the subject (names, titles \
         and folders leak the intent)"
    );
    say!(
        "  then record its answers: art-cli blind-read {} --record answers.txt --verdict \"reads\" \
         (or \"doesn't read: ...\")",
        path.display()
    );
    Ok(())
}

/// Writes `blind-read.md` next to the script: the questions, the
/// answers as given, the intent they're measured against, the verdict.
fn record(
    path: &Path,
    script: &art_engine::script::Script,
    code: &str,
    answers: &Path,
    verdict: &str,
) -> Result<(), Problem> {
    let answers_text = std::fs::read_to_string(answers)
        .map_err(|e| failed(format!("can't read {}: {e}", answers.display())))?;
    if answers_text.trim().is_empty() {
        return Err(failed(format!("{} is empty", answers.display())));
    }
    let verdict = verdict.trim();
    let reads = verdict.eq_ignore_ascii_case("reads");
    if !reads
        && !verdict.to_lowercase().starts_with("doesn't read")
        && !verdict.to_lowercase().starts_with("does not read")
    {
        return Err(Problem::Usage(
            "the verdict is \"reads\" or \"doesn't read: why\" (what the reader got wrong against \
             the intent)"
                .into(),
        ));
    }
    let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let file = folder.join("blind-read.md");
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "piece".to_owned());
    let mut text = format!(
        "# Blind read: {name}\n\nThe picture was given to a reader as `piece-{code}.png` with the \
         questions below and nothing else (no script, no title, no notes).\n\n## Questions\n\n"
    );
    for (i, question) in BLIND_READ_QUESTIONS.iter().enumerate() {
        text.push_str(&format!("{}. {question}\n", i + 1));
    }
    text.push_str("\n## Answers\n\n");
    text.push_str(answers_text.trim_end());
    text.push_str("\n\n## The intent\n\n");
    match &script.intent {
        Some(intent) => {
            for line in intent.describe() {
                text.push_str(&format!("- {line}\n"));
            }
        }
        None => text.push_str("(no intent block in the script: write one)\n"),
    }
    text.push_str(&format!(
        "\n## Verdict\n\n{}\n",
        if reads {
            "**Reads.** The answers match the intent.".to_owned()
        } else {
            format!("**{verdict}**")
        }
    ));
    std::fs::write(&file, text)
        .map_err(|e| failed(format!("can't write {}: {e}", file.display())))?;
    say!(
        "recorded {}: {}",
        file.display(),
        if reads { "reads" } else { verdict }
    );
    if !reads {
        say!("  the picture doesn't read yet: fix what the reader got wrong, then test again");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_depends_on_the_pixels_only() {
        let a = Image::new(8, 8, [10, 20, 30, 255]);
        let mut b = a.clone();
        assert_eq!(pixel_code(&a), pixel_code(&b));
        b.set_pixel(3, 3, [11, 20, 30, 255]);
        assert_ne!(pixel_code(&a), pixel_code(&b));
        assert_eq!(pixel_code(&a).len(), 4);
        assert!(pixel_code(&a).chars().all(|c| c.is_ascii_hexdigit()));
    }
}
