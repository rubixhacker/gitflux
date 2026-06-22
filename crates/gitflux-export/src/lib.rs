//! Video Export orchestration seam for Gitflux.
//!
//! This crate will coordinate deterministic frame rendering and FFmpeg-driven
//! encoding. It owns Export Manifest data and Video Export requests without
//! embedding media codec support in the core Repository Replay pipeline.

use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
    fs,
    num::NonZeroU64,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use gitflux_render::{
    FrameCount, FrameFileStem, FrameSequenceOutput, OffscreenRenderer, RenderError, RenderPlan,
    RendererSettings,
};
use serde::{Deserialize, Serialize};

const MANIFEST_SUFFIX: &str = "gitflux-manifest.json";

/// Request to produce a Video Export from a render plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoExportRequest {
    render_plan: RenderPlan,
    output_path: PathBuf,
    output_preset: OutputPreset,
}

impl VideoExportRequest {
    /// Creates a Video Export request, deriving and validating the output preset.
    pub fn try_new(
        render_plan: RenderPlan,
        output_path: impl Into<PathBuf>,
    ) -> Result<Self, ExportError> {
        let output_path = output_path.into();
        let output_preset = OutputPreset::from_output_path(&output_path)?;

        Ok(Self {
            render_plan,
            output_path,
            output_preset,
        })
    }

    /// Returns the render plan to export.
    #[must_use]
    pub fn render_plan(&self) -> &RenderPlan {
        &self.render_plan
    }

    /// Returns the requested output path.
    #[must_use]
    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    /// Returns the output preset selected for this request.
    #[must_use]
    pub fn output_preset(&self) -> OutputPreset {
        self.output_preset
    }
}

/// Sidecar data describing the inputs used to produce a Video Export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportManifest {
    output_path: PathBuf,
    manifest_path: PathBuf,
    encoder: VideoEncoder,
    output_preset: OutputPreset,
    repository: ExportRepositoryIdentity,
    selected_ref: ExportSelectedRefState,
    config_hash: String,
    pacing: ExportPacing,
    output_settings: ExportOutputSettings,
    gitflux_version: String,
    ffmpeg: ExportFfmpegPlan,
    event_timestamps: Vec<ExportEventTimestamp>,
}

impl ExportManifest {
    /// Creates an Export Manifest from a validated Video Export request.
    #[must_use]
    pub fn from_request(request: &VideoExportRequest, encoder: VideoEncoder) -> Self {
        Self::from_request_context(
            request,
            encoder,
            ExportManifestContext::from_request(request, env!("CARGO_PKG_VERSION")),
            FfmpegCommandPlan::for_png_sequence(
                FfmpegBinary::default(),
                request,
                "frames/frame_%06d.png",
                request
                    .render_plan()
                    .configuration()
                    .frames_per_second()
                    .get(),
                0,
            ),
            Vec::new(),
        )
    }

    /// Creates an Export Manifest from request, context, command plan, and progress events.
    #[must_use]
    pub fn from_request_context(
        request: &VideoExportRequest,
        encoder: VideoEncoder,
        context: ExportManifestContext,
        command_plan: FfmpegCommandPlan,
        progress_events: Vec<ExportProgressEvent>,
    ) -> Self {
        Self {
            output_path: request.output_path().to_path_buf(),
            manifest_path: default_manifest_path(request.output_path()),
            encoder,
            output_preset: request.output_preset(),
            repository: context.repository,
            selected_ref: context.selected_ref,
            config_hash: context.config_hash,
            pacing: context.pacing,
            output_settings: context.output_settings,
            gitflux_version: context.gitflux_version,
            ffmpeg: ExportFfmpegPlan::from_command_plan(request.output_preset(), &command_plan),
            event_timestamps: progress_events
                .into_iter()
                .map(ExportEventTimestamp::from_progress_event)
                .collect(),
        }
    }

    /// Returns the exported media path.
    #[must_use]
    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    /// Returns the sidecar manifest path.
    #[must_use]
    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    /// Returns the encoder delegated to by Video Export.
    #[must_use]
    pub fn encoder(&self) -> VideoEncoder {
        self.encoder
    }

    /// Returns the output preset used by the export.
    #[must_use]
    pub fn output_preset(&self) -> OutputPreset {
        self.output_preset
    }

    /// Returns repository identity metadata.
    #[must_use]
    pub fn repository(&self) -> &ExportRepositoryIdentity {
        &self.repository
    }

    /// Returns selected Mainline/ref metadata.
    #[must_use]
    pub fn selected_ref(&self) -> &ExportSelectedRefState {
        &self.selected_ref
    }

    /// Returns the render configuration hash.
    #[must_use]
    pub fn config_hash(&self) -> &str {
        &self.config_hash
    }

    /// Returns the replay pacing manifest record.
    #[must_use]
    pub fn pacing(&self) -> &ExportPacing {
        &self.pacing
    }

    /// Returns output settings metadata.
    #[must_use]
    pub fn output_settings(&self) -> &ExportOutputSettings {
        &self.output_settings
    }

    /// Returns the Gitflux version that produced the export.
    #[must_use]
    pub fn gitflux_version(&self) -> &str {
        &self.gitflux_version
    }

    /// Returns FFmpeg command metadata.
    #[must_use]
    pub fn ffmpeg(&self) -> &ExportFfmpegPlan {
        &self.ffmpeg
    }

    /// Returns progress event timestamps captured during export.
    #[must_use]
    pub fn event_timestamps(&self) -> &[ExportEventTimestamp] {
        &self.event_timestamps
    }

    fn to_sidecar(&self) -> ExportManifestSidecar {
        ExportManifestSidecar {
            schema_version: 1,
            repository: self.repository.clone(),
            selected_ref: self.selected_ref.clone(),
            config_hash: self.config_hash.clone(),
            pacing: self.pacing.clone(),
            output_settings: self.output_settings.clone(),
            gitflux_version: self.gitflux_version.clone(),
            ffmpeg: self.ffmpeg.clone(),
            event_timestamps: self.event_timestamps.clone(),
        }
    }
}

/// Serializable sidecar shape written next to exported media.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportManifestSidecar {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Repository identity.
    pub repository: ExportRepositoryIdentity,
    /// Selected Mainline/ref state.
    pub selected_ref: ExportSelectedRefState,
    /// Hash of the Render Configuration input.
    pub config_hash: String,
    /// Replay pacing mode and duration policy.
    pub pacing: ExportPacing,
    /// Output path, container preset, frame size, and frame rate.
    pub output_settings: ExportOutputSettings,
    /// Gitflux package version.
    pub gitflux_version: String,
    /// FFmpeg preset and command details.
    pub ffmpeg: ExportFfmpegPlan,
    /// Timestamp mapping for progress events emitted during export.
    pub event_timestamps: Vec<ExportEventTimestamp>,
}

/// Context supplied by callers that know repository/configuration identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportManifestContext {
    repository: ExportRepositoryIdentity,
    selected_ref: ExportSelectedRefState,
    config_hash: String,
    pacing: ExportPacing,
    output_settings: ExportOutputSettings,
    gitflux_version: String,
    progress_events: Vec<ExportProgressEvent>,
}

impl ExportManifestContext {
    /// Creates a manifest context.
    #[must_use]
    pub fn new(
        repository: ExportRepositoryIdentity,
        selected_ref: ExportSelectedRefState,
        config_hash: impl Into<String>,
        pacing: ExportPacing,
        output_settings: ExportOutputSettings,
        gitflux_version: impl Into<String>,
    ) -> Self {
        Self {
            repository,
            selected_ref,
            config_hash: config_hash.into(),
            pacing,
            output_settings,
            gitflux_version: gitflux_version.into(),
            progress_events: Vec::new(),
        }
    }

    /// Includes progress events captured before the export crate took over.
    #[must_use]
    pub fn with_progress_events(mut self, progress_events: Vec<ExportProgressEvent>) -> Self {
        self.progress_events = progress_events;
        self
    }

    /// Builds a conservative context when callers only have an export request.
    #[must_use]
    pub fn from_request(request: &VideoExportRequest, gitflux_version: impl Into<String>) -> Self {
        let configuration = request.render_plan().configuration();
        let frame_size = configuration.frame_size();

        Self::new(
            ExportRepositoryIdentity::new("", None),
            ExportSelectedRefState::new(
                "unspecified",
                request.render_plan().replay().mainline().as_str(),
                None,
            ),
            "unspecified",
            ExportPacing::new("adaptive", "auto"),
            ExportOutputSettings::new(
                request.output_path(),
                request.output_preset(),
                frame_size.width(),
                frame_size.height(),
                configuration.frames_per_second().get(),
            ),
            gitflux_version,
        )
    }
}

/// Repository identity captured in an export manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRepositoryIdentity {
    path: String,
    remote_url: Option<String>,
}

impl ExportRepositoryIdentity {
    /// Creates repository identity metadata.
    #[must_use]
    pub fn new(path: impl Into<String>, remote_url: Option<String>) -> Self {
        Self {
            path: path.into(),
            remote_url,
        }
    }

    /// Returns the repository path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the remote URL, if discovered.
    #[must_use]
    pub fn remote_url(&self) -> Option<&str> {
        self.remote_url.as_deref()
    }
}

/// Selected Mainline/ref state captured in an export manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSelectedRefState {
    selection_mode: String,
    mainline: String,
    mainline_tip: Option<String>,
}

impl ExportSelectedRefState {
    /// Creates selected ref-state metadata.
    #[must_use]
    pub fn new(
        selection_mode: impl Into<String>,
        mainline: impl Into<String>,
        mainline_tip: Option<String>,
    ) -> Self {
        Self {
            selection_mode: selection_mode.into(),
            mainline: mainline.into(),
            mainline_tip,
        }
    }

    /// Returns whether Mainline was detected or explicitly selected.
    #[must_use]
    pub fn selection_mode(&self) -> &str {
        &self.selection_mode
    }

    /// Returns the selected Mainline name.
    #[must_use]
    pub fn mainline(&self) -> &str {
        &self.mainline
    }

    /// Returns the selected Mainline tip OID, if discovered.
    #[must_use]
    pub fn mainline_tip(&self) -> Option<&str> {
        self.mainline_tip.as_deref()
    }
}

/// Replay pacing metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportPacing {
    mode: String,
    duration: String,
}

impl ExportPacing {
    /// Creates replay pacing metadata.
    #[must_use]
    pub fn new(mode: impl Into<String>, duration: impl Into<String>) -> Self {
        Self {
            mode: mode.into(),
            duration: duration.into(),
        }
    }

    /// Returns the pacing mode.
    #[must_use]
    pub fn mode(&self) -> &str {
        &self.mode
    }
}

/// Output settings metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportOutputSettings {
    path: String,
    preset: OutputPreset,
    frame_width: u32,
    frame_height: u32,
    frames_per_second: u32,
}

impl ExportOutputSettings {
    /// Creates output settings metadata.
    #[must_use]
    pub fn new(
        path: impl AsRef<Path>,
        preset: OutputPreset,
        frame_width: u32,
        frame_height: u32,
        frames_per_second: u32,
    ) -> Self {
        Self {
            path: path.as_ref().display().to_string(),
            preset,
            frame_width,
            frame_height,
            frames_per_second,
        }
    }

    /// Returns the output media path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// FFmpeg command metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFfmpegPlan {
    preset: OutputPreset,
    program: String,
    arguments: Vec<String>,
    command: Vec<String>,
}

impl ExportFfmpegPlan {
    fn from_command_plan(preset: OutputPreset, command_plan: &FfmpegCommandPlan) -> Self {
        let program = command_plan.program().display().to_string();
        let arguments = command_plan.arguments().to_vec();
        let mut command = Vec::with_capacity(arguments.len() + 1);
        command.push(program.clone());
        command.extend(arguments.clone());

        Self {
            preset,
            program,
            arguments,
            command,
        }
    }

    /// Returns the complete FFmpeg command vector.
    #[must_use]
    pub fn command(&self) -> &[String] {
        &self.command
    }
}

/// Timestamp captured for a progress event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportEventTimestamp {
    phase: ExportProgressPhase,
    timestamp_unix_ms: u128,
}

impl ExportEventTimestamp {
    fn from_progress_event(event: ExportProgressEvent) -> Self {
        Self {
            phase: event.phase,
            timestamp_unix_ms: event.timestamp_unix_ms,
        }
    }

    /// Returns the event phase.
    #[must_use]
    pub fn phase(&self) -> ExportProgressPhase {
        self.phase
    }
}

/// Ordered progress event emitted by export and CLI orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportProgressEvent {
    event: &'static str,
    phase: ExportProgressPhase,
    timestamp_unix_ms: u128,
    details: BTreeMap<String, String>,
}

impl ExportProgressEvent {
    /// Creates a progress event for a phase.
    #[must_use]
    pub fn new(phase: ExportProgressPhase) -> Self {
        Self {
            event: "render_progress",
            phase,
            timestamp_unix_ms: timestamp_unix_ms(),
            details: BTreeMap::new(),
        }
    }

    /// Adds string details to a progress event.
    #[must_use]
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }

    /// Returns the progress phase.
    #[must_use]
    pub fn phase(&self) -> ExportProgressPhase {
        self.phase
    }
}

/// Progress phases reported during render/export orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportProgressPhase {
    /// Repository ingestion has completed.
    Ingestion,
    /// Scene construction/render plan has completed.
    SceneConstruction,
    /// Deterministic frame rendering has completed.
    FrameRendering,
    /// FFmpeg encoding has completed.
    FfmpegEncoding,
    /// Export manifest sidecar has been written.
    ManifestWriting,
    /// Non-fatal warnings were emitted.
    Warnings,
    /// Export completed.
    Completion,
}

/// External encoder used for Video Export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoEncoder {
    /// An installed FFmpeg binary.
    Ffmpeg,
}

/// Container and codec preset for a Video Export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputPreset {
    /// MP4 container encoded through H.264 for broad compatibility.
    Mp4,
    /// WebM container encoded through VP9.
    WebM,
}

impl OutputPreset {
    fn from_output_path(output_path: &Path) -> Result<Self, ExportError> {
        match output_path
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some(extension) if extension.eq_ignore_ascii_case("mp4") => Ok(Self::Mp4),
            Some(extension) if extension.eq_ignore_ascii_case("webm") => Ok(Self::WebM),
            _ => Err(ExportError::UnsupportedOutputPreset {
                output_path: output_path.to_path_buf(),
            }),
        }
    }

    fn ffmpeg_arguments(self) -> &'static [&'static str] {
        match self {
            Self::Mp4 => &[
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
            ],
            Self::WebM => &["-c:v", "libvpx-vp9", "-pix_fmt", "yuva420p"],
        }
    }
}

/// FFmpeg executable selected for export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegBinary {
    path: PathBuf,
}

impl FfmpegBinary {
    /// Uses a specific FFmpeg binary path.
    #[must_use]
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the executable path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Default for FfmpegBinary {
    fn default() -> Self {
        Self::from_path("ffmpeg")
    }
}

/// Options for rendering deterministic frames and encoding them through FFmpeg.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoExportOptions {
    ffmpeg_binary: FfmpegBinary,
    frame_count: FrameCount,
    renderer_settings: RendererSettings,
    work_directory: PathBuf,
}

impl VideoExportOptions {
    /// Creates Video Export options.
    #[must_use]
    pub fn new(ffmpeg_binary: FfmpegBinary, frame_count: FrameCount) -> Self {
        Self {
            ffmpeg_binary,
            frame_count,
            renderer_settings: RendererSettings::default(),
            work_directory: default_work_directory(),
        }
    }

    /// Uses a specific FFmpeg binary.
    #[must_use]
    pub fn with_ffmpeg_binary(mut self, ffmpeg_binary: FfmpegBinary) -> Self {
        self.ffmpeg_binary = ffmpeg_binary;
        self
    }

    /// Uses specific renderer settings.
    #[must_use]
    pub fn with_renderer_settings(mut self, renderer_settings: RendererSettings) -> Self {
        self.renderer_settings = renderer_settings;
        self
    }

    /// Uses a specific temporary work directory for deterministic frames.
    #[must_use]
    pub fn with_work_directory(mut self, work_directory: impl Into<PathBuf>) -> Self {
        self.work_directory = work_directory.into();
        self
    }

    /// Returns the FFmpeg binary.
    #[must_use]
    pub fn ffmpeg_binary(&self) -> &FfmpegBinary {
        &self.ffmpeg_binary
    }

    /// Returns the requested frame count.
    #[must_use]
    pub fn frame_count(&self) -> FrameCount {
        self.frame_count
    }
}

impl Default for VideoExportOptions {
    fn default() -> Self {
        Self::new(
            FfmpegBinary::default(),
            FrameCount::new(NonZeroU64::new(1).expect("one is non-zero")),
        )
    }
}

/// Concrete FFmpeg invocation for encoding a deterministic frame sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegCommandPlan {
    program: PathBuf,
    arguments: Vec<String>,
}

impl FfmpegCommandPlan {
    /// Builds an FFmpeg invocation for PNG frame-sequence input.
    #[must_use]
    pub fn for_png_sequence(
        binary: FfmpegBinary,
        request: &VideoExportRequest,
        input_pattern: impl Into<String>,
        frames_per_second: u32,
        first_frame_index: u64,
    ) -> Self {
        let mut arguments = vec![
            "-y".to_owned(),
            "-framerate".to_owned(),
            frames_per_second.to_string(),
            "-start_number".to_owned(),
            first_frame_index.to_string(),
            "-i".to_owned(),
            input_pattern.into(),
        ];
        arguments.extend(
            request
                .output_preset()
                .ffmpeg_arguments()
                .iter()
                .map(ToString::to_string),
        );
        arguments.push(request.output_path().display().to_string());

        Self {
            program: binary.path,
            arguments,
        }
    }

    /// Returns the FFmpeg executable.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Returns FFmpeg arguments in execution order.
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// Verifies that the planned FFmpeg executable can be launched and reports a healthy version.
    pub fn validate_binary(&self) -> Result<(), ExportError> {
        let output = Command::new(&self.program)
            .arg("-version")
            .output()
            .map_err(|source| ExportError::FfmpegNotFound {
                binary: self.program.clone(),
                source,
            })?;

        if output.status.success() {
            return Ok(());
        }

        Err(ExportError::FfmpegFailed {
            binary: self.program.clone(),
            exit_code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }

    /// Runs the planned FFmpeg invocation.
    pub fn run(&self) -> Result<(), ExportError> {
        let output = Command::new(&self.program)
            .args(&self.arguments)
            .output()
            .map_err(|source| ExportError::FfmpegNotFound {
                binary: self.program.clone(),
                source,
            })?;

        if output.status.success() {
            return Ok(());
        }

        Err(ExportError::FfmpegFailed {
            binary: self.program.clone(),
            exit_code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

/// Video Export failure modes.
#[derive(Debug)]
pub enum ExportError {
    /// The output path does not map to a supported preset.
    UnsupportedOutputPreset {
        /// Requested output path.
        output_path: PathBuf,
    },
    /// The configured FFmpeg executable could not be launched.
    FfmpegNotFound {
        /// FFmpeg binary that was attempted.
        binary: PathBuf,
        /// Process launch error.
        source: std::io::Error,
    },
    /// FFmpeg launched but reported an encoding failure.
    FfmpegFailed {
        /// FFmpeg binary that was attempted.
        binary: PathBuf,
        /// Process exit code, when the platform reported one.
        exit_code: Option<i32>,
        /// FFmpeg stderr.
        stderr: String,
    },
    /// FFmpeg reported success but no output file was produced.
    OutputNotCreated {
        /// Requested output path.
        output_path: PathBuf,
        /// Filesystem error from checking the output path.
        source: std::io::Error,
    },
    /// FFmpeg reported success but wrote an empty output file.
    OutputEmpty {
        /// Requested output path.
        output_path: PathBuf,
    },
    /// Deterministic frame rendering failed.
    Render(RenderError),
    /// Export manifest sidecar could not be written.
    ManifestWrite {
        /// Manifest sidecar path.
        manifest_path: PathBuf,
        /// Filesystem or serialization error.
        source: std::io::Error,
    },
}

impl Display for ExportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedOutputPreset { output_path } => write!(
                formatter,
                "unsupported Video Export output preset for {}; supported presets: .mp4, .webm",
                output_path.display()
            ),
            Self::FfmpegNotFound { binary, source } => write!(
                formatter,
                "FFmpeg was not found or could not be launched at '{}': {source}. Install FFmpeg and ensure it is on PATH, or pass --ffmpeg-path <path-to-ffmpeg>.",
                binary.display()
            ),
            Self::FfmpegFailed {
                binary,
                exit_code,
                stderr,
            } => {
                write!(
                    formatter,
                    "FFmpeg failed while encoding with '{}' (exit code: {}): {}",
                    binary.display(),
                    exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "unknown".to_owned()),
                    if stderr.is_empty() {
                        "no stderr output"
                    } else {
                        stderr
                    }
                )
            }
            Self::OutputNotCreated {
                output_path,
                source,
            } => write!(
                formatter,
                "FFmpeg completed but did not create output file '{}': {source}",
                output_path.display()
            ),
            Self::OutputEmpty { output_path } => write!(
                formatter,
                "FFmpeg completed but output file '{}' is empty",
                output_path.display()
            ),
            Self::Render(error) => write!(formatter, "could not render export frames: {error}"),
            Self::ManifestWrite {
                manifest_path,
                source,
            } => write!(
                formatter,
                "could not write export manifest '{}': {source}",
                manifest_path.display()
            ),
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FfmpegNotFound { source, .. } => Some(source),
            Self::OutputNotCreated { source, .. } => Some(source),
            Self::Render(error) => Some(error),
            Self::ManifestWrite { source, .. } => Some(source),
            Self::UnsupportedOutputPreset { .. }
            | Self::FfmpegFailed { .. }
            | Self::OutputEmpty { .. } => None,
        }
    }
}

/// Creates a scaffold Export Manifest for a future FFmpeg export.
#[must_use]
pub fn scaffold_export_manifest(request: &VideoExportRequest) -> ExportManifest {
    ExportManifest::from_request(request, VideoEncoder::Ffmpeg)
}

/// Renders deterministic frames and encodes them with FFmpeg.
pub fn export_video(
    request: &VideoExportRequest,
    options: &VideoExportOptions,
) -> Result<ExportManifest, ExportError> {
    export_video_with_manifest_context(
        request,
        options,
        ExportManifestContext::from_request(request, env!("CARGO_PKG_VERSION")),
        |_| {},
    )
}

/// Renders deterministic frames, encodes them with FFmpeg, and writes an Export Manifest sidecar.
pub fn export_video_with_manifest_context(
    request: &VideoExportRequest,
    options: &VideoExportOptions,
    context: ExportManifestContext,
    mut on_progress: impl FnMut(ExportProgressEvent),
) -> Result<ExportManifest, ExportError> {
    let frame_output = FrameSequenceOutput::new(
        options.work_directory.join("frames"),
        FrameFileStem::new("frame"),
    );
    let command_plan = FfmpegCommandPlan::for_png_sequence(
        options.ffmpeg_binary.clone(),
        request,
        ffmpeg_input_pattern(&frame_output),
        request
            .render_plan()
            .configuration()
            .frames_per_second()
            .get(),
        frame_output.first_frame_index().get(),
    );

    command_plan.validate_binary()?;

    let mut progress_events = Vec::new();
    let export_result = (|| {
        let renderer =
            OffscreenRenderer::new(options.renderer_settings).map_err(ExportError::Render)?;
        renderer
            .render_frame_sequence(request.render_plan(), &frame_output, options.frame_count())
            .map_err(ExportError::Render)?;
        record_progress(
            &mut progress_events,
            &mut on_progress,
            ExportProgressEvent::new(ExportProgressPhase::FrameRendering)
                .with_detail("frame_count", options.frame_count().get().to_string()),
        );
        command_plan.run()?;
        record_progress(
            &mut progress_events,
            &mut on_progress,
            ExportProgressEvent::new(ExportProgressPhase::FfmpegEncoding)
                .with_detail("output_path", request.output_path().display().to_string()),
        );
        verify_output_file(request.output_path())
    })();
    let _ = std::fs::remove_dir_all(&options.work_directory);
    export_result?;

    let mut manifest_events = context.progress_events.clone();
    manifest_events.extend(progress_events.clone());
    let manifest = ExportManifest::from_request_context(
        request,
        VideoEncoder::Ffmpeg,
        context,
        command_plan,
        manifest_events,
    );
    write_manifest_sidecar(&manifest)?;

    let manifest_written_event = ExportProgressEvent::new(ExportProgressPhase::ManifestWriting)
        .with_detail(
            "manifest_path",
            default_manifest_path(request.output_path())
                .display()
                .to_string(),
        );
    let warnings_event =
        ExportProgressEvent::new(ExportProgressPhase::Warnings).with_detail("count", "0");
    let completion_event = ExportProgressEvent::new(ExportProgressPhase::Completion)
        .with_detail("output_path", request.output_path().display().to_string());
    record_progress(
        &mut progress_events,
        &mut on_progress,
        manifest_written_event,
    );
    record_progress(&mut progress_events, &mut on_progress, warnings_event);
    record_progress(&mut progress_events, &mut on_progress, completion_event);

    Ok(manifest)
}

fn record_progress(
    progress_events: &mut Vec<ExportProgressEvent>,
    on_progress: &mut impl FnMut(ExportProgressEvent),
    event: ExportProgressEvent,
) {
    on_progress(event.clone());
    progress_events.push(event);
}

fn ffmpeg_input_pattern(frame_output: &FrameSequenceOutput) -> String {
    frame_output
        .directory()
        .join(format!("{}_%06d.png", frame_output.file_stem().as_str()))
        .display()
        .to_string()
}

fn verify_output_file(output_path: &Path) -> Result<(), ExportError> {
    let metadata =
        std::fs::metadata(output_path).map_err(|source| ExportError::OutputNotCreated {
            output_path: output_path.to_path_buf(),
            source,
        })?;

    if metadata.len() == 0 {
        return Err(ExportError::OutputEmpty {
            output_path: output_path.to_path_buf(),
        });
    }

    Ok(())
}

fn write_manifest_sidecar(manifest: &ExportManifest) -> Result<(), ExportError> {
    let contents = serde_json::to_vec_pretty(&manifest.to_sidecar()).map_err(|source| {
        ExportError::ManifestWrite {
            manifest_path: manifest.manifest_path().to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
        }
    })?;
    fs::write(manifest.manifest_path(), contents).map_err(|source| ExportError::ManifestWrite {
        manifest_path: manifest.manifest_path().to_path_buf(),
        source,
    })
}

fn default_manifest_path(output_path: &Path) -> PathBuf {
    let file_name = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export");
    output_path.with_file_name(format!("{file_name}.{MANIFEST_SUFFIX}"))
}

fn timestamp_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn default_work_directory() -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    std::env::temp_dir().join(format!("gitflux-export-{}-{unique}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::{
        export_video_with_manifest_context, scaffold_export_manifest, verify_output_file,
        ExportManifestContext, ExportOutputSettings, ExportPacing, ExportProgressPhase,
        ExportRepositoryIdentity, ExportSelectedRefState, FfmpegBinary, FfmpegCommandPlan,
        OutputPreset, VideoEncoder, VideoExportOptions, VideoExportRequest,
    };
    use std::num::NonZeroU64;

    use gitflux_render::{FrameCount, RenderPlan};
    use gitflux_scene::{
        Layout, Mainline, RenderConfiguration, RepositoryReplay, Theme, VisualMetaphor,
    };

    #[test]
    fn scaffold_manifest_records_ffmpeg_encoder() {
        let replay = RepositoryReplay::new(Mainline::new("main"));
        let configuration = RenderConfiguration::new(
            VisualMetaphor::new("flow"),
            Theme::new("default"),
            Layout::RepositoryGraph,
        );
        let request =
            VideoExportRequest::try_new(RenderPlan::new(replay, configuration), "out/gitflux.mp4")
                .expect("supported Video Export request");

        let manifest = scaffold_export_manifest(&request);

        assert_eq!(manifest.output_path().to_string_lossy(), "out/gitflux.mp4");
        assert_eq!(manifest.encoder(), VideoEncoder::Ffmpeg);
    }

    #[test]
    fn mp4_command_plan_uses_image_sequence_input() {
        let request = VideoExportRequest::try_new(render_plan(), "out/gitflux.mp4")
            .expect("supported Video Export request");

        let plan = FfmpegCommandPlan::for_png_sequence(
            FfmpegBinary::from_path("/opt/homebrew/bin/ffmpeg"),
            &request,
            "frames/frame_%06d.png",
            30,
            0,
        );

        assert_eq!(request.output_preset(), OutputPreset::Mp4);
        assert_eq!(plan.program().to_string_lossy(), "/opt/homebrew/bin/ffmpeg");
        assert_eq!(
            plan.arguments(),
            [
                "-y",
                "-framerate",
                "30",
                "-start_number",
                "0",
                "-i",
                "frames/frame_%06d.png",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
                "out/gitflux.mp4"
            ]
        );
    }

    #[test]
    fn webm_command_plan_uses_vp9_preset() {
        let request = VideoExportRequest::try_new(render_plan(), "out/gitflux.webm")
            .expect("supported Video Export request");

        let plan = FfmpegCommandPlan::for_png_sequence(
            FfmpegBinary::default(),
            &request,
            "frames/frame_%06d.png",
            24,
            1,
        );

        assert_eq!(request.output_preset(), OutputPreset::WebM);
        assert!(plan.arguments().contains(&"-c:v".to_owned()));
        assert!(plan.arguments().contains(&"libvpx-vp9".to_owned()));
        assert!(plan.arguments().contains(&"out/gitflux.webm".to_owned()));
    }

    #[test]
    fn unsupported_output_extension_is_rejected() {
        let request = VideoExportRequest::try_new(render_plan(), "out/gitflux.mov");

        assert!(request
            .expect_err("unsupported extension should fail")
            .to_string()
            .contains("supported presets: .mp4, .webm"));
    }

    #[test]
    #[cfg(unix)]
    fn fake_ffmpeg_receives_planned_arguments() {
        let temp_dir = unique_temp_dir("fake-ffmpeg-invocation");
        std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let args_path = temp_dir.join("args.txt");
        let fake_ffmpeg = temp_dir.join("ffmpeg");
        write_executable_script(
            &fake_ffmpeg,
            &format!(
                "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then exit 0; fi\nprintf '%s\\n' \"$@\" > '{}'\nfor output_path do :; done\nprintf 'video\\n' > \"$output_path\"\n",
                args_path.display()
            ),
        );
        let request = VideoExportRequest::try_new(render_plan(), temp_dir.join("gitflux.mp4"))
            .expect("supported Video Export request");
        let plan = FfmpegCommandPlan::for_png_sequence(
            FfmpegBinary::from_path(&fake_ffmpeg),
            &request,
            temp_dir.join("frames/frame_%06d.png").display().to_string(),
            30,
            0,
        );

        plan.run().expect("fake ffmpeg should run");

        let args = std::fs::read_to_string(args_path).expect("fake ffmpeg should record args");
        assert!(args.contains("-framerate\n30\n"));
        assert!(args.contains("-i\n"));
        assert!(args.contains("frames/frame_%06d.png"));
        assert!(args.contains("-c:v\nlibx264\n"));
    }

    #[test]
    fn missing_ffmpeg_binary_reports_remediation() {
        let request = VideoExportRequest::try_new(render_plan(), "out/gitflux.webm")
            .expect("supported Video Export request");
        let plan = FfmpegCommandPlan::for_png_sequence(
            FfmpegBinary::from_path("definitely-not-gitflux-ffmpeg"),
            &request,
            "frames/frame_%06d.png",
            24,
            0,
        );

        let error = plan
            .validate_binary()
            .expect_err("missing ffmpeg should fail early");
        let message = error.to_string();

        assert!(message.contains("FFmpeg was not found"));
        assert!(message.contains("Install FFmpeg"));
        assert!(message.contains("--ffmpeg-path"));
    }

    #[test]
    fn missing_encoded_output_is_rejected() {
        let temp_dir = unique_temp_dir("missing-output");
        let output_path = temp_dir.join("missing.mp4");

        let error = verify_output_file(&output_path).expect_err("missing output should fail");
        let message = error.to_string();

        assert!(message.contains("did not create output file"));
        assert!(message.contains("missing.mp4"));
    }

    #[test]
    fn empty_encoded_output_is_rejected() {
        let temp_dir = unique_temp_dir("empty-output");
        std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let output_path = temp_dir.join("empty.mp4");
        std::fs::write(&output_path, "").expect("empty output should be written");

        let error = verify_output_file(&output_path).expect_err("empty output should fail");
        let message = error.to_string();

        assert!(message.contains("output file"));
        assert!(message.contains("is empty"));
    }

    #[test]
    #[cfg(unix)]
    fn ffmpeg_validation_rejects_nonzero_version_probe() {
        let temp_dir = unique_temp_dir("fake-ffmpeg-version-failure");
        std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let fake_ffmpeg = temp_dir.join("ffmpeg");
        write_executable_script(
            &fake_ffmpeg,
            "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then echo 'broken ffmpeg' >&2; exit 42; fi\nexit 0\n",
        );
        let request = VideoExportRequest::try_new(render_plan(), temp_dir.join("gitflux.mp4"))
            .expect("supported Video Export request");
        let plan = FfmpegCommandPlan::for_png_sequence(
            FfmpegBinary::from_path(&fake_ffmpeg),
            &request,
            temp_dir.join("frames/frame_%06d.png").display().to_string(),
            30,
            0,
        );

        let error = plan
            .validate_binary()
            .expect_err("unhealthy ffmpeg should fail validation");
        let message = error.to_string();

        assert!(message.contains("exit code: 42"));
        assert!(message.contains("broken ffmpeg"));
    }

    #[test]
    #[cfg(unix)]
    fn manifest_write_failure_does_not_report_terminal_progress() {
        let temp_dir = unique_temp_dir("manifest-write-failure");
        std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let fake_ffmpeg = temp_dir.join("ffmpeg");
        write_executable_script(
            &fake_ffmpeg,
            "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then exit 0; fi\nfor output_path do :; done\nprintf 'video\\n' > \"$output_path\"\n",
        );
        let output_path = temp_dir.join("gitflux.mp4");
        let manifest_path = temp_dir.join("gitflux.mp4.gitflux-manifest.json");
        std::fs::create_dir_all(&manifest_path).expect("manifest path conflict should be created");
        let request = VideoExportRequest::try_new(render_plan(), &output_path)
            .expect("supported Video Export request");
        let options = VideoExportOptions::new(
            FfmpegBinary::from_path(&fake_ffmpeg),
            FrameCount::new(NonZeroU64::new(1).expect("one is non-zero")),
        )
        .with_work_directory(temp_dir.join("work"));
        let frame_size = request.render_plan().configuration().frame_size();
        let context = ExportManifestContext::new(
            ExportRepositoryIdentity::new(temp_dir.display().to_string(), None),
            ExportSelectedRefState::new("explicit", "main", None),
            "sha256:test",
            ExportPacing::new("adaptive", "auto"),
            ExportOutputSettings::new(
                request.output_path(),
                request.output_preset(),
                frame_size.width(),
                frame_size.height(),
                request
                    .render_plan()
                    .configuration()
                    .frames_per_second()
                    .get(),
            ),
            "test",
        );
        let mut progress = Vec::new();

        let error = export_video_with_manifest_context(&request, &options, context, |event| {
            progress.push(event);
        })
        .expect_err("manifest path conflict should fail the export");
        let phases = progress
            .iter()
            .map(|event| event.phase())
            .collect::<Vec<_>>();

        assert!(error
            .to_string()
            .contains("could not write export manifest"));
        assert!(!phases.contains(&ExportProgressPhase::ManifestWriting));
        assert!(!phases.contains(&ExportProgressPhase::Warnings));
        assert!(!phases.contains(&ExportProgressPhase::Completion));
        assert!(manifest_path.is_dir());
    }

    fn render_plan() -> RenderPlan {
        let replay = RepositoryReplay::new(Mainline::new("main"));
        let configuration = RenderConfiguration::new(
            VisualMetaphor::new("flow"),
            Theme::new("default"),
            Layout::RepositoryGraph,
        );

        RenderPlan::new(replay, configuration)
    }

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "gitflux-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[cfg(unix)]
    fn write_executable_script(path: &std::path::Path, contents: &str) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(path, contents).expect("script should be written");
        let mut permissions = std::fs::metadata(path)
            .expect("script metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("script should be executable");
    }
}
