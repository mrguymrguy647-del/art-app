//! Faster animation must change nothing: every frame painted with the
//! static layers reused is the same picture, pixel for pixel, as the
//! frame painted in full, on every example animation and the gallery
//! loop; and the report says the same things.

use std::path::{Path, PathBuf};

use art_engine::document::Background;
use art_engine::image::Image;
use art_engine::script::{PlayOptions, Player, Script, Sequence};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn png(document: &art_engine::document::Document) -> Vec<u8> {
    let mut bytes = Vec::new();
    Image::of_document(document, Background::Transparent)
        .write_png(&mut bytes, true)
        .unwrap();
    bytes
}

/// Frames `which` of every animation in `path`, with and without reuse.
fn check(path: &Path, scale: f32, which: &[usize]) -> usize {
    let text = std::fs::read_to_string(path).unwrap();
    let script = Script::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let folder = path.parent().map(Path::to_path_buf);
    let mut skipped = 0;
    for (name, animation) in script.animations.iter() {
        let options = PlayOptions {
            scale,
            folder: folder.clone(),
            animation: Some(name.clone()),
            ..PlayOptions::default()
        };
        let mut sequence = Sequence::new(script.clone(), options.clone());
        let last = animation.frames - 1;
        let frames: Vec<u32> = which
            .iter()
            .map(|&i| (i as u32).min(last))
            .chain([last])
            .collect();
        for frame in frames {
            let (fast, fast_report) = sequence
                .paint(frame, |_, _| {})
                .unwrap_or_else(|e| panic!("{} frame {frame}: {e}", path.display()));
            let mut player = Player::new(
                script.clone(),
                PlayOptions {
                    frame,
                    ..options.clone()
                },
            )
            .unwrap();
            let mut full = player.create_document().unwrap();
            player.run(&mut full, |_, _| {}).unwrap();
            assert_eq!(
                png(&fast),
                png(&full),
                "{} \"{name}\" frame {frame}: the reused frame differs",
                path.display()
            );
            let full_report = player.report();
            assert_eq!(
                fast_report.parts,
                full_report.parts,
                "{} \"{name}\" frame {frame}: the parts differ",
                path.display()
            );
            assert_eq!(
                fast_report.painted_by,
                full_report.painted_by,
                "{} \"{name}\" frame {frame}: painted_by differs",
                path.display()
            );
            assert_eq!(fast_report.warnings, full_report.warnings);
        }
        skipped += sequence.reuse().map_or(0, |r| r.skip.len());
    }
    skipped
}

#[test]
fn every_example_animation_is_the_same_with_reuse() {
    let mut animated = 0;
    for entry in std::fs::read_dir(root().join("examples")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "json")
            && std::fs::read_to_string(&path)
                .unwrap()
                .contains("\"animations\"")
        {
            animated += 1;
            check(&path, 1.0, &[0, 1, 3]);
        }
    }
    assert!(animated >= 3, "{animated} animated examples");
}

#[test]
fn the_gallery_loop_is_the_same_with_reuse_and_reuses_most_of_it() {
    let path = root().join("skills/art-app/gallery/sunset-pond/script.json");
    let skipped = check(&path, 0.25, &[0, 7, 23]);
    // The hills, bank, pads and finishing layers don't move: most of
    // the script's commands are painted once.
    assert!(skipped >= 10, "only {skipped} commands skipped");
}
