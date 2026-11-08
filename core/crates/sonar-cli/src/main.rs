//! `sonar` — headless entry point to every core feature.
//! Subcommands are added task by task (see docs/ROADMAP.md); T0.2 replaces
//! this hand-rolled parsing with `clap`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("version") | Some("--version") | Some("-V") => {
            println!("sonar {}", sonar_core::version());
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: sonar <version>   (more commands arrive per ROADMAP)");
            ExitCode::from(2)
        }
    }
}
