//! `quill`: Duck Quill on the command line. All the work is in the
//! library (`lib.rs`); this file only starts it.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    quill_cli::run(&args)
}
