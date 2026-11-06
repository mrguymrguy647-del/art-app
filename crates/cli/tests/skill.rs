//! The skill for AI assistants (`skills/art-app/SKILL.md`) must only
//! mention commands, options and files that exist, and its examples
//! must work: an assistant follows it to the letter.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use art_engine::script::{PlayOptions, Player, Script};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn skill() -> String {
    std::fs::read_to_string(root().join("skills/art-app/SKILL.md")).unwrap()
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
    assert!(front.lines().any(|l| l == "name: art-app"), "{front}");
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
fn every_art_cli_command_and_option_it_mentions_exists() {
    let skill = skill();
    let help = Command::new(env!("CARGO_BIN_EXE_art-cli"))
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
                "art-cli has no --{option}"
            );
            options += 1;
        }
    }
    assert!(options >= 10, "only {options} options mentioned");
    // `art-cli <command>` in code (prose may say "the art-cli renderer").
    let mut code = inline_code(&skill).join("\n");
    code.push_str(&blocks(&skill, "bash").join("\n"));
    for (i, _) in code.match_indices("art-cli ") {
        let command: String = code[i + 8..]
            .chars()
            .take_while(char::is_ascii_lowercase)
            .collect();
        if !command.is_empty() {
            assert!(
                help.contains(&format!("art-cli {command} ")),
                "art-cli has no command {command:?}"
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
    // Words of art-cli itself.
    known.extend(["check", "render", "info"].map(str::to_owned));
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
    let library = root().join("skills/art-app/library");
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
            art_engine::script::Script::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let options = art_engine::script::PlayOptions {
            folder: Some(library.clone()),
            ..art_engine::script::PlayOptions::default()
        };
        let mut player = art_engine::script::Player::new(script, options).unwrap();
        let mut document = player.create_document().unwrap();
        player
            .run(&mut document, |_, _| {})
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let report = player.report();
        let mut warnings = report.warnings.clone();
        warnings.extend(art_engine::script::unused_warnings(&[&report]));
        assert!(warnings.is_empty(), "{name}: {warnings:?}");
    }
    assert!(parts >= 8, "{parts} examples");
}

/// Every gallery piece has its script, pictures and notes, is listed in
/// the index, and paints without mistakes or warnings.
#[test]
fn every_gallery_piece_is_complete_and_paints_cleanly() {
    let gallery = root().join("skills/art-app/gallery");
    let index: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(gallery.join("index.json")).unwrap())
            .unwrap();
    let pieces = index["pieces"].as_array().unwrap();
    assert!(pieces.len() >= 2);
    for piece in pieces {
        let folder = gallery.join(piece["folder"].as_str().unwrap());
        for file in ["script.json", "final.png", "process.png", "notes.md"] {
            assert!(
                folder.join(file).exists(),
                "{} is missing {file}",
                folder.display()
            );
        }
        let text = std::fs::read_to_string(folder.join("script.json")).unwrap();
        let script = art_engine::script::Script::parse(&text).unwrap();
        let options = art_engine::script::PlayOptions {
            folder: Some(folder.clone()),
            scale: 0.25,
            ..art_engine::script::PlayOptions::default()
        };
        let mut player = art_engine::script::Player::new(script, options).unwrap();
        let mut document = player.create_document().unwrap();
        player
            .run(&mut document, |_, _| {})
            .unwrap_or_else(|e| panic!("{}: {e}", folder.display()));
        let report = player.report();
        let mut warnings = report.warnings.clone();
        warnings.extend(art_engine::script::unused_warnings(&[&report]));
        assert!(warnings.is_empty(), "{}: {warnings:?}", folder.display());
    }
}

/// The senses are tested against reality: the gallery's known mistakes
/// are flagged, and what's right about the pieces isn't.
#[test]
fn the_critique_catches_the_gallerys_known_mistakes() {
    let gallery = root().join("skills/art-app/gallery");
    let critique = |piece: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_art-cli"))
            .arg("critique")
            .arg(gallery.join(piece).join("script.json"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(output.status.success(), "{text}");
        text
    };
    // The sunset pond: the green lily pads are out of key with the warm
    // light; the eye goes to the duck, as meant; the values aren't flat.
    let pond = critique("sunset-pond");
    assert!(
        pond.contains("out of key: a saturated yellow-green at x ") && pond.contains("(lilypad, "),
        "{pond}"
    );
    assert!(pond.contains("focal point: the eye goes to duck"), "{pond}");
    assert!(!pond.contains("looks flat"), "{pond}");
    assert!(
        pond.contains("focal contrast: ") && pond.contains("(good;"),
        "{pond}"
    );
    // The portrait: nothing out of key, the eye inside the duck.
    let portrait = critique("duck-portrait");
    assert!(portrait.contains("out of key: nothing"), "{portrait}");
    assert!(portrait.contains("inside the focal part"), "{portrait}");
    assert!(!portrait.contains("looks flat"), "{portrait}");
}

/// Every reference picture has the right to be here (a licence from the
/// allowed set, a source, an author), its palette and values pictures,
/// and a breakdown; and the study tools work on it.
#[test]
fn every_reference_picture_is_licensed_and_studied() {
    let folder = root().join("skills/art-app/references");
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
    let out = std::env::temp_dir().join(format!("art-cli-references-{}", std::process::id()));
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
            let output = Command::new(env!("CARGO_BIN_EXE_art-cli"))
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
        let output = Command::new(env!("CARGO_BIN_EXE_art-cli"))
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
