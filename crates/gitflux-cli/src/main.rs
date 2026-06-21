//! Command-line adapter for Gitflux.
//!
//! The CLI is the imperative shell for Repository Replay workflows. It keeps
//! argument handling separate from Repository Ingestion, scene construction,
//! Render Configuration, GPU rendering, and Video Export orchestration.

use std::env;
use std::process::ExitCode;

const HELP: &str = "\
Gitflux command-line interface

Usage:
  gitflux [OPTIONS]

Options:
  -h, --help       Print help
  -V, --version    Print version
";

fn main() -> ExitCode {
    let args = env::args().skip(1);

    match run(args) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn run(args: impl IntoIterator<Item = String>) -> Result<String, String> {
    match args.into_iter().next().as_deref() {
        None | Some("-h") | Some("--help") => Ok(HELP.to_owned()),
        Some("-V") | Some("--version") => Ok(format!("gitflux {}\n", env!("CARGO_PKG_VERSION"))),
        Some(flag) => Err(format!("unrecognized option: {flag}\n\n{HELP}")),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn prints_help_by_default() {
        let output = run(Vec::new()).expect("help should render");

        assert!(output.contains("Usage:"));
        assert!(output.contains("--version"));
    }

    #[test]
    fn prints_version() {
        let output = run(["--version".to_owned()]).expect("version should render");

        assert!(output.starts_with("gitflux "));
    }

    #[test]
    fn rejects_unknown_options() {
        let error = run(["--missing".to_owned()]).expect_err("unknown flags should fail");

        assert!(error.contains("unrecognized option: --missing"));
    }
}
