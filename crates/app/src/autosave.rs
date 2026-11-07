//! Autosave: a copy of unsaved work in the settings folder's `recovery`
//! folder, so a crash or a power cut loses at most a couple of minutes.
//!
//! - It never writes over your own files: only into `recovery`.
//! - It only copies when the picture has unsaved changes, at most every
//!   two minutes, and only after a couple of seconds without painting,
//!   so a stroke never waits. The copy is written by a background thread.
//! - When you save (or choose not to save), the copy is deleted.
//! - Next time Duck Quill starts, a copy left behind is offered back.
//!
//! Each running Duck Quill has its own file, `<session>.quill`, with
//! `<session>.json` beside it saying what it is, and holds a lock on
//! `<session>.lock` while it runs. The system lets go of the lock when
//! Duck Quill stops, even if it crashes, so a copy whose lock is free was
//! left behind, and one Duck Quill never offers another's work while it's
//! still open.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use quill_engine::document::Document;
use quill_engine::file;
use quill_engine::history::History;
use serde::{Deserialize, Serialize};

use crate::settings::full_path;

/// At most this often.
pub const EVERY: Duration = Duration::from_secs(120);
/// Only after this long without input.
pub const IDLE: Duration = Duration::from_secs(2);

/// The note beside each copy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// The picture's name when it was copied ("Untitled" if never saved).
    pub name: String,
    /// The file it was opened from or saved to, if any.
    pub original: Option<PathBuf>,
    /// When the copy was written (seconds since 1970).
    pub copied: u64,
}

/// A copy left behind by a Duck Quill that has stopped.
#[derive(Clone, Debug, PartialEq)]
pub struct LeftBehind {
    pub picture: PathBuf,
    pub note: Note,
}

pub struct Autosave {
    /// `None`: there's nowhere to keep copies, so autosave is off.
    folder: Option<PathBuf>,
    /// This session's file name, without the ending.
    session: String,
    /// The picture's generation (see `History::generation`) last copied.
    copied: Option<u64>,
    last_copy: Option<SystemTime>,
    /// Held while this Duck Quill runs (see the top of this file).
    lock: Option<std::fs::File>,
    /// A copy being written in the background.
    writing: Option<Receiver<Result<(), String>>>,
    /// The last thing that went wrong, for the status bar.
    pub problem: Option<String>,
}

fn seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Autosave {
    /// Autosave into `folder` (normally the settings folder's `recovery`).
    pub fn new(folder: Option<PathBuf>, now: SystemTime) -> Autosave {
        // (The counter keeps two in one run apart, as in the tests.)
        static MADE: AtomicU64 = AtomicU64::new(0);
        let n = MADE.fetch_add(1, Ordering::Relaxed);
        let session = format!("session-{}-{}-{n}", seconds(now), std::process::id());
        // Without a lock this Duck Quill's copy could be taken for one
        // left behind, so without one there's no autosave.
        let lock = folder.as_ref().and_then(|folder| {
            std::fs::create_dir_all(folder).ok()?;
            let file = std::fs::File::create(folder.join(format!("{session}.lock"))).ok()?;
            file.try_lock().ok()?;
            Some(file)
        });
        if lock.is_some()
            && let Some(folder) = &folder
        {
            tidy_locks(folder);
        }
        Autosave {
            folder: lock.is_some().then_some(folder).flatten(),
            session,
            copied: None,
            last_copy: None,
            lock,
            writing: None,
            problem: None,
        }
    }

    fn files(&self) -> Option<(PathBuf, PathBuf)> {
        let folder = self.folder.as_ref()?;
        Some((
            folder.join(format!("{}.{}", self.session, file::FILE_EXTENSION)),
            folder.join(format!("{}.json", self.session)),
        ))
    }

    /// Copies left behind by Duck Quills that have stopped, newest first.
    pub fn left_behind(&self) -> Vec<LeftBehind> {
        let Some(folder) = &self.folder else {
            return Vec::new();
        };
        let Ok(listing) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut found: Vec<LeftBehind> = listing
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "json"))
            .filter_map(|note_path| {
                let note: Note =
                    serde_json::from_str(&std::fs::read_to_string(&note_path).ok()?).ok()?;
                let picture = note_path.with_extension(file::FILE_EXTENSION);
                (stopped(&note_path.with_extension("lock")) && picture.exists())
                    .then_some(LeftBehind { picture, note })
            })
            .collect();
        found.sort_by_key(|left| std::cmp::Reverse(left.note.copied));
        found
    }

    /// Takes a copy left behind over as this session's own (after it's
    /// been restored), so it stays safe until the picture is saved.
    pub fn adopt(&mut self, left: &LeftBehind) {
        let Some((picture, note_path)) = self.files() else {
            return;
        };
        let _ = std::fs::rename(&left.picture, &picture);
        let _ = write_note(&note_path, &left.note);
        Self::throw_away(left);
    }

    /// Deletes a copy left behind (the person chose not to restore it).
    pub fn throw_away(left: &LeftBehind) {
        for ending in [file::FILE_EXTENSION, "json", "lock"] {
            let _ = std::fs::remove_file(left.picture.with_extension(ending));
        }
    }

    /// Called every frame. Copies the picture when it's due; deletes the
    /// copy once there's nothing unsaved. `idle`: no input for a while
    /// and no stroke in progress.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        document: &Document,
        history: &History,
        name: &str,
        original: Option<&Path>,
        idle: bool,
        now: SystemTime,
    ) {
        let Some((picture, note_path)) = self.files() else {
            return;
        };
        // A copy finished?
        if let Some(receiver) = &self.writing
            && let Ok(result) = receiver.try_recv()
        {
            self.writing = None;
            self.problem = result.err();
        }
        if self.writing.is_some() {
            return;
        }
        if !history.is_modified() {
            // Saved (or nothing done yet): no copy needed.
            if self.copied.is_some() || picture.exists() {
                let _ = std::fs::remove_file(&picture);
                let _ = std::fs::remove_file(&note_path);
                self.copied = None;
            }
            return;
        }
        let due = self
            .last_copy
            .is_none_or(|t| now.duration_since(t).unwrap_or_default() >= EVERY);
        let changed = self.copied != Some(history.generation());
        if !(due && changed && idle) {
            return;
        }
        // The copy of the document is made here (quick, while nothing is
        // being painted); compressing and writing it happens in the background.
        let copy = document.clone();
        let note = Note {
            name: name.to_owned(),
            // A full path, so Save after a restore finds the same file
            // whatever folder the next Duck Quill starts in.
            original: original.map(full_path),
            copied: seconds(now),
        };
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let result = write_copy(&copy, &picture, &note_path, &note);
            let _ = sender.send(result);
        });
        self.writing = Some(receiver);
        self.copied = Some(history.generation());
        self.last_copy = Some(now);
    }

    /// Waits for a copy being written (for tests, and before quitting).
    pub fn wait(&mut self) {
        if let Some(receiver) = self.writing.take()
            && let Ok(result) = receiver.recv()
        {
            self.problem = result.err();
        }
    }

    /// Quitting with nothing unsaved, or after choosing not to save: the
    /// copy isn't needed any more.
    pub fn finish(&mut self) {
        self.wait();
        // Let go of the lock first: Windows can't delete an open file.
        self.lock = None;
        if let Some((picture, note)) = self.files() {
            let _ = std::fs::remove_file(picture);
            let _ = std::fs::remove_file(&note);
            let _ = std::fs::remove_file(note.with_extension("lock"));
        }
    }
}

/// Deletes the lock files of Duck Quills that stopped with nothing
/// unsaved (a crash with no copy): they'd otherwise stay forever. A lock
/// with a copy beside it is kept: that copy is offered back.
fn tidy_locks(folder: &Path) {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return;
    };
    for lock in listing.filter_map(|entry| entry.ok().map(|e| e.path())) {
        if lock.extension().is_some_and(|e| e == "lock")
            && !lock.with_extension(file::FILE_EXTENSION).exists()
            && stopped(&lock)
        {
            let _ = std::fs::remove_file(&lock);
            let _ = std::fs::remove_file(lock.with_extension("json"));
        }
    }
}

/// True if the Duck Quill holding this lock file has stopped (or there
/// isn't one).
fn stopped(lock: &Path) -> bool {
    match std::fs::OpenOptions::new().write(true).open(lock) {
        Ok(file) => file.try_lock().is_ok(),
        Err(_) => true,
    }
}

fn write_note(path: &Path, note: &Note) -> Result<(), String> {
    let text = serde_json::to_string_pretty(note).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn write_copy(
    document: &Document,
    picture: &Path,
    note_path: &Path,
    note: &Note,
) -> Result<(), String> {
    if let Some(folder) = picture.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    // `file::save` writes a temporary file and renames it, so a copy is
    // never half written.
    file::save(document, picture).map_err(|e| e.to_string())?;
    write_note(note_path, note)
}

/// "3 minutes ago", for the restore window.
pub fn how_long_ago(then: u64, now: SystemTime) -> String {
    let minutes = seconds(now).saturating_sub(then) / 60;
    match minutes {
        0 => "less than a minute ago".into(),
        1 => "a minute ago".into(),
        2..=59 => format!("{minutes} minutes ago"),
        60..=119 => "an hour ago".into(),
        _ if minutes < 48 * 60 => format!("{} hours ago", minutes / 60),
        _ => format!("{} days ago", minutes / (24 * 60)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_engine::history::TileCommand;
    use quill_engine::tile::Rgba8;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("quill-autosave-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    /// A change, as painting makes one.
    fn paint(document: &mut Document, history: &mut History, x: i32) {
        let layer = document.active_id();
        let surface = &mut document.active_layer_mut().surface;
        let coord = quill_engine::surface::TileCoord::containing(x, 5);
        let before = std::collections::HashMap::from([(coord, surface.tile(coord).cloned())]);
        surface.set_pixel(x, 5, Rgba8::new(255, 0, 0, 255));
        history.push(Box::new(TileCommand::new("Dot", layer, before)));
    }

    #[test]
    fn unsaved_work_is_copied_when_idle_and_the_copy_goes_once_saved() {
        let folder = folder("copy");
        let start = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut autosave = Autosave::new(Some(folder.clone()), start);
        let (picture, _) = autosave.files().unwrap();
        let mut document = Document::new(64, 64);
        let mut history = History::default();
        // Nothing unsaved: nothing copied.
        autosave.tick(&document, &history, "Untitled", None, true, start);
        assert!(!picture.exists());
        paint(&mut document, &mut history, 3);
        // Busy painting: it waits.
        autosave.tick(&document, &history, "Untitled", None, false, start);
        autosave.wait();
        assert!(!picture.exists());
        autosave.tick(&document, &history, "Untitled", None, true, start);
        autosave.wait();
        let copy = file::open(&picture).unwrap();
        assert_eq!(copy.active_layer().surface.pixel(3, 5).a, 255);
        // Another change a minute later: not yet (at most every two minutes).
        paint(&mut document, &mut history, 9);
        autosave.tick(
            &document,
            &history,
            "Untitled",
            None,
            true,
            start + Duration::from_secs(60),
        );
        autosave.wait();
        assert_eq!(
            file::open(&picture)
                .unwrap()
                .active_layer()
                .surface
                .pixel(9, 5)
                .a,
            0
        );
        autosave.tick(&document, &history, "Untitled", None, true, start + EVERY);
        autosave.wait();
        assert_eq!(
            file::open(&picture)
                .unwrap()
                .active_layer()
                .surface
                .pixel(9, 5)
                .a,
            255
        );
        // Saved: the copy goes.
        history.mark_saved();
        autosave.tick(
            &document,
            &history,
            "Untitled",
            None,
            true,
            start + EVERY * 2,
        );
        assert!(!picture.exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_copy_left_behind_is_offered_only_once_its_duck_quill_has_stopped() {
        let folder = folder("left");
        let start = UNIX_EPOCH + Duration::from_secs(2_000_000);
        let mut crashed = Autosave::new(Some(folder.clone()), start);
        let mut document = Document::new(64, 64);
        let mut history = History::default();
        paint(&mut document, &mut history, 1);
        // (A full path, as the note keeps it, on Windows and Linux alike.)
        let original_path = std::env::temp_dir().join("pictures").join("duck.quill");
        let original = original_path.as_path();
        crashed.tick(
            &document,
            &history,
            "duck.quill",
            Some(original),
            true,
            start,
        );
        crashed.wait();
        // Another Duck Quill starting while the first still runs: nothing
        // is offered.
        let mut next = Autosave::new(Some(folder.clone()), start);
        assert!(next.left_behind().is_empty());
        // The first one stops without tidying up (as in a crash).
        drop(crashed);
        let now = start + Duration::from_secs(300);
        let found = next.left_behind();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].note.name, "duck.quill");
        assert_eq!(found[0].note.original.as_deref(), Some(original));
        assert_eq!(how_long_ago(found[0].note.copied, now), "5 minutes ago");
        // Restored: it becomes this session's copy, so it stays safe.
        next.adopt(&found[0]);
        let (picture, _) = next.files().unwrap();
        assert!(picture.exists());
        assert!(
            Autosave::new(Some(folder.clone()), now)
                .left_behind()
                .is_empty()
        );
        // Once this one stops too, it's offered again; thrown away, gone.
        drop(next);
        let left = Autosave::new(Some(folder.clone()), now).left_behind();
        assert_eq!(left.len(), 1);
        Autosave::throw_away(&left[0]);
        assert!(
            Autosave::new(Some(folder.clone()), now)
                .left_behind()
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_note_keeps_the_original_as_a_full_path() {
        let folder = folder("full");
        let now = SystemTime::now();
        let mut autosave = Autosave::new(Some(folder.clone()), now);
        let mut document = Document::new(16, 16);
        let mut history = History::default();
        paint(&mut document, &mut history, 1);
        // As given on the command line, from some working folder.
        let given = Path::new("../r/pen.quill");
        autosave.tick(&document, &history, "pen.quill", Some(given), true, now);
        autosave.wait();
        let (_, note_path) = autosave.files().unwrap();
        let note: Note =
            serde_json::from_str(&std::fs::read_to_string(note_path).unwrap()).unwrap();
        let original = note.original.unwrap();
        assert!(original.is_absolute(), "{}", original.display());
        assert_eq!(original, std::path::absolute(given).unwrap());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn lock_files_of_stopped_runs_with_nothing_unsaved_are_tidied_away() {
        let folder = folder("locks");
        std::fs::create_dir_all(&folder).unwrap();
        let now = SystemTime::now();
        // A run that crashed with nothing unsaved: only its lock is left.
        let empty = folder.join("session-1-1-0.lock");
        std::fs::write(&empty, "").unwrap();
        // One that crashed with a copy: that's kept (it's offered back).
        let with_copy = folder.join("session-2-2-0.lock");
        std::fs::write(&with_copy, "").unwrap();
        std::fs::write(with_copy.with_extension(file::FILE_EXTENSION), "copy").unwrap();
        // One still running, with nothing unsaved: its lock is in use.
        let running = Autosave::new(Some(folder.clone()), now);
        let running_lock = folder.join(format!("{}.lock", running.session));
        assert!(!empty.exists(), "the stopped run's lock is gone");
        assert!(with_copy.exists(), "kept beside its copy");
        // Starting another one leaves the running one's lock alone.
        let _another = Autosave::new(Some(folder.clone()), now);
        assert!(running_lock.exists(), "a running Duck Quill keeps its lock");
        drop(running);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn without_a_folder_autosave_does_nothing() {
        let mut autosave = Autosave::new(None, SystemTime::now());
        let mut document = Document::new(8, 8);
        let mut history = History::default();
        paint(&mut document, &mut history, 1);
        autosave.tick(&document, &history, "x", None, true, SystemTime::now());
        autosave.finish();
        assert!(autosave.left_behind().is_empty());
    }
}
