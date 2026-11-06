//! Timings on a big picture: 4000 x 3000 with 20 painted layers, the size
//! Duck Quill must stay quick at. Not run with the other tests (they'd be
//! slow in a debug build); run it with:
//!
//!   cargo test --release -p quill_engine --test big_canvas -- --ignored --nocapture
//!
//! It prints how long each thing takes, so a change that costs speed shows.

use std::time::Instant;

use quill_engine::brush::{BrushMode, BrushSettings};
use quill_engine::document::{Background, Document};
use quill_engine::fill::{FloodFill, Paint, SampleFrom, flood_fill_within, paint_mask};
use quill_engine::filters::{self, Filter};
use quill_engine::image::Image;
use quill_engine::layer_commands::LayerPresenceCommand;
use quill_engine::selection::{self, Selection};
use quill_engine::stroke::{Stroke, StrokePoint};

const WIDTH: u32 = 4000;
const HEIGHT: u32 = 3000;
const LAYERS: usize = 20;

fn time<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = f();
    println!(
        "{what:<48} {:>8.1} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
    result
}

/// 20 layers, each with a big soft shape on it, so every layer has tiles.
fn big_document() -> Document {
    let mut document = Document::new(WIDTH, HEIGHT);
    let limits = document.bounds();
    for i in 0..LAYERS {
        if i > 0 {
            LayerPresenceCommand::add_layer(&mut document);
        }
        let x = (i as f32 * 180.0) % 3000.0;
        let shape = selection::ellipse(500.0 + x, 1500.0, 600.0, 900.0, limits);
        let colour = [(i * 12) as u8, 80, 160, 255];
        paint_mask(
            &mut document.active_layer_mut().surface,
            &shape,
            &Paint::Color(colour),
            0.5,
            BrushMode::Paint,
            limits,
        );
    }
    document
}

/// A long, wavy stroke across the canvas, as the pen would give it.
fn long_stroke(document: &mut Document, clip: Option<std::sync::Arc<quill_engine::fill::Mask>>) {
    let brush = BrushSettings {
        size: 60.0,
        ..BrushSettings::default()
    };
    let mut stroke = Stroke::new(brush, document.bounds());
    if let Some(clip) = clip {
        stroke = stroke.clipped(clip);
    }
    let surface = &mut document.active_layer_mut().surface;
    for i in 0..=2000 {
        let t = i as f32 / 2000.0;
        let point = StrokePoint::new(200.0 + 3600.0 * t, 1500.0 + 800.0 * (t * 12.0).sin(), 0.8);
        stroke.add_point(surface, point);
    }
    stroke.finish(surface);
}

#[test]
#[ignore = "timings: run with --release --ignored --nocapture"]
fn timings_on_a_4000_by_3000_picture_with_20_layers() {
    let mut document = time("make 20 painted layers", big_document);
    let limits = document.bounds();
    time("long stroke, size 60, no selection", || {
        long_stroke(&mut document, None)
    });
    // The same blur without a selection, on a copy, to compare.
    let mut plain = document.clone();
    time("blur 8 px, no selection", || {
        filters::apply(
            &mut plain.active_layer_mut().surface,
            &[Filter::Blur(8.0)],
            limits,
        )
    });
    drop(plain);
    let selection = time("select a big ellipse", || {
        Selection::new(selection::ellipse(2000.0, 1500.0, 1500.0, 1000.0, limits)).unwrap()
    });
    let feathered = time("feather it by 30 px", || {
        selection.feathered(30.0, limits).unwrap()
    });
    time("its outline (marching ants)", || feathered.outline().len());
    document.set_selection(Some(feathered));
    let clip = document.clip();
    time("long stroke, size 60, inside the selection", || {
        long_stroke(&mut document, clip.clone())
    });
    let fill = FloodFill {
        x: 2000.0,
        y: 1500.0,
        tolerance: 0.1,
        sample: SampleFrom::Layer,
        expand: 1,
    };
    let layer = document.active_id();
    time("paint bucket inside the selection", || {
        flood_fill_within(
            &mut document,
            layer,
            &fill,
            &Paint::Color([255, 0, 0, 255]),
            1.0,
            clip.as_deref(),
        )
    });
    time("blur 8 px inside the selection", || {
        filters::apply_within(
            &mut document.active_layer_mut().surface,
            &[Filter::Blur(8.0)],
            limits,
            clip.as_deref(),
        )
    });
    time("copy the selection", || {
        selection::copy(
            &document.active_layer().surface,
            document.selection().unwrap(),
        )
    });
    time("the whole picture, all 20 layers combined", || {
        Image::of_document(&document, Background::Paper)
    });
    time("a copy of the document (as autosave would)", || {
        document.clone()
    });
}
