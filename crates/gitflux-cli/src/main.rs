//! Command-line adapter for Gitflux.
//!
//! The CLI is the imperative shell for Repository Replay workflows. It keeps
//! argument handling separate from Repository Ingestion, scene construction,
//! Render Configuration, GPU rendering, and Video Export orchestration.

use std::env;
use std::fs;
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::process::ExitCode;

use gitflux_export::{
    export_video, ExportManifest, FfmpegBinary, VideoExportOptions, VideoExportRequest,
};
use gitflux_ingestion::{ingest_repository, RepositoryIngestionRequest};
use gitflux_render::{FrameCount, RenderPlan};
use gitflux_scene::{Layout, Mainline, RenderConfiguration};

const HELP: &str = "\
Gitflux command-line interface

Usage:
  gitflux [OPTIONS]
  gitflux render <repository-path> --output <output-path>

Options:
  -h, --help       Print help
  -V, --version    Print version
  --ffmpeg-path    Use a specific FFmpeg binary for Video Export
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
    let execution = execute_render(&config)?;

    if config.json {
        return Ok(render_json_progress(&config, &execution));
    }

    Ok(render_human_summary(&config, &execution))
}

fn execute_render(config: &RenderCommand) -> Result<RenderExecution, String> {
    let ingestion_request = config.ingestion_request();
    let replay = ingest_repository(&ingestion_request)
        .map_err(|error| format!("failed to ingest repository: {error}"))?
        .replay()
        .clone();
    let export_request = VideoExportRequest::try_new(
        RenderPlan::new(replay, config.render_configuration.clone()),
        &config.output_path,
    )
    .map_err(|error| error.to_string())?;
    let export_options = VideoExportOptions::new(
        config.ffmpeg_binary.clone(),
        FrameCount::new(NonZeroU64::new(1).expect("one frame is non-zero")),
    );
    let export_manifest =
        export_video(&export_request, &export_options).map_err(|error| error.to_string())?;

    Ok(RenderExecution { export_manifest })
}

fn render_human_summary(config: &RenderCommand, execution: &RenderExecution) -> String {
    let mut output = String::new();
    output.push_str("Gitflux Render complete\n");
    output.push_str(&format!(
        "Repository path: {}\n",
        config.repository_path.display()
    ));
    output.push_str(&format!(
        "Output target: {}\n",
        config.output_path.display()
    ));
    output.push_str(&format!("Mainline: {}\n", config.mainline.as_label()));
    output.push_str(&format!(
        "FFmpeg: {}\n",
        config.ffmpeg_binary.path().display()
    ));
    output.push_str(&format!(
        "Exported: {}\n",
        execution.export_manifest.output_path().display()
    ));
    output.push_str(&format!(
        "Render Configuration: {}\n",
        config.render_configuration_label()
    ));

    for phase in RENDER_PHASES {
        output.push_str(&format!("- {phase}\n"));
    }

    output
}

fn render_json_progress(config: &RenderCommand, execution: &RenderExecution) -> String {
    let mut output = String::new();

    for (index, phase) in RENDER_PHASES.iter().enumerate() {
        let event = serde_json::json!({
            "event": "render_progress",
            "phase": phase,
            "phase_index": index,
            "phase_count": RENDER_PHASES.len(),
            "mainline": config.mainline.as_label(),
            "render_configuration": config.render_configuration_label(),
            "output_path": execution.export_manifest.output_path(),
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
    let mut ffmpeg_binary = FfmpegBinary::default();
    let mut mainline = MainlineSelection::Detect;
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
            "--mainline" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing Mainline name after --mainline".to_owned())?;
                mainline = MainlineSelection::Explicit(Mainline::new(value));
            }
            "--ffmpeg-path" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing FFmpeg path after --ffmpeg-path".to_owned())?;
                ffmpeg_binary = FfmpegBinary::from_path(value);
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
        ffmpeg_binary,
        mainline,
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
    ffmpeg_binary: FfmpegBinary,
    mainline: MainlineSelection,
    render_configuration: RenderConfiguration,
    json: bool,
}

struct RenderExecution {
    export_manifest: ExportManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MainlineSelection {
    Detect,
    Explicit(Mainline),
}

impl MainlineSelection {
    fn as_label(&self) -> &str {
        match self {
            Self::Detect => "auto",
            Self::Explicit(mainline) => mainline.as_str(),
        }
    }
}

impl RenderCommand {
    fn ingestion_request(&self) -> RepositoryIngestionRequest {
        match &self.mainline {
            MainlineSelection::Detect => {
                RepositoryIngestionRequest::detect_mainline(&self.repository_path)
            }
            MainlineSelection::Explicit(mainline) => {
                RepositoryIngestionRequest::new(&self.repository_path, mainline.clone())
            }
        }
    }

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
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

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
        let fixture = CliGitFixture::new("human-readable");
        let ffmpeg = write_fake_ffmpeg("human-readable");
        let output = run([
            "render".to_owned(),
            fixture.path().display().to_string(),
            "--output".to_owned(),
            fixture.path().join("out.mp4").display().to_string(),
            "--ffmpeg-path".to_owned(),
            ffmpeg.display().to_string(),
        ])
        .expect("render should export");

        assert!(output.contains("Repository Ingestion"));
        assert!(output.contains("Repository Replay"));
        assert!(output.contains("Render Configuration"));
        assert!(output.contains("Render"));
        assert!(output.contains("Video Export"));
        assert!(output.contains("Export Manifest"));
    }

    #[test]
    fn render_reports_cli_mainline_override() {
        let fixture = CliGitFixture::new("mainline");
        let ffmpeg = write_fake_ffmpeg("mainline");
        let output = run([
            "render".to_owned(),
            fixture.path().display().to_string(),
            "--output".to_owned(),
            fixture.path().join("out.mp4").display().to_string(),
            "--ffmpeg-path".to_owned(),
            ffmpeg.display().to_string(),
            "--mainline".to_owned(),
            "main".to_owned(),
        ])
        .expect("render should accept explicit Mainline");

        assert!(output.contains("Mainline: main"));
    }

    #[test]
    fn render_reports_cli_ffmpeg_path_override() {
        let fixture = CliGitFixture::new("ffmpeg-path");
        let ffmpeg = write_fake_ffmpeg("ffmpeg-path");
        let output = run([
            "render".to_owned(),
            fixture.path().display().to_string(),
            "--output".to_owned(),
            fixture.path().join("out.mp4").display().to_string(),
            "--ffmpeg-path".to_owned(),
            ffmpeg.display().to_string(),
        ])
        .expect("render should accept explicit FFmpeg path");

        assert!(output.contains(&format!("FFmpeg: {}", ffmpeg.display())));
    }

    #[test]
    fn render_rejects_missing_ffmpeg_path_value() {
        let error = run([
            "render".to_owned(),
            ".".to_owned(),
            "--output".to_owned(),
            "out.mp4".to_owned(),
            "--ffmpeg-path".to_owned(),
        ])
        .expect_err("missing FFmpeg path should fail");

        assert!(error.contains("missing FFmpeg path after --ffmpeg-path"));
    }

    #[test]
    fn render_json_reports_structured_progress_events() {
        let fixture = CliGitFixture::new("json");
        let ffmpeg = write_fake_ffmpeg("json");
        let output = run([
            "render".to_owned(),
            fixture.path().display().to_string(),
            "--output".to_owned(),
            fixture.path().join("out.mp4").display().to_string(),
            "--ffmpeg-path".to_owned(),
            ffmpeg.display().to_string(),
            "--json".to_owned(),
        ])
        .expect("JSON render should export");
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
        assert!(fixture.path().join("out.mp4").exists());
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
        let fixture = CliGitFixture::new("config");
        let ffmpeg = write_fake_ffmpeg("config");
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
            fixture.path().display().to_string(),
            "--output".to_owned(),
            fixture.path().join("out.mp4").display().to_string(),
            "--ffmpeg-path".to_owned(),
            ffmpeg.display().to_string(),
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

    struct CliGitFixture {
        path: PathBuf,
    }

    impl CliGitFixture {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "gitflux-cli-{name}-{}-{}",
                std::process::id(),
                std::thread::current().name().unwrap_or("test")
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("fixture directory should be created");
            run_git(&path, ["init", "-b", "main"]);
            run_git(&path, ["config", "user.name", "Ada"]);
            run_git(&path, ["config", "user.email", "ada@example.test"]);
            fs::write(path.join("README.md"), "hello\n").expect("fixture file should be written");
            run_git(&path, ["add", "README.md"]);
            run_git(&path, ["commit", "--no-gpg-sign", "-m", "Initial commit"]);

            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    fn run_git<const N: usize>(cwd: &Path, args: [&str; N]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", "1970-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "1970-01-01T00:00:00Z")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git command should launch");
        assert!(status.success(), "git command should succeed");
    }

    #[cfg(unix)]
    fn write_fake_ffmpeg(name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!(
            "gitflux-cli-fake-ffmpeg-{name}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::write(
            &path,
            "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then exit 0; fi\nfor output_path do :; done\nprintf 'video\\n' > \"$output_path\"\n",
        )
        .expect("fake FFmpeg should be written");
        let mut permissions = fs::metadata(&path)
            .expect("fake FFmpeg metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("fake FFmpeg should be executable");
        path
    }

    #[cfg(not(unix))]
    fn write_fake_ffmpeg(_name: &str) -> PathBuf {
        panic!("fake FFmpeg test helper is only implemented for Unix test runners")
    }
}
