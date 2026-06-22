//! Minimal Interactive Preview orchestration for Gitflux.
//!
//! This crate keeps preview state explicit and thin. Repository replay loading
//! and Render Configuration parsing stay with the same callers that power
//! export; preview receives the shared `RenderPlan` and interprets it through a
//! small window adapter boundary.

use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gitflux_render::{FrameIndex, OffscreenRenderer, RenderError, RenderPlan, RendererSettings};
use gitflux_scene::RenderConfiguration;
use minifb::{Key, KeyRepeat, Window, WindowOptions};

/// User-visible source for a Preview Render Configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewConfigurationSource {
    /// Preview uses `RenderConfiguration::default()`.
    Defaults,
    /// Preview was initialized from a TOML Render Configuration file.
    File(PathBuf),
}

impl PreviewConfigurationSource {
    /// Returns the source label used in status output.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Defaults => "defaults".to_owned(),
            Self::File(path) => path.display().to_string(),
        }
    }

    /// Returns the config file path when hot reload can observe one.
    #[must_use]
    pub fn file_path(&self) -> Option<&Path> {
        match self {
            Self::Defaults => None,
            Self::File(path) => Some(path),
        }
    }
}

/// Explicit preview reload policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewReloadPolicy {
    /// Check config file metadata and reload changed files during preview.
    ConfigFileMetadata,
    /// Do not reload configuration during preview.
    Disabled,
}

impl PreviewReloadPolicy {
    /// Returns true when preview should watch a config file.
    #[must_use]
    pub fn watches_config_file(self) -> bool {
        matches!(self, Self::ConfigFileMetadata)
    }
}

/// Minimal window request derived from the shared Render Configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewWindowPlan {
    title: String,
    width: u32,
    height: u32,
}

impl PreviewWindowPlan {
    /// Creates a preview window plan.
    #[must_use]
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            title: title.into(),
            width,
            height,
        }
    }

    /// Returns the window title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the requested window width.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the requested window height.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }
}

/// Complete immutable plan for one preview session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSessionPlan {
    render_plan: RenderPlan,
    configuration_source: PreviewConfigurationSource,
    reload_policy: PreviewReloadPolicy,
    window: PreviewWindowPlan,
}

impl PreviewSessionPlan {
    /// Creates a Preview Session Plan from the shared renderer input.
    #[must_use]
    pub fn new(
        render_plan: RenderPlan,
        configuration_source: PreviewConfigurationSource,
        reload_policy: PreviewReloadPolicy,
    ) -> Self {
        let frame_size = render_plan.configuration().frame_size();
        let mainline = render_plan.replay().mainline().as_str().to_owned();
        Self {
            render_plan,
            configuration_source,
            reload_policy,
            window: PreviewWindowPlan::new(
                format!("Gitflux Preview - {mainline}"),
                frame_size.width(),
                frame_size.height(),
            ),
        }
    }

    /// Returns the shared renderer plan.
    #[must_use]
    pub fn render_plan(&self) -> &RenderPlan {
        &self.render_plan
    }

    /// Returns the Render Configuration source.
    #[must_use]
    pub fn configuration_source(&self) -> &PreviewConfigurationSource {
        &self.configuration_source
    }

    /// Returns the preview reload policy.
    #[must_use]
    pub fn reload_policy(&self) -> PreviewReloadPolicy {
        self.reload_policy
    }

    /// Returns the window plan.
    #[must_use]
    pub fn window(&self) -> &PreviewWindowPlan {
        &self.window
    }

    /// Returns a file watcher for the plan when hot reload is available.
    pub fn config_watcher(&self) -> Result<Option<ConfigFileWatcher>, PreviewError> {
        if !self.reload_policy.watches_config_file() {
            return Ok(None);
        }

        self.configuration_source
            .file_path()
            .map(ConfigFileWatcher::new)
            .transpose()
    }
}

/// A changed Render Configuration loaded during preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewConfigurationReload {
    path: PathBuf,
    configuration: RenderConfiguration,
}

impl PreviewConfigurationReload {
    /// Returns the reloaded path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the reloaded Render Configuration.
    #[must_use]
    pub fn configuration(&self) -> &RenderConfiguration {
        &self.configuration
    }
}

/// Metadata-based watcher for preview config hot reload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFileWatcher {
    path: PathBuf,
    observed: ConfigFileSnapshot,
}

impl ConfigFileWatcher {
    /// Creates a watcher from the current file metadata.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, PreviewError> {
        let path = path.into();
        let observed = ConfigFileSnapshot::read(&path)?;
        Ok(Self { path, observed })
    }

    /// Returns a reloaded configuration when metadata has changed.
    pub fn reload_if_changed(
        &mut self,
    ) -> Result<Option<PreviewConfigurationReload>, PreviewError> {
        let current = ConfigFileSnapshot::read(&self.path)?;
        if current == self.observed {
            return Ok(None);
        }

        let contents =
            fs::read_to_string(&self.path).map_err(|source| PreviewError::ReadConfiguration {
                path: self.path.clone(),
                source,
            })?;
        let configuration = RenderConfiguration::from_toml_str(&contents).map_err(|source| {
            PreviewError::LoadConfiguration {
                path: self.path.clone(),
                source: source.to_string(),
            }
        })?;
        self.observed = current;

        Ok(Some(PreviewConfigurationReload {
            path: self.path.clone(),
            configuration,
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigFileSnapshot {
    modified: Option<SystemTime>,
    len: u64,
}

impl ConfigFileSnapshot {
    fn read(path: &Path) -> Result<Self, PreviewError> {
        let metadata = fs::metadata(path).map_err(|source| PreviewError::ReadConfiguration {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

/// Result of asking the preview adapter to open/render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRunSummary {
    window_status: PreviewWindowStatus,
    window: PreviewWindowPlan,
    rendered_initial_frame: bool,
    reload_watch: PreviewReloadWatch,
}

impl PreviewRunSummary {
    /// Creates a preview run summary.
    #[must_use]
    pub fn new(
        window_status: PreviewWindowStatus,
        window: PreviewWindowPlan,
        rendered_initial_frame: bool,
        reload_watch: PreviewReloadWatch,
    ) -> Self {
        Self {
            window_status,
            window,
            rendered_initial_frame,
            reload_watch,
        }
    }

    /// Returns the window status.
    #[must_use]
    pub fn window_status(&self) -> &PreviewWindowStatus {
        &self.window_status
    }

    /// Returns the window dimensions represented by this run summary.
    #[must_use]
    pub fn window(&self) -> &PreviewWindowPlan {
        &self.window
    }

    /// Returns true when preview exercised the shared renderer path.
    #[must_use]
    pub fn rendered_initial_frame(&self) -> bool {
        self.rendered_initial_frame
    }

    /// Returns the reload watch status.
    #[must_use]
    pub fn reload_watch(&self) -> PreviewReloadWatch {
        self.reload_watch
    }
}

/// Typed preview window status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewWindowStatus {
    /// A native preview adapter opened a window.
    Opened,
    /// A headless-friendly adapter produced the exact window plan.
    Planned(PreviewWindowPlan),
}

/// Typed hot reload watch status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewReloadWatch {
    /// Preview is watching the Render Configuration file.
    WatchingConfigFile,
    /// Preview has no config file to watch.
    NoConfigFile,
    /// Preview reload was disabled.
    Disabled,
}

/// Boundary for native or headless preview implementations.
pub trait PreviewWindowAdapter {
    /// Opens/runs a preview according to the supplied plan.
    fn run(&mut self, plan: &PreviewSessionPlan) -> Result<PreviewRunSummary, PreviewError>;
}

/// Headless-friendly adapter that validates the session plan only.
#[derive(Debug, Default)]
pub struct PlanningPreviewWindowAdapter;

impl PreviewWindowAdapter for PlanningPreviewWindowAdapter {
    fn run(&mut self, plan: &PreviewSessionPlan) -> Result<PreviewRunSummary, PreviewError> {
        Ok(PreviewRunSummary::new(
            PreviewWindowStatus::Planned(plan.window().clone()),
            plan.window().clone(),
            false,
            reload_watch_for_plan(plan),
        ))
    }
}

/// Minimal renderer-backed adapter used by the CLI until a native surface event
/// loop is introduced.
#[derive(Debug, Default)]
pub struct SharedRendererPreviewAdapter {
    settings: RendererSettings,
}

impl SharedRendererPreviewAdapter {
    /// Creates an adapter with renderer settings.
    #[must_use]
    pub fn new(settings: RendererSettings) -> Self {
        Self { settings }
    }
}

impl PreviewWindowAdapter for SharedRendererPreviewAdapter {
    fn run(&mut self, plan: &PreviewSessionPlan) -> Result<PreviewRunSummary, PreviewError> {
        let renderer = OffscreenRenderer::new(self.settings)?;
        let _frame = renderer.render_frame(plan.render_plan(), FrameIndex::new(0))?;

        Ok(PreviewRunSummary::new(
            PreviewWindowStatus::Planned(plan.window().clone()),
            plan.window().clone(),
            true,
            reload_watch_for_plan(plan),
        ))
    }
}

/// Minimal native window adapter backed by the shared offscreen renderer.
#[derive(Debug)]
pub struct NativePreviewWindowAdapter {
    settings: RendererSettings,
    max_frame_index: u64,
}

impl NativePreviewWindowAdapter {
    /// Creates a native preview adapter.
    #[must_use]
    pub fn new(settings: RendererSettings) -> Self {
        Self {
            settings,
            max_frame_index: 600,
        }
    }
}

impl Default for NativePreviewWindowAdapter {
    fn default() -> Self {
        Self::new(RendererSettings::default())
    }
}

impl PreviewWindowAdapter for NativePreviewWindowAdapter {
    fn run(&mut self, plan: &PreviewSessionPlan) -> Result<PreviewRunSummary, PreviewError> {
        let renderer = OffscreenRenderer::new(self.settings)?;
        let mut render_plan = plan.render_plan().clone();
        let mut watcher = plan.config_watcher()?;
        let mut frame_index = FrameIndex::new(0);
        let mut playing = false;
        let mut buffer = render_preview_buffer(&renderer, &render_plan, frame_index)?;
        let mut window = Window::new(
            plan.window().title(),
            buffer.width,
            buffer.height,
            WindowOptions::default(),
        )
        .map_err(|source| PreviewError::WindowOpen(source.to_string()))?;

        window.set_target_fps(render_plan.configuration().frames_per_second().get() as usize);

        while window.is_open() && !window.is_key_down(Key::Escape) {
            let mut needs_render = false;

            if window.is_key_pressed(Key::Space, KeyRepeat::No) {
                playing = !playing;
            }

            if window.is_key_pressed(Key::Right, KeyRepeat::Yes) || playing {
                frame_index = FrameIndex::new((frame_index.get() + 1).min(self.max_frame_index));
                needs_render = true;
            }

            if window.is_key_pressed(Key::Left, KeyRepeat::Yes) {
                frame_index = FrameIndex::new(frame_index.get().saturating_sub(1));
                needs_render = true;
            }

            if let Some(watcher) = watcher.as_mut() {
                if let Some(reload) = watcher.reload_if_changed()? {
                    render_plan =
                        RenderPlan::new(render_plan.replay().clone(), reload.configuration);
                    buffer = render_preview_buffer(&renderer, &render_plan, frame_index)?;
                    reopen_window_if_needed(
                        &mut window,
                        plan.window().title(),
                        &buffer,
                        render_plan.configuration().frames_per_second().get(),
                    )?;
                    needs_render = false;
                }
            }

            if needs_render {
                buffer = render_preview_buffer(&renderer, &render_plan, frame_index)?;
                reopen_window_if_needed(
                    &mut window,
                    plan.window().title(),
                    &buffer,
                    render_plan.configuration().frames_per_second().get(),
                )?;
            }

            window
                .update_with_buffer(&buffer.pixels, buffer.width, buffer.height)
                .map_err(|source| PreviewError::WindowUpdate(source.to_string()))?;
        }
        let final_window = PreviewWindowPlan::new(
            plan.window().title(),
            buffer.width as u32,
            buffer.height as u32,
        );

        Ok(PreviewRunSummary::new(
            PreviewWindowStatus::Opened,
            final_window,
            true,
            reload_watch_for_plan(plan),
        ))
    }
}

struct PreviewFrameBuffer {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

fn render_preview_buffer(
    renderer: &OffscreenRenderer,
    plan: &RenderPlan,
    frame_index: FrameIndex,
) -> Result<PreviewFrameBuffer, PreviewError> {
    let frame = renderer.render_frame(plan, frame_index)?;

    Ok(PreviewFrameBuffer {
        width: frame.size().width() as usize,
        height: frame.size().height() as usize,
        pixels: rgba_to_minifb_pixels(frame.rgba()),
    })
}

fn reopen_window_if_needed(
    window: &mut Window,
    title: &str,
    buffer: &PreviewFrameBuffer,
    frames_per_second: u32,
) -> Result<(), PreviewError> {
    let (width, height) = window.get_size();
    if width == buffer.width && height == buffer.height {
        return Ok(());
    }

    let mut replacement = Window::new(title, buffer.width, buffer.height, WindowOptions::default())
        .map_err(|source| PreviewError::WindowOpen(source.to_string()))?;
    replacement.set_target_fps(frames_per_second as usize);
    *window = replacement;

    Ok(())
}

fn rgba_to_minifb_pixels(rgba: &[u8]) -> Vec<u32> {
    rgba.chunks_exact(4)
        .map(|pixel| {
            let [red, green, blue, _alpha] = pixel else {
                unreachable!("chunks_exact(4) yields four-byte pixels");
            };
            ((*red as u32) << 16) | ((*green as u32) << 8) | (*blue as u32)
        })
        .collect()
}

/// Runs a preview session with the supplied adapter.
pub fn run_preview_session(
    plan: &PreviewSessionPlan,
    adapter: &mut dyn PreviewWindowAdapter,
) -> Result<PreviewRunSummary, PreviewError> {
    adapter.run(plan)
}

fn reload_watch_for_plan(plan: &PreviewSessionPlan) -> PreviewReloadWatch {
    match (
        plan.reload_policy(),
        plan.configuration_source().file_path().is_some(),
    ) {
        (PreviewReloadPolicy::Disabled, _) => PreviewReloadWatch::Disabled,
        (PreviewReloadPolicy::ConfigFileMetadata, true) => PreviewReloadWatch::WatchingConfigFile,
        (PreviewReloadPolicy::ConfigFileMetadata, false) => PreviewReloadWatch::NoConfigFile,
    }
}

/// Preview orchestration failure modes.
#[derive(Debug)]
pub enum PreviewError {
    /// Config metadata or contents could not be read.
    ReadConfiguration {
        /// Configuration path.
        path: PathBuf,
        /// I/O error.
        source: std::io::Error,
    },
    /// Config contents failed Render Configuration validation.
    LoadConfiguration {
        /// Configuration path.
        path: PathBuf,
        /// Validation diagnostics.
        source: String,
    },
    /// Shared renderer failed during preview.
    Render(RenderError),
    /// Native preview window could not be opened.
    WindowOpen(String),
    /// Native preview window could not be updated.
    WindowUpdate(String),
}

impl Display for PreviewError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadConfiguration { path, source } => {
                write!(
                    formatter,
                    "failed to read Preview Render Configuration {}: {source}",
                    path.display()
                )
            }
            Self::LoadConfiguration { path, source } => {
                write!(
                    formatter,
                    "failed to reload Preview Render Configuration {}:\n{source}",
                    path.display()
                )
            }
            Self::Render(source) => write!(formatter, "failed to render Preview frame: {source}"),
            Self::WindowOpen(source) => {
                write!(formatter, "failed to open Preview window: {source}")
            }
            Self::WindowUpdate(source) => {
                write!(formatter, "failed to update Preview window: {source}")
            }
        }
    }
}

impl std::error::Error for PreviewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadConfiguration { source, .. } => Some(source),
            Self::LoadConfiguration { .. } => None,
            Self::Render(source) => Some(source),
            Self::WindowOpen(_) | Self::WindowUpdate(_) => None,
        }
    }
}

impl From<RenderError> for PreviewError {
    fn from(value: RenderError) -> Self {
        Self::Render(value)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use gitflux_render::RenderPlan;
    use gitflux_scene::{Mainline, RenderConfiguration, RepositoryReplay};

    use super::{
        run_preview_session, ConfigFileWatcher, PlanningPreviewWindowAdapter,
        PreviewConfigurationSource, PreviewReloadPolicy, PreviewReloadWatch, PreviewSessionPlan,
        PreviewWindowStatus,
    };

    #[test]
    fn preview_plan_uses_render_plan_frame_size() {
        let render_plan = RenderPlan::new(
            RepositoryReplay::new(Mainline::new("main")),
            RenderConfiguration::default(),
        );
        let plan = PreviewSessionPlan::new(
            render_plan,
            PreviewConfigurationSource::Defaults,
            PreviewReloadPolicy::ConfigFileMetadata,
        );

        assert_eq!(plan.window().title(), "Gitflux Preview - main");
        assert_eq!(plan.window().width(), 1920);
        assert_eq!(plan.window().height(), 1080);
    }

    #[test]
    fn planning_adapter_reports_window_plan_without_rendering() {
        let render_plan = RenderPlan::new(
            RepositoryReplay::new(Mainline::new("main")),
            RenderConfiguration::default(),
        );
        let plan = PreviewSessionPlan::new(
            render_plan,
            PreviewConfigurationSource::Defaults,
            PreviewReloadPolicy::ConfigFileMetadata,
        );
        let mut adapter = PlanningPreviewWindowAdapter;
        let summary = run_preview_session(&plan, &mut adapter).expect("preview should plan");

        assert_eq!(
            summary.window_status(),
            &PreviewWindowStatus::Planned(plan.window().clone())
        );
        assert!(!summary.rendered_initial_frame());
        assert_eq!(summary.reload_watch(), PreviewReloadWatch::NoConfigFile);
    }

    #[test]
    fn preview_buffer_converts_rgba_pixels_to_window_pixels() {
        let pixels =
            super::rgba_to_minifb_pixels(&[0x12, 0x34, 0x56, 0xff, 0xaa, 0xbb, 0xcc, 0x80]);

        assert_eq!(pixels, [0x123456, 0xaabbcc]);
    }

    #[test]
    fn config_watcher_reloads_when_metadata_changes() {
        let path = std::env::temp_dir().join(format!(
            "gitflux-preview-config-{}-{}.toml",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::write(&path, valid_config("first", 1280)).expect("config should write");
        let mut watcher = ConfigFileWatcher::new(&path).expect("watcher should initialize");

        assert!(watcher
            .reload_if_changed()
            .expect("unchanged config should not fail")
            .is_none());

        std::thread::sleep(Duration::from_millis(5));
        fs::write(&path, valid_config("second", 640)).expect("config should update");
        let reload = watcher
            .reload_if_changed()
            .expect("changed config should reload")
            .expect("changed config should be reported");

        assert_eq!(reload.path(), path.as_path());
        assert_eq!(reload.configuration().theme().name(), "second");
        assert_eq!(reload.configuration().frame_size().width(), 640);
    }

    fn valid_config(theme: &str, width: u32) -> String {
        format!(
            r##"
frame_width = {width}
frame_height = 720
frames_per_second = 30

[theme]
name = "{theme}"
background_color = "#101010"
entity_color = "#32d583"
contributor_color = "#fdb022"

[layout]
kind = "repository_graph"
entity_spacing = 140
settle_iterations = 80
"##
        )
    }
}
