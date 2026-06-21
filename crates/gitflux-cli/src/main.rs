//! Command-line adapter for Gitflux.
//!
//! The CLI is the imperative shell for Repository Replay workflows. It keeps
//! argument handling separate from Repository Ingestion, scene construction,
//! Render Configuration, GPU rendering, and Video Export orchestration.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use gitflux_scene::{Layout, RenderConfiguration};

const HELP: &str = "\
Gitflux command-line interface

Usage:
  gitflux [OPTIONS]
  gitflux render <repository-path> --output <output-path>

Options:
  -h, --help       Print help
  -V, --version    Print version
";

const RENDER_PHASES: &[&str] = &[
    "Repository Ingestion",
    "Repository Replay",
    "Render Configuration",
    "Render",
    "Video Export",
    "Export Manifest",
];

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
    let mut args = args.into_iter();

    match args.next().as_deref() {
        None | Some("-h") | Some("--help") => Ok(HELP.to_owned()),
        Some("-V") | Some("--version") => Ok(format!("gitflux {}\n", env!("CARGO_PKG_VERSION"))),
        Some("render") => run_render(args),
        Some(flag) => Err(format!("unrecognized option: {flag}\n\n{HELP}")),
    }
}

fn run_render(args: impl IntoIterator<Item = String>) -> Result<String, String> {
    let config = parse_render_args(args)?;

    if config.json {
        return Ok(render_json_progress(&config));
    }

    let mut output = String::new();
    output.push_str("Gitflux Render tracer\n");
    output.push_str(&format!(
        "Repository path: {}\n",
        config.repository_path.display()
    ));
    output.push_str(&format!(
        "Output target: {}\n",
        config.output_path.display()
    ));
    output.push_str(&format!(
        "Render Configuration: {}\n",
        config.render_configuration_label()
    ));

    for phase in RENDER_PHASES {
        output.push_str(&format!("- {phase}\n"));
    }

    Ok(output)
}

fn render_json_progress(config: &RenderCommand) -> String {
    let mut output = String::new();

    for (index, phase) in RENDER_PHASES.iter().enumerate() {
        let event = serde_json::json!({
            "event": "render_progress",
            "phase": phase,
            "phase_index": index,
            "phase_count": RENDER_PHASES.len(),
            "render_configuration": config.render_configuration_label(),
        });
        output.push_str(&event.to_string());
        output.push('\n');
    }

    output
}

fn parse_render_args(args: impl IntoIterator<Item = String>) -> Result<RenderCommand, String> {
    let mut args = args.into_iter();
    let repository_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing repository path\n\n{HELP}"))?;
    let mut output_path = None;
    let mut config_path = None;
    let mut json = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                output_path = args.next().map(PathBuf::from);
                if output_path.is_none() {
                    return Err("missing output path after --output".to_owned());
                }
            }
            "--config" => {
                config_path = args.next().map(PathBuf::from);
                if config_path.is_none() {
                    return Err("missing render configuration path after --config".to_owned());
                }
            }
            "--json" => json = true,
            flag => return Err(format!("unrecognized render option: {flag}")),
        }
    }

    let output_path =
        output_path.ok_or_else(|| "missing required --output <output-path>".to_owned())?;

    if !repository_path.exists() {
        return Err(format!(
            "repository path does not exist: {}",
            repository_path.display()
        ));
    }

    if !repository_path.is_dir() {
        return Err(format!(
            "repository path is not a directory: {}",
            repository_path.display()
        ));
    }

    let render_configuration = load_render_configuration(config_path.as_ref())?;

    Ok(RenderCommand {
        repository_path,
        output_path,
        config_path,
        render_configuration,
        json,
    })
}

fn load_render_configuration(config_path: Option<&PathBuf>) -> Result<RenderConfiguration, String> {
    let Some(config_path) = config_path else {
        return Ok(RenderConfiguration::default());
    };

    let contents = fs::read_to_string(config_path).map_err(|error| {
        format!(
            "failed to read Render Configuration {}: {error}",
            config_path.display()
        )
    })?;

    RenderConfiguration::from_toml_str(&contents).map_err(|error| {
        format!(
            "failed to load Render Configuration {}:\n{error}",
            config_path.display()
        )
    })
}

struct RenderCommand {
    repository_path: PathBuf,
    output_path: PathBuf,
    config_path: Option<PathBuf>,
    render_configuration: RenderConfiguration,
    json: bool,
}

impl RenderCommand {
    fn render_configuration_label(&self) -> String {
        let source = self
            .config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "defaults".to_owned());
        let frame_size = self.render_configuration.frame_size();
        let layout_name = match self.render_configuration.layout() {
            Layout::RepositoryGraph | Layout::RepositoryGraphWithParameters(_) => {
                "Repository Graph"
            }
            Layout::Named(_) => "Named Layout",
        };

        format!(
            "{} (theme: {}, {}x{}, {} FPS, layout: {})",
            source,
            self.render_configuration.theme().name(),
            frame_size.width(),
            frame_size.height(),
            self.render_configuration.frames_per_second().get(),
            layout_name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::run;
    use std::fs;
    use std::path::PathBuf;

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

    #[test]
    fn render_reports_human_readable_phases() {
        let output = run([
            "render".to_owned(),
            ".".to_owned(),
            "--output".to_owned(),
            "out.mp4".to_owned(),
        ])
        .expect("render tracer should succeed");

        assert!(output.contains("Repository Ingestion"));
        assert!(output.contains("Repository Replay"));
        assert!(output.contains("Render Configuration"));
        assert!(output.contains("Render"));
        assert!(output.contains("Video Export"));
        assert!(output.contains("Export Manifest"));
    }

    #[test]
    fn render_json_reports_structured_progress_events() {
        let output = run([
            "render".to_owned(),
            ".".to_owned(),
            "--output".to_owned(),
            "out.mp4".to_owned(),
            "--json".to_owned(),
        ])
        .expect("render tracer should succeed");
        let events: Vec<serde_json::Value> = output
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()
            .expect("json progress should be newline-delimited JSON");

        let phases = events
            .iter()
            .map(|event| event["phase"].as_str().expect("phase should be a string"))
            .collect::<Vec<_>>();

        assert_eq!(
            phases,
            [
                "Repository Ingestion",
                "Repository Replay",
                "Render Configuration",
                "Render",
                "Video Export",
                "Export Manifest"
            ]
        );
        assert!(events
            .iter()
            .all(|event| event["event"].as_str() == Some("render_progress")));
    }

    #[test]
    fn render_rejects_missing_repository_path() {
        let error = run([
            "render".to_owned(),
            "does-not-exist".to_owned(),
            "--output".to_owned(),
            "out.mp4".to_owned(),
        ])
        .expect_err("missing repository path should fail");

        assert!(error.contains("repository path does not exist"));
        assert!(error.contains("does-not-exist"));
    }

    #[test]
    fn render_requires_output_path() {
        let error = run(["render".to_owned(), ".".to_owned()])
            .expect_err("missing output target should fail");

        assert!(error.contains("missing required --output <output-path>"));
    }

    #[test]
    fn render_rejects_missing_configuration_file() {
        let error = run([
            "render".to_owned(),
            ".".to_owned(),
            "--output".to_owned(),
            "out.mp4".to_owned(),
            "--config".to_owned(),
            "render.toml".to_owned(),
        ])
        .expect_err("missing Render Configuration file should fail");

        assert!(error.contains("failed to read Render Configuration render.toml"));
    }

    #[test]
    fn render_loads_and_reports_configuration_file() {
        let config_path = write_temp_render_config(
            "valid",
            r##"
frame_width = 1280
frame_height = 720
frames_per_second = 30

[theme]
name = "terminal"
background_color = "#101010"
entity_color = "#32d583"
contributor_color = "#fdb022"

[layout]
kind = "repository_graph"
entity_spacing = 140
settle_iterations = 80
"##,
        );

        let output = run([
            "render".to_owned(),
            ".".to_owned(),
            "--output".to_owned(),
            "out.mp4".to_owned(),
            "--config".to_owned(),
            config_path.display().to_string(),
        ])
        .expect("valid Render Configuration should load");

        assert!(output.contains("Render Configuration:"));
        assert!(output.contains("terminal"));
        assert!(output.contains("1280x720"));
        assert!(output.contains("30 FPS"));
        assert!(output.contains("Repository Graph"));
    }

    #[test]
    fn render_rejects_invalid_configuration_file_with_diagnostics() {
        let config_path = write_temp_render_config(
            "invalid",
            r##"
frame_width = 0
frame_height = 720
frames_per_second = 30

[theme]
name = "bad"
background_color = "blue"
entity_color = "#32d583"
contributor_color = "#fdb022"

[layout]
kind = "repository_graph"
entity_spacing = 140
settle_iterations = 80
"##,
        );

        let error = run([
            "render".to_owned(),
            ".".to_owned(),
            "--output".to_owned(),
            "out.mp4".to_owned(),
            "--config".to_owned(),
            config_path.display().to_string(),
        ])
        .expect_err("invalid Render Configuration should fail");

        assert!(error.contains("invalid Render Configuration"));
        assert!(error.contains("frame_width"));
        assert!(error.contains("theme.background_color"));
        assert!(error.contains("#RRGGBB"));
    }

    fn write_temp_render_config(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "gitflux-{name}-{}-{}.toml",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::write(&path, contents).expect("temp Render Configuration should be written");
        path
    }
}
