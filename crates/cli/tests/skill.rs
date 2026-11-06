//! The skill for AI assistants (`skills/duck-quill/SKILL.md`) must only
//! mention commands, options and files that exist, and its examples
//! must work: an assistant follows it to the letter.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use quill_engine::script::{PlayOptions, Player, Script};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn skill() -> String {
    std::fs::read_to_string(root().join("skills/duck-quill/SKILL.md")).unwrap()
}

/// The text between each pair of single backticks (not in code blocks).
fn inline_code(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        if line.starts_with("```") {
            in_block = !in_block;
            continue;
        }
        if in_block {
            continue;
        }
        found.extend(line.split('`').skip(1).step_by(2).map(str::to_owned));
    }
    found
}

/// The code blocks marked `lang`.
fn blocks<'a>(text: &'a str, lang: &str) -> Vec<&'a str> {
    text.split(&format!("```{lang}\n"))
        .skip(1)
        .map(|block| block.split("```").next().unwrap_or(""))
        .collect()
}

#[test]
fn it_is_short_and_says_what_it_is_for() {
    let skill = skill();
    assert!(
        skill.lines().count() < 200,
        "{} lines",
        skill.lines().count()
    );
    let front = skill
        .strip_prefix("---\n")
        .and_then(|rest| rest.split("\n---\n").next())
        .expect("starts with frontmatter between --- lines");
    assert!(front.lines().any(|l| l == "name: duck-quill"), "{front}");
    let description = front
        .lines()
        .find_map(|l| l.strip_prefix("description: "))
        .expect("a description");
    assert!(description.len() > 80, "say when to use it: {description}");
}

#[test]
fn its_examples_play_without_mistakes() {
    let skill = skill();
    let json = blocks(&skill, "json");
    assert!(json.len() >= 4);
    // The first one is a whole script; the others are commands that
    // follow on from it (later ones use symbols defined earlier).
    let script = Script::parse(json[0]).unwrap_or_else(|e| panic!("{e}\n{}", json[0]));
    let mut text = json[0].trim().trim_end_matches('}').trim_end().to_owned();
    text = text.trim_end_matches(']').trim_end().to_owned();
    // Played as a script in examples/, which is what the paths assume.
    let options = PlayOptions {
        dry_run: true,
        folder: Some(root().join("examples")),
        ..PlayOptions::default()
    };
    for block in &json[1..] {
        if block.trim_start().starts_with("{\n") && block.contains("\"commands\"") {
            // A whole script of its own (an animation): it plays on its own.
            let own = Script::parse(block).unwrap_or_else(|e| panic!("{e}\n{block}"));
            let mut player = Player::new(own, options.clone()).unwrap();
            let mut document = player.create_document().unwrap();
            player
                .run(&mut document, |_, _| {})
                .unwrap_or_else(|e| panic!("{e}\n{block}"));
            continue;
        }
        let commands = block.trim().replace("}\n{", "},\n{");
        text = format!("{text},\n{commands}");
    }
    // Inline examples, like a `snapshot` command, too (a `canvas` one is
    // checked on its own: a script has one canvas).
    for code in inline_code(&skill) {
        if code.starts_with("{ \"cmd\": \"canvas\"") {
            let own = format!(
                r##"{{ "palette": {{ "ink": "#202020", "body": "#F2C230" }}, "commands": [ {code} ] }}"##
            );
            Script::parse(&own).unwrap_or_else(|e| panic!("{e}\n{code}"));
            continue;
        }
        if code.starts_with("{ \"cmd\"") {
            text = format!("{text},\n{code}");
        }
    }
    text.push_str("\n] }");
    let all = Script::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert!(all.commands.len() > script.commands.len() + 5);
    let mut player = Player::new(all, options).unwrap();
    let mut document = player.create_document().unwrap();
    player
        .run(&mut document, |_, _| {})
        .unwrap_or_else(|e| panic!("{e}\n{text}"));
}

#[test]
fn every_quill_cli_command_and_option_it_mentions_exists() {
    let skill = skill();
    let help = Command::new(env!("CARGO_BIN_EXE_quill"))
        .arg("help")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&help.stdout).into_owned();
    let mut options = 0;
    // (Lines with `cargo` have cargo's own options.)
    let words = skill
        .lines()
        .filter(|line| !line.contains("cargo "))
        .flat_map(|line| line.split(|c: char| c.is_whitespace() || "`()".contains(c)));
    for word in words {
        let option_like = word
            .strip_prefix("--")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_lowercase()));
        if let (true, Some(option)) = (option_like, word.strip_prefix("--")) {
            let option: String = option
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            assert!(
                help.contains(&format!("--{option}")),
                "quill has no --{option}"
            );
            options += 1;
        }
    }
    assert!(options >= 10, "only {options} options mentioned");
    // `quill <command>` in code (prose may say "the quill renderer").
    let mut code = inline_code(&skill).join("\n");
    code.push_str(&blocks(&skill, "bash").join("\n"));
    for (i, _) in code.match_indices("quill ") {
        let command: String = code[i + "quill ".len()..]
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || *c == '-')
            .collect();
        if !command.is_empty() {
            assert!(
                help.contains(&format!("quill {command} ")),
                "quill has no command {command:?}"
            );
        }
    }
}

#[test]
fn every_script_command_and_field_it_mentions_exists() {
    // Everything in the script format is in script-format.md, which is
    // itself tested: in `backticks` or "quotes".
    let format = std::fs::read_to_string(root().join("docs/script-format.md")).unwrap();
    let mut known: HashSet<String> = inline_code(&format).into_iter().collect();
    for quoted in format.split('"').skip(1).step_by(2) {
        known.insert(quoted.to_owned());
    }
    // Words of quill itself.
    known.extend(
        [
            "check", "render", "info", "views", "critique", "palette", "values", "compare",
            "gallery", "export", "measure",
        ]
        .map(str::to_owned),
    );
    let mut checked = 0;
    for code in inline_code(&skill()) {
        let is_word = !code.is_empty()
            && code
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if is_word {
            assert!(known.contains(&code), "`{code}` isn't in the script format");
            checked += 1;
        }
    }
    assert!(checked >= 30, "only {checked} words checked");
}

#[test]
fn every_file_it_mentions_exists() {
    let skill = skill();
    let mut checked = 0;
    for code in inline_code(&skill) {
        let path = code.split_whitespace().next().unwrap_or("");
        let is_repo_path = ["docs/", "examples/", "skills/", "crates/"]
            .iter()
            .any(|start| path.starts_with(start));
        if is_repo_path {
            assert!(root().join(path).exists(), "{path} doesn't exist");
            checked += 1;
        }
    }
    assert!(checked >= 4, "only {checked} files checked");
}

/// Every part in the library has a working example that paints without
/// mistakes or warnings, a preview picture, and a row in the README.
#[test]
fn every_library_part_has_a_clean_example_and_a_preview() {
    let library = root().join("skills/duck-quill/library");
    let readme = std::fs::read_to_string(library.join("README.md")).unwrap();
    let mut parts = 0;
    for entry in std::fs::read_dir(&library).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let Some(part) = name.strip_suffix("-example.json") else {
            continue;
        };
        parts += 1;
        assert!(
            library.join(format!("{part}.png")).exists(),
            "{part}.png is missing"
        );
        assert!(
            part == "pond" || readme.contains(&format!("`{part}.json`")),
            "{part} isn't in the README"
        );
        let text = std::fs::read_to_string(&path).unwrap();
        let script =
            quill_engine::script::Script::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let options = quill_engine::script::PlayOptions {
            folder: Some(library.clone()),
            ..quill_engine::script::PlayOptions::default()
        };
        let mut player = quill_engine::script::Player::new(script, options).unwrap();
        let mut document = player.create_document().unwrap();
        player
            .run(&mut document, |_, _| {})
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let report = player.report();
        let mut warnings = report.warnings.clone();
        warnings.extend(quill_engine::script::unused_warnings(&[&report]));
        assert!(warnings.is_empty(), "{name}: {warnings:?}");
    }
    assert!(parts >= 8, "{parts} examples");
}

/// Every gallery piece has its script, pictures and notes, is listed in
/// the index, and paints without mistakes or warnings.
#[test]
fn every_gallery_piece_is_complete_and_paints_cleanly() {
    let gallery = root().join("skills/duck-quill/gallery");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(gallery.join("index.json")).unwrap())
            .unwrap();
    let pieces = index["pieces"].as_array().unwrap();
    assert!(pieces.len() >= 2);
    for piece in pieces {
        let folder = gallery.join(piece["folder"].as_str().unwrap());
        for file in [
            "script.json",
            "final.png",
            "process.png",
            "views.png",
            "critique.md",
            "blind-read.md",
            "notes.md",
        ] {
            assert!(
                folder.join(file).exists(),
                "{} is missing {file}",
                folder.display()
            );
        }
        let notes = std::fs::read_to_string(folder.join("notes.md")).unwrap();
        assert!(
            notes.contains("## Proposed lessons"),
            "{} notes lack the section",
            folder.display()
        );
        assert!(
            piece.get("rating").is_some(),
            "{} has no rating field",
            folder.display()
        );
        let text = std::fs::read_to_string(folder.join("script.json")).unwrap();
        assert!(
            text.contains("\"intent\""),
            "{} has no intent",
            folder.display()
        );
        let text = std::fs::read_to_string(folder.join("script.json")).unwrap();
        let script = quill_engine::script::Script::parse(&text).unwrap();
        let options = quill_engine::script::PlayOptions {
            folder: Some(folder.clone()),
            scale: 0.25,
            ..quill_engine::script::PlayOptions::default()
        };
        let mut player = quill_engine::script::Player::new(script, options).unwrap();
        let mut document = player.create_document().unwrap();
        player
            .run(&mut document, |_, _| {})
            .unwrap_or_else(|e| panic!("{}: {e}", folder.display()));
        let report = player.report();
        let mut warnings = report.warnings.clone();
        warnings.extend(quill_engine::script::unused_warnings(&[&report]));
        assert!(warnings.is_empty(), "{}: {warnings:?}", folder.display());
    }
}

/// The senses are tested against reality: the gallery's known mistakes
/// are flagged, and what's right about the pieces isn't.
#[test]
fn the_critique_catches_the_gallerys_known_mistakes() {
    let gallery = root().join("skills/duck-quill/gallery");
    let critique = |piece: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_quill"))
            .arg("critique")
            .arg(gallery.join(piece).join("script.json"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(output.status.success(), "{text}");
        text
    };
    // The sunset pond: the green lily pads are out of key with the warm
    // light; the smudge clouds are soft blobs with no direction; the far
    // hills' blurred silhouette lets the sky through (the haze band);
    // the eye goes to the duck, as meant, and it has the strongest
    // contrast; the values aren't flat; the sun is a light, not "flat".
    let pond = critique("sunset-pond");
    assert!(
        pond.contains("out of key: a saturated yellow-green at x ") && pond.contains("(lilypad, "),
        "{pond}"
    );
    assert!(pond.contains("texture: clouds is blotchy inside"), "{pond}");
    assert!(
        pond.contains("edge: layer \"Far hills\"'s")
            && pond.contains("edge is soft and the lighter background shows through"),
        "{pond}"
    );
    assert!(pond.contains("focal point: the eye goes to duck"), "{pond}");
    assert!(
        pond.contains("focal contrast: the focal point has the strongest contrast"),
        "{pond}"
    );
    assert!(!pond.contains("looks flat"), "{pond}");
    assert!(!pond.contains("sun is flat over"), "{pond}");
    // The portrait: the marbled body is blotchy (soft blobs, no
    // direction); nothing out of key; the eye inside the duck; the rim
    // light is not mistaken for a haze band.
    let portrait = critique("duck-portrait");
    assert!(
        portrait.contains("texture: duck is blotchy inside"),
        "{portrait}"
    );
    assert!(portrait.contains("out of key: nothing"), "{portrait}");
    assert!(portrait.contains("inside the focal part"), "{portrait}");
    assert!(!portrait.contains("looks flat"), "{portrait}");
    assert!(!portrait.contains("edge: duck's"), "{portrait}");
}

/// Every reference picture has the right to be here (a licence from the
/// allowed set, a source, an author), its palette and values pictures,
/// and a breakdown; and the study tools work on it.
#[test]
fn every_reference_picture_is_licensed_and_studied() {
    let folder = root().join("skills/duck-quill/references");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("index.json")).unwrap()).unwrap();
    let allowed: Vec<&str> = index["allowed_licences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(allowed, ["public domain", "CC0"]);
    let pictures = index["pictures"].as_array().unwrap();
    assert!(pictures.len() >= 4);
    let readme = std::fs::read_to_string(folder.join("README.md")).unwrap();
    let out = std::env::temp_dir().join(format!("quill-references-{}", std::process::id()));
    for picture in pictures {
        let file = picture["file"].as_str().unwrap();
        let licence = picture["licence"].as_str().unwrap();
        assert!(allowed.contains(&licence), "{file}: licence {licence:?}");
        for field in ["title", "author", "source"] {
            assert!(
                picture[field]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty()),
                "{file} needs a {field}"
            );
        }
        assert!(picture["tags"].as_array().is_some_and(|t| !t.is_empty()));
        assert!(folder.join(file).exists(), "{file} is missing");
        let stem = Path::new(file).file_stem().unwrap().to_str().unwrap();
        for suffix in ["-palette.png", "-values.png", "-breakdown.md"] {
            assert!(
                folder.join(format!("{stem}{suffix}")).exists(),
                "{stem}{suffix} is missing"
            );
        }
        assert_eq!(
            picture["breakdown"].as_str().unwrap(),
            format!("{stem}-breakdown.md")
        );
        let breakdown =
            std::fs::read_to_string(folder.join(format!("{stem}-breakdown.md"))).unwrap();
        for heading in [
            "**Shapes.**",
            "**Values**",
            "**Palette**",
            "**Why it works.**",
        ] {
            assert!(
                breakdown.contains(heading),
                "{stem}'s breakdown needs {heading}"
            );
        }
        assert!(
            readme.contains("public domain"),
            "the README says what's allowed"
        );
        // The tools open it (PNG and JPEG) and are deterministic.
        let run = || {
            let output = Command::new(env!("CARGO_BIN_EXE_quill"))
                .arg("palette")
                .arg(folder.join(file))
                .arg("--out")
                .arg(&out)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        assert_eq!(run(), run(), "{file}: the same palette every time");
        let output = Command::new(env!("CARGO_BIN_EXE_quill"))
            .arg("values")
            .arg(folder.join(file))
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap();
        assert!(output.status.success());
    }
    std::fs::remove_dir_all(out).ok();
}

/// The skill tells the painting process: seven stages with a check for
/// each, the detail budget, and the blind-read test with its warning.
#[test]
fn it_tells_the_painting_process() {
    let skill = skill();
    for stage in [
        "| 1 Intent",
        "| 2 Thumbnail",
        "| 3 Values",
        "| 4 Colour",
        "| 5 Light and form",
        "| 6 Detail",
        "| 7 Finish",
    ] {
        assert!(skill.contains(stage), "the process lacks {stage:?}");
    }
    for phrase in [
        "Detail budget",
        "quill blind-read picture.json",
        "never the script, the notes or a named file",
        "Numbers are warnings, never goals",
        "rating outranks every number",
    ] {
        assert!(skill.contains(phrase), "the skill doesn't say {phrase:?}");
    }
}

/// The learning folder: lessons each name a piece or picture that shows
/// them, mistakes have their before pictures and findings, every style
/// names its pieces, every exercise has a script, a picture and a page,
/// and every reference has a study that paints cleanly.
#[test]
fn the_learning_folder_is_complete() {
    let skill = root().join("skills/duck-quill");
    // Lessons: numbered, one line each, each with a backticked piece or
    // picture, and the rule for adding them.
    let lessons = std::fs::read_to_string(skill.join("lessons.md")).unwrap();
    assert!(lessons.contains("Agents never edit this file directly"));
    let numbered: Vec<&str> = lessons
        .lines()
        .filter(|l| l.chars().next().is_some_and(|c| c.is_ascii_digit()) && l.contains(". "))
        .collect();
    assert!(
        numbered.len() >= 25 && numbered.len() <= 40,
        "{} lessons",
        numbered.len()
    );
    for (i, line) in numbered.iter().enumerate() {
        assert!(
            line.starts_with(&format!("{}. ", i + 1)),
            "lesson {} is out of order: {line}",
            i + 1
        );
    }
    for mention in [
        "gallery/sunset-pond",
        "gallery/duck-portrait",
        "mistakes.md",
        "references/",
    ] {
        assert!(lessons.contains(mention), "lessons never mention {mention}");
    }
    // Mistakes: each has a before picture that exists, what was wrong,
    // the critique's finding, and the fix.
    let mistakes = std::fs::read_to_string(skill.join("mistakes.md")).unwrap();
    let entries = mistakes.matches("\n## ").count();
    assert!(entries >= 6, "{entries} mistakes");
    for line in mistakes.lines() {
        if let Some(rest) = line.strip_prefix("![before](") {
            let file = rest.trim_end_matches(')');
            assert!(skill.join(file).exists(), "{file} is missing");
        }
    }
    assert_eq!(mistakes.matches("![before](").count(), entries);
    assert_eq!(mistakes.matches("**Wrong:**").count(), entries);
    assert_eq!(mistakes.matches("**Critique:**").count(), entries);
    assert_eq!(mistakes.matches("**Fix:**").count(), entries);
    // Styles: palette rules, settings, what to avoid, pieces.
    for style in ["storybook", "painterly", "pixel"] {
        let text = std::fs::read_to_string(skill.join(format!("styles/{style}.md"))).unwrap();
        for heading in [
            "**Palette rules.**",
            "**What to avoid.**",
            "**Pieces that show it.**",
        ] {
            assert!(text.contains(heading), "{style}.md needs {heading}");
        }
    }
    // Exercises: a page, a script that paints cleanly and a picture each.
    let exercises = skill.join("exercises");
    let readme = std::fs::read_to_string(exercises.join("README.md")).unwrap();
    let mut count = 0;
    for entry in std::fs::read_dir(&exercises).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        count += 1;
        assert!(
            exercises.join(format!("{stem}.png")).exists(),
            "{stem}.png is missing"
        );
        let page = std::fs::read_to_string(exercises.join(format!("{stem}.md")))
            .unwrap_or_else(|_| panic!("{stem}.md is missing"));
        for heading in [
            "## Do",
            "## What a good result looks like",
            "## Lessons it practises",
        ] {
            assert!(page.contains(heading), "{stem}.md needs {heading}");
        }
        assert!(
            readme.contains(&format!("{stem}.md")),
            "the README doesn't list {stem}"
        );
        paints_cleanly(&path);
    }
    assert!(count >= 5, "{count} exercises");
    // Reference studies.
    let references = skill.join("references");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(references.join("index.json")).unwrap())
            .unwrap();
    for picture in index["pictures"].as_array().unwrap() {
        let stem = Path::new(picture["file"].as_str().unwrap())
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let study = references.join(format!("{stem}-study.json"));
        assert!(study.exists(), "{stem}-study.json is missing");
        paints_cleanly(&study);
        let breakdown =
            std::fs::read_to_string(references.join(format!("{stem}-breakdown.md"))).unwrap();
        assert!(
            breakdown.contains("**Study.**"),
            "{stem}'s breakdown has no study line"
        );
    }
}

/// Plays a script at a quarter size and insists on no mistakes or warnings.
fn paints_cleanly(path: &Path) {
    let text = std::fs::read_to_string(path).unwrap();
    let script = quill_engine::script::Script::parse(&text)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let options = quill_engine::script::PlayOptions {
        folder: path.parent().map(Path::to_path_buf),
        scale: 0.5,
        ..quill_engine::script::PlayOptions::default()
    };
    let mut player = quill_engine::script::Player::new(script, options).unwrap();
    let mut document = player.create_document().unwrap();
    player
        .run(&mut document, |_, _| {})
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let report = player.report();
    let mut warnings = report.warnings.clone();
    warnings.extend(quill_engine::script::unused_warnings(&[&report]));
    assert!(warnings.is_empty(), "{}: {warnings:?}", path.display());
}

/// `gallery add` builds a complete folder and an index entry; `gallery
/// rate` writes the person's rating without hand-editing JSON.
#[test]
fn gallery_add_and_rate_build_the_folder_and_the_index() {
    let out = std::env::temp_dir().join(format!("quill-gallery-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("pond.json");
    std::fs::write(
        &script,
        r##"{ "title": "Tiny pond", "palette": { "sky": "#8EC1E6", "sun": "#FFF1C0", "ink": "#202020" },
          "intent": { "subject": "a duck at sunset", "focal": "duck", "mood": "calm, warm", "key_colors": ["sky", "sun"] },
          "commands": [
            { "cmd": "canvas", "width": 160, "height": 100, "paper": "sky" },
            { "cmd": "layer", "name": "Sun" },
            { "cmd": "group", "name": "sun", "commands": [
              { "cmd": "ellipse", "center": [120, 30], "radius": 15, "fill": "sun" } ] },
            { "cmd": "layer", "name": "Duck" },
            { "cmd": "group", "name": "duck", "commands": [
              { "cmd": "ellipse", "center": [60, 60], "radius": [20, 12], "fill": "ink" } ] } ] }"##,
    )
    .unwrap();
    let gallery = out.join("gallery");
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_quill"))
            .arg("gallery")
            .args(args)
            .output()
            .unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned()
                + &String::from_utf8_lossy(&output.stderr),
        )
    };
    let (ok, text) = run(&[
        "add",
        script.to_str().unwrap(),
        "--name",
        "tiny-pond",
        "--out",
        gallery.to_str().unwrap(),
        "--tags",
        "test,Pond",
    ]);
    assert!(ok, "{text}");
    let folder = gallery.join("tiny-pond");
    for file in [
        "script.json",
        "final.png",
        "process.png",
        "views.png",
        "critique.md",
        "blind-read.md",
        "notes.md",
    ] {
        assert!(folder.join(file).exists(), "{file} is missing: {text}");
    }
    let critique = std::fs::read_to_string(folder.join("critique.md")).unwrap();
    assert!(
        critique.contains("focal point: the eye goes to"),
        "{critique}"
    );
    let blind = std::fs::read_to_string(folder.join("blind-read.md")).unwrap();
    assert!(blind.contains("What do you see?"));
    let notes = std::fs::read_to_string(folder.join("notes.md")).unwrap();
    assert!(notes.contains("## Proposed lessons"));
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(gallery.join("index.json")).unwrap())
            .unwrap();
    let piece = &index["pieces"][0];
    assert_eq!(piece["folder"], "tiny-pond");
    assert_eq!(piece["title"], "Tiny pond");
    assert_eq!(piece["size"], serde_json::json!([160, 100]));
    let tags: Vec<&str> = piece["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert!(
        tags.contains(&"calm")
            && tags.contains(&"warm")
            && tags.contains(&"test")
            && tags.contains(&"pond"),
        "{tags:?}"
    );
    assert!(piece["rating"].is_null());

    let (ok, text) = run(&[
        "rate",
        "tiny-pond",
        "4",
        "the duck reads",
        "--out",
        gallery.to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(gallery.join("index.json")).unwrap())
            .unwrap();
    assert_eq!(index["pieces"][0]["rating"]["score"], 4);
    assert_eq!(index["pieces"][0]["rating"]["comment"], "the duck reads");
    assert!(index["pieces"][0]["rating"]["date"].as_str().unwrap().len() == 10);
    // Adding again keeps the rating; a bad score is refused.
    let (ok, _) = run(&[
        "add",
        script.to_str().unwrap(),
        "--name",
        "tiny-pond",
        "--out",
        gallery.to_str().unwrap(),
    ]);
    assert!(ok);
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(gallery.join("index.json")).unwrap())
            .unwrap();
    assert_eq!(index["pieces"][0]["rating"]["score"], 4);
    let (ok, text) = run(&[
        "rate",
        "tiny-pond",
        "9",
        "too good",
        "--out",
        gallery.to_str().unwrap(),
    ]);
    assert!(!ok && text.contains("1 to 5"), "{text}");
    std::fs::remove_dir_all(out).ok();
}

/// The benchmark: four subjects with briefs and intents, a scoring guide
/// with a sentence per score, three scorers and the fixed mapping; and
/// the results table with the baselines scored by the numbers and by a
/// separate agent.
#[test]
fn the_benchmark_has_its_subjects_guide_and_baselines() {
    let guide =
        std::fs::read_to_string(root().join("skills/duck-quill/exercises/benchmark.md")).unwrap();
    for subject in [
        "Sunset pond",
        "Forest at dawn",
        "Character portrait",
        "Pixel walk",
    ] {
        assert!(
            guide.contains(&format!("| {subject} |")),
            "no subject {subject}"
        );
    }
    for thing in [
        "**Readability**",
        "**Values**",
        "**Colour**",
        "**Focal point**",
        "**Detail**",
        "**Overall appeal**",
    ] {
        assert!(guide.contains(thing), "the guide lacks {thing}");
    }
    assert!(guide.contains("## The fixed mapping"));
    assert!(guide.contains("quill critique piece.json --score"));
    assert!(guide.contains("The person's rating outranks every number"));
    let results =
        std::fs::read_to_string(root().join("skills/duck-quill/gallery/benchmark.md")).unwrap();
    for piece in ["sunset-pond-simple", "sunset-pond", "duck-portrait"] {
        let rows: Vec<&str> = results
            .lines()
            .filter(|l| l.contains(&format!("| {piece} |")))
            .collect();
        assert!(
            rows.iter().any(|r| r.contains("| numbers |")),
            "{piece}: no numbers row"
        );
        assert!(
            rows.iter().any(|r| r.contains("| agent |")),
            "{piece}: no agent row"
        );
        assert!(
            rows.iter().any(|r| r.contains("| person |")),
            "{piece}: no person row"
        );
    }
}
