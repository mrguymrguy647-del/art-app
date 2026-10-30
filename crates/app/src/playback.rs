//! "Play strokes": a painting script painted on the canvas a bit at a
//! time, so you can watch it happen. A bar at the bottom of the window has
//! play/pause, a speed slider, "skip to end" and "stop".
//!
//! While it plays, painting and the side panels are locked (zooming and
//! moving around still work). When it's done, everything it painted
//! becomes one undo step.

use art_engine::document::Document;
use art_engine::history::CommandGroup;
use art_engine::script::{Command as ScriptCommand, Player, Progress, Script, record};
use eframe::egui::{self, Button, ProgressBar, Slider, vec2};

/// Speed limits, in pixels of brush travel per second.
const SLOWEST: f32 = 50.0;
const FASTEST: f32 = 100_000.0;
const DEFAULT_SPEED: f32 = 4_000.0;
/// One frame never plays more than this many seconds' worth (after a
/// pause or a slow frame).
const LONGEST_FRAME: f32 = 0.1;

/// What the user asked for in the playback bar.
#[derive(PartialEq)]
pub enum BarAction {
    None,
    SkipToEnd,
    Stop,
}

pub struct Playback {
    player: Player,
    /// The script file's name, to show.
    name: String,
    /// What's being played, with colour names replaced (for "Export strokes").
    commands: Vec<ScriptCommand>,
    /// Commands that go before it in the recording (the paper colour and
    /// the layer selected when playing started).
    preamble: Vec<ScriptCommand>,
    paused: bool,
    /// Pixels of brush travel per second.
    speed: f32,
}

impl Playback {
    /// Starts playing. `player` must be ready (`create_document` or
    /// `start_on` done). `fresh` means it plays onto a new picture made
    /// from its canvas; otherwise onto the open picture, whose selected
    /// layer is `active_layer`.
    pub fn new(
        player: Player,
        script: &Script,
        name: String,
        fresh: bool,
        active_layer: &str,
    ) -> Self {
        let commands = record::without_palette(&script.commands, &script.palette);
        // On the open picture, the recording must say which paper and
        // layer the script started with. (A new picture already matches
        // the script's canvas.)
        let mut preamble = Vec::new();
        if !fresh {
            if let Some(ScriptCommand::Canvas(canvas)) = commands.first()
                && let Some(paper) = &canvas.paper
            {
                preamble.push(ScriptCommand::Paper(art_engine::script::PaperCmd {
                    color: paper.clone(),
                }));
            }
            preamble.push(record::select_layer(active_layer));
        }
        Self {
            player,
            name,
            commands,
            preamble,
            paused: false,
            speed: DEFAULT_SPEED,
        }
    }

    /// Plays a little more (called every frame). Returns `Some` when
    /// playing has ended: `Ok` when finished, `Err` with the problem if
    /// the script went wrong part way.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        document: &mut Document,
    ) -> Option<Result<(), String>> {
        if self.paused {
            return None;
        }
        let seconds = ctx.input(|i| i.stable_dt).min(LONGEST_FRAME);
        let budget = self.speed * seconds;
        loop {
            match self.player.advance(document, budget) {
                // Snapshots don't matter when watching.
                Ok(Progress::Event(_)) => continue,
                Ok(Progress::Working) => {
                    ctx.request_repaint();
                    return None;
                }
                Ok(Progress::Finished) => return Some(Ok(())),
                Err(error) => return Some(Err(error.to_string())),
            }
        }
    }

    /// Plays everything that's left, straight away.
    pub fn skip_to_end(&mut self, document: &mut Document) -> Result<(), String> {
        loop {
            match self.player.advance(document, f32::INFINITY) {
                Ok(Progress::Finished) => return Ok(()),
                Ok(_) => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }

    /// The bar at the bottom of the window.
    pub fn bar(&mut self, ui: &mut egui::Ui) -> BarAction {
        let mut action = BarAction::None;
        let (done, total) = self.player.position();
        ui.horizontal(|ui| {
            ui.strong(format!("Playing {}", self.name));
            let fraction = done.saturating_sub(1) as f32 / (total.max(2) - 1) as f32;
            ui.add(
                ProgressBar::new(fraction)
                    .desired_width(160.0)
                    .text(format!("{} / {}", done.saturating_sub(1), total - 1)),
            )
            .on_hover_text("Commands played so far");
            let size = vec2(80.0, 26.0);
            let pause_label = if self.paused { "Play" } else { "Pause" };
            if ui.add(Button::new(pause_label).min_size(size)).clicked() {
                self.paused = !self.paused;
            }
            if ui.add(Button::new("Skip to end").min_size(size)).clicked() {
                action = BarAction::SkipToEnd;
            }
            if ui
                .add(Button::new("Stop").min_size(size))
                .on_hover_text("Stop here and keep what's painted so far")
                .clicked()
            {
                action = BarAction::Stop;
            }
            ui.add(
                Slider::new(&mut self.speed, SLOWEST..=FASTEST)
                    .logarithmic(true)
                    .text("Speed")
                    .custom_formatter(|v, _| format!("{v:.0} px/s")),
            );
        });
        action
    }

    /// Ends playing: everything painted becomes one undo step (`None` if
    /// nothing changed), with what was played written down as script
    /// commands (for "Export strokes").
    pub fn finish(mut self, document: &mut Document) -> (Option<CommandGroup>, Vec<ScriptCommand>) {
        let steps = self.player.take_undo(document);
        let group = CommandGroup::new("Play strokes", steps);
        // The commands started so far (skipping `canvas`).
        let (started, _) = self.player.position();
        let mut script = self.preamble;
        script.extend(self.commands.into_iter().take(started).skip(1));
        ((!group.is_empty()).then_some(group), script)
    }
}
