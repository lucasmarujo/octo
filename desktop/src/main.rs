#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod alerts;
mod link;
mod pair;
mod settings;
mod updates;

use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;
use orkai::cli::Internal;

#[derive(Parser)]
#[command(name = "orkai-desktop", version, about = "Every agent of every project in one window")]
struct Args {
    #[command(subcommand)]
    internal: Option<Internal>,
}

fn main() -> ExitCode {
    let internal = match Args::try_parse() {
        Ok(args) => args.internal,
        Err(err) if matches!(err.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => err.exit(),
        Err(_) => None,
    };
    let result = match internal {
        Some(internal) => internal.run(),
        None => link::run(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("orkai: {err}");
            ExitCode::FAILURE
        }
    }
}
