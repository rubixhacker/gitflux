//! Video Export orchestration seam for Gitflux.
//!
//! This crate will coordinate deterministic frame rendering and FFmpeg-driven
//! encoding. It owns Export Manifest data and Video Export requests without
//! embedding media codec support in the core Repository Replay pipeline.

use std::path::{Path, PathBuf};

use gitflux_render::RenderPlan;

/// Request to produce a Video Export from a render plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoExportRequest {
    render_plan: RenderPlan,
    output_path: PathBuf,
}

impl VideoExportRequest {
    /// Creates a Video Export request.
    #[must_use]
    pub fn new(render_plan: RenderPlan, output_path: impl Into<PathBuf>) -> Self {
        Self {
            render_plan,
            output_path: output_path.into(),
        }
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
}

/// Sidecar data describing the inputs used to produce a Video Export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportManifest {
    output_path: PathBuf,
    encoder: VideoEncoder,
}

impl ExportManifest {
    /// Creates an Export Manifest.
    #[must_use]
    pub fn new(output_path: impl Into<PathBuf>, encoder: VideoEncoder) -> Self {
        Self {
            output_path: output_path.into(),
            encoder,
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
}

/// External encoder used for Video Export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoEncoder {
    /// An installed FFmpeg binary.
    Ffmpeg,
}

/// Creates a scaffold Export Manifest for a future FFmpeg export.
#[must_use]
pub fn scaffold_export_manifest(request: &VideoExportRequest) -> ExportManifest {
    ExportManifest::new(request.output_path(), VideoEncoder::Ffmpeg)
}

#[cfg(test)]
mod tests {
    use super::{scaffold_export_manifest, VideoEncoder, VideoExportRequest};
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
            VideoExportRequest::new(RenderPlan::new(replay, configuration), "out/gitflux.mp4");

        let manifest = scaffold_export_manifest(&request);

        assert_eq!(manifest.output_path().to_string_lossy(), "out/gitflux.mp4");
        assert_eq!(manifest.encoder(), VideoEncoder::Ffmpeg);
    }
}
