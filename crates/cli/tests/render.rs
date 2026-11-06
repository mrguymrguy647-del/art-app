//! Runs the real art-cli program, the way it's used.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn art_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_art-cli"))
}

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/sunset.json")
}

/// A fresh, empty folder for one test's output.
fn folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("art-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

#[test]
fn the_example_renders_the_same_every_time() {
    let out = folder("same");
    let render = |sub: &str| {
        let status = art_cli()
            .args([
                "render",
                example().to_str().unwrap(),
                "--preview",
                "--sheet",
                "--out",
            ])
            .arg(out.join(sub))
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::read(out.join(sub).join("sunset-preview.png")).unwrap()
    };
    let first = render("a");
    let second = render("b");
    assert!(!first.is_empty());
    assert_eq!(first, second, "the same script must give the same picture");
    assert!(out.join("a/sunset-sheet.png").exists());
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn snapshots_crops_and_the_artapp_file_are_saved() {
    let out = folder("files");
    let output = art_cli()
        .args([
            "render",
            example().to_str().unwrap(),
            "--snapshots",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.join("sunset.png").exists());
    assert!(out.join("sunset.artapp").exists());
    assert!(out.join("snapshots/03-sky-and-sun.png").exists());

    let crop = art_cli()
        .args([
            "render",
            example().to_str().unwrap(),
            "--crop",
            "10,20,30,40",
            "--zoom",
            "2",
            "--out",
        ])
        .arg(&out)
        .status()
        .unwrap();
    assert!(crop.success());
    assert!(out.join("sunset-crop-10-20-30-40.png").exists());

    let info = art_cli()
        .arg("info")
        .arg(out.join("sunset.artapp"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&info.stdout);
    assert!(text.contains("canvas 1600 x 1000"), "{text}");
    assert!(
        text.contains("Reflection  [visible, opacity 100%, screen]"),
        "{text}"
    );
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn mistakes_are_reported_with_the_command_number() {
    let out = folder("broken");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("broken.json");
    std::fs::write(
        &script,
        r#"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100 },
            { "cmd": "brush", "colour": "red" }
        ] }"#,
    )
    .unwrap();
    for command in ["check", "render"] {
        let output = art_cli()
            .arg(command)
            .arg(&script)
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap();
        let output = if command == "check" {
            art_cli().arg(command).arg(&script).output().unwrap()
        } else {
            output
        };
        assert_eq!(output.status.code(), Some(1), "{command}");
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(
            message.contains("command 2 (brush): unknown field `colour`"),
            "{message}"
        );
    }
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn checking_the_example_finds_no_problems() {
    let output = art_cli().arg("check").arg(example()).output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("OK:"), "{text}");
    assert!(
        text.contains("snapshots: sky and sun, blocked in, sea details, finished"),
        "{text}"
    );
}

#[test]
fn wrong_usage_gets_help() {
    let output = art_cli().args(["render"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("art-cli help"));
}

#[test]
fn every_example_script_is_free_of_mistakes() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut count = 0;
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "json") {
            let output = art_cli().arg("check").arg(&path).output().unwrap();
            assert!(
                output.status.success(),
                "{}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
            count += 1;
        }
    }
    assert!(count >= 2);
}

#[test]
fn a_reader_that_stops_early_is_not_an_error() {
    // Like `art-cli render ... | head -1`: the reading end of the output
    // is closed before art-cli gets to print anything.
    let out = folder("pipe");
    let mut child = art_cli()
        .args(["render", example().to_str().unwrap(), "--preview", "--out"])
        .arg(&out)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "exit {:?}: {errors}",
        output.status.code()
    );
    assert!(errors.is_empty(), "nothing on the error output: {errors}");
    assert!(
        out.join("sunset-preview.png").exists(),
        "the work was still done"
    );
    std::fs::remove_dir_all(out).ok();
}

/// A small see-through sprite with a few mistakes in it.
const SPRITE: &str = r##"{
  "palette": { "ink": "#202020", "yellow": "#F2C230", "teal": "#208080" },
  "commands": [
    { "cmd": "canvas", "width": 128, "height": 128 },
    { "cmd": "define", "name": "dot", "commands": [
      { "cmd": "layer", "name": "Body" },
      { "cmd": "ellipse", "center": [0, 0], "radius": 20, "fill": "yellow" } ] },
    { "cmd": "define", "name": "spare", "commands": [] },
    { "cmd": "layer", "name": "Sketch" },
    { "cmd": "use", "name": "dot", "translate": [64, 64] },
    { "cmd": "layer", "name": "Labels" },
    { "cmd": "text", "text": "Hi", "at": [64, 90], "size": 16, "color": "ink", "align": "center" },
    { "cmd": "rect", "x": 200, "y": 10, "width": 10, "height": 10, "fill": "ink" }
  ]
}"##;

#[test]
fn check_says_where_things_are_and_what_looks_wrong() {
    let out = folder("check-sprite");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("sprite.json");
    std::fs::write(&script, SPRITE).unwrap();
    let output = art_cli().arg("check").arg(&script).output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "4 warnings)",
        "    Sketch: empty",
        "    Body: x 44..84, y 44..84",
        "    command 5 (use \"dot\"): x 44..84, y 44..84",
        "    command 8 (rect) draws entirely outside the canvas (x 200..210, y 10..20)",
        "    symbol \"spare\" (command 3) is defined but never used",
        "    palette colour \"teal\" is never used",
        "    layer \"Sketch\" is empty",
    ] {
        assert!(
            text.contains(expected),
            "{expected:?} missing from:\n{text}"
        );
    }
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn seeing_options_and_the_short_summary() {
    let out = folder("see-sprite");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("sprite.json");
    std::fs::write(&script, SPRITE).unwrap();
    let render = |extra: &[&str]| {
        let output = art_cli()
            .arg("render")
            .arg(&script)
            .args(["--out", out.to_str().unwrap()])
            .args(extra)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    };

    // The summary is short, with the layers' boxes and the warnings.
    let summary = render(&["--transparent", "--bg", "#4080A0", "--grid", "32"]);
    assert!(summary.lines().count() <= 12, "{summary}");
    assert!(summary.contains("Body: x 44..84, y 44..84"), "{summary}");
    assert!(summary.contains("warnings (4):"), "{summary}");
    // The picture keeps its see-through background; the view has the
    // colour behind it (and the grid).
    let png = std::fs::read(out.join("sprite.png")).unwrap();
    let view = std::fs::read(out.join("sprite-view.png")).unwrap();
    assert_ne!(png, view);
    assert!(summary.contains("sprite-view.png"), "{summary}");

    // Only some layers.
    let only = render(&["--layers", "Body"]);
    assert!(only.contains("Labels (hidden)"), "{only}");
    assert_ne!(std::fs::read(out.join("sprite.png")).unwrap(), png);
    let missing = art_cli()
        .arg("render")
        .arg(&script)
        .args(["--out", out.to_str().unwrap(), "--hide", "Skech"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("no layer called \"Skech\""),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn check_lists_the_outermost_groups_unless_asked_for_detail() {
    let out = folder("check-nested");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("sheet.json");
    std::fs::write(
        &script,
        r##"{ "commands": [
            { "cmd": "canvas", "width": 512, "height": 128 },
            { "cmd": "define", "name": "dot", "commands": [
                { "cmd": "ellipse", "center": [0, 0], "radius": 10, "fill": "#202020" } ] },
            { "cmd": "group", "name": "sheet", "commands": [
                { "cmd": "repeat", "count": 4, "step": { "translate": [128, 0] }, "commands": [
                    { "cmd": "use", "name": "dot", "translate": [64, 64] } ] } ] }
        ] }"##,
    )
    .unwrap();
    let check = |detail: bool| {
        let mut command = art_cli();
        command.arg("check");
        if detail {
            command.arg("--detail");
        }
        let output = command.arg(&script).output().unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let short = check(false);
    assert!(
        short.contains("    command 3 (group \"sheet\"): x 54..458, y 54..74 (4 inside)"),
        "{short}"
    );
    assert!(!short.contains("use \"dot\""), "{short}");
    let full = check(true);
    assert!(
        full.contains("      command 3 (group \"sheet\") > 1 (repeat, copy 4) > 1 (use \"dot\"): x 438..458, y 54..74"),
        "{full}"
    );
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn crops_can_go_past_the_edges_and_info_reads_scripts() {
    let out = folder("edges");
    let crop = art_cli()
        .args([
            "render",
            example().to_str().unwrap(),
            "--crop",
            "-20,-10,100,60",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert!(crop.status.success());
    assert!(out.join("sunset-crop--20--10-100-60.png").exists());
    assert!(String::from_utf8_lossy(&crop.stdout).contains("outside the canvas"));
    let outside = art_cli()
        .args([
            "render",
            example().to_str().unwrap(),
            "--crop",
            "5000,0,10,10",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(outside.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&outside.stderr)
            .contains("entirely outside the 1600 x 1000 canvas")
    );

    let info = art_cli().arg("info").arg(example()).output().unwrap();
    let text = String::from_utf8_lossy(&info.stdout);
    assert!(
        text.contains("Clouds  [visible, opacity 100%, normal]  painted x 0..1600, y 51..486, by commands 11-19"),
        "{text}"
    );
    std::fs::remove_dir_all(out).ok();
}

/// A small animation: a box that an animation shows at two places.
const ANIMATED: &str = r##"{ "commands": [
    { "cmd": "canvas", "width": 64, "height": 64 },
    { "cmd": "define", "name": "box", "commands": [
        { "cmd": "rect", "x": 0, "y": 0, "width": 10, "height": 10, "fill": "#202020" } ] }
  ], "animations": {
    "slide": { "frames": 3, "fps": 6, "loop": true, "commands": [
        { "cmd": "use", "name": "box", "translate": [10, 10] } ] },
    "still": { "frames": 1, "commands": [ { "cmd": "use", "name": "box" } ] }
  } }"##;

#[test]
fn frames_of_an_animation_are_painted_and_checked() {
    let out = folder("animated");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("anim.json");
    std::fs::write(&script, ANIMATED).unwrap();
    let run = |extra: &[&str]| {
        art_cli()
            .arg("render")
            .arg(&script)
            .args(["--out", out.to_str().unwrap()])
            .args(extra)
            .output()
            .unwrap()
    };
    let output = run(&[]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("frame 0 was painted"), "{text}");
    assert!(out.join("anim-slide-000.png").exists());
    assert!(
        !out.join("anim-slide-000.artapp").exists(),
        "frames are flat pictures"
    );

    assert!(run(&["--all-frames"]).status.success());
    assert!(out.join("anim-slide-002.png").exists());
    assert!(
        run(&["--animation", "still", "--frame", "0"])
            .status
            .success()
    );
    assert!(out.join("anim-still-000.png").exists());
    let wrong = run(&["--frames", "1-5"]);
    assert_eq!(wrong.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("no frame 5"));
    let usage = run(&["--all-frames", "--sheet"]);
    assert_eq!(usage.status.code(), Some(2));

    let check = art_cli().arg("check").arg(&script).output().unwrap();
    assert!(check.status.success());
    let text = String::from_utf8_lossy(&check.stdout);
    assert!(
        text.contains("animations (every frame painted without mistakes): slide (3 frames, 6 fps, loops); still (1 frames, 12 fps)"),
        "{text}"
    );
    assert!(
        text.contains("animation \"slide\" > command 1 (use \"box\")"),
        "{text}"
    );
    // A mistake in a later frame's... well, in the animation's commands, is found.
    std::fs::write(
        &script,
        ANIMATED.replace(
            "\"name\": \"box\", \"translate\"",
            "\"name\": \"bax\", \"translate\"",
        ),
    )
    .unwrap();
    let check = art_cli().arg("check").arg(&script).output().unwrap();
    assert_eq!(check.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&check.stderr)
            .contains("animation \"slide\" > command 1 (use \"bax\")")
    );
    std::fs::remove_dir_all(out).ok();
}

/// A looping animation with a part that jumps, one whose loop pops, and
/// one that's fine.
const BAD_MOTION: &str = r##"{ "commands": [
    { "cmd": "canvas", "width": 100, "height": 100 },
    { "cmd": "define", "name": "dot", "commands": [ { "cmd": "ellipse", "center": [0, 0], "radius": 5, "fill": "#202020" } ] }
  ], "animations": { "bad": { "frames": 4, "fps": 4, "loop": true, "commands": [
    { "cmd": "use", "name": "dot", "translate": { "keys": [[0, [10, 50]], [3, [70, 50]]] } },
    { "cmd": "group", "name": "jumper", "translate": { "keys": [[0, [20, 20]], [1, [20, 20], "step"], [2, [80, 80]]] }, "commands": [
      { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "#202020" } ] },
    { "cmd": "group", "name": "steady", "translate": { "keys": [[0, [50, 10]], [2, [50, 14]], [4, [50, 10]]] }, "commands": [
      { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "#202020" } ] }
  ] } } }"##;

#[test]
fn the_motion_summary_shows_steps_and_warns_about_jumps_and_pops() {
    let out = folder("motion");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("motion.json");
    std::fs::write(&script, BAD_MOTION).unwrap();
    let check = art_cli().arg("check").arg(&script).output().unwrap();
    assert!(check.status.success());
    let text = String::from_utf8_lossy(&check.stdout);
    for expected in [
        "motion in \"bad\" (4 frames, loops):",
        "command 1 (use \"dot\"): middle (10, 50) in frame 0 to (70, 50) in frame 3; biggest step 20 px (frame 0 to 1)",
        "command 3 (group \"steady\"): middle (52, 12) in frame 0 to (52, 14) in frame 3; biggest step 2 px",
        "animation \"bad\": the loop pops: command 1 (use \"dot\") moves 60 px from frame 3 back to frame 0 (its biggest step inside the loop is 20 px)",
        "animation \"bad\": command 2 (group \"jumper\") jumps 85 px between frames 1 and 2",
    ] {
        assert!(
            text.contains(expected),
            "{expected:?} missing from:\n{text}"
        );
    }
    assert!(
        !text.contains("steady\") jumps") && !text.contains("pops: command 3"),
        "{text}"
    );

    // The frame sheet, onion skin and GIF are saved with their names.
    let render = |extra: &[&str]| {
        let output = art_cli()
            .arg("render")
            .arg(&script)
            .args(["--out", out.to_str().unwrap()])
            .args(extra)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    render(&["--frame-sheet", "2"]);
    assert!(out.join("motion-bad-frames.png").exists());
    render(&["--gif"]);
    let gif = std::fs::read(out.join("motion-bad.gif")).unwrap();
    assert!(gif.starts_with(b"GIF89a"));
    assert_eq!(
        gif.windows(3).filter(|w| w == b"\x21\xf9\x04").count(),
        4,
        "four frames"
    );
    render(&["--frame", "0", "--onion"]);
    assert!(out.join("motion-bad-000-onion.png").exists());
    let usage = art_cli()
        .arg("render")
        .arg(&script)
        .args(["--out", out.to_str().unwrap(), "--onion"])
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
    std::fs::remove_dir_all(out).ok();
}

/// Symbols and colours used only in later frames or other animations,
/// and a part that leaves the canvas for frames 2 to 4.
const LATER_USE: &str = r##"{ "palette": { "ink": "#202020", "red": "#E02020", "blue": "#2020E0" }, "commands": [
    { "cmd": "canvas", "width": 50, "height": 50 },
    { "cmd": "define", "name": "open", "commands": [ { "cmd": "rect", "x": 0, "y": 0, "width": 4, "height": 4, "fill": "ink" } ] },
    { "cmd": "define", "name": "blink", "commands": [ { "cmd": "rect", "x": 0, "y": 0, "width": 4, "height": 1, "fill": "ink" } ] },
    { "cmd": "define", "name": "never", "commands": [ { "cmd": "rect", "x": 0, "y": 0, "width": 4, "height": 1, "fill": "blue" } ] },
    { "cmd": "define", "name": "face", "params": { "eye": "open" }, "commands": [
        { "cmd": "use", "name": "$eye", "translate": [10, 10] } ] }
  ], "animations": {
    "idle": { "frames": 6, "commands": [
        { "cmd": "use", "name": "face", "with": { "eye": { "keys": [[0, "open"], [5, "blink"]] } } },
        { "cmd": "group", "name": "flyer", "translate": { "keys": [[0, [20, 20]], [2, [80, 80], "step"], [5, [20, 20], "step"]] }, "commands": [
            { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "ink" } ] },
        { "cmd": "group", "name": "lost", "translate": [90, 90], "commands": [
            { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "ink" } ] },
        { "cmd": "group", "name": "masked", "mask": { "circle": [{ "keys": [[0, 25], [2, 200, "step"], [5, 25, "step"]] }, 25, 5] }, "commands": [
            { "cmd": "rect", "x": 20, "y": 20, "width": 5, "height": 5, "fill": "ink" } ] } ] },
    "hurt": { "frames": 1, "commands": [
        { "cmd": "rect", "x": 20, "y": 20, "width": 4, "height": 4, "fill": "red" } ] }
  } }"##;

#[test]
fn check_counts_use_over_every_frame_and_says_which_frames_warn() {
    let out = folder("later-use");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("later.json");
    std::fs::write(&script, LATER_USE).unwrap();
    let check = |extra: &[&str]| {
        let output = art_cli()
            .arg("check")
            .arg(&script)
            .args(extra)
            .output()
            .unwrap();
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    let (code, text, _) = check(&[]);
    assert_eq!(code, Some(0));
    // "blink" (frame 5) and "red" (the other animation) are used; "never" and "blue" aren't.
    assert!(!text.contains("\"blink\""), "{text}");
    assert!(!text.contains("colour \"red\""), "{text}");
    assert!(
        text.contains("symbol \"never\" (command 4) is defined but never used"),
        "{text}"
    );
    assert!(
        text.contains("palette colour \"blue\" is never used"),
        "{text}"
    );
    // The flyer is off the canvas in frames 2 to 4 but on it in the
    // others: an exit and a return, not a mistake. "lost" never is on it.
    assert!(
        !text.contains("\"flyer\") > 1 (rect) draws entirely outside"),
        "{text}"
    );
    assert!(
        text.contains("animation \"idle\", frames 0-5: command 3 (group \"lost\") > 1 (rect) draws entirely outside the canvas (x 90..95, y 90..95)"),
        "{text}"
    );
    assert_eq!(text.matches("draws entirely outside").count(), 1, "{text}");
    // A warning found in some frames says which, as a range.
    assert!(
        text.contains(
            "animation \"idle\", frames 2-4: command 4 (group \"masked\"): its mask covers nothing"
        ),
        "{text}"
    );
    // Which frame the summary shows, and how to pick another.
    assert!(text.contains("(the layers and parts below are frame 0 of \"idle\"; --animation and --frame show another)"), "{text}");
    let (code, text, _) = check(&["--animation", "idle", "--frame", "3"]);
    assert_eq!(code, Some(0));
    assert!(text.contains("frame 3 of \"idle\""), "{text}");
    assert!(
        text.contains("command 2 (group \"flyer\"): x 80..85, y 80..85"),
        "{text}"
    );
    let (code, _, errors) = check(&["--animation", "idle", "--frame", "9"]);
    assert_eq!(code, Some(1));
    assert!(errors.contains("no frame 9"), "{errors}");
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn export_writes_the_sheet_its_json_frames_and_a_gif() {
    let out = folder("export");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/duck-animated.json");
    let output = art_cli()
        .args([
            "export",
            script.to_str().unwrap(),
            "--sheet",
            "--frames",
            "--gif",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("sheet 1024 x 256, 2 animations, 12 frames"),
        "{text}"
    );
    // The JSON says where the frames are, as docs/sprite-sheet-format.md promises.
    let json = std::fs::read_to_string(out.join("duck-animated.json")).unwrap();
    for expected in [
        "\"image\": \"duck-animated.png\"",
        "\"frame\": { \"width\": 128, \"height\": 128 }",
        "\"columns\": 8",
        "\"pivot\": [64.0,116.0]",
        "\"idle\": {\"row\":0,\"frames\":[0,1,2,3],\"fps\":6.0,\"loop\":true}",
        "\"walk\": {\"row\":1,\"frames\":[8,9,10,11,12,13,14,15],\"fps\":12.0,\"loop\":true}",
    ] {
        assert!(
            json.contains(expected),
            "{expected:?} missing from:\n{json}"
        );
    }
    assert!(out.join("duck-animated.png").exists());
    assert!(out.join("duck-animated-walk-007.png").exists());
    assert!(out.join("duck-animated-idle-003.png").exists());
    assert!(
        std::fs::read(out.join("duck-animated-walk.gif"))
            .unwrap()
            .starts_with(b"GIF89a")
    );

    // --scale enlarges with sharp pixels, and the JSON says so.
    let scaled = art_cli()
        .args([
            "export",
            script.to_str().unwrap(),
            "--animation",
            "walk",
            "--scale",
            "3",
            "--out",
        ])
        .arg(out.join("big"))
        .output()
        .unwrap();
    assert!(scaled.status.success());
    let json = std::fs::read_to_string(out.join("big/duck-animated.json")).unwrap();
    assert!(
        json.contains("\"frame\": { \"width\": 384, \"height\": 384 }"),
        "{json}"
    );
    assert!(json.contains("\"scale\": 3"), "{json}");
    assert!(json.contains("\"pivot\": [192.0,348.0]"), "{json}");
    assert!(!json.contains("\"idle\""), "only the walk");

    // An MP4 through ffmpeg, or the frames and the command when there's none.
    let video = art_cli()
        .args([
            "export",
            script.to_str().unwrap(),
            "--animation",
            "idle",
            "--mp4",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert!(video.status.success());
    let text = String::from_utf8_lossy(&video.stdout);
    let made = out.join("duck-animated-idle.mp4").exists();
    let told = text.contains("ffmpeg -y -loglevel error -framerate 6")
        && out.join("duck-animated-idle-mp4-frames/003.png").exists();
    assert!(made || told, "{text}");

    let picture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/sunset.json");
    let wrong = art_cli()
        .args(["export", picture.to_str().unwrap(), "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(wrong.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("no animations"));
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn measure_says_how_big_a_text_is_before_painting() {
    let output = art_cli()
        .args(["measure", "Duck Forge", "--size", "48", "--fit", "180"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("\"Duck Forge\" at size 48: "), "{text}");
    assert!(text.contains(" px (1 line, each 58.1 px high)"), "{text}");
    assert!(text.contains("to fit in 180 px, use size 33."), "{text}");
    let usage = art_cli().args(["measure"]).output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
}

/// Fixes from phase 2's reviews: `--pixels` prints a pixel canvas as
/// letters, `--scale` names the file by its size, `export` says what
/// it skipped, and a layer painted in some frame isn't "empty".
#[test]
fn review_fixes_pixels_scale_names_export_skips_and_frame_empty_layers() {
    let out = folder("review-fixes");
    std::fs::create_dir_all(&out).unwrap();
    let pixel = out.join("pixel.json");
    std::fs::write(
        &pixel,
        r##"{ "palette": { "ink": "#000000", "red": "#FF0000" }, "commands": [
          { "cmd": "canvas", "width": 6, "height": 3, "pixel": { "supersample": 1 } },
          { "cmd": "pixels", "at": [0, 0], "rows": ["kk....", "..rr..", "....kk"], "colors": { "k": "ink", "r": "red" } } ] }"##,
    )
    .unwrap();
    let output = art_cli()
        .args(["render", pixel.to_str().unwrap(), "--pixels", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("a = ink #000000"), "{text}");
    assert!(text.contains("b = red #FF0000"), "{text}");
    assert!(
        text.contains("\n    aa....\n    ..bb..\n    ....aa\n"),
        "{text}"
    );

    // --scale 0.5 saves name-50pct.png (not -preview).
    let output = art_cli()
        .args([
            "render",
            example().to_str().unwrap(),
            "--scale",
            "0.5",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("sunset-50pct.png"), "{text}");
    assert!(out.join("sunset-50pct.png").exists());

    // export --animation says which animations it left out.
    let animated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/duck-animated.json");
    let output = art_cli()
        .args([
            "export",
            animated.to_str().unwrap(),
            "--frames",
            "--animation",
            "walk",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("skipped (not --animation walk): idle"),
        "{text}"
    );

    // A layer that an animation paints in a later frame isn't empty.
    let entrance = out.join("entrance.json");
    std::fs::write(
        &entrance,
        r##"{ "commands": [
          { "cmd": "canvas", "width": 50, "height": 50 },
          { "cmd": "layer", "name": "Later" },
          { "cmd": "layer", "name": "Never" } ],
          "animations": { "drop": { "frames": 4, "commands": [
            { "cmd": "layer", "name": "Later" },
            { "cmd": "group", "name": "ball", "translate": { "keys": [[0, [25, -40]], [3, [25, 25]]] }, "commands": [
              { "cmd": "ellipse", "center": [0, 0], "radius": 5, "fill": "#000000" } ] } ] } } }"##,
    )
    .unwrap();
    let output = art_cli()
        .args(["check", entrance.to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(!text.contains("layer \"Later\" is empty"), "{text}");
    assert!(
        text.contains("layer \"Never\" is empty in every frame"),
        "{text}"
    );
    assert!(
        !text.contains("draws entirely outside"),
        "an entrance: {text}"
    );
    assert!(
        text.contains("Later: empty (animation command 2)"),
        "{text}"
    );
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn check_prints_the_intent_and_warns_about_a_focal_part_that_is_not_there() {
    let out = folder("intent");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("intent.json");
    std::fs::write(
        &script,
        r##"{ "palette": { "sky": "#8EC1E6", "sun": "#FFF1C0" },
          "intent": { "subject": "a duck at sunset", "focal": "duck", "light": "top right", "mood": "calm", "key_colors": ["sky", "sun"] },
          "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "sky" },
            { "cmd": "group", "name": "duck", "commands": [
              { "cmd": "ellipse", "center": [100, 50], "radius": 20, "fill": "sun" } ] } ] }"##,
    )
    .unwrap();
    let output = art_cli()
        .args(["check", script.to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("  intent:\n    subject: a duck at sunset\n    focal point: \"duck\"\n    light: from the top right (315 degrees)\n    mood: calm\n    key colours: sky, sun\n    focal point on the canvas: (100, 50)"), "{text}");
    assert!(!text.contains("warning"), "{text}");
    let text2 = std::fs::read_to_string(&script)
        .unwrap()
        .replace("\"focal\": \"duck\"", "\"focal\": \"goose\"");
    std::fs::write(&script, text2).unwrap();
    let output = art_cli()
        .args(["check", script.to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("names \"goose\", but no group or use is called that"),
        "{text}"
    );
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn views_saves_a_sheet_of_seven_or_single_views_and_never_changes_the_picture() {
    let out = folder("views");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("pond.json");
    std::fs::write(
        &script,
        r##"{ "palette": { "sky": "#8EC1E6", "sun": "#FFF1C0", "ink": "#202020" },
          "intent": { "subject": "a duck at sunset", "focal": "duck" },
          "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "sky" },
            { "cmd": "group", "name": "sun", "commands": [
              { "cmd": "ellipse", "center": [160, 30], "radius": 15, "fill": "sun" } ] },
            { "cmd": "group", "name": "duck", "commands": [
              { "cmd": "ellipse", "center": [60, 60], "radius": [20, 12], "fill": "ink" } ] } ] }"##,
    )
    .unwrap();
    let output = art_cli()
        .args(["views", script.to_str().unwrap(), "--out"])
        .arg(&out)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("saved ") && text.contains("pond-views.png"),
        "{text}"
    );
    assert!(
        text.contains("the intent's focal point is (60, 60)"),
        "{text}"
    );
    assert!(
        text.contains("silhouettes (outermost parts, in their colours): sun, duck"),
        "{text}"
    );
    assert!(text.contains("the eye goes to cell ("), "{text}");
    let sheet = std::fs::read(out.join("pond-views.png")).unwrap();
    assert!(!sheet.is_empty());
    assert!(
        !out.join("pond-gray.png").exists(),
        "no single views without a flag"
    );

    // One view alone, and the same every time.
    let single = || {
        let output = art_cli()
            .args([
                "views",
                script.to_str().unwrap(),
                "--gray",
                "--silhouette",
                "--out",
            ])
            .arg(&out)
            .output()
            .unwrap();
        assert!(output.status.success());
        (
            std::fs::read(out.join("pond-gray.png")).unwrap(),
            std::fs::read(out.join("pond-silhouette.png")).unwrap(),
        )
    };
    let first = single();
    assert_eq!(first, single(), "the same picture gives the same views");
    assert!(!out.join("pond-heat.png").exists());
    let second_sheet = art_cli()
        .args(["views", script.to_str().unwrap(), "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(second_sheet.status.success());
    assert_eq!(sheet, std::fs::read(out.join("pond-views.png")).unwrap());

    // Mistakes are explained.
    let output = art_cli()
        .args(["views", script.to_str().unwrap(), "--frame", "2"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("this script has no animations"), "{err}");
    let output = art_cli()
        .args(["views", script.to_str().unwrap(), "--sheet"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn critique_reports_values_focal_point_key_and_readability_with_numbers() {
    let out = folder("critique");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("flat.json");
    // A mid-grey picture with a faint disc: flat, and the disc merges.
    std::fs::write(
        &script,
        r##"{ "palette": { "paper": "#808080", "faint": "#8C8C8C", "orange": "#E08030", "green": "#30C040" },
          "intent": { "subject": "a test", "focal": "disc", "key_colors": ["orange", "paper"] },
          "commands": [
            { "cmd": "canvas", "width": 400, "height": 300, "paper": "paper" },
            { "cmd": "group", "name": "disc", "commands": [
              { "cmd": "ellipse", "center": [200, 150], "radius": 60, "fill": "faint" } ] },
            { "cmd": "group", "name": "leaf", "commands": [
              { "cmd": "ellipse", "center": [60, 60], "radius": 40, "fill": "green" } ] } ] }"##,
    )
    .unwrap();
    let output = art_cli()
        .args(["critique", script.to_str().unwrap(), "--detail"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("intent: a test"), "{text}");
    assert!(text.contains("the picture looks flat"), "{text}");
    assert!(
        text.contains("out of key: a saturated green at x "),
        "{text}"
    );
    assert!(text.contains("(leaf, "), "{text}");
    assert!(
        text.contains("readability: disc merges with its surroundings"),
        "{text}"
    );
    assert!(text.contains("the focal point is dead centre"), "{text}");
    assert!(text.contains("every part's numbers:"), "{text}");
    assert!(text.contains("Numbers can't judge beauty"), "{text}");
    let again = art_cli()
        .args(["critique", script.to_str().unwrap(), "--detail"])
        .output()
        .unwrap();
    // The same findings every time (the first line has the time taken).
    let findings = |text: &[u8]| {
        String::from_utf8_lossy(text)
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        findings(&output.stdout),
        findings(&again.stdout),
        "the same picture gives the same critique"
    );

    // Frames of an animation: one line each, and the worst named.
    let walk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/duck-animated.json");
    let output = art_cli()
        .args(["critique", walk.to_str().unwrap(), "--frames", "0-2"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("frame 0: ") && text.contains("frame 2: "),
        "{text}"
    );
    assert!(text.contains("worst frame: "), "{text}");
    let output = art_cli()
        .args(["critique", script.to_str().unwrap(), "--frames", "0-2"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no animations"));
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn palette_values_and_compare_work_on_pictures_and_scripts() {
    let out = folder("study");
    std::fs::create_dir_all(&out).unwrap();
    // A picture: half red, half blue, saved as a PNG by the renderer.
    let script = out.join("halves.json");
    std::fs::write(
        &script,
        r##"{ "palette": { "red": "#C82828", "blue": "#2828C8" }, "commands": [
            { "cmd": "canvas", "width": 64, "height": 32, "paper": "red" },
            { "cmd": "rect", "x": 32, "y": 0, "width": 32, "height": 32, "fill": "blue" } ] }"##,
    )
    .unwrap();
    let status = art_cli()
        .args(["render", script.to_str().unwrap(), "--out"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    let picture = out.join("halves.png");

    let output = art_cli()
        .args([
            "palette",
            picture.to_str().unwrap(),
            "--colors",
            "2",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("\"palette\": { \"c1\": \"#2828C8\", \"c2\": \"#C82828\" }"),
        "{text}"
    );
    assert!(text.contains("shares: c1 50%, c2 50%"), "{text}");
    assert!(out.join("halves-palette.png").exists());

    let output = art_cli()
        .args(["values", script.to_str().unwrap(), "--out"])
        .arg(&out)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("values of ") && text.contains("dark 100%"),
        "{text}"
    );
    assert!(text.contains("value group when squinted"), "{text}");
    assert!(out.join("halves-values.png").exists());

    let output = art_cli()
        .args([
            "compare",
            picture.to_str().unwrap(),
            script.to_str().unwrap(),
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("values similar: 100; colours similar: 100"),
        "{text}"
    );
    assert!(out.join("compare.png").exists());

    let output = art_cli()
        .args(["compare", picture.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let output = art_cli()
        .args(["palette", script.to_str().unwrap(), "--colors", "0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    std::fs::remove_dir_all(out).ok();
}

#[test]
fn the_blind_read_packet_leaks_nothing_and_the_record_is_written() {
    let out = folder("blind");
    std::fs::create_dir_all(&out).unwrap();
    let script = out.join("sunset-duck.json");
    std::fs::write(
        &script,
        r##"{ "title": "A duck at sunset", "palette": { "sky": "#F0A050", "ink": "#202020" },
          "intent": { "subject": "a duck at sunset", "focal": "duck", "mood": "calm" },
          "commands": [
            { "cmd": "canvas", "width": 120, "height": 80, "paper": "sky" },
            { "cmd": "group", "name": "duck", "commands": [
              { "cmd": "ellipse", "center": [60, 50], "radius": [18, 10], "fill": "ink" } ] } ] }"##,
    )
    .unwrap();
    let packets = out.join("packets");
    let output = art_cli()
        .args(["blind-read", script.to_str().unwrap(), "--out"])
        .arg(&packets)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    let packet = std::fs::read_dir(&packets)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let name = packet.file_name().unwrap().to_str().unwrap().to_owned();
    assert!(
        name.starts_with("blind-read-") && name.len() == 15,
        "{name}"
    );
    let code = &name[11..];
    let mut files: Vec<String> = std::fs::read_dir(&packet)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(
        files,
        [format!("piece-{code}.png"), "questions.txt".to_owned()]
    );
    let questions = std::fs::read_to_string(packet.join("questions.txt")).unwrap();
    assert!(questions.contains("What do you see?") && questions.contains("mood"));
    for leak in ["duck", "sunset", "script", "intent", "calm"] {
        assert!(
            !questions.to_lowercase().contains(leak),
            "the questions leak {leak:?}"
        );
        assert!(!name.contains(leak));
    }
    // The same picture gets the same name.
    let again = art_cli()
        .args(["blind-read", script.to_str().unwrap(), "--out"])
        .arg(&packets)
        .output()
        .unwrap();
    assert!(again.status.success());
    assert_eq!(std::fs::read_dir(&packets).unwrap().count(), 1);
    // Recording the answers writes blind-read.md next to the script.
    let answers = out.join("answers.txt");
    std::fs::write(
        &answers,
        "1. A dark oval on orange.\n2. The oval.\n3. Warm, quiet.\n",
    )
    .unwrap();
    let output = art_cli()
        .args([
            "blind-read",
            script.to_str().unwrap(),
            "--record",
            answers.to_str().unwrap(),
            "--verdict",
            "doesn't read: it saw an oval, not a duck",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    let record = std::fs::read_to_string(out.join("blind-read.md")).unwrap();
    assert!(record.contains("A dark oval on orange."));
    assert!(record.contains("subject: a duck at sunset"), "{record}");
    assert!(
        record.contains("**doesn't read: it saw an oval, not a duck**"),
        "{record}"
    );
    assert!(record.contains(&format!("piece-{code}.png")));
    let output = art_cli()
        .args([
            "blind-read",
            script.to_str().unwrap(),
            "--record",
            answers.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "--record needs --verdict");
    std::fs::remove_dir_all(out).ok();
}
