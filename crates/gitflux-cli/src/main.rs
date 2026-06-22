//! Command-line adapter for Gitflux.
//!
//! The CLI is the imperative shell for Repository Replay workflows. It keeps
//! argument handling separate from Repository Ingestion, scene construction,
//! Render Configuration, GPU rendering, and Video Export orchestration.

use std::env;
use std::fs;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use gitflux_export::{
    export_video_with_manifest_context, ExportManifest, ExportManifestContext,
    ExportOutputSettings, ExportPacing, ExportProgressEvent, ExportProgressPhase,
    ExportRepositoryIdentity, ExportSelectedRefState, FfmpegBinary, VideoExportOptions,
    VideoExportRequest,
};
use gitflux_ingestion::{ingest_repository, RepositoryIngestionRequest};
use gitflux_preview::{
    run_preview_session, NativePreviewWindowAdapter, PreviewConfigurationSource,
    PreviewReloadPolicy, PreviewReloadWatch, PreviewRunSummary, PreviewSessionPlan,
    PreviewWindowAdapter, PreviewWindowStatus,
};
use gitflux_render::{FrameCount, OffscreenRenderer, RenderPlan, RendererSettings};
use gitflux_scene::{
    Layout, Mainline, RenderConfiguration, ReplayPacingDuration, RepositoryGraphScene,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

const HELP: &str = "\
Gitflux command-line interface

Usage:
  gitflux [OPTIONS]
  gitflux render <repository-path> --output <output-path>
  gitflux preview <repository-path>
  gitflux diagnostics <repository-path>

Options:
  -h, --help       Print help
  -V, --version    Print version
  --ffmpeg-path    Use a specific FFmpeg binary for Video Export
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
    let mut args = args.into_iter();

    match args.next().as_deref() {
        None | Some("-h") | Some("--help") => Ok(HELP.to_owned()),
        Some("-V") | Some("--version") => Ok(format!("gitflux {}\n", env!("CARGO_PKG_VERSION"))),
        Some("render") => run_render(args),
        Some("preview") => run_preview(args),
        Some("diagnostics") | Some("doctor") => run_diagnostics(args),
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

fn run_preview(args: impl IntoIterator<Item = String>) -> Result<String, String> {
    let config = parse_preview_args(args)?;
    let execution = execute_preview(&config)?;

    Ok(preview_human_summary(&config, &execution))
}

fn run_diagnostics(args: impl IntoIterator<Item = String>) -> Result<String, String> {
    let config = parse_diagnostics_args(args)?;
    let report = execute_diagnostics(&config);

    if config.json {
        return serde_json::to_string_pretty(&report)
            .map(|json| format!("{json}\n"))
            .map_err(|error| format!("failed to serialize diagnostics: {error}"));
    }

    Ok(diagnostics_human_summary(&report))
}

fn execute_render(config: &RenderCommand) -> Result<RenderExecution, String> {
    let ingestion_request = config.ingestion_request();
    let ingestion_summary = ingest_repository(&ingestion_request)
        .map_err(|error| format!("failed to ingest repository: {error}"))?;
    let replay = ingestion_summary.replay().clone();
    let mut progress_events = vec![ExportProgressEvent::new(ExportProgressPhase::Ingestion)
        .with_detail(
            "repository_path",
            config.repository_path.display().to_string(),
        )
        .with_detail("mainline", replay.mainline().as_str())];
    progress_events.push(
        ExportProgressEvent::new(ExportProgressPhase::SceneConstruction)
            .with_detail("commit_events", replay.commit_events().len().to_string()),
    );
    let render_plan = RenderPlan::new(replay, config.render_configuration.clone());
    let frame_count = render_frame_count(&render_plan);
    let export_request = VideoExportRequest::try_new(render_plan, &config.output_path)
        .map_err(|error| error.to_string())?;
    let export_options = VideoExportOptions::new(config.ffmpeg_binary.clone(), frame_count);
    let manifest_context = config
        .manifest_context(&export_request)?
        .with_progress_events(progress_events.clone());
    let mut export_progress = Vec::new();
    let export_manifest = export_video_with_manifest_context(
        &export_request,
        &export_options,
        manifest_context,
        |event| export_progress.push(event),
    )
    .map_err(|error| error.to_string())?;
    progress_events.extend(export_progress);

    Ok(RenderExecution {
        export_manifest,
        progress_events,
    })
}

fn execute_preview(config: &PreviewCommand) -> Result<PreviewExecution, String> {
    let mut adapter = NativePreviewWindowAdapter::default();
    execute_preview_with_adapter(config, &mut adapter)
}

fn render_frame_count(render_plan: &RenderPlan) -> FrameCount {
    let scene =
        RepositoryGraphScene::from_replay(render_plan.replay(), render_plan.configuration());
    let last_activity_frame = scene
        .activities()
        .iter()
        .map(|activity| {
            activity.playback_frame()
                + activity
                    .file_changes()
                    .iter()
                    .map(|file_change| file_change.playback_frame_offset())
                    .max()
                    .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);

    FrameCount::new(NonZeroU64::new(last_activity_frame + 1).expect("frame count is non-zero"))
}

fn execute_preview_with_adapter(
    config: &PreviewCommand,
    adapter: &mut dyn PreviewWindowAdapter,
) -> Result<PreviewExecution, String> {
    let ingestion_request = config.ingestion_request();
    let ingestion_summary = ingest_repository(&ingestion_request)
        .map_err(|error| format!("failed to ingest repository: {error}"))?;
    let replay = ingestion_summary.replay().clone();
    let plan = PreviewSessionPlan::new(
        RenderPlan::new(replay, config.render_configuration.clone()),
        config.configuration_source(),
        config.reload_policy,
    );
    let config_watcher = plan
        .config_watcher()
        .map_err(|error| error.to_string())?
        .is_some();
    let summary = run_preview_session(&plan, adapter).map_err(|error| error.to_string())?;

    Ok(PreviewExecution {
        summary,
        config_watcher,
    })
}

fn execute_diagnostics(config: &DiagnosticsCommand) -> DiagnosticsReport {
    execute_diagnostics_with_probes(config, probe_gpu_backend, |binary| {
        probe_ffmpeg_binary(binary)
    })
}

fn execute_diagnostics_with_probes(
    config: &DiagnosticsCommand,
    gpu_probe: impl FnOnce() -> GpuDiagnostic,
    ffmpeg_probe: impl FnOnce(&FfmpegBinary) -> FfmpegDiagnostic,
) -> DiagnosticsReport {
    let refs = inspect_repository_refs(config);
    let cache = inspect_cache_readiness(&config.repository_path, &refs);

    DiagnosticsReport {
        schema_version: 1,
        repository_path: config.repository_path.display().to_string(),
        gpu: gpu_probe(),
        ffmpeg: ffmpeg_probe(&config.ffmpeg_binary),
        refs,
        cache,
    }
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

    for event in &execution.progress_events {
        output.push_str(&format!("- {}\n", render_phase_label(event.phase())));
    }

    output
}

fn render_json_progress(_config: &RenderCommand, execution: &RenderExecution) -> String {
    let mut output = String::new();

    for event in &execution.progress_events {
        output.push_str(
            &serde_json::to_string(event).expect("ExportProgressEvent should serialize to JSON"),
        );
        output.push('\n');
    }

    output
}

fn preview_human_summary(config: &PreviewCommand, execution: &PreviewExecution) -> String {
    let mut output = String::new();
    let window = execution.summary.window();

    output.push_str("Gitflux Preview complete\n");
    output.push_str(&format!(
        "Repository path: {}\n",
        config.repository_path.display()
    ));
    output.push_str(&format!("Mainline: {}\n", config.mainline.as_label()));
    output.push_str(&format!(
        "Render Configuration: {}\n",
        config.render_configuration_label()
    ));
    output.push_str(&format!(
        "Window: {} ({}x{})\n",
        preview_window_status_label(execution.summary.window_status()),
        window.width(),
        window.height()
    ));
    output.push_str(&format!(
        "Shared renderer frame: {}\n",
        if execution.summary.rendered_initial_frame() {
            "rendered"
        } else {
            "not rendered"
        }
    ));
    output.push_str(&format!(
        "Config hot reload: {}\n",
        preview_reload_label(execution.summary.reload_watch(), execution.config_watcher)
    ));

    output
}

fn diagnostics_human_summary(report: &DiagnosticsReport) -> String {
    let mut output = String::new();

    output.push_str("Gitflux Diagnostics\n");
    output.push_str(&format!("Repository path: {}\n", report.repository_path));
    output.push_str(&format!(
        "GPU/backend: {}",
        readiness_label(report.gpu.ready)
    ));
    if let Some(detail) = &report.gpu.detail {
        output.push_str(&format!(" ({detail})"));
    }
    output.push('\n');
    output.push_str(&format!(
        "FFmpeg: {} at {}",
        readiness_label(report.ffmpeg.available),
        report.ffmpeg.path
    ));
    if let Some(version) = &report.ffmpeg.version {
        output.push_str(&format!(" ({version})"));
    }
    if let Some(error) = &report.ffmpeg.error {
        output.push_str(&format!(" ({error})"));
    }
    output.push('\n');
    output.push_str(&format!("Ref selection: {}\n", report.refs.selection_mode));
    output.push_str(&format!("Mainline: {}\n", report.refs.mainline));
    if let Some(tip) = &report.refs.mainline_tip {
        output.push_str(&format!("Mainline tip: {tip}\n"));
    }
    if let Some(head) = &report.refs.head {
        output.push_str(&format!("HEAD: {head}\n"));
    }
    if !report.refs.local_branches.is_empty() {
        output.push_str(&format!(
            "Local branches: {}\n",
            report.refs.local_branches.join(", ")
        ));
    }
    if let Some(error) = &report.refs.error {
        output.push_str(&format!("Ref diagnostics: {error}\n"));
    }
    output.push_str(&format!("Cache location: {}\n", report.cache.location));
    output.push_str(&format!("Cache status: {}\n", report.cache.status));
    output.push_str(&format!(
        "Cache match policy: {}\n",
        report.cache.match_policy
    ));
    output.push_str(&format!(
        "Cache matches current inputs: {}\n",
        match report.cache.matches_current_inputs {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unknown",
        }
    ));
    output.push_str(&format!("Cache detail: {}\n", report.cache.detail));

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
        return Err(missing_repository_path_error(&repository_path));
    }

    validate_repository_directory(&repository_path)?;

    let render_configuration =
        load_render_configuration(config_path.as_ref(), "Render Configuration")?;

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

fn parse_preview_args(args: impl IntoIterator<Item = String>) -> Result<PreviewCommand, String> {
    let mut args = args.into_iter();
    let repository_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing repository path\n\n{HELP}"))?;
    let mut config_path = None;
    let mut mainline = MainlineSelection::Detect;
    let mut reload_policy = PreviewReloadPolicy::ConfigFileMetadata;

    while let Some(arg) = args.next() {
        match arg.as_str() {
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
            "--no-config-reload" => reload_policy = PreviewReloadPolicy::Disabled,
            flag => return Err(format!("unrecognized preview option: {flag}")),
        }
    }

    if !repository_path.exists() {
        return Err(missing_repository_path_error(&repository_path));
    }

    validate_repository_directory(&repository_path)?;

    let render_configuration =
        load_render_configuration(config_path.as_ref(), "Preview Render Configuration")?;

    Ok(PreviewCommand {
        repository_path,
        config_path,
        mainline,
        render_configuration,
        reload_policy,
    })
}

fn parse_diagnostics_args(
    args: impl IntoIterator<Item = String>,
) -> Result<DiagnosticsCommand, String> {
    let mut args = args.into_iter();
    let repository_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing repository path\n\n{HELP}"))?;
    let mut ffmpeg_binary = FfmpegBinary::default();
    let mut mainline = MainlineSelection::Detect;
    let mut json = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ffmpeg-path" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing FFmpeg path after --ffmpeg-path".to_owned())?;
                ffmpeg_binary = FfmpegBinary::from_path(value);
            }
            "--mainline" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing Mainline name after --mainline".to_owned())?;
                mainline = MainlineSelection::Explicit(Mainline::new(value));
            }
            "--json" => json = true,
            flag => return Err(format!("unrecognized diagnostics option: {flag}")),
        }
    }

    if !repository_path.exists() {
        return Err(missing_repository_path_error(&repository_path));
    }

    validate_repository_directory(&repository_path)?;

    Ok(DiagnosticsCommand {
        repository_path,
        ffmpeg_binary,
        mainline,
        json,
    })
}

fn load_render_configuration(
    config_path: Option<&PathBuf>,
    configuration_label: &str,
) -> Result<RenderConfiguration, String> {
    let Some(config_path) = config_path else {
        return Ok(RenderConfiguration::default());
    };

    let contents = fs::read_to_string(config_path).map_err(|error| {
        format!(
            "failed to read {configuration_label} {}: {error}",
            config_path.display()
        )
    })?;

    RenderConfiguration::from_toml_str(&contents).map_err(|error| {
        format!(
            "failed to load {configuration_label} {}:\n{error}",
            config_path.display()
        )
    })
}

fn validate_repository_directory(repository_path: &Path) -> Result<(), String> {
    if !repository_path.is_dir() {
        return Err(format!(
            "repository path is not a directory: {}",
            repository_path.display()
        ));
    }

    Ok(())
}

fn missing_repository_path_error(repository_path: &Path) -> String {
    format!(
        "repository path does not exist: {}",
        repository_path.display()
    )
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
    progress_events: Vec<ExportProgressEvent>,
}

struct PreviewCommand {
    repository_path: PathBuf,
    config_path: Option<PathBuf>,
    mainline: MainlineSelection,
    render_configuration: RenderConfiguration,
    reload_policy: PreviewReloadPolicy,
}

struct PreviewExecution {
    summary: PreviewRunSummary,
    config_watcher: bool,
}

struct DiagnosticsCommand {
    repository_path: PathBuf,
    ffmpeg_binary: FfmpegBinary,
    mainline: MainlineSelection,
    json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DiagnosticsReport {
    schema_version: u32,
    repository_path: String,
    gpu: GpuDiagnostic,
    ffmpeg: FfmpegDiagnostic,
    refs: RefDiagnostic,
    cache: CacheDiagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GpuDiagnostic {
    backend: String,
    ready: bool,
    detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FfmpegDiagnostic {
    path: String,
    available: bool,
    version: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RefDiagnostic {
    selection_mode: String,
    mainline: String,
    mainline_tip: Option<String>,
    head: Option<String>,
    local_branches: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CacheDiagnostic {
    location: String,
    status: String,
    match_policy: String,
    current_inputs_key: String,
    matches_current_inputs: Option<bool>,
    detail: String,
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

    fn selection_mode(&self) -> &str {
        match self {
            Self::Detect => "detected",
            Self::Explicit(_) => "explicit",
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
        render_configuration_label(&self.render_configuration, self.config_path.as_ref())
    }

    fn manifest_context(
        &self,
        export_request: &VideoExportRequest,
    ) -> Result<ExportManifestContext, String> {
        let repository = git2::Repository::open(&self.repository_path).map_err(|error| {
            format!(
                "failed to inspect repository identity {}: {error}",
                self.repository_path.display()
            )
        })?;
        let selected_mainline = export_request.render_plan().replay().mainline().as_str();
        let mainline_ref = format!("refs/heads/{selected_mainline}");
        let mainline_tip = repository
            .revparse_single(&mainline_ref)
            .ok()
            .map(|object| object.id().to_string());
        let remote_url = repository
            .find_remote("origin")
            .ok()
            .and_then(|remote| remote.url().map(ToOwned::to_owned));
        let frame_size = self.render_configuration.frame_size();

        Ok(ExportManifestContext::new(
            ExportRepositoryIdentity::new(self.repository_path.display().to_string(), remote_url),
            ExportSelectedRefState::new(
                self.mainline.selection_mode(),
                selected_mainline,
                mainline_tip,
            ),
            self.config_hash(),
            self.export_pacing(),
            ExportOutputSettings::new(
                export_request.output_path(),
                export_request.output_preset(),
                frame_size.width(),
                frame_size.height(),
                self.render_configuration.frames_per_second().get(),
            ),
            env!("CARGO_PKG_VERSION"),
        ))
    }

    fn config_hash(&self) -> String {
        let mut hasher = Sha256::new();
        if let Some(config_path) = &self.config_path {
            if let Ok(contents) = fs::read(config_path) {
                hasher.update(contents);
            }
        } else {
            hasher.update(b"default:");
            hasher.update(self.render_configuration_label().as_bytes());
        }

        format!("sha256:{:x}", hasher.finalize())
    }

    fn export_pacing(&self) -> ExportPacing {
        let pacing = self.render_configuration.replay_pacing();
        let duration = match pacing.duration() {
            ReplayPacingDuration::Auto => "auto".to_owned(),
            ReplayPacingDuration::Target { duration_seconds } => {
                format!("target:{duration_seconds}s")
            }
        };

        ExportPacing::new("adaptive", duration)
    }
}

impl PreviewCommand {
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

    fn configuration_source(&self) -> PreviewConfigurationSource {
        self.config_path
            .clone()
            .map(PreviewConfigurationSource::File)
            .unwrap_or(PreviewConfigurationSource::Defaults)
    }

    fn render_configuration_label(&self) -> String {
        render_configuration_label(&self.render_configuration, self.config_path.as_ref())
    }
}

impl DiagnosticsCommand {
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
}

fn probe_gpu_backend() -> GpuDiagnostic {
    match OffscreenRenderer::new(RendererSettings::default()) {
        Ok(_) => GpuDiagnostic {
            backend: "wgpu-offscreen".to_owned(),
            ready: true,
            detail: Some("offscreen renderer initialized".to_owned()),
        },
        Err(error) => GpuDiagnostic {
            backend: "wgpu-offscreen".to_owned(),
            ready: false,
            detail: Some(error.to_string()),
        },
    }
}

fn probe_ffmpeg_binary(binary: &FfmpegBinary) -> FfmpegDiagnostic {
    match Command::new(binary.path()).arg("-version").output() {
        Ok(output) if output.status.success() => {
            let version =
                first_nonempty_line(&output.stdout).or_else(|| first_nonempty_line(&output.stderr));

            if version
                .as_deref()
                .is_some_and(|line| line.starts_with("ffmpeg version "))
            {
                FfmpegDiagnostic {
                    path: binary.path().display().to_string(),
                    available: true,
                    version,
                    error: None,
                }
            } else {
                FfmpegDiagnostic {
                    path: binary.path().display().to_string(),
                    available: false,
                    version: None,
                    error: Some(
                        version
                            .map(|line| {
                                format!(
                                    "version probe did not produce recognizable FFmpeg output: {line}"
                                )
                            })
                            .unwrap_or_else(|| {
                                "version probe did not produce recognizable FFmpeg output"
                                    .to_owned()
                            }),
                    ),
                }
            }
        }
        Ok(output) => FfmpegDiagnostic {
            path: binary.path().display().to_string(),
            available: false,
            version: None,
            error: Some(format!(
                "version probe exited with {}{}",
                output
                    .status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".to_owned()),
                command_output_detail(&output.stderr)
            )),
        },
        Err(error) => FfmpegDiagnostic {
            path: binary.path().display().to_string(),
            available: false,
            version: None,
            error: Some(error.to_string()),
        },
    }
}

fn first_nonempty_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn command_output_detail(bytes: &[u8]) -> String {
    first_nonempty_line(bytes)
        .map(|line| format!(": {line}"))
        .unwrap_or_default()
}

fn inspect_repository_refs(config: &DiagnosticsCommand) -> RefDiagnostic {
    let mut diagnostic = RefDiagnostic {
        selection_mode: config.mainline.selection_mode().to_owned(),
        mainline: config.mainline.as_label().to_owned(),
        mainline_tip: None,
        head: None,
        local_branches: Vec::new(),
        error: None,
    };

    let repository = match git2::Repository::open(&config.repository_path) {
        Ok(repository) => repository,
        Err(error) => {
            diagnostic.error = Some(format!("failed to open Git repository: {error}"));
            return diagnostic;
        }
    };

    diagnostic.head = repository.head().ok().and_then(|head| {
        head.shorthand()
            .map(ToOwned::to_owned)
            .or_else(|| head.target().map(|target| target.to_string()))
    });
    diagnostic.local_branches = local_branch_names(&repository);

    match ingest_repository(&config.ingestion_request()) {
        Ok(summary) => {
            diagnostic.mainline = summary.replay().mainline().as_str().to_owned();
            diagnostic.mainline_tip = mainline_tip(&repository, summary.replay().mainline());
        }
        Err(error) => {
            diagnostic.error = Some(error.to_string());
        }
    }

    diagnostic
}

fn local_branch_names(repository: &git2::Repository) -> Vec<String> {
    let mut branches = Vec::new();
    if let Ok(iter) = repository.branches(Some(git2::BranchType::Local)) {
        for branch in iter.flatten() {
            if let Ok(Some(name)) = branch.0.name() {
                branches.push(name.to_owned());
            }
        }
    }
    branches.sort();
    branches
}

fn mainline_tip(repository: &git2::Repository, mainline: &Mainline) -> Option<String> {
    repository
        .revparse_single(&format!("refs/heads/{}", mainline.as_str()))
        .ok()
        .map(|object| object.id().to_string())
}

fn inspect_cache_readiness(repository_path: &Path, refs: &RefDiagnostic) -> CacheDiagnostic {
    let location = git2::Repository::open(repository_path)
        .ok()
        .map(|repository| repository.path().join("gitflux").join("cache"))
        .unwrap_or_else(|| repository_path.join(".git").join("gitflux").join("cache"));
    let status = if location.exists() {
        "present_without_metadata"
    } else {
        "not_present"
    };
    let current_inputs_key = diagnostic_inputs_key(repository_path, refs);

    CacheDiagnostic {
        location: location.display().to_string(),
        status: status.to_owned(),
        match_policy: "disabled_until_cache_metadata_exists".to_owned(),
        current_inputs_key,
        matches_current_inputs: Some(false),
        detail: "Gitflux has no cache metadata subsystem yet, so diagnostics cannot claim any cached data matches the current repository inputs.".to_owned(),
    }
}

fn diagnostic_inputs_key(repository_path: &Path, refs: &RefDiagnostic) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repository_path.display().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(refs.selection_mode.as_bytes());
    hasher.update(b"\0");
    hasher.update(refs.mainline.as_bytes());
    hasher.update(b"\0");
    if let Some(tip) = &refs.mainline_tip {
        hasher.update(tip.as_bytes());
    }

    format!("sha256:{:x}", hasher.finalize())
}

fn readiness_label(ready: bool) -> &'static str {
    if ready {
        "ready"
    } else {
        "unavailable"
    }
}

fn render_phase_label(phase: ExportProgressPhase) -> &'static str {
    match phase {
        ExportProgressPhase::Ingestion => "Repository Ingestion",
        ExportProgressPhase::SceneConstruction => "Scene Construction",
        ExportProgressPhase::FrameRendering => "Frame Rendering",
        ExportProgressPhase::FfmpegEncoding => "FFmpeg Encoding",
        ExportProgressPhase::ManifestWriting => "Export Manifest",
        ExportProgressPhase::Warnings => "Warnings",
        ExportProgressPhase::Completion => "Completion",
    }
}

fn render_configuration_label(
    render_configuration: &RenderConfiguration,
    config_path: Option<&PathBuf>,
) -> String {
    let source = config_path
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "defaults".to_owned());
    let frame_size = render_configuration.frame_size();
    let layout_name = match render_configuration.layout() {
        Layout::RepositoryGraph | Layout::RepositoryGraphWithParameters(_) => "Repository Graph",
        Layout::Named(_) => "Named Layout",
    };

    format!(
        "{} (theme: {}, {}x{}, {} FPS, layout: {})",
        source,
        render_configuration.theme().name(),
        frame_size.width(),
        frame_size.height(),
        render_configuration.frames_per_second().get(),
        layout_name
    )
}

fn preview_window_status_label(status: &PreviewWindowStatus) -> &'static str {
    match status {
        PreviewWindowStatus::Opened => "opened",
        PreviewWindowStatus::Planned(_) => "planned by minimal adapter",
    }
}

fn preview_reload_label(status: PreviewReloadWatch, watcher_initialized: bool) -> &'static str {
    match (status, watcher_initialized) {
        (PreviewReloadWatch::WatchingConfigFile, true) => "watching config file metadata",
        (PreviewReloadWatch::WatchingConfigFile, false) => "planned for config file metadata",
        (PreviewReloadWatch::NoConfigFile, _) => "no config file supplied",
        (PreviewReloadWatch::Disabled, _) => "disabled",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        diagnostics_human_summary, execute_diagnostics_with_probes, execute_preview_with_adapter,
        parse_diagnostics_args, parse_preview_args, parse_render_args, preview_human_summary,
        probe_ffmpeg_binary, run, FfmpegBinary, FfmpegDiagnostic, GpuDiagnostic,
    };
    use gitflux_preview::{PlanningPreviewWindowAdapter, PreviewReloadPolicy};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::UNIX_EPOCH;

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
        assert!(output.contains("Scene Construction"));
        assert!(output.contains("Frame Rendering"));
        assert!(output.contains("FFmpeg Encoding"));
        assert!(output.contains("Export Manifest"));
        assert!(output.contains("Warnings"));
        assert!(output.contains("Completion"));
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
        let output_path = fixture.path().join("out.mp4");
        fs::write(fixture.path().join("second.txt"), "second\n")
            .expect("second fixture file should be written");
        run_git(fixture.path(), ["add", "second.txt"]);
        run_git(
            fixture.path(),
            ["commit", "--no-gpg-sign", "-m", "Second commit"],
        );
        let config_path = write_temp_render_config(
            "json-render",
            r##"
frame_width = 64
frame_height = 36
frames_per_second = 2

[theme]
name = "json-test"
background_color = "#101010"
entity_color = "#32d583"
contributor_color = "#fdb022"

[layout]
kind = "repository_graph"
entity_spacing = 40
settle_iterations = 8
"##,
        );
        let output = run([
            "render".to_owned(),
            fixture.path().display().to_string(),
            "--output".to_owned(),
            output_path.display().to_string(),
            "--ffmpeg-path".to_owned(),
            ffmpeg.display().to_string(),
            "--config".to_owned(),
            config_path.display().to_string(),
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
                "ingestion",
                "scene_construction",
                "frame_rendering",
                "ffmpeg_encoding",
                "manifest_writing",
                "warnings",
                "completion"
            ]
        );
        assert!(events
            .iter()
            .all(|event| event["event"].as_str() == Some("render_progress")));
        assert!(events
            .iter()
            .all(|event| event["timestamp_unix_ms"].as_u64().is_some()));
        let rendered_frame_count = events
            .iter()
            .find(|event| event["phase"].as_str() == Some("frame_rendering"))
            .and_then(|event| event["details"]["frame_count"].as_str())
            .and_then(|frame_count| frame_count.parse::<u64>().ok())
            .expect("frame rendering event should include frame count");
        assert!(rendered_frame_count > 1);
        assert!(output_path.exists());
        let manifest_path = output_path.with_file_name("out.mp4.gitflux-manifest.json");
        assert!(manifest_path.exists());
        let completion_timestamp = events
            .iter()
            .find(|event| event["phase"].as_str() == Some("completion"))
            .and_then(|event| event["timestamp_unix_ms"].as_u64())
            .expect("completion timestamp should be present");
        let manifest_modified_timestamp = manifest_path
            .metadata()
            .expect("manifest metadata should be readable")
            .modified()
            .expect("manifest modified time should be readable")
            .duration_since(UNIX_EPOCH)
            .expect("manifest modified time should be after Unix epoch")
            .as_millis() as u64;
        assert!(completion_timestamp >= manifest_modified_timestamp);
    }

    #[test]
    fn render_writes_export_manifest_sidecar() {
        let fixture = CliGitFixture::new("manifest");
        let ffmpeg = write_fake_ffmpeg("manifest");
        let output_path = fixture.path().join("out.mp4");

        run([
            "render".to_owned(),
            fixture.path().display().to_string(),
            "--output".to_owned(),
            output_path.display().to_string(),
            "--ffmpeg-path".to_owned(),
            ffmpeg.display().to_string(),
            "--mainline".to_owned(),
            "main".to_owned(),
            "--json".to_owned(),
        ])
        .expect("render should export and write manifest");

        let manifest_path = output_path.with_file_name("out.mp4.gitflux-manifest.json");
        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&manifest_path).expect("manifest sidecar should be readable"),
        )
        .expect("manifest should be JSON");

        assert_eq!(manifest["schema_version"], 1);
        assert_eq!(
            manifest["repository"]["path"].as_str(),
            Some(
                fixture
                    .path()
                    .to_str()
                    .expect("fixture path should be utf8")
            )
        );
        assert_eq!(
            manifest["selected_ref"]["selection_mode"].as_str(),
            Some("explicit")
        );
        assert_eq!(manifest["selected_ref"]["mainline"].as_str(), Some("main"));
        assert!(manifest["selected_ref"]["mainline_tip"]
            .as_str()
            .is_some_and(|tip| tip.len() == 40));
        assert!(manifest["config_hash"]
            .as_str()
            .is_some_and(|hash| hash.starts_with("sha256:")));
        assert_eq!(manifest["pacing"]["mode"].as_str(), Some("adaptive"));
        assert_eq!(
            manifest["output_settings"]["path"].as_str(),
            Some(output_path.to_str().expect("output path should be utf8"))
        );
        assert_eq!(manifest["output_settings"]["preset"].as_str(), Some("mp4"));
        assert_eq!(
            manifest["output_settings"]["frame_width"].as_u64(),
            Some(1920)
        );
        assert_eq!(
            manifest["output_settings"]["frame_height"].as_u64(),
            Some(1080)
        );
        assert_eq!(
            manifest["output_settings"]["frames_per_second"].as_u64(),
            Some(60)
        );
        assert_eq!(
            manifest["gitflux_version"].as_str(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(manifest["ffmpeg"]["preset"].as_str(), Some("mp4"));
        assert!(manifest["ffmpeg"]["command"]
            .as_array()
            .expect("ffmpeg command should be an array")
            .iter()
            .any(|value| value.as_str() == Some("-framerate")));

        let timestamp_phases = manifest["event_timestamps"]
            .as_array()
            .expect("event timestamp mapping should be an array")
            .iter()
            .map(|event| event["phase"].as_str().expect("phase should be a string"))
            .collect::<Vec<_>>();
        assert_eq!(
            timestamp_phases,
            [
                "ingestion",
                "scene_construction",
                "frame_rendering",
                "ffmpeg_encoding"
            ]
        );
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
    fn preview_plan_reports_shared_replay_and_render_configuration() {
        let fixture = CliGitFixture::new("preview-plan");
        let config_path = write_temp_render_config(
            "preview-valid",
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

        let command = parse_preview_args([
            fixture.path().display().to_string(),
            "--config".to_owned(),
            config_path.display().to_string(),
            "--mainline".to_owned(),
            "main".to_owned(),
        ])
        .expect("preview should build a session plan");
        let mut adapter = PlanningPreviewWindowAdapter;
        let execution = execute_preview_with_adapter(&command, &mut adapter)
            .expect("injected preview adapter should execute");
        let output = preview_human_summary(&command, &execution);

        assert!(output.contains("Gitflux Preview complete"));
        assert!(output.contains("Mainline: main"));
        assert!(output.contains("terminal"));
        assert!(output.contains("1280x720"));
        assert!(output.contains("Window: planned by minimal adapter (1280x720)"));
        assert!(output.contains("Shared renderer frame: not rendered"));
        assert!(output.contains("Config hot reload: watching config file metadata"));
    }

    #[test]
    fn preview_defaults_to_metadata_reload() {
        let fixture = CliGitFixture::new("preview-parse");
        let command = parse_preview_args([fixture.path().display().to_string()])
            .expect("preview defaults should parse");

        assert_eq!(
            command.reload_policy,
            PreviewReloadPolicy::ConfigFileMetadata
        );
    }

    #[test]
    fn preview_rejects_public_plan_only_flag() {
        let fixture = CliGitFixture::new("preview-flags");
        let result = parse_preview_args([
            fixture.path().display().to_string(),
            "--plan-only".to_owned(),
        ]);
        let error = match result {
            Ok(_) => panic!("plan-only should not be public CLI"),
            Err(error) => error,
        };

        assert!(error.contains("unrecognized preview option: --plan-only"));
    }

    #[test]
    fn preview_accepts_no_reload_flag() {
        let fixture = CliGitFixture::new("preview-no-reload");
        let command = parse_preview_args([
            fixture.path().display().to_string(),
            "--no-config-reload".to_owned(),
        ])
        .expect("preview flags should parse");

        assert_eq!(command.reload_policy, PreviewReloadPolicy::Disabled);
    }

    #[test]
    fn preview_rejects_invalid_configuration_file_with_diagnostics() {
        let fixture = CliGitFixture::new("preview-invalid");
        let config_path = write_temp_render_config(
            "preview-invalid",
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
            "preview".to_owned(),
            fixture.path().display().to_string(),
            "--config".to_owned(),
            config_path.display().to_string(),
        ])
        .expect_err("invalid Preview Render Configuration should fail");

        assert!(error.contains("failed to load Preview Render Configuration"));
        assert!(error.contains("frame_width"));
        assert!(error.contains("theme.background_color"));
    }

    #[test]
    fn preview_default_command_can_use_injected_test_adapter() {
        let fixture = CliGitFixture::new("preview-injected");
        let command = parse_preview_args([fixture.path().display().to_string()])
            .expect("default preview command should parse");
        let mut adapter = PlanningPreviewWindowAdapter;
        let execution = execute_preview_with_adapter(&command, &mut adapter)
            .expect("injected preview adapter should execute");
        let output = preview_human_summary(&command, &execution);

        assert!(output.contains("Gitflux Preview complete"));
        assert!(output.contains("Window: planned by minimal adapter (1920x1080)"));
    }

    #[test]
    fn diagnostics_reports_human_readable_readiness() {
        let fixture = CliGitFixture::new("diagnostics-human");
        let ffmpeg = write_fake_ffmpeg("diagnostics-human");
        let command = parse_diagnostics_args([
            fixture.path().display().to_string(),
            "--ffmpeg-path".to_owned(),
            ffmpeg.display().to_string(),
        ])
        .expect("diagnostics command should parse");
        let report = execute_diagnostics_with_probes(
            &command,
            || GpuDiagnostic {
                backend: "test-gpu".to_owned(),
                ready: true,
                detail: Some("test backend initialized".to_owned()),
            },
            |_| FfmpegDiagnostic {
                path: ffmpeg.display().to_string(),
                available: true,
                version: Some("ffmpeg version test".to_owned()),
                error: None,
            },
        );
        let output = diagnostics_human_summary(&report);

        assert!(output.contains("Gitflux Diagnostics"));
        assert!(output.contains("GPU/backend: ready (test backend initialized)"));
        assert!(output.contains("FFmpeg: ready"));
        assert!(output.contains("ffmpeg version test"));
        assert!(output.contains("Ref selection: detected"));
        assert!(output.contains("Mainline: main"));
        assert!(output.contains("Mainline tip:"));
        assert!(output.contains("Local branches: main"));
        assert!(output.contains("Cache location:"));
        assert!(output.contains("Cache matches current inputs: no"));
    }

    #[test]
    fn diagnostics_json_serializes_typed_report() {
        let fixture = CliGitFixture::new("diagnostics-json");
        let ffmpeg = write_fake_ffmpeg("diagnostics-json");
        let command = parse_diagnostics_args([
            fixture.path().display().to_string(),
            "--ffmpeg-path".to_owned(),
            ffmpeg.display().to_string(),
            "--json".to_owned(),
        ])
        .expect("diagnostics command should parse");
        let report = execute_diagnostics_with_probes(
            &command,
            || GpuDiagnostic {
                backend: "test-gpu".to_owned(),
                ready: false,
                detail: Some("not available in test".to_owned()),
            },
            |_| FfmpegDiagnostic {
                path: ffmpeg.display().to_string(),
                available: true,
                version: Some("ffmpeg version test".to_owned()),
                error: None,
            },
        );
        let json = serde_json::to_value(&report).expect("diagnostics should serialize");

        assert_eq!(json["schema_version"].as_u64(), Some(1));
        assert_eq!(json["gpu"]["ready"].as_bool(), Some(false));
        assert_eq!(json["ffmpeg"]["available"].as_bool(), Some(true));
        assert_eq!(
            json["ffmpeg"]["version"].as_str(),
            Some("ffmpeg version test")
        );
        assert_eq!(json["refs"]["selection_mode"].as_str(), Some("detected"));
        assert_eq!(json["refs"]["mainline"].as_str(), Some("main"));
        assert!(json["refs"]["mainline_tip"]
            .as_str()
            .is_some_and(|tip| tip.len() == 40));
        assert_eq!(
            json["cache"]["match_policy"].as_str(),
            Some("disabled_until_cache_metadata_exists")
        );
        assert_eq!(
            json["cache"]["matches_current_inputs"].as_bool(),
            Some(false)
        );
        assert!(json["cache"]["current_inputs_key"]
            .as_str()
            .is_some_and(|key| key.starts_with("sha256:")));
    }

    #[test]
    fn diagnostics_supports_explicit_mainline_override() {
        let fixture = CliGitFixture::new("diagnostics-mainline");
        run_git(fixture.path(), ["checkout", "-b", "release"]);
        fs::write(fixture.path().join("RELEASE.md"), "release\n")
            .expect("release file should be written");
        run_git(fixture.path(), ["add", "RELEASE.md"]);
        run_git(
            fixture.path(),
            ["commit", "--no-gpg-sign", "-m", "Release commit"],
        );
        let command = parse_diagnostics_args([
            fixture.path().display().to_string(),
            "--mainline".to_owned(),
            "release".to_owned(),
        ])
        .expect("diagnostics command should parse");
        let report = execute_diagnostics_with_probes(
            &command,
            || GpuDiagnostic {
                backend: "test-gpu".to_owned(),
                ready: true,
                detail: None,
            },
            |_| FfmpegDiagnostic {
                path: "ffmpeg".to_owned(),
                available: false,
                version: None,
                error: Some("not tested".to_owned()),
            },
        );

        assert_eq!(report.refs.selection_mode, "explicit");
        assert_eq!(report.refs.mainline, "release");
        assert!(report.refs.mainline_tip.is_some());
        assert!(report
            .refs
            .local_branches
            .iter()
            .any(|branch| branch == "main"));
        assert!(report
            .refs
            .local_branches
            .iter()
            .any(|branch| branch == "release"));
    }

    #[test]
    #[cfg(unix)]
    fn diagnostics_rejects_successful_non_ffmpeg_version_probe() {
        let binary = write_executable_script(
            "diagnostics-not-ffmpeg",
            "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then printf 'true 1.0\\n'; exit 0; fi\nexit 0\n",
        );

        let diagnostic = probe_ffmpeg_binary(&FfmpegBinary::from_path(binary));

        assert!(!diagnostic.available);
        assert_eq!(diagnostic.version, None);
        assert!(diagnostic
            .error
            .as_deref()
            .is_some_and(|error| error.contains("recognizable FFmpeg output")));
    }

    #[test]
    #[cfg(unix)]
    fn diagnostics_rejects_empty_successful_ffmpeg_probe() {
        let binary = write_executable_script(
            "diagnostics-empty-version",
            "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then exit 0; fi\nexit 0\n",
        );

        let diagnostic = probe_ffmpeg_binary(&FfmpegBinary::from_path(binary));

        assert!(!diagnostic.available);
        assert_eq!(diagnostic.version, None);
        assert!(diagnostic
            .error
            .as_deref()
            .is_some_and(|error| error.contains("recognizable FFmpeg output")));
    }

    #[test]
    fn identical_config_contents_have_stable_hash_across_paths() {
        let fixture = CliGitFixture::new("config-hash");
        let contents = r##"
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
"##;
        let first_config = write_temp_render_config("same-config-a", contents);
        let second_config = write_temp_render_config("same-config-b", contents);

        let first_command = parse_render_args([
            fixture.path().display().to_string(),
            "--output".to_owned(),
            fixture.path().join("first.mp4").display().to_string(),
            "--config".to_owned(),
            first_config.display().to_string(),
        ])
        .expect("first config should parse");
        let second_command = parse_render_args([
            fixture.path().display().to_string(),
            "--output".to_owned(),
            fixture.path().join("second.mp4").display().to_string(),
            "--config".to_owned(),
            second_config.display().to_string(),
        ])
        .expect("second config should parse");

        assert_eq!(first_command.config_hash(), second_command.config_hash());
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
        write_executable_script(
            &format!("cli-fake-ffmpeg-{name}"),
            "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then printf 'ffmpeg version fake\\n'; exit 0; fi\nfor output_path do :; done\nprintf 'video\\n' > \"$output_path\"\n",
        )
    }

    #[cfg(unix)]
    fn write_executable_script(name: &str, contents: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!(
            "gitflux-{name}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::write(&path, contents).expect("script should be written");
        let mut permissions = fs::metadata(&path)
            .expect("script metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("script should be executable");
        path
    }

    #[cfg(not(unix))]
    fn write_fake_ffmpeg(_name: &str) -> PathBuf {
        panic!("fake FFmpeg test helper is only implemented for Unix test runners")
    }
}
