//! GPU rendering seam for Gitflux.
//!
//! This crate will own direct wgpu frame production for Repository Replay
//! previews and exports. The scaffold keeps Render Configuration and
//! Repository Replay data explicit while avoiding any GPU dependency until the
//! renderer is implemented.

use gitflux_scene::{RenderConfiguration, RepositoryReplay};

/// Deterministic input for producing rendered frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPlan {
    replay: RepositoryReplay,
    configuration: RenderConfiguration,
}

impl RenderPlan {
    /// Creates a render plan from a Repository Replay and Render Configuration.
    #[must_use]
    pub fn new(replay: RepositoryReplay, configuration: RenderConfiguration) -> Self {
        Self {
            replay,
            configuration,
        }
    }

    /// Returns the Repository Replay to render.
    #[must_use]
    pub fn replay(&self) -> &RepositoryReplay {
        &self.replay
    }

    /// Returns the Render Configuration.
    #[must_use]
    pub fn configuration(&self) -> &RenderConfiguration {
        &self.configuration
    }
}

/// A future GPU renderer capability descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererCapability {
    /// The renderer can produce offscreen frames for Video Export.
    OffscreenFrameProduction,
    /// The renderer can power an Interactive Preview.
    InteractivePreview,
}

/// Returns the renderer capabilities represented by this crate boundary.
#[must_use]
pub fn planned_capabilities() -> &'static [RendererCapability] {
    &[
        RendererCapability::OffscreenFrameProduction,
        RendererCapability::InteractivePreview,
    ]
}

#[cfg(test)]
mod tests {
    use super::{planned_capabilities, RenderPlan, RendererCapability};
    use gitflux_scene::{
        Layout, Mainline, RenderConfiguration, RepositoryReplay, Theme, VisualMetaphor,
    };

    #[test]
    fn render_plan_preserves_inputs() {
        let replay = RepositoryReplay::new(Mainline::new("main"));
        let configuration = RenderConfiguration::new(
            VisualMetaphor::new("flow"),
            Theme::new("default"),
            Layout::RepositoryGraph,
        );

        let plan = RenderPlan::new(replay, configuration);

        assert_eq!(plan.replay().mainline().as_str(), "main");
        assert_eq!(plan.configuration().theme().as_str(), "default");
    }

    #[test]
    fn planned_capabilities_include_offscreen_rendering() {
        assert!(planned_capabilities().contains(&RendererCapability::OffscreenFrameProduction));
    }
}
