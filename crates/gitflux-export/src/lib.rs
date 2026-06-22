//! Video Export orchestration seam for Gitflux.
//!
//! This crate will coordinate deterministic frame rendering and FFmpeg-driven
//! encoding. It owns Export Manifest data and Video Export requests without
//! embedding media codec support in the core Repository Replay pipeline.

use std::{
    fmt::{Display, Formatter},
    num::NonZeroU64,
    path::{Path, PathBuf},
    process::Command,
};

use gitflux_render::{
    FrameCount, FrameFileStem, FrameSequenceOutput, OffscreenRenderer, RenderError, RenderPlan,
    RendererSettings,
};

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
    encoder: VideoEncoder,
    output_preset: OutputPreset,
}

impl ExportManifest {
    /// Creates an Export Manifest from a validated Video Export request.
    #[must_use]
    pub fn from_request(request: &VideoExportRequest, encoder: VideoEncoder) -> Self {
        Self {
            output_path: request.output_path().to_path_buf(),
            encoder,
            output_preset: request.output_preset(),
        }
    }

    /// Returns the exported media path.
    #[must_use]
    pub fn output_path(&self) -> &Path {
        &self.output_path
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
}

/// External encoder used for Video Export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoEncoder {
    /// An installed FFmpeg binary.
    Ffmpeg,
}

/// Container and codec preset for a Video Export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FfmpegNotFound { source, .. } => Some(source),
            Self::OutputNotCreated { source, .. } => Some(source),
            Self::Render(error) => Some(error),
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

    let export_result = (|| {
        let renderer =
            OffscreenRenderer::new(options.renderer_settings).map_err(ExportError::Render)?;
        renderer
            .render_frame_sequence(request.render_plan(), &frame_output, options.frame_count())
            .map_err(ExportError::Render)?;
        command_plan.run()?;
        verify_output_file(request.output_path())
    })();
    let _ = std::fs::remove_dir_all(&options.work_directory);
    export_result?;

    Ok(ExportManifest::from_request(request, VideoEncoder::Ffmpeg))
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
        scaffold_export_manifest, verify_output_file, FfmpegBinary, FfmpegCommandPlan,
        OutputPreset, VideoEncoder, VideoExportRequest,
    };
    use gitflux_render::RenderPlan;
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
