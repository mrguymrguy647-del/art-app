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
