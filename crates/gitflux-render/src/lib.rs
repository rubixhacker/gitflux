//! Windowless GPU rendering for Gitflux.
//!
//! This crate owns deterministic wgpu frame production for Repository Replay
//! exports and smoke-testable PNG frame sequences. It deliberately keeps the
//! offscreen path independent from any preview window or surface.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
    fs,
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::mpsc,
};

use bytemuck::{Pod, Zeroable};
use gitflux_scene::{
    RenderConfiguration, RepositoryGraphScene, RepositoryReplay, SceneEmphasis, ScenePosition,
    Theme,
};

const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const BYTES_PER_PIXEL: u32 = 4;

const SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
"#;

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

/// Windowless renderer settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererSettings {
    power_preference: GpuPowerPreference,
}

impl RendererSettings {
    /// Creates renderer settings.
    #[must_use]
    pub fn new(power_preference: GpuPowerPreference) -> Self {
        Self { power_preference }
    }

    /// Returns the requested GPU power preference.
    #[must_use]
    pub fn power_preference(self) -> GpuPowerPreference {
        self.power_preference
    }
}

impl Default for RendererSettings {
    fn default() -> Self {
        Self {
            power_preference: GpuPowerPreference::LowPower,
        }
    }
}

/// Adapter preference for offscreen rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuPowerPreference {
    /// Prefer a lower-power adapter when available.
    LowPower,
    /// Prefer a higher-performance adapter when available.
    HighPerformance,
}

impl From<GpuPowerPreference> for wgpu::PowerPreference {
    fn from(value: GpuPowerPreference) -> Self {
        match value {
            GpuPowerPreference::LowPower => Self::LowPower,
            GpuPowerPreference::HighPerformance => Self::HighPerformance,
        }
    }
}

/// Output frame dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderFrameSize {
    width: u32,
    height: u32,
}

impl RenderFrameSize {
    /// Creates frame dimensions.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Returns the frame width in pixels.
    #[must_use]
    pub fn width(self) -> u32 {
        self.width
    }

    /// Returns the frame height in pixels.
    #[must_use]
    pub fn height(self) -> u32 {
        self.height
    }
}

/// A rendered RGBA frame copied back from the GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFrame {
    index: FrameIndex,
    size: RenderFrameSize,
    frames_per_second: u32,
    rgba: Vec<u8>,
}

impl RenderedFrame {
    /// Returns the rendered frame index.
    #[must_use]
    pub fn index(&self) -> FrameIndex {
        self.index
    }

    /// Returns the rendered frame size.
    #[must_use]
    pub fn size(&self) -> RenderFrameSize {
        self.size
    }

    /// Returns the configured render frame rate.
    #[must_use]
    pub fn frames_per_second(&self) -> u32 {
        self.frames_per_second
    }

    /// Returns tightly packed RGBA8 pixels.
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

/// Zero-based frame index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameIndex(u64);

impl FrameIndex {
    /// Creates a frame index.
    #[must_use]
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric frame index.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Number of frames to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCount(NonZeroU64);

impl FrameCount {
    /// Creates a non-zero frame count.
    #[must_use]
    pub fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// Returns the frame count.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

/// Target directory and naming policy for PNG frame sequences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameSequenceOutput {
    directory: PathBuf,
    file_stem: FrameFileStem,
    first_frame_index: FrameIndex,
}

impl FrameSequenceOutput {
    /// Creates a PNG frame-sequence output target.
    #[must_use]
    pub fn new(directory: impl Into<PathBuf>, file_stem: FrameFileStem) -> Self {
        Self {
            directory: directory.into(),
            file_stem,
            first_frame_index: FrameIndex::new(0),
        }
    }

    /// Uses a non-zero first frame index for the output sequence.
    #[must_use]
    pub fn with_first_frame_index(mut self, first_frame_index: FrameIndex) -> Self {
        self.first_frame_index = first_frame_index;
        self
    }

    /// Returns the output directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Returns the output file stem.
    #[must_use]
    pub fn file_stem(&self) -> &FrameFileStem {
        &self.file_stem
    }

    /// Returns the first frame index written by this target.
    #[must_use]
    pub fn first_frame_index(&self) -> FrameIndex {
        self.first_frame_index
    }

    /// Returns the PNG path for a frame index.
    #[must_use]
    pub fn frame_path(&self, frame_index: FrameIndex) -> PathBuf {
        self.directory.join(format!(
            "{}_{:06}.png",
            self.file_stem.as_str(),
            frame_index.get()
        ))
    }
}

/// File stem used by frame-sequence PNGs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameFileStem(String);

impl FrameFileStem {
    /// Creates a frame file stem.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the frame file stem.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Files produced by a frame-sequence render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameSequenceManifest {
    frames_per_second: u32,
    frame_size: RenderFrameSize,
    frame_paths: Vec<PathBuf>,
}

impl FrameSequenceManifest {
    /// Returns the frame rate used for the output sequence.
    #[must_use]
    pub fn frames_per_second(&self) -> u32 {
        self.frames_per_second
    }

    /// Returns the size of each rendered frame.
    #[must_use]
    pub fn frame_size(&self) -> RenderFrameSize {
        self.frame_size
    }

    /// Returns PNG frame paths in render order.
    #[must_use]
    pub fn frame_paths(&self) -> &[PathBuf] {
        &self.frame_paths
    }
}

/// Windowless wgpu renderer.
pub struct OffscreenRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
}

impl OffscreenRenderer {
    /// Creates a renderer that can produce frames without a preview window.
    pub fn new(settings: RendererSettings) -> Result<Self, RenderError> {
        pollster::block_on(Self::new_async(settings))
    }

    async fn new_async(settings: RendererSettings) -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: settings.power_preference().into(),
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| RenderError::GpuUnavailable)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("gitflux-offscreen-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| RenderError::RequestDevice(error.to_string()))?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gitflux-offscreen-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = create_pipeline(&device, &shader);

        Ok(Self {
            device,
            queue,
            pipeline,
        })
    }

    /// Renders a single offscreen frame.
    pub fn render_frame(
        &self,
        plan: &RenderPlan,
        frame_index: FrameIndex,
    ) -> Result<RenderedFrame, RenderError> {
        let scene = RepositoryGraphScene::from_replay(plan.replay(), plan.configuration());
        let frame_size =
            RenderFrameSize::new(scene.frame_size().width(), scene.frame_size().height());
        let batches = RenderBatches::from_scene(&scene, plan.configuration().theme(), frame_index);
        let rgba = self.render_batches(frame_size, plan.configuration().theme(), &batches)?;

        Ok(RenderedFrame {
            index: frame_index,
            size: frame_size,
            frames_per_second: scene.frames_per_second(),
            rgba,
        })
    }

    /// Renders a PNG frame sequence.
    pub fn render_frame_sequence(
        &self,
        plan: &RenderPlan,
        output: &FrameSequenceOutput,
        frame_count: FrameCount,
    ) -> Result<FrameSequenceManifest, RenderError> {
        fs::create_dir_all(output.directory()).map_err(RenderError::CreateOutputDirectory)?;

        let mut frame_paths = Vec::new();
        for frame_indices in frame_sequence_indices(output, frame_count) {
            let frame = self.render_frame(plan, frame_indices.playback_frame_index())?;
            let frame_path = output.frame_path(frame_indices.output_frame_index());
            write_png(&frame, &frame_path)?;
            frame_paths.push(frame_path);
        }

        let frame_size = RenderFrameSize::new(
            plan.configuration().frame_size().width(),
            plan.configuration().frame_size().height(),
        );
        Ok(FrameSequenceManifest {
            frames_per_second: plan.configuration().frames_per_second().get(),
            frame_size,
            frame_paths,
        })
    }

    fn render_batches(
        &self,
        frame_size: RenderFrameSize,
        theme: &Theme,
        batches: &RenderBatches,
    ) -> Result<Vec<u8>, RenderError> {
        let texture_extent = wgpu::Extent3d {
            width: frame_size.width(),
            height: frame_size.height(),
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gitflux-offscreen-target"),
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let unpadded_bytes_per_row = frame_size.width() * BYTES_PER_PIXEL;
        let padded_bytes_per_row = align_to_copy_bytes_per_row(unpadded_bytes_per_row);
        let output_buffer_size = u64::from(padded_bytes_per_row) * u64::from(frame_size.height());
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gitflux-offscreen-readback"),
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gitflux-offscreen-encoder"),
            });

        {
            let color_attachment = wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(parse_color(theme.background_color().as_hex())),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gitflux-offscreen-pass"),
                color_attachments: &[Some(color_attachment)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            for batch in batches.iter() {
                if batch.vertices.is_empty() {
                    continue;
                }
                let vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(batch.kind.label()),
                    size: std::mem::size_of_val(batch.vertices.as_slice()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.queue
                    .write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(&batch.vertices));
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.draw(0..batch.vertices.len() as u32, 0..1);
            }
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(frame_size.height()),
                },
            },
            texture_extent,
        );
        self.queue.submit(Some(encoder.finish()));

        let buffer_slice = output_buffer.slice(..);
        let (sender, receiver) = mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| RenderError::Readback(error.to_string()))?
            .map_err(|error| RenderError::Readback(error.to_string()))?;

        let mapped = buffer_slice.get_mapped_range();
        let mut pixels = Vec::with_capacity(
            (u64::from(unpadded_bytes_per_row) * u64::from(frame_size.height())) as usize,
        );
        for row in mapped
            .chunks(padded_bytes_per_row as usize)
            .take(frame_size.height() as usize)
        {
            pixels.extend_from_slice(&row[..unpadded_bytes_per_row as usize]);
        }
        drop(mapped);
        output_buffer.unmap();

        Ok(pixels)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameSequenceIndexPair {
    playback_frame_index: FrameIndex,
    output_frame_index: FrameIndex,
}

impl FrameSequenceIndexPair {
    fn playback_frame_index(self) -> FrameIndex {
        self.playback_frame_index
    }

    fn output_frame_index(self) -> FrameIndex {
        self.output_frame_index
    }
}

fn frame_sequence_indices(
    output: &FrameSequenceOutput,
    frame_count: FrameCount,
) -> Vec<FrameSequenceIndexPair> {
    (0..frame_count.get())
        .map(|offset| FrameSequenceIndexPair {
            playback_frame_index: FrameIndex::new(offset),
            output_frame_index: FrameIndex::new(output.first_frame_index().get() + offset),
        })
        .collect()
}

/// A future GPU renderer capability descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererCapability {
    /// The renderer can produce offscreen frames for Video Export.
    OffscreenFrameProduction,
    /// The renderer can power an Interactive Preview.
    InteractivePreview,
    /// The renderer can write PNG frame sequences.
    PngFrameSequenceOutput,
}

/// Returns the renderer capabilities represented by this crate boundary.
#[must_use]
pub fn planned_capabilities() -> &'static [RendererCapability] {
    &[
        RendererCapability::OffscreenFrameProduction,
        RendererCapability::InteractivePreview,
        RendererCapability::PngFrameSequenceOutput,
    ]
}

/// Renderer failure modes.
#[derive(Debug)]
pub enum RenderError {
    /// No usable headless GPU adapter was available.
    GpuUnavailable,
    /// wgpu could not create a device.
    RequestDevice(String),
    /// GPU readback failed.
    Readback(String),
    /// Output directory creation failed.
    CreateOutputDirectory(std::io::Error),
    /// PNG encoding failed for a rendered frame.
    WritePng {
        /// Path that failed to write.
        path: PathBuf,
        /// Image encoding error.
        source: image::ImageError,
    },
}

impl Display for RenderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GpuUnavailable => formatter.write_str("no usable headless GPU adapter available"),
            Self::RequestDevice(error) => {
                write!(formatter, "could not create wgpu device: {error}")
            }
            Self::Readback(error) => write!(formatter, "could not read rendered frame: {error}"),
            Self::CreateOutputDirectory(error) => {
                write!(
                    formatter,
                    "could not create frame output directory: {error}"
                )
            }
            Self::WritePng { path, source } => {
                write!(
                    formatter,
                    "could not write PNG frame {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateOutputDirectory(error) => Some(error),
            Self::WritePng { source, .. } => Some(source),
            Self::GpuUnavailable | Self::RequestDevice(_) | Self::Readback(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderBatchKind {
    Contributor,
    Directory,
    File,
    Summary,
    Activity,
}

impl RenderBatchKind {
    fn label(self) -> &'static str {
        match self {
            Self::Contributor => "gitflux-contributor-batch",
            Self::Directory => "gitflux-directory-batch",
            Self::File => "gitflux-file-batch",
            Self::Summary => "gitflux-summary-batch",
            Self::Activity => "gitflux-activity-batch",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RenderBatch {
    kind: RenderBatchKind,
    vertices: Vec<Vertex>,
}

#[derive(Debug, Clone, PartialEq)]
struct RenderBatches {
    contributors: RenderBatch,
    directories: RenderBatch,
    files: RenderBatch,
    summaries: RenderBatch,
    activities: RenderBatch,
}

impl RenderBatches {
    fn from_scene(scene: &RepositoryGraphScene, theme: &Theme, frame_index: FrameIndex) -> Self {
        let transform = SceneTransform::from_scene(scene);
        let entity_color = color_from_hex(theme.entity_color().as_hex());
        let contributor_color = color_from_hex(theme.contributor_color().as_hex());
        let directory_color = scale_color(entity_color, 0.64, 0.88);
        let summary_color = scale_color(entity_color, 0.44, 0.70);
        let activity_color = [1.0, 1.0, 1.0, 0.96];
        let active_file_ids = active_file_ids(scene, frame_index);

        let mut contributors = RenderBatch::new(RenderBatchKind::Contributor);
        let contributor_size = transform.size_to_clip_space(20.0, 20.0);
        for contributor in scene.contributors() {
            let (x, y) = transform.to_pixels(contributor.position());
            contributors.push_rect(x, y, contributor_size, contributor_color);
        }

        let mut directories = RenderBatch::new(RenderBatchKind::Directory);
        let directory_size = transform.size_to_clip_space(22.0, 14.0);
        for directory in scene.directories() {
            let (x, y) = transform.to_pixels(directory.position());
            directories.push_rect(x, y, directory_size, directory_color);
        }

        let mut files = RenderBatch::new(RenderBatchKind::File);
        let file_size = transform.size_to_clip_space(14.0, 14.0);
        let mut file_positions = BTreeMap::new();
        for file in scene.files() {
            let (x, y) = transform.to_pixels(file.position());
            file_positions.insert(file.id().as_str().to_owned(), (x, y));
            let alpha = match file.emphasis() {
                SceneEmphasis::Normal => 0.92,
                SceneEmphasis::DeEmphasized => 0.42,
            };
            files.push_rect(
                x,
                y,
                file_size,
                [entity_color[0], entity_color[1], entity_color[2], alpha],
            );
        }

        let mut summaries = RenderBatch::new(RenderBatchKind::Summary);
        for summary in scene.visual_summaries() {
            let (x, y) = transform.to_pixels(summary.position());
            let size = 16.0 + summary.weight().get().min(8) as f32;
            summaries.push_rect(
                x,
                y,
                transform.size_to_clip_space(size, size),
                summary_color,
            );
        }

        let mut activities = RenderBatch::new(RenderBatchKind::Activity);
        let activity_size = transform.size_to_clip_space(24.0, 24.0);
        for file_id in active_file_ids {
            if let Some((x, y)) = file_positions.get(&file_id) {
                activities.push_rect(*x, *y, activity_size, activity_color);
            }
        }

        Self {
            contributors,
            directories,
            files,
            summaries,
            activities,
        }
    }

    fn iter(&self) -> impl Iterator<Item = &RenderBatch> {
        [
            &self.summaries,
            &self.directories,
            &self.files,
            &self.contributors,
            &self.activities,
        ]
        .into_iter()
    }
}

impl RenderBatch {
    fn new(kind: RenderBatchKind) -> Self {
        Self {
            kind,
            vertices: Vec::new(),
        }
    }

    fn push_rect(&mut self, center_x: f32, center_y: f32, size: ClipSize, color: [f32; 4]) {
        let width = size.width();
        let height = size.height();
        let left = center_x - width / 2.0;
        let right = center_x + width / 2.0;
        let top = center_y - height / 2.0;
        let bottom = center_y + height / 2.0;
        self.vertices.extend_from_slice(&[
            Vertex::new(left, top, color),
            Vertex::new(right, top, color),
            Vertex::new(right, bottom, color),
            Vertex::new(left, top, color),
            Vertex::new(right, bottom, color),
            Vertex::new(left, bottom, color),
        ]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ClipSize {
    width: f32,
    height: f32,
}

impl ClipSize {
    fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    fn width(self) -> f32 {
        self.width
    }

    fn height(self) -> f32 {
        self.height
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

    fn new(x: f32, y: f32, color: [f32; 4]) -> Self {
        Self {
            position: [x, y],
            color,
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SceneTransform {
    min_x: f32,
    min_y: f32,
    scale: f32,
    offset_x: f32,
    offset_y: f32,
    width: f32,
    height: f32,
}

impl SceneTransform {
    fn from_scene(scene: &RepositoryGraphScene) -> Self {
        let mut positions = Vec::new();
        positions.extend(scene.contributors().iter().map(|item| item.position()));
        positions.extend(scene.directories().iter().map(|item| item.position()));
        positions.extend(scene.files().iter().map(|item| item.position()));
        positions.extend(scene.visual_summaries().iter().map(|item| item.position()));

        let frame_size = scene.frame_size();
        let width = frame_size.width() as f32;
        let height = frame_size.height() as f32;
        if positions.is_empty() {
            return Self {
                min_x: 0.0,
                min_y: 0.0,
                scale: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
                width,
                height,
            };
        }

        let min_x = positions
            .iter()
            .map(|position| position.x())
            .min()
            .unwrap_or(0) as f32;
        let max_x = positions
            .iter()
            .map(|position| position.x())
            .max()
            .unwrap_or(0) as f32;
        let min_y = positions
            .iter()
            .map(|position| position.y())
            .min()
            .unwrap_or(0) as f32;
        let max_y = positions
            .iter()
            .map(|position| position.y())
            .max()
            .unwrap_or(0) as f32;
        let content_width = (max_x - min_x).max(1.0);
        let content_height = (max_y - min_y).max(1.0);
        let margin_x = (width * 0.10).max(24.0);
        let margin_y = (height * 0.14).max(24.0);
        let scale_x = (width - margin_x * 2.0).max(1.0) / content_width;
        let scale_y = (height - margin_y * 2.0).max(1.0) / content_height;
        let scale = scale_x.min(scale_y).min(1.0);
        let rendered_width = content_width * scale;
        let rendered_height = content_height * scale;

        Self {
            min_x,
            min_y,
            scale,
            offset_x: (width - rendered_width) / 2.0,
            offset_y: (height - rendered_height) / 2.0,
            width,
            height,
        }
    }

    fn to_pixels(self, position: ScenePosition) -> (f32, f32) {
        let pixel_x = self.offset_x + (position.x() as f32 - self.min_x) * self.scale;
        let pixel_y = self.offset_y + (position.y() as f32 - self.min_y) * self.scale;
        (
            (pixel_x / self.width) * 2.0 - 1.0,
            1.0 - (pixel_y / self.height) * 2.0,
        )
    }

    fn size_to_clip_space(self, width: f32, height: f32) -> ClipSize {
        ClipSize::new((width / self.width) * 2.0, (height / self.height) * 2.0)
    }
}

fn create_pipeline(device: &wgpu::Device, shader: &wgpu::ShaderModule) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("gitflux-offscreen-pipeline"),
        layout: None,
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Vertex::layout()],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: OUTPUT_FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn active_file_ids(scene: &RepositoryGraphScene, frame_index: FrameIndex) -> BTreeSet<String> {
    let activity_window = (u64::from(scene.frames_per_second()) / 2).max(1);
    scene
        .activities()
        .iter()
        .flat_map(|activity| {
            activity
                .file_changes()
                .iter()
                .map(|file_change| (activity.playback_frame(), file_change))
        })
        .filter_map(|file_change| {
            let (activity_frame, file_change) = file_change;
            let start = activity_frame + file_change.playback_frame_offset();
            let end = start + activity_window;
            (start..=end)
                .contains(&frame_index.get())
                .then(|| file_change.file_id().as_str().to_owned())
        })
        .collect()
}

fn write_png(frame: &RenderedFrame, path: &Path) -> Result<(), RenderError> {
    image::save_buffer(
        path,
        frame.rgba(),
        frame.size().width(),
        frame.size().height(),
        image::ColorType::Rgba8,
    )
    .map_err(|source| RenderError::WritePng {
        path: path.to_path_buf(),
        source,
    })
}

fn align_to_copy_bytes_per_row(value: u32) -> u32 {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    value.div_ceil(align) * align
}

fn parse_color(value: &str) -> wgpu::Color {
    let [red, green, blue, alpha] = color_from_hex(value);
    wgpu::Color {
        r: red.into(),
        g: green.into(),
        b: blue.into(),
        a: alpha.into(),
    }
}

fn color_from_hex(value: &str) -> [f32; 4] {
    let red = u8::from_str_radix(&value[1..3], 16).expect("validated render color") as f32 / 255.0;
    let green =
        u8::from_str_radix(&value[3..5], 16).expect("validated render color") as f32 / 255.0;
    let blue = u8::from_str_radix(&value[5..7], 16).expect("validated render color") as f32 / 255.0;
    [red, green, blue, 1.0]
}

fn scale_color(mut color: [f32; 4], scale: f32, alpha: f32) -> [f32; 4] {
    color[0] *= scale;
    color[1] *= scale;
    color[2] *= scale;
    color[3] = alpha;
    color
}

#[cfg(test)]
mod tests {
    use super::{
        active_file_ids, frame_sequence_indices, planned_capabilities, FrameCount, FrameFileStem,
        FrameIndex, FrameSequenceOutput, GpuPowerPreference, OffscreenRenderer, RenderFrameSize,
        RenderPlan, RendererCapability, RendererSettings,
    };
    use gitflux_scene::{
        CommitEvent, CommitId, Contributor, FileChange, FileChangeKind, Layout, Mainline,
        RenderConfiguration, RepositoryEntity, RepositoryGraphScene, RepositoryReplay, Theme,
        VisualMetaphor,
    };
    use std::{collections::BTreeSet, num::NonZeroU64, path::PathBuf};

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
        assert!(planned_capabilities().contains(&RendererCapability::PngFrameSequenceOutput));
    }

    #[test]
    fn frame_sequence_output_paths_are_deterministic() {
        let output = FrameSequenceOutput::new("frames", FrameFileStem::new("gitflux"))
            .with_first_frame_index(FrameIndex::new(7));

        assert_eq!(output.first_frame_index(), FrameIndex::new(7));
        assert_eq!(
            output.frame_path(FrameIndex::new(42)),
            PathBuf::from("frames/gitflux_000042.png")
        );
    }

    #[test]
    fn frame_sequence_output_index_is_independent_from_playback_index() {
        let output = FrameSequenceOutput::new("frames", FrameFileStem::new("gitflux"))
            .with_first_frame_index(FrameIndex::new(100));
        let pairs = frame_sequence_indices(&output, FrameCount::new(non_zero_frame_count(3)));

        assert_eq!(
            pairs
                .iter()
                .map(|pair| pair.playback_frame_index())
                .collect::<Vec<_>>(),
            vec![FrameIndex::new(0), FrameIndex::new(1), FrameIndex::new(2)]
        );
        assert_eq!(
            pairs
                .iter()
                .map(|pair| pair.output_frame_index())
                .collect::<Vec<_>>(),
            vec![
                FrameIndex::new(100),
                FrameIndex::new(101),
                FrameIndex::new(102)
            ]
        );
    }

    #[test]
    fn renderer_settings_preserve_power_preference() {
        let settings = RendererSettings::new(GpuPowerPreference::HighPerformance);

        assert_eq!(
            settings.power_preference(),
            GpuPowerPreference::HighPerformance
        );
    }

    #[test]
    fn render_frame_size_preserves_dimensions() {
        let size = RenderFrameSize::new(320, 180);

        assert_eq!(size.width(), 320);
        assert_eq!(size.height(), 180);
    }

    #[test]
    fn activity_batches_respect_file_change_frame_offsets() {
        let mut replay = RepositoryReplay::new(Mainline::new("main"));
        let file_changes = (0..6)
            .map(|index| {
                FileChange::new(
                    RepositoryEntity::new(format!("src/generated/file_{index}.rs")),
                    FileChangeKind::Modified,
                )
            })
            .collect();
        replay.push_commit_event(CommitEvent::new(
            CommitId::new("large"),
            Contributor::automation("Generator"),
            file_changes,
        ));
        let configuration = RenderConfiguration::from_toml_str(
            r##"
frame_width = 96
frame_height = 64
frames_per_second = 60

[theme]
name = "smoke"
background_color = "#05070b"
entity_color = "#7dd3fc"
contributor_color = "#facc15"

[layout]
kind = "repository_graph"
entity_spacing = 24
settle_iterations = 10
"##,
        )
        .expect("large commit render configuration should parse");
        let scene = RepositoryGraphScene::from_replay(&replay, &configuration);

        assert!(!active_file_ids(&scene, FrameIndex::new(0)).contains("src/generated/file_5.rs"));
        assert!(active_file_ids(&scene, FrameIndex::new(120)).contains("src/generated/file_5.rs"));
    }

    #[test]
    fn offscreen_renderer_smoke_test_is_tolerant() {
        let Some(renderer) = gpu_renderer_or_skip("offscreen renderer smoke test") else {
            return;
        };
        let plan = smoke_render_plan();

        let frame = renderer
            .render_frame(&plan, FrameIndex::new(0))
            .expect("offscreen frame should render when a GPU adapter is available");

        assert_eq!(frame.size(), RenderFrameSize::new(96, 64));
        assert_eq!(frame.frames_per_second(), 12);
        assert_eq!(frame.rgba().len(), 96 * 64 * 4);
        let opaque = frame
            .rgba()
            .chunks_exact(4)
            .filter(|pixel| pixel[3] == 255)
            .count();
        let distinct_colors: BTreeSet<[u8; 4]> = frame
            .rgba()
            .chunks_exact(4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
            .collect();
        let all_white = frame
            .rgba()
            .chunks_exact(4)
            .all(|pixel| pixel == [255, 255, 255, 255]);
        assert!(opaque > 0);
        assert!(
            distinct_colors.len() > 1,
            "rendered frame should include theme background and visible graph marks"
        );
        assert!(!all_white, "rendered frame must not be a solid white field");
    }

    #[test]
    fn png_sequence_smoke_test_is_tolerant() {
        let Some(renderer) = gpu_renderer_or_skip("PNG sequence smoke test") else {
            return;
        };
        let output = FrameSequenceOutput::new(
            std::env::temp_dir().join(format!("gitflux-render-test-{}", std::process::id())),
            FrameFileStem::new("smoke"),
        );

        let manifest = renderer
            .render_frame_sequence(
                &smoke_render_plan(),
                &output,
                FrameCount::new(non_zero_frame_count(1)),
            )
            .expect("PNG sequence should render when a GPU adapter is available");

        assert_eq!(manifest.frames_per_second(), 12);
        assert_eq!(manifest.frame_size(), RenderFrameSize::new(96, 64));
        assert_eq!(manifest.frame_paths().len(), 1);
        assert!(manifest.frame_paths()[0].is_file());

        let image = image::open(&manifest.frame_paths()[0])
            .expect("smoke PNG should decode")
            .to_rgba8();
        assert_eq!(image.width(), 96);
        assert_eq!(image.height(), 64);

        let _ = std::fs::remove_dir_all(output.directory());
    }

    fn smoke_render_plan() -> RenderPlan {
        let mut replay = RepositoryReplay::new(Mainline::new("main"));
        replay.push_commit_event(CommitEvent::new(
            CommitId::new("abc123"),
            Contributor::human("Ada"),
            vec![
                FileChange::new(RepositoryEntity::new("src/lib.rs"), FileChangeKind::Added),
                FileChange::new(
                    RepositoryEntity::new("src/render.rs"),
                    FileChangeKind::Modified,
                ),
            ],
        ));

        let configuration = RenderConfiguration::from_toml_str(
            r##"
frame_width = 96
frame_height = 64
frames_per_second = 12

[theme]
name = "smoke"
background_color = "#05070b"
entity_color = "#7dd3fc"
contributor_color = "#facc15"

[layout]
kind = "repository_graph"
entity_spacing = 24
settle_iterations = 10
"##,
        )
        .expect("smoke render configuration should parse");

        RenderPlan::new(replay, configuration)
    }

    fn non_zero_frame_count(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).expect("non-zero frame count")
    }

    fn gpu_renderer_or_skip(test_name: &str) -> Option<OffscreenRenderer> {
        match OffscreenRenderer::new(RendererSettings::default()) {
            Ok(renderer) => Some(renderer),
            Err(error) if explicit_gpu_skip_enabled() => {
                eprintln!("skipping {test_name}: {error}");
                None
            }
            Err(error) => panic!(
                "{test_name} could not create a headless GPU renderer: {error}. \
                 Set GITFLUX_SKIP_GPU_SMOKE_TESTS=1 to skip explicitly on unsupported runners."
            ),
        }
    }

    fn explicit_gpu_skip_enabled() -> bool {
        std::env::var("GITFLUX_SKIP_GPU_SMOKE_TESTS").as_deref() == Ok("1")
    }
}
