#![forbid(unsafe_code)]
//! Binary entry point for the `mustard` CLI.
//!
//! Thin shell: parse arguments ([`mustard_cli::cli`]) and hand off to the
//! dispatch table. All real logic lives in the library, so a caller can drive
//! an install without spawning a process.

use std::process::ExitCode;

fn main() -> ExitCode {
    match mustard_cli::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("[mustard] error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
