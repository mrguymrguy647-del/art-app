//! `quill gallery add` and `gallery rate`: finished pieces go into the
//! gallery with everything the process asks for (the picture, the
//! process sheet, the views sheet, the critique, a blind-read skeleton,
//! notes with a "Proposed lessons" section, an index entry with tags),
//! and the person's rating goes into the index without editing JSON by
//! hand. A rating is the person's alone: it outranks every number.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::critique::paint_subject;
use crate::views::{ViewsOptions, views};
use crate::{Problem, failed, load, parse_render, render};

/// The three questions of the blind-read test, in their fixed order.
pub const BLIND_READ_QUESTIONS: [&str; 3] = [
    "What do you see?",
    "What is the focal point: where does your eye go first, and what is it?",
    "What mood does it have?",
];

pub fn gallery(args: &[String]) -> Result<(), Problem> {
    match args.first().map(String::as_str) {
        Some("add") => add(&args[1..]),
        Some("rate") => rate(&args[1..]),
        _ => Err(Problem::Usage(
            "gallery needs `add <script.json> --name <name> [--out folder] [--title \"...\"] \
             [--tags a,b]` or `rate <name> <score 1-5> \"comment\" [--out folder]`"
                .into(),
        )),
    }
}

struct AddOptions {
    script: PathBuf,
    name: String,
    out: PathBuf,
    title: Option<String>,
    tags: Vec<String>,
}

fn parse_add(args: &[String]) -> Result<AddOptions, Problem> {
    let mut options = AddOptions {
        script: PathBuf::new(),
        name: String::new(),
        out: PathBuf::from("skills/duck-quill/gallery"),
        title: None,
        tags: Vec::new(),
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
            "--name" => options.name = value("--name")?,
            "--out" => options.out = PathBuf::from(value("--out")?),
            "--title" => options.title = Some(value("--title")?),
            "--tags" => {
                options.tags = value("--tags")?
                    .split(',')
                    .map(|t| t.trim().to_lowercase())
                    .filter(|t| !t.is_empty())
                    .collect();
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
    options.script =
        script.ok_or_else(|| Problem::Usage("gallery add needs a script file".into()))?;
    if options.name.is_empty() {
        return Err(Problem::Usage(
            "gallery add needs --name (the piece's folder name, like sunset-pond)".into(),
        ));
    }
    if !options
        .name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(Problem::Usage(format!(
            "--name is a folder name in lower-case letters, digits and dashes, not {:?}",
            options.name
        )));
    }
    Ok(options)
}

/// What the index says about itself (kept current by `add`).
const ABOUT: &str = "Finished pieces made with Duck Quill's scripts: each folder has script.json \
    (the painting, with its intent), final.png (as painted), process.png (one picture per stage, \
    from --snapshots --sheet), views.png (the views sheet), critique.md (the report as printed), \
    blind-read.md (the blind-read test), and notes.md (what each tool did, what's still weak, and \
    proposed lessons). `rating` is the person's alone (quill gallery rate), 1 to 5 with a \
    comment; it outranks every number. Include library parts with paths relative to the script.";

fn read_index(folder: &Path) -> Result<Map<String, Value>, Problem> {
    let path = folder.join("index.json");
    if !path.exists() {
        let mut index = Map::new();
        index.insert("about".into(), json!(ABOUT));
        index.insert("pieces".into(), json!([]));
        return Ok(index);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| failed(format!("can't read {}: {e}", path.display())))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| failed(format!("{} isn't valid JSON: {e}", path.display())))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(failed(format!("{} isn't a JSON object", path.display()))),
    }
}

fn write_index(folder: &Path, index: &Map<String, Value>) -> Result<(), Problem> {
    let path = folder.join("index.json");
    let text = serde_json::to_string_pretty(&Value::Object(index.clone()))
        .map_err(|e| failed(format!("can't write the index: {e}")))?;
    std::fs::write(&path, text + "\n")
        .map_err(|e| failed(format!("can't write {}: {e}", path.display())))
}

/// The features a script uses, as tags: the painterly tools (noise,
/// texture, clip, lock, mask, the filters, outline, rim, stamp, brush
/// tips), library includes, text, pixel canvases and animation.
fn feature_tags(script_text: &str, script: &quill_engine::script::Script) -> Vec<String> {
    let mut tags = Vec::new();
    let has = |key: &str| script_text.contains(&format!("\"{key}\""));
    for (key, tag) in [
        ("noise", "noise"),
        ("texture", "texture"),
        ("clip", "clip"),
        ("lock", "lock"),
        ("mask", "mask"),
        ("blur", "blur"),
        ("glow", "glow"),
        ("shadow", "shadow"),
        ("adjust", "adjust"),
        ("gradient_map", "gradient_map"),
        ("outline", "outline"),
        ("rim", "rim"),
        ("stamp", "stamp"),
        ("tip", "brush tips"),
        ("dynamics", "dynamics"),
        ("hatch", "hatch"),
        ("scatter", "scatter"),
        ("include", "library"),
        ("text", "text"),
        ("pixel", "pixel art"),
    ] {
        if has(key) {
            tags.push(tag.to_owned());
        }
    }
    if !script.animations.is_empty() {
        tags.push("animated".to_owned());
        if script.animations.values().any(|a| a.looping) {
            tags.push("loop".to_owned());
        }
    }
    tags
}

fn add(args: &[String]) -> Result<(), Problem> {
    let options = parse_add(args)?;
    let script = load(&options.script)?;
    let source_text = std::fs::read_to_string(&options.script)
        .map_err(|e| failed(format!("can't read {}: {e}", options.script.display())))?;
    let folder = options.out.join(&options.name);
    std::fs::create_dir_all(&folder)
        .map_err(|e| failed(format!("can't make {}: {e}", folder.display())))?;
    let target = folder.join("script.json");
    let same_file = options
        .script
        .canonicalize()
        .ok()
        .zip(target.canonicalize().ok())
        .is_some_and(|(a, b)| a == b);
    if !same_file {
        // The script moves: its includes must still point at the same files.
        let text = with_includes_relative_to(&source_text, &options.script, &folder)?;
        std::fs::write(&target, text)
            .map_err(|e| failed(format!("can't write {}: {e}", target.display())))?;
    }
    if script.intent.is_none() {
        say!(
            "warning: the script has no intent block; the gallery's critique and blind read are \
             judged against the intent, so write one"
        );
    }
    let target_text = target
        .to_str()
        .map_or_else(|| target.to_string_lossy().into_owned(), str::to_owned);

    // Paint: final.png and process.png (every stage), through `render`.
    let scratch = folder.join(".painting");
    let _ = std::fs::remove_dir_all(&scratch);
    let render_options = parse_render(&[
        target_text.clone(),
        "--snapshots".into(),
        "--sheet".into(),
        "--out".into(),
        scratch.to_string_lossy().into_owned(),
    ])?;
    render(&render_options)?;
    let painted = find_file(&scratch, |n| {
        n.ends_with(".png") && !n.ends_with("-sheet.png") && !n.contains("-view")
    })?;
    std::fs::rename(&painted, folder.join("final.png"))
        .map_err(|e| failed(format!("can't move the picture: {e}")))?;
    let sheet = find_file(&scratch, |n| n.ends_with("-sheet.png"))?;
    std::fs::rename(&sheet, folder.join("process.png"))
        .map_err(|e| failed(format!("can't move the process sheet: {e}")))?;

    // The views sheet.
    views(&ViewsOptions {
        script: target.clone(),
        out: scratch.clone(),
        animation: None,
        frame: 0,
        only: Vec::new(),
    })?;
    let views_sheet = find_file(&scratch, |n| n.ends_with("-views.png"))?;
    std::fs::rename(&views_sheet, folder.join("views.png"))
        .map_err(|e| failed(format!("can't move the views sheet: {e}")))?;
    let _ = std::fs::remove_dir_all(&scratch);

    // The critique, as printed.
    let shown = script.animations.keys().next().cloned();
    let painted = paint_subject(&target, &script, shown, 0)?;
    let critique = painted.critique();
    let mut report = format!(
        "# Critique: {}\n\nAs `quill critique script.json --detail` printed it. Numbers are \
         warnings, never goals: a finding is a reason to look, and the person's rating outranks \
         every number.\n\n```\n",
        options.name
    );
    for note in &painted.notes {
        report.push_str(&format!("warning: {note}\n"));
    }
    for line in critique.lines() {
        report.push_str(&line);
        report.push('\n');
    }
    report.push_str("every part's numbers:\n");
    for line in critique.detail_lines() {
        report.push_str("  ");
        report.push_str(&line);
        report.push('\n');
    }
    report.push_str("```\n");
    std::fs::write(folder.join("critique.md"), report)
        .map_err(|e| failed(format!("can't write critique.md: {e}")))?;

    // The blind-read skeleton, kept if it's already been filled in.
    let blind = folder.join("blind-read.md");
    if !blind.exists() {
        let mut text = format!(
            "# Blind read: {}\n\nNot done yet. Run `quill blind-read script.json` to make a \
             packet (the picture under a neutral name and the questions, nothing else), give \
             only that folder to a separate agent or a fresh session, then record its answers \
             with `quill blind-read script.json --record answers.txt --verdict \"reads\"` (or \
             `\"doesn't read: ...\"`). Never send the script, the notes or a named file.\n\n## \
             Questions\n\n",
            options.name
        );
        for (i, question) in BLIND_READ_QUESTIONS.iter().enumerate() {
            text.push_str(&format!("{}. {question}\n", i + 1));
        }
        text.push_str("\n## Answers\n\n(none yet)\n\n## Verdict\n\n(none yet)\n");
        std::fs::write(&blind, text)
            .map_err(|e| failed(format!("can't write blind-read.md: {e}")))?;
    }

    // Notes: a skeleton, or the existing notes with the required section.
    let notes = folder.join("notes.md");
    let title = options
        .title
        .clone()
        .or_else(|| script.title.clone())
        .unwrap_or_else(|| options.name.clone());
    if notes.exists() {
        let text = std::fs::read_to_string(&notes)
            .map_err(|e| failed(format!("can't read notes.md: {e}")))?;
        if !text.contains("## Proposed lessons") {
            let text = text.trim_end().to_owned() + PROPOSED_LESSONS;
            std::fs::write(&notes, text)
                .map_err(|e| failed(format!("can't write notes.md: {e}")))?;
        }
    } else {
        let text = format!(
            "# {title}\n\n(What the piece is, in a few lines.)\n\n## What each tool did\n\n- \
             (one line per big move: layer, tool, why)\n\n## Honest notes\n\n- (what's still \
             weak, in plain words)\n{PROPOSED_LESSONS}"
        );
        std::fs::write(&notes, text).map_err(|e| failed(format!("can't write notes.md: {e}")))?;
    }

    // The index entry: title, size, tags from the intent and the tools
    // used (and --tags), the rating kept if there is one.
    let mut index = read_index(&options.out)?;
    index.insert("about".into(), json!(ABOUT));
    let pieces = index
        .entry("pieces")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .map(std::mem::take)
        .unwrap_or_default();
    let mut tags: Vec<String> = Vec::new();
    if let Some(intent) = &script.intent
        && let Some(mood) = &intent.mood
    {
        for word in mood.split(',') {
            let word = word.trim().to_lowercase();
            if !word.is_empty() && word.split_whitespace().count() <= 2 {
                tags.push(word);
            }
        }
    }
    tags.extend(feature_tags(&source_text, &script));
    tags.extend(options.tags.iter().cloned());
    tags.dedup();
    let mut seen = std::collections::BTreeSet::new();
    tags.retain(|t| seen.insert(t.clone()));
    let old = pieces
        .iter()
        .find(|p| p["folder"].as_str() == Some(options.name.as_str()))
        .cloned();
    let rating = old
        .as_ref()
        .and_then(|p| p.get("rating").cloned())
        .unwrap_or(Value::Null);
    let mut entry = Map::new();
    entry.insert("folder".into(), json!(options.name));
    entry.insert("title".into(), json!(title));
    entry.insert(
        "size".into(),
        json!([painted.image.width(), painted.image.height()]),
    );
    entry.insert("animated".into(), json!(!script.animations.is_empty()));
    entry.insert("tags".into(), json!(tags));
    if let Some(old) = &old {
        for (key, value) in old.as_object().into_iter().flatten() {
            if !entry.contains_key(key) && key != "rating" {
                entry.insert(key.clone(), value.clone());
            }
        }
    }
    entry.insert("rating".into(), rating);
    let mut pieces: Vec<Value> = pieces
        .into_iter()
        .filter(|p| p["folder"].as_str() != Some(options.name.as_str()))
        .collect();
    pieces.push(Value::Object(entry));
    index.insert("pieces".into(), Value::Array(pieces));
    write_index(&options.out, &index)?;

    say!("added {} to {}:", options.name, options.out.display());
    for file in [
        "script.json",
        "final.png",
        "process.png",
        "views.png",
        "critique.md",
        "blind-read.md",
        "notes.md",
    ] {
        say!("  {}", folder.join(file).display());
    }
    say!("  index.json: tags {}", tags.join(", "));
    say!(
        "  next: fill in notes.md (what each tool did, honest notes, proposed lessons), run the \
         blind-read test, and ask the person to rate it (quill gallery rate {} 4 \"...\")",
        options.name
    );
    Ok(())
}

const PROPOSED_LESSONS: &str = "\n\n## Proposed lessons\n\n- (none yet: a rule this piece \
    teaches, one line, for a reviewer who didn't paint it to put forward; it goes into \
    lessons.md only when the person agrees or the before/after numbers back it)\n";

/// The first file in `folder` whose name passes `test`.
fn find_file(folder: &Path, test: impl Fn(&str) -> bool) -> Result<PathBuf, Problem> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|e| failed(format!("can't read {}: {e}", folder.display())))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    names.sort();
    names
        .into_iter()
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(&test))
        .ok_or_else(|| {
            failed(format!(
                "the expected picture wasn't painted in {}",
                folder.display()
            ))
        })
}

/// The script's text with every `include` path rewritten so it points
/// at the same file from `to`.
fn with_includes_relative_to(text: &str, from: &Path, to: &Path) -> Result<String, Problem> {
    if !text.contains("\"include\"") {
        return Ok(text.to_owned());
    }
    let mut value: Value = serde_json::from_str(text)
        .map_err(|e| failed(format!("{} isn't valid JSON: {e}", from.display())))?;
    let base = from.parent().map(Path::to_path_buf).unwrap_or_default();
    let to_abs = normalised(&to.canonicalize().unwrap_or_else(|_| to.to_path_buf()));
    fn walk(value: &mut Value, base: &Path, to: &Path) {
        match value {
            Value::Object(map) => {
                if map.get("cmd").and_then(Value::as_str) == Some("include")
                    && let Some(Value::String(file)) = map.get("file")
                {
                    let source = normalised(&base.join(file));
                    if let Some(relative) = pathdiff(&source, to) {
                        map.insert("file".into(), Value::String(relative));
                    }
                }
                for v in map.values_mut() {
                    walk(v, base, to);
                }
            }
            Value::Array(items) => {
                for v in items {
                    walk(v, base, to);
                }
            }
            _ => {}
        }
    }
    walk(&mut value, &base, &to_abs);
    serde_json::to_string_pretty(&value)
        .map(|s| s + "\n")
        .map_err(|e| failed(format!("can't write the script: {e}")))
}

/// `path` with its `.` and `..` parts folded away (no file system
/// needed, so a path that doesn't exist yet works too).
fn normalised(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path` relative to `from` (both absolute), with forward slashes.
fn pathdiff(path: &Path, from: &Path) -> Option<String> {
    let a: Vec<_> = path.components().collect();
    let b: Vec<_> = from.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".to_owned(); b.len() - common];
    for c in &a[common..] {
        parts.push(c.as_os_str().to_str()?.to_owned());
    }
    Some(parts.join("/"))
}

fn rate(args: &[String]) -> Result<(), Problem> {
    let mut out = PathBuf::from("skills/duck-quill/gallery");
    let mut words: Vec<String> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--out" => {
                out = PathBuf::from(
                    rest.next()
                        .ok_or_else(|| Problem::Usage("--out needs a value".into()))?,
                )
            }
            flag if flag.starts_with("--") => {
                return Err(Problem::Usage(format!("unknown option {flag}")));
            }
            word => words.push(word.to_owned()),
        }
    }
    let [name, score, comment] = words.as_slice() else {
        return Err(Problem::Usage(
            "gallery rate needs a piece name, a score from 1 to 5 and a comment in quotes, like \
             `quill gallery rate sunset-pond 4 \"the duck reads, the clouds don't\"`"
                .into(),
        ));
    };
    let score: u8 = score
        .parse()
        .ok()
        .filter(|s| (1..=5).contains(s))
        .ok_or_else(|| {
            Problem::Usage(format!(
                "the score is a whole number from 1 to 5, not {score}"
            ))
        })?;
    if comment.trim().is_empty() {
        return Err(Problem::Usage(
            "the comment can't be empty: a word on why is worth more than the number".into(),
        ));
    }
    let mut index = read_index(&out)?;
    let Some(pieces) = index.get_mut("pieces").and_then(Value::as_array_mut) else {
        return Err(failed("the index has no pieces"));
    };
    let Some(piece) = pieces
        .iter_mut()
        .find(|p| p["folder"].as_str() == Some(name.as_str()))
    else {
        let names: Vec<&str> = pieces.iter().filter_map(|p| p["folder"].as_str()).collect();
        return Err(failed(format!(
            "there's no piece called {name:?} in {} (pieces: {})",
            out.display(),
            names.join(", ")
        )));
    };
    let date = today();
    piece["rating"] = json!({ "score": score, "comment": comment.trim(), "date": date });
    write_index(&out, &index)?;
    say!(
        "rated {name}: {score} of 5, \"{}\" ({date})",
        comment.trim()
    );
    say!("  the person's rating outranks every number: it's what the piece is measured by");
    Ok(())
}

/// Today as YYYY-MM-DD (UTC), from the clock.
fn today() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = seconds / 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_rewritten_relative_to_the_new_folder() {
        let diff = pathdiff(
            Path::new("/repo/skills/duck-quill/library/tree.json"),
            Path::new("/repo/skills/duck-quill/gallery/forest"),
        );
        assert_eq!(diff.as_deref(), Some("../../library/tree.json"));
        let text = r#"{ "commands": [ { "cmd": "include", "file": "../lib/tree.json" } ] }"#;
        let out = with_includes_relative_to(
            text,
            Path::new("/repo/examples/forest.json"),
            Path::new("/repo/skills/duck-quill/gallery/forest"),
        )
        .unwrap_or_else(|_| panic!("should rewrite"));
        assert!(
            out.contains("\"file\": \"../../../../lib/tree.json\""),
            "{out}"
        );
    }

    #[test]
    fn the_date_is_civil() {
        assert_eq!(today().len(), 10);
        assert!(today().starts_with("20"));
    }
}
