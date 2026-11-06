//! Values that change with the frame: keyframes and easing.
//!
//! In a script, any number of a `group` or `use` (and a symbol
//! parameter) can be written as keyframes instead:
//!
//! ```json
//! { "keys": [[0, 0], [4, 20], [8, 0]], "ease": "in-out", "shift": 4 }
//! ```
//!
//! `keys` are `[frame, value]` pairs (a third element names the easing
//! into that key). Numbers, lists of numbers and colours blend between
//! keys; anything else holds until the next key. `shift` plays the same
//! keys that many frames later, wrapping round in a looping animation.
//! See `docs/animation-design.md`.

use serde_json::Value;

use crate::fill::Rgba;

/// Where an animation is: which frame, and how long it is (frames, and
/// whether it loops), if it's an animation at all.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clock {
    pub frame: u32,
    pub length: Option<(u32, bool)>,
    /// Frames per second, for keys written in seconds (`"1.5s"`).
    pub fps: f32,
    /// Plain numbers in keys are seconds (the animation's `"time"`).
    pub seconds: bool,
}

impl Clock {
    /// A single picture: frame 0, nothing else.
    pub fn still() -> Self {
        Clock {
            frame: 0,
            length: None,
            fps: 12.0,
            seconds: false,
        }
    }

    /// A key's time as a frame number: a number (frames, or seconds if
    /// the animation counts in seconds) or text like `"1.5s"` (seconds).
    fn key_frame(&self, value: &Value) -> Result<f32, String> {
        match value {
            Value::Number(n) => {
                let v = n.as_f64().unwrap_or(0.0) as f32;
                Ok(if self.seconds { v * self.fps } else { v })
            }
            Value::String(text) => {
                let seconds: f32 = text
                    .strip_suffix('s')
                    .and_then(|t| t.trim().parse().ok())
                    .ok_or_else(|| {
                        format!("a key's time is a frame number or seconds like \"1.5s\", not \"{text}\"")
                    })?;
                Ok(seconds * self.fps)
            }
            _ => Err("each key starts with its frame number (or seconds, like \"1.5s\")".into()),
        }
    }
}

/// The named easing curves: how far along (0 to 1) a value is at time
/// `t` (0 to 1) between two keys.
pub fn ease(name: &str, t: f32) -> Result<f32, String> {
    let t = t.clamp(0.0, 1.0);
    Ok(match name {
        "linear" => t,
        "in" => t * t,
        "out" => 1.0 - (1.0 - t) * (1.0 - t),
        "in-out" => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - 2.0 * (1.0 - t) * (1.0 - t)
            }
        }
        "step" => 0.0,
        other => {
            return Err(format!(
                "`ease` is \"linear\", \"in\", \"out\", \"in-out\" or \"step\", not \"{other}\""
            ));
        }
    })
}

/// True if `value` is written as keyframes.
pub fn is_keyed(value: &Value) -> bool {
    value.as_object().is_some_and(|o| o.contains_key("keys"))
}

/// The value of keyframes `keyed` at the clock's frame. `inner` resolves
/// each key's value first (so a key can be a parameter), and `color`
/// says what a colour name means, so colours can blend.
pub fn evaluate(
    keyed: &Value,
    clock: Clock,
    inner: &dyn Fn(&Value) -> Result<Value, String>,
    color: &dyn Fn(&str) -> Option<Rgba>,
) -> Result<Value, String> {
    let object = keyed
        .as_object()
        .ok_or("keyframes are an object like {\"keys\": [[0, 0], [8, 20]]}")?;
    for field in object.keys() {
        if !matches!(field.as_str(), "keys" | "ease" | "shift") {
            return Err(format!(
                "keyframes have `keys`, `ease` and `shift`, not `{field}`"
            ));
        }
    }
    let default_ease = match object.get("ease") {
        None => "linear".to_owned(),
        Some(Value::String(name)) => {
            ease(name, 0.0)?;
            name.clone()
        }
        Some(_) => return Err("`ease` is the name of a curve, like \"in-out\"".into()),
    };
    let shift = match object.get("shift") {
        None => 0.0,
        Some(value @ (Value::Number(_) | Value::String(_))) => clock
            .key_frame(value)
            .map_err(|_| "`shift` is a number of frames (or seconds, like \"0.5s\")")?,
        Some(_) => return Err("`shift` is a number of frames".into()),
    };
    let Some(Value::Array(keys)) = object.get("keys") else {
        return Err("`keys` is a list of [frame, value] pairs".into());
    };
    if keys.is_empty() {
        return Err("`keys` needs at least one [frame, value] pair".into());
    }
    let mut parsed: Vec<(f32, Value, String)> = Vec::with_capacity(keys.len());
    for key in keys {
        let Value::Array(parts) = key else {
            return Err("each key is [frame, value] (or [frame, value, \"ease\"])".into());
        };
        let frame = match parts.first() {
            Some(value) => clock.key_frame(value)?,
            None => return Err("each key starts with its frame number".into()),
        };
        let value = parts
            .get(1)
            .ok_or("each key is [frame, value]: the value is missing")?;
        let easing = match parts.get(2) {
            None => default_ease.clone(),
            Some(Value::String(name)) => {
                ease(name, 0.0)?;
                name.clone()
            }
            Some(_) => return Err("a key's third element is the easing's name".into()),
        };
        if parts.len() > 3 {
            return Err("a key is [frame, value] or [frame, value, \"ease\"], nothing more".into());
        }
        if let Some((last, _, _)) = parsed.last()
            && frame <= *last
        {
            return Err(format!(
                "keys must be in frame order, each later than the one before ({last} then {frame})"
            ));
        }
        parsed.push((frame, inner(value)?, easing));
    }

    // Where in the keys this frame is.
    let mut time = clock.frame as f32 - shift;
    if let Some((frames, true)) = clock.length {
        time = time.rem_euclid(frames as f32);
    }
    let (first, _, _) = &parsed[0];
    if time <= *first {
        return Ok(parsed[0].1.clone());
    }
    for pair in parsed.windows(2) {
        let (from, a, _) = &pair[0];
        let (to, b, easing) = &pair[1];
        if time < *to {
            let t = ease(easing, (time - from) / (to - from))?;
            // Exactly on a key: that key's value, as written.
            if t <= 0.0 {
                return Ok(a.clone());
            }
            return Ok(blend(a, b, t, color).unwrap_or_else(|| a.clone()));
        }
    }
    Ok(parsed[parsed.len() - 1].1.clone())
}

/// `a` and `b` mixed, `t` of the way from `a` to `b`: numbers, lists of
/// them, and colours. `None` for anything else (it holds instead).
fn blend(a: &Value, b: &Value, t: f32, color: &dyn Fn(&str) -> Option<Rgba>) -> Option<Value> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64()?, y.as_f64()?);
            let mixed = x + (y - x) * f64::from(t);
            // Written with few decimals, so values stay readable in messages.
            Some(Value::from((mixed * 10_000.0).round() / 10_000.0))
        }
        (Value::Array(xs), Value::Array(ys)) if xs.len() == ys.len() => xs
            .iter()
            .zip(ys)
            .map(|(x, y)| blend(x, y, t, color))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        (Value::String(x), Value::String(y)) => {
            let (x, y) = (color(x)?, color(y)?);
            let channel = |i: usize| {
                let (p, q) = (f32::from(x[i]), f32::from(y[i]));
                (p + (q - p) * t).round().clamp(0.0, 255.0) as u8
            };
            let [r, g, b, a] = [channel(0), channel(1), channel(2), channel(3)];
            Some(Value::String(if a == 255 {
                format!("#{r:02X}{g:02X}{b:02X}")
            } else {
                format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
            }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(keyed: Value, frame: u32, length: Option<(u32, bool)>) -> Result<Value, String> {
        let plain = |v: &Value| Ok(v.clone());
        let color = |name: &str| crate::script::parse_hex(name).ok();
        evaluate(
            &keyed,
            Clock {
                frame,
                length,
                fps: 12.0,
                seconds: false,
            },
            &plain,
            &color,
        )
    }

    #[test]
    fn easings_match_their_formulas() {
        assert_eq!(ease("linear", 0.3).unwrap(), 0.3);
        assert_eq!(ease("in", 0.5).unwrap(), 0.25);
        assert_eq!(ease("out", 0.5).unwrap(), 0.75);
        assert_eq!(ease("in-out", 0.25).unwrap(), 0.125);
        assert_eq!(ease("in-out", 0.75).unwrap(), 0.875);
        assert_eq!(ease("in-out", 0.5).unwrap(), 0.5);
        assert_eq!(ease("step", 0.9).unwrap(), 0.0);
        for name in ["linear", "in", "out", "in-out"] {
            assert_eq!(ease(name, 0.0).unwrap(), 0.0, "{name}");
            assert_eq!(ease(name, 1.0).unwrap(), 1.0, "{name}");
        }
        assert!(ease("bounce", 0.5).unwrap_err().contains("not \"bounce\""));
    }

    #[test]
    fn values_blend_between_keys_and_hold_outside() {
        let keyed = json!({ "keys": [[0, 0], [4, 20], [8, 0]] });
        assert_eq!(at(keyed.clone(), 2, None).unwrap(), json!(10.0));
        assert_eq!(at(keyed.clone(), 4, None).unwrap(), json!(20));
        assert_eq!(at(keyed.clone(), 7, None).unwrap(), json!(5.0));
        assert_eq!(
            at(keyed.clone(), 12, None).unwrap(),
            json!(0),
            "after the last key"
        );
        let late = json!({ "keys": [[4, 20], [8, 0]] });
        assert_eq!(
            at(late, 1, None).unwrap(),
            json!(20),
            "before the first key"
        );
        // Lists element by element, and easing.
        let moved = json!({ "keys": [[0, [0, 100]], [10, [50, 0]]], "ease": "in" });
        assert_eq!(at(moved, 5, None).unwrap(), json!([12.5, 75.0]));
        let per_key = json!({ "keys": [[0, 0], [10, 100, "step"], [20, 200]] });
        assert_eq!(
            at(per_key.clone(), 9, None).unwrap(),
            json!(0),
            "step holds"
        );
        assert_eq!(at(per_key, 15, None).unwrap(), json!(150.0));
        // Colours channel by channel; other strings hold.
        let colors = json!({ "keys": [[0, "#000000"], [2, "#FF0080"]] });
        assert_eq!(at(colors, 1, None).unwrap(), json!("#800040"));
        let names = json!({ "keys": [[0, "closed"], [3, "open"], [5, "closed"]] });
        assert_eq!(at(names.clone(), 2, None).unwrap(), json!("closed"));
        assert_eq!(at(names, 3, None).unwrap(), json!("open"));
    }

    #[test]
    fn shift_plays_the_keys_later_and_wraps_in_loops() {
        let keyed = json!({ "keys": [[0, 0], [4, 20], [8, 0]], "shift": 4 });
        // Frame 6 is the keys' frame 2.
        assert_eq!(at(keyed.clone(), 6, Some((8, true))).unwrap(), json!(10.0));
        // Frame 2 is the keys' frame -2: wrapped to 6 in an 8-frame loop...
        assert_eq!(at(keyed.clone(), 2, Some((8, true))).unwrap(), json!(10.0));
        // ...and held at the first key without a loop.
        assert_eq!(at(keyed, 2, Some((8, false))).unwrap(), json!(0));
    }

    #[test]
    fn keyframe_mistakes_are_explained() {
        let problem = |keyed: Value| at(keyed, 0, None).unwrap_err();
        assert!(problem(json!({ "keys": [] })).contains("at least one"));
        assert!(problem(json!({ "keys": [[4, 1], [2, 0]] })).contains("frame order"));
        assert!(problem(json!({ "keys": [[0, 1]], "speed": 2 })).contains("not `speed`"));
        assert!(problem(json!({ "keys": [[0, 1]], "ease": "fast" })).contains("not \"fast\""));
        assert!(problem(json!({ "keys": [[0]] })).contains("value is missing"));
        assert!(problem(json!({ "keys": [["a", 1]] })).contains("frame number"));
    }
}
