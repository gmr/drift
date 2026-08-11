use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use jiff::Timestamp;

/// Report the changes between two git refs that `.driftignore` does not cover.
#[derive(Debug, Parser)]
#[command(name = "drift", version, about, long_about = None)]
struct Args {
    /// The older ref, excluded from the range
    from: String,

    /// The newer ref, included in the range, and the source of `.driftignore`
    to: String,

    /// Repository to inspect
    #[arg(short = 'C', long = "repo", default_value = ".", value_name = "PATH")]
    repo: PathBuf,

    /// Count every path the range touched, even if it was changed back [default]
    #[arg(long, conflicts_with = "tree")]
    log: bool,

    /// Count only the paths whose content differs between the two refs
    #[arg(long)]
    tree: bool,

    /// Indent the JSON output
    #[arg(long)]
    pretty: bool,

    /// Exit 1 when drift is detected
    #[arg(long)]
    fail_on_drift: bool,
}

impl Args {
    fn mode(&self) -> drift::Mode {
        if self.tree {
            drift::Mode::Tree
        } else {
            drift::Mode::Log
        }
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(drift_detected) => {
            if drift_detected && args.fail_on_drift {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("drift: {error}");
            let mut source = std::error::Error::source(&error);
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            ExitCode::from(2)
        }
    }
}

/// Run one analysis and print the report, returning whether drift was detected.
fn run(args: &Args) -> drift::Result<bool> {
    let repo = drift::open(&args.repo)?;
    let report = drift::analyze(&repo, &args.from, &args.to, args.mode(), Timestamp::now())?;
    let json = if args.pretty {
        serde_json::to_string_pretty(&report)?
    } else {
        serde_json::to_string(&report)?
    };
    // A closed stdout, as in `drift a b | head -1`, is not an error worth a panic or
    // an exit code of its own; the reader stopped asking.
    if let Err(error) = writeln!(std::io::stdout(), "{json}")
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(drift::Error::Output(error));
    }
    Ok(report.drift_detected)
}
