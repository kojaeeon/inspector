mod report;
#[cfg(windows)]
mod win_events;

use std::{
    env,
    error::Error,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

const USAGE: &str = "Usage:
  inspector watch [--duration SECONDS] [--out NEW_FILE]
  inspector summarize FILE

Defaults: --duration 120 --out office-auth.jsonl
Duration: 1–3600 seconds. Existing output files are preserved.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("inspector: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let command = args.next();
    match command.as_deref().and_then(|arg| arg.to_str()) {
        Some("watch") => {
            let mut seconds = None;
            let mut out = None;
            while let Some(flag) = args.next() {
                let value = args.next().ok_or("missing option value")?;
                match flag.to_str() {
                    Some("--duration") if seconds.is_none() => {
                        let count: u64 = value
                            .to_str()
                            .and_then(|text| text.parse().ok())
                            .filter(|count| (1..=3600).contains(count))
                            .ok_or("duration must be a whole number from 1 to 3600")?;
                        seconds = Some(count);
                    }
                    Some("--out") if out.is_none() => out = Some(PathBuf::from(value)),
                    _ => return Err(format!("unknown or repeated option: {:?}", flag).into()),
                }
            }
            watch(
                Duration::from_secs(seconds.unwrap_or(120)),
                &out.unwrap_or_else(|| PathBuf::from("office-auth.jsonl")),
            )
        }
        Some("summarize") => {
            let file = args.next().ok_or("summarize requires a JSONL file")?;
            if args.next().is_some() {
                return Err("summarize takes one JSONL file".into());
            }
            report::summarize_file(Path::new(&file), io::stdout().lock())?;
            Ok(())
        }
        Some("--help" | "-h") if args.next().is_none() => {
            writeln!(io::stdout().lock(), "{USAGE}")?;
            Ok(())
        }
        None if command.is_none() => {
            writeln!(io::stdout().lock(), "{USAGE}")?;
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

#[cfg(windows)]
fn watch(duration: Duration, path: &Path) -> Result<(), Box<dyn Error>> {
    win_events::watch(duration, path)?;
    Ok(())
}

#[cfg(not(windows))]
fn watch(_duration: Duration, _path: &Path) -> Result<(), Box<dyn Error>> {
    Err("watch requires Windows; summarize can read an existing capture here".into())
}
