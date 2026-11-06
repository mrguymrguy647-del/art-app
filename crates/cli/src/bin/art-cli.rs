//! `art-cli`: the old name of `quill`, kept so scripts and agents written
//! before the rename keep working.
//!
//! The note goes to the error output, not the normal output, so a program
//! that reads what `art-cli` prints sees exactly what it saw before.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("art-cli is now called quill");
    let args: Vec<String> = std::env::args().skip(1).collect();
    quill_cli::run(&args)
}
