//! Faster animation: a layer that only frame-independent commands paint
//! on is painted once and copied into every other frame.
//!
//! A command is *frame-dependent* when it, or anything inside it, has
//! keyframes, or when it belongs to the animation's own commands. A
//! layer is *dynamic* when a frame-dependent command paints on it, or
//! when a command that runs every frame reads it at paint time (a
//! bucket fill, a `layer` mask, a `"from": "below"` layer) before its
//! last painter has finished (a command that reads a dynamic layer is
//! dynamic in turn). Everything else is *static*. The first frame of a
//! [`Sequence`] is painted in full; for each later frame the static
//! commands that paint only on static layers are skipped, and each
//! static layer's pixels are copied in from the first frame right after
//! the point where its last painter would have finished, so a command
//! that reads it later sees what it saw in the first frame. Commands
//! that set state (brush, layer selection, define) still run, so
//! everything after sees the same state. The pixels are exactly the
//! same by construction, and tests prove it.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use super::animate;
use super::player::{Event, Part, PlayOptions, Player, Report};
use super::{Command, Script, ScriptError};
use crate::document::Document;
use crate::layer::LayerId;
use crate::surface::Surface;

/// What one full frame taught about a script: which layers never
/// change, which top-level commands can be skipped, and the pixels (and
/// report entries) they'd have made.
#[derive(Clone, Debug, Default)]
pub struct Reuse {
    /// The layers no frame-dependent command paints on or reads.
    pub static_layers: HashSet<LayerId>,
    /// The script's own commands (numbered from 1, as `check` prints
    /// them) that paint only on static layers.
    pub skip: BTreeSet<usize>,
    /// The static layers' pixels, from the frame painted in full.
    pub surfaces: HashMap<LayerId, Surface>,
    /// For each static layer, the number of the last of the script's
    /// own commands that painted on it: its pixels go in once that
    /// command is done (0: before anything).
    pub points: HashMap<LayerId, usize>,
    /// The report's parts that skipped commands made, with the number of
    /// the command each started in.
    pub parts: Vec<(usize, Part)>,
    /// `painted_by` entries of skipped commands.
    pub painted_by: HashMap<LayerId, BTreeSet<usize>>,
    /// Warnings skipped commands raised.
    pub warnings: Vec<String>,
}

/// True if the command, or anything inside it, has keyframes.
pub fn is_frame_dependent(command: &Command) -> bool {
    fn keyed(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => animate::is_keyed(value) || map.values().any(keyed),
            serde_json::Value::Array(items) => items.iter().any(keyed),
            _ => false,
        }
    }
    serde_json::to_value(command).is_ok_and(|v| keyed(&v))
}

/// The number of the script's own command a part started in (1 up), or
/// `None` for a part of the animation's commands.
fn command_number(part: &Part) -> Option<usize> {
    let rest = part.path.strip_prefix("command ")?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

impl Reuse {
    /// Works out what can be reused from a frame painted in full.
    pub fn analyse(script: &Script, report: &Report, document: &Document) -> Reuse {
        let own = script.commands.len();
        let numbers: Vec<usize> = (1..=own).collect();
        let mut dynamic: HashSet<usize> = script
            .commands
            .iter()
            .enumerate()
            .filter(|(_, c)| is_frame_dependent(c))
            .map(|(i, _)| i + 1)
            .collect();
        // What each command painted and read, by number; the animation's
        // own commands (numbered after the script's) are always dynamic.
        let mut paints: HashMap<usize, HashSet<LayerId>> = HashMap::new();
        for (&layer, commands) in &report.painted_by {
            for &n in commands {
                paints.entry(n).or_default().insert(layer);
                if n > own {
                    dynamic.insert(n);
                }
            }
        }
        let mut reads: HashMap<usize, HashSet<LayerId>> = HashMap::new();
        for (&layer, commands) in &report.read_by {
            for &n in commands {
                reads.entry(n).or_default().insert(layer);
                if n > own {
                    dynamic.insert(n);
                }
            }
        }
        // The last command to paint on each layer.
        let mut points: HashMap<LayerId, usize> = HashMap::new();
        for (n, layers) in &paints {
            for layer in layers {
                let point = points.entry(*layer).or_insert(0);
                *point = (*point).max(*n);
            }
        }
        let all_layers: Vec<LayerId> = document.layers().iter().map(|l| l.id()).collect();
        // Dynamic layers: painted by a dynamic command, or read before
        // their last painter is done by a command that runs every frame.
        // A static command that reads a dynamic layer is dynamic in
        // turn. Repeat until nothing changes.
        let mut forced: HashSet<LayerId> = HashSet::new();
        let (dynamic_layers, skip) = loop {
            let before = (dynamic.len(), forced.len());
            let mut dynamic_layers: HashSet<LayerId> = forced.clone();
            for n in &dynamic {
                if let Some(layers) = paints.get(n) {
                    dynamic_layers.extend(layers.iter().copied());
                }
            }
            let skip: BTreeSet<usize> = numbers
                .iter()
                .copied()
                .filter(|n| !dynamic.contains(n))
                .filter(|n| {
                    paints.get(n).is_some_and(|p| {
                        !p.is_empty() && p.iter().all(|l| !dynamic_layers.contains(l))
                    })
                })
                .collect();
            // Commands that run every frame and read a static layer that
            // isn't finished yet by then: the layer can't be copied in
            // time, so it's painted every frame.
            for (&n, read) in &reads {
                let runs = n > own || !skip.contains(&n);
                if !runs {
                    continue;
                }
                for layer in read {
                    if !dynamic_layers.contains(layer)
                        && points.get(layer).is_some_and(|&point| point >= n)
                    {
                        forced.insert(*layer);
                    }
                }
            }
            // A static command that reads a dynamic layer is dynamic.
            for &n in &numbers {
                if !dynamic.contains(&n)
                    && reads.get(&n).is_some_and(|r| {
                        r.iter()
                            .any(|l| dynamic_layers.contains(l) || forced.contains(l))
                    })
                {
                    dynamic.insert(n);
                }
            }
            if before == (dynamic.len(), forced.len()) {
                break (dynamic_layers, skip);
            }
        };
        let static_layers: HashSet<LayerId> = all_layers
            .iter()
            .copied()
            .filter(|id| !dynamic_layers.contains(id))
            .collect();
        let points: HashMap<LayerId, usize> = static_layers
            .iter()
            .map(|id| (*id, points.get(id).copied().unwrap_or(0)))
            .collect();
        let surfaces = document
            .layers()
            .iter()
            .filter(|l| static_layers.contains(&l.id()))
            .map(|l| (l.id(), l.surface.clone()))
            .collect();
        let parts = report
            .parts
            .iter()
            .filter_map(|p| command_number(p).map(|n| (n, p.clone())))
            .filter(|(n, _)| skip.contains(n))
            .collect();
        let painted_by = report
            .painted_by
            .iter()
            .map(|(&layer, commands)| {
                (
                    layer,
                    commands
                        .iter()
                        .copied()
                        .filter(|n| skip.contains(n))
                        .collect::<BTreeSet<usize>>(),
                )
            })
            .filter(|(_, numbers)| !numbers.is_empty())
            .collect();
        let warnings = report
            .warnings
            .iter()
            .filter(|w| {
                w.strip_prefix("command ")
                    .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
                    .and_then(|n| n.parse::<usize>().ok())
                    .is_some_and(|n| skip.contains(&n))
            })
            .cloned()
            .collect();
        Reuse {
            static_layers,
            skip,
            surfaces,
            points,
            parts,
            painted_by,
            warnings,
        }
    }
}

/// Frames of one animation painted in order, the static layers painted
/// once. The first frame asked for is painted in full; every later one
/// reuses what it taught.
pub struct Sequence {
    script: Script,
    options: PlayOptions,
    reuse: Option<Arc<Reuse>>,
}

impl Sequence {
    /// For `options.animation` (`frame` is set per paint).
    pub fn new(script: Script, options: PlayOptions) -> Self {
        Self {
            script,
            options,
            reuse: None,
        }
    }

    /// What the last full paint taught, once a frame has been painted.
    pub fn reuse(&self) -> Option<&Reuse> {
        self.reuse.as_deref()
    }

    /// One line on what's reused: "12 of 20 layers painted once, 31 of
    /// 55 commands skipped", or `None` before the first frame.
    pub fn summary(&self, document: &Document) -> Option<String> {
        let reuse = self.reuse.as_deref()?;
        let layers = document.layers().len();
        let own = self.script.commands.len();
        Some(format!(
            "{} of {layers} layers painted once, {} of {own} commands skipped in later frames",
            reuse.static_layers.len(),
            reuse.skip.len()
        ))
    }

    /// Paints `frame`. `on_event` is called at every event, as `run` does.
    pub fn paint(
        &mut self,
        frame: u32,
        on_event: impl FnMut(&Event, &Document),
    ) -> Result<(Document, Report), ScriptError> {
        let options = PlayOptions {
            frame,
            reuse: self.reuse.clone(),
            ..self.options.clone()
        };
        let mut player = Player::new(self.script.clone(), options)?;
        let mut document = player.create_document()?;
        player.run(&mut document, on_event)?;
        let report = player.report();
        // QUILL_NO_REUSE paints every frame in full (for measuring, and for
        // checking a picture against the reused one).
        // (ART_NO_REUSE is its name from before the rename.)
        let switched_off = ["QUILL_NO_REUSE", "ART_NO_REUSE"]
            .iter()
            .any(|name| std::env::var_os(name).is_some());
        if self.reuse.is_none() && !switched_off {
            self.reuse = Some(Arc::new(Reuse::analyse(&self.script, &report, &document)));
        }
        Ok((document, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = r##"{ "palette": { "ink": "#202020", "sky": "#8EC5E8", "red": "#C04040", "gold": "#F2C230" },
      "commands": [
        { "cmd": "canvas", "width": 160, "height": 100, "paper": "sky" },
        { "cmd": "layer", "name": "Sky" },
        { "cmd": "gradient", "from": [0, 0], "to": [0, 100], "colors": ["#5A9BD0", "sky"] },
        { "cmd": "layer", "name": "Stencil" },
        { "cmd": "ellipse", "center": [80, 50], "radius": 30, "fill": "ink" },
        { "cmd": "layer", "name": "Hero" },
        { "cmd": "define", "name": "hero", "params": { "lift": 0 }, "commands": [
          { "cmd": "group", "translate": [0, "$lift"], "commands": [
            { "cmd": "ellipse", "center": [0, 0], "radius": 10, "fill": "red" } ] } ] },
        { "cmd": "group", "name": "masked", "mask": { "layer": "Stencil" }, "commands": [
          { "cmd": "use", "name": "hero", "translate": [80, 50], "with": { "lift": { "keys": [[0, -20], [4, 20]] } } } ] },
        { "cmd": "layer", "name": "Ground" },
        { "cmd": "rect", "x": 0, "y": 80, "width": 160, "height": 20, "fill": "gold" }
      ],
      "animations": { "bob": { "frames": 5, "fps": 5, "commands": [
        { "cmd": "layer", "name": "Bob" },
        { "cmd": "group", "translate": { "keys": [[0, [20, 20]], [4, [20, 60]]] }, "commands": [
          { "cmd": "ellipse", "center": [0, 0], "radius": 6, "fill": "ink" } ] } ] } } }"##;

    /// A layer read before its last painter has run can't be copied in
    /// time: it's painted every frame, and so is whatever reads it.
    #[test]
    fn a_layer_read_before_it_is_finished_stays_dynamic() {
        let text = r##"{ "palette": { "ink": "#202020", "red": "#C04040" }, "commands": [
            { "cmd": "canvas", "width": 80, "height": 60 },
            { "cmd": "layer", "name": "A" },
            { "cmd": "ellipse", "center": [40, 30], "radius": 20, "fill": "ink" },
            { "cmd": "layer", "name": "B" },
            { "cmd": "group", "mask": { "layer": "A" }, "translate": { "keys": [[0, [0, 0]], [3, [10, 0]]] }, "commands": [
              { "cmd": "rect", "x": 20, "y": 20, "width": 40, "height": 20, "fill": "red" } ] },
            { "cmd": "layer", "name": "A" },
            { "cmd": "ellipse", "center": [10, 10], "radius": 5, "fill": "ink" } ],
          "animations": { "go": { "frames": 4, "fps": 4, "commands": [ { "cmd": "layer", "name": "C" } ] } } }"##;
        let script = Script::parse(text).unwrap();
        let mut sequence = Sequence::new(
            script,
            PlayOptions {
                animation: Some("go".into()),
                ..PlayOptions::default()
            },
        );
        let (first, _) = sequence.paint(0, |_, _| {}).unwrap();
        let reuse = sequence.reuse().unwrap();
        let static_names: Vec<&str> = first
            .layers()
            .iter()
            .filter(|l| reuse.static_layers.contains(&l.id()))
            .map(|l| l.name())
            .collect();
        // (C, made by the animation and never painted, is static and empty.)
        assert_eq!(static_names, ["C"], "A is read at 5 and painted again at 7");
        assert!(reuse.skip.is_empty());
    }

    #[test]
    fn keyframes_anywhere_make_a_command_frame_dependent() {
        let script = Script::parse(SCRIPT).unwrap();
        let dynamic: Vec<usize> = script
            .commands
            .iter()
            .enumerate()
            .filter(|(_, c)| is_frame_dependent(c))
            .map(|(i, _)| i + 1)
            .collect();
        assert_eq!(dynamic, [8], "only the masked group has keyframes");
    }

    #[test]
    fn static_layers_are_skipped_and_read_layers_stay_dynamic() {
        let script = Script::parse(SCRIPT).unwrap();
        let options = PlayOptions {
            animation: Some("bob".into()),
            ..PlayOptions::default()
        };
        let mut sequence = Sequence::new(script.clone(), options);
        let (first, _) = sequence.paint(0, |_, _| {}).unwrap();
        let reuse = sequence.reuse().unwrap().clone();
        // The sky, the stencil and the ground are painted once: the
        // stencil is read by the keyframed group's mask, but its last
        // painter is done by then, so its pixels are put in place first.
        // The hero's layer takes the keyframed paint (and the animation's
        // own commands paint on their own layer: had they not selected
        // one, the ground would be dynamic too).
        let mut names: Vec<String> = reuse
            .static_layers
            .iter()
            .filter_map(|id| first.layer(*id).map(|l| l.name().to_owned()))
            .collect();
        names.sort();
        assert_eq!(names, ["Ground", "Sky", "Stencil"]);
        assert_eq!(reuse.skip.iter().copied().collect::<Vec<_>>(), [3, 5, 10]);
        let (later, report) = sequence.paint(3, |_, _| {}).unwrap();
        // The masked group and the use inside it, in order, as a full
        // paint reports them.
        assert_eq!(report.parts.len(), 2);
        assert!(report.parts[0].path.ends_with("(group \"masked\")"));
        assert_eq!(report.parts[1].inside, Some(0));
        assert!(
            report.painted_by.values().flatten().any(|&n| n == 3),
            "the sky counts as painted"
        );
        // And the pixels are those of a full paint.
        let mut player = Player::new(
            script,
            PlayOptions {
                animation: Some("bob".into()),
                frame: 3,
                ..PlayOptions::default()
            },
        )
        .unwrap();
        let mut full = player.create_document().unwrap();
        player.run(&mut full, |_, _| {}).unwrap();
        for (a, b) in later.layers().iter().zip(full.layers()) {
            assert_eq!(a.name(), b.name());
            for y in 0..100 {
                for x in 0..160 {
                    assert_eq!(
                        a.surface.pixel(x, y),
                        b.surface.pixel(x, y),
                        "{} at {x},{y}",
                        a.name()
                    );
                }
            }
        }
    }
}
