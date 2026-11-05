//! Sprite sheets: every frame of every animation in one picture, with a
//! small JSON file saying where each frame is, so a game (like Duck
//! Forge) can cut the sheet and play the animations.
//!
//! Layout: one row per animation, frames left to right, every cell the
//! size of one frame (the canvas, times `scale`), rows padded on the
//! right. Frames are numbered across the grid, row by row, and each
//! animation lists its frame numbers. The format is described in
//! `docs/sprite-sheet-format.md`.

use serde::Serialize;

use crate::image::Image;

/// One animation's frames, ready for the sheet. All frames are the
/// canvas size.
pub struct SheetAnimation {
    pub name: String,
    pub frames: Vec<Image>,
    pub fps: f32,
    pub looping: bool,
}

/// What the JSON file says.
#[derive(Serialize)]
struct Metadata<'a> {
    image: &'a str,
    frame: Size,
    columns: u32,
    rows: u32,
    scale: u32,
    pivot: [f32; 2],
    animations: Vec<(&'a str, AnimationEntry)>,
}

#[derive(Serialize)]
struct Size {
    width: u32,
    height: u32,
}

#[derive(Serialize)]
struct AnimationEntry {
    row: u32,
    frames: Vec<u32>,
    fps: f32,
    #[serde(rename = "loop")]
    looping: bool,
}

/// Lays the animations out as a sheet and describes it. `image_name` is
/// the sheet's file name, written into the JSON; `pivot` is in canvas
/// pixels (it's scaled with the frames); `scale` enlarges with sharp
/// pixels (1 = as painted). Returns the sheet and the JSON text.
pub fn sprite_sheet(
    animations: &[SheetAnimation],
    image_name: &str,
    pivot: [f32; 2],
    scale: u32,
) -> Result<(Image, String), String> {
    let scale = scale.max(1);
    let first = animations
        .iter()
        .find_map(|a| a.frames.first())
        .ok_or("a sprite sheet needs at least one frame")?;
    let (width, height) = (first.width() * scale, first.height() * scale);
    let columns = animations.iter().map(|a| a.frames.len()).max().unwrap_or(0) as u32;
    let rows = animations.len() as u32;
    let mut sheet = Image::new(columns * width, rows * height, [0, 0, 0, 0]);
    let mut entries = Vec::new();
    for (row, animation) in animations.iter().enumerate() {
        let mut numbers = Vec::with_capacity(animation.frames.len());
        for (column, frame) in animation.frames.iter().enumerate() {
            if frame.width() * scale != width || frame.height() * scale != height {
                return Err(format!(
                    "every frame must be the same size: \"{}\" frame {column} is {} x {}, \
                     the first is {} x {}",
                    animation.name,
                    frame.width(),
                    frame.height(),
                    first.width(),
                    first.height()
                ));
            }
            let enlarged;
            let frame = if scale > 1 {
                enlarged = frame.magnify(scale);
                &enlarged
            } else {
                frame
            };
            sheet.paste(
                frame,
                (column as u32 * width) as i32,
                (row as u32 * height) as i32,
            );
            numbers.push(row as u32 * columns + column as u32);
        }
        entries.push((
            animation.name.as_str(),
            AnimationEntry {
                row: row as u32,
                frames: numbers,
                fps: animation.fps,
                looping: animation.looping,
            },
        ));
    }
    let metadata = Metadata {
        image: image_name,
        frame: Size { width, height },
        columns,
        rows,
        scale,
        pivot: [pivot[0] * scale as f32, pivot[1] * scale as f32],
        animations: entries,
    };
    Ok((sheet, metadata_json(&metadata)))
}

/// The JSON, one animation per line, so it's easy to read.
fn metadata_json(metadata: &Metadata) -> String {
    let entry = |value: &dyn erased::Json| value.json();
    let mut out = String::from("{\n");
    out += &format!("  \"image\": {},\n", entry(&metadata.image));
    out += &format!(
        "  \"frame\": {{ \"width\": {}, \"height\": {} }},\n",
        metadata.frame.width, metadata.frame.height
    );
    out += &format!("  \"columns\": {},\n", metadata.columns);
    out += &format!("  \"rows\": {},\n", metadata.rows);
    out += &format!("  \"scale\": {},\n", metadata.scale);
    out += &format!("  \"pivot\": {},\n", entry(&metadata.pivot));
    out += "  \"animations\": {\n";
    let count = metadata.animations.len();
    for (i, (name, animation)) in metadata.animations.iter().enumerate() {
        out += &format!(
            "    {}: {}{}\n",
            entry(name),
            entry(animation),
            if i + 1 < count { "," } else { "" }
        );
    }
    out += "  }\n}\n";
    out
}

/// A small helper so values of different types can be written as JSON
/// through one closure.
mod erased {
    pub trait Json {
        fn json(&self) -> String;
    }

    impl<T: serde::Serialize> Json for T {
        fn json(&self) -> String {
            serde_json::to_string(self).unwrap_or_else(|_| "null".to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(color: [u8; 4]) -> Image {
        let mut image = Image::new(4, 3, [0, 0, 0, 0]);
        image.set_pixel(1, 1, color);
        image
    }

    #[test]
    fn frames_sit_in_rows_and_the_json_says_where() {
        let animations = [
            SheetAnimation {
                name: "idle".into(),
                frames: vec![frame([255, 0, 0, 255]), frame([0, 255, 0, 255])],
                fps: 8.0,
                looping: true,
            },
            SheetAnimation {
                name: "walk".into(),
                frames: vec![frame([0, 0, 255, 255]); 3],
                fps: 12.0,
                looping: false,
            },
        ];
        let (sheet, json) = sprite_sheet(&animations, "duck.png", [2.0, 3.0], 1).unwrap();
        assert_eq!((sheet.width(), sheet.height()), (12, 6));
        assert_eq!(sheet.pixel(1, 1), [255, 0, 0, 255]);
        assert_eq!(sheet.pixel(5, 1), [0, 255, 0, 255]);
        assert_eq!(sheet.pixel(9, 4), [0, 0, 255, 255]);
        assert_eq!(sheet.pixel(9, 1), [0, 0, 0, 0], "the short row is padded");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["image"], "duck.png");
        assert_eq!(value["frame"]["width"], 4);
        assert_eq!(value["columns"], 3);
        assert_eq!(value["pivot"], serde_json::json!([2.0, 3.0]));
        assert_eq!(
            value["animations"]["idle"]["frames"],
            serde_json::json!([0, 1])
        );
        assert_eq!(
            value["animations"]["walk"]["frames"],
            serde_json::json!([3, 4, 5])
        );
        assert_eq!(value["animations"]["walk"]["fps"], 12.0);
        assert_eq!(value["animations"]["idle"]["loop"], true);
        assert_eq!(value["animations"]["walk"]["loop"], false);
    }

    #[test]
    fn scale_enlarges_frames_and_the_pivot_with_sharp_pixels() {
        let animations = [SheetAnimation {
            name: "a".into(),
            frames: vec![frame([255, 0, 0, 255])],
            fps: 1.0,
            looping: false,
        }];
        let (sheet, json) = sprite_sheet(&animations, "a.png", [1.5, 1.0], 4).unwrap();
        assert_eq!((sheet.width(), sheet.height()), (16, 12));
        assert_eq!(sheet.pixel(4, 4), [255, 0, 0, 255]);
        assert_eq!(sheet.pixel(7, 7), [255, 0, 0, 255]);
        assert_eq!(sheet.pixel(8, 4), [0, 0, 0, 0]);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["scale"], 4);
        assert_eq!(value["frame"]["width"], 16);
        assert_eq!(value["pivot"], serde_json::json!([6.0, 4.0]));
    }

    #[test]
    fn frames_of_different_sizes_are_refused() {
        let animations = [SheetAnimation {
            name: "a".into(),
            frames: vec![frame([255, 0, 0, 255]), Image::new(5, 3, [0, 0, 0, 0])],
            fps: 1.0,
            looping: false,
        }];
        let error = sprite_sheet(&animations, "a.png", [0.0, 0.0], 1).unwrap_err();
        assert!(error.contains("frame 1 is 5 x 3"), "{error}");
        assert!(sprite_sheet(&[], "a.png", [0.0, 0.0], 1).is_err());
    }
}
