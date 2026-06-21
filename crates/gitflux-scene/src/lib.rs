//! Scene and layout data for Repository Replay rendering.
//!
//! This crate owns the deterministic core data shared by Repository Ingestion,
//! GPU rendering, and Video Export orchestration. It names the Repository
//! Replay timeline, Repository Graph layout, repository entities, contributors,
//! and Render Configuration without depending on Git, wgpu, or FFmpeg adapters.

use std::path::PathBuf;

use toml::Value;

/// A deterministic playback model for a repository's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryReplay {
    mainline: Mainline,
    commit_events: Vec<CommitEvent>,
}

impl RepositoryReplay {
    /// Creates a Repository Replay for the given Mainline.
    #[must_use]
    pub fn new(mainline: Mainline) -> Self {
        Self {
            mainline,
            commit_events: Vec::new(),
        }
    }

    /// Returns the Mainline used for replay settlement.
    #[must_use]
    pub fn mainline(&self) -> &Mainline {
        &self.mainline
    }

    /// Returns the Commit Events in playback order.
    #[must_use]
    pub fn commit_events(&self) -> &[CommitEvent] {
        &self.commit_events
    }

    /// Appends a Commit Event to the Repository Replay timeline.
    pub fn push_commit_event(&mut self, commit_event: CommitEvent) {
        self.commit_events.push(commit_event);
    }
}

/// The branch treated as the primary history path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mainline(String);

impl Mainline {
    /// Creates a Mainline name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the Mainline name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A timeline unit in a Repository Replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitEvent {
    id: CommitId,
    contributor: Contributor,
    file_changes: Vec<FileChange>,
}

impl CommitEvent {
    /// Creates a Commit Event.
    #[must_use]
    pub fn new(id: CommitId, contributor: Contributor, file_changes: Vec<FileChange>) -> Self {
        Self {
            id,
            contributor,
            file_changes,
        }
    }

    /// Returns the Commit Event identifier.
    #[must_use]
    pub fn id(&self) -> &CommitId {
        &self.id
    }

    /// Returns the Contributor for this Commit Event.
    #[must_use]
    pub fn contributor(&self) -> &Contributor {
        &self.contributor
    }

    /// Returns the visible File Changes in this Commit Event.
    #[must_use]
    pub fn file_changes(&self) -> &[FileChange] {
        &self.file_changes
    }
}

/// A stable commit identifier as provided by Repository Ingestion.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommitId(String);

impl CommitId {
    /// Creates a Commit Event identifier.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A visible change to a repository entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    entity: RepositoryEntity,
    kind: FileChangeKind,
}

impl FileChange {
    /// Creates a File Change.
    #[must_use]
    pub fn new(entity: RepositoryEntity, kind: FileChangeKind) -> Self {
        Self { entity, kind }
    }

    /// Returns the Repository Entity affected by the change.
    #[must_use]
    pub fn entity(&self) -> &RepositoryEntity {
        &self.entity
    }

    /// Returns the change kind.
    #[must_use]
    pub fn kind(&self) -> &FileChangeKind {
        &self.kind
    }
}

/// The visible kind of a File Change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    /// A Repository Entity was added.
    Added,
    /// A Repository Entity was modified.
    Modified,
    /// A Repository Entity was deleted.
    Deleted,
    /// A Repository Entity was moved or renamed.
    Moved,
}

/// A visual participant in a Repository Replay.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepositoryEntity {
    path: PathBuf,
}

impl RepositoryEntity {
    /// Creates a Repository Entity from a repository-relative path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the repository-relative path for the entity.
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

/// A normalized person or service identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contributor {
    display_name: String,
    kind: ContributorKind,
}

impl Contributor {
    /// Creates a human Contributor.
    #[must_use]
    pub fn human(display_name: impl Into<String>) -> Self {
        Self {
            display_name: display_name.into(),
            kind: ContributorKind::Human,
        }
    }

    /// Creates an Automation Contributor.
    #[must_use]
    pub fn automation(display_name: impl Into<String>) -> Self {
        Self {
            display_name: display_name.into(),
            kind: ContributorKind::Automation,
        }
    }

    /// Returns the display name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the Contributor kind.
    #[must_use]
    pub fn kind(&self) -> ContributorKind {
        self.kind
    }
}

/// Classification for a Contributor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContributorKind {
    /// A person identity.
    Human,
    /// A bot, script, dependency service, or other non-human identity.
    Automation,
}

/// A reusable set of parameters for rendering a Repository Replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderConfiguration {
    visual_metaphor: VisualMetaphor,
    frame_size: FrameSize,
    frames_per_second: FramesPerSecond,
    theme: Theme,
    layout: Layout,
}

impl RenderConfiguration {
    /// Creates a Render Configuration.
    #[must_use]
    pub fn new(visual_metaphor: VisualMetaphor, theme: Theme, layout: Layout) -> Self {
        Self {
            visual_metaphor,
            frame_size: FrameSize::new(1920, 1080).expect("default frame size is valid"),
            frames_per_second: FramesPerSecond::new(60).expect("default FPS is valid"),
            theme,
            layout,
        }
    }

    /// Parses a TOML Render Configuration.
    pub fn from_toml_str(input: &str) -> Result<Self, RenderConfigurationError> {
        let value: Value = toml::from_str(input).map_err(|error| {
            RenderConfigurationError::single(
                "toml",
                format!("valid TOML Render Configuration ({error})"),
            )
        })?;
        let raw = RawRenderConfiguration::try_from_value(value)?;

        Self::try_from_raw(raw)
    }

    /// Returns the Visual Metaphor.
    #[must_use]
    pub fn visual_metaphor(&self) -> &VisualMetaphor {
        &self.visual_metaphor
    }

    /// Returns the output frame size.
    #[must_use]
    pub fn frame_size(&self) -> FrameSize {
        self.frame_size
    }

    /// Returns the Render frame rate.
    #[must_use]
    pub fn frames_per_second(&self) -> FramesPerSecond {
        self.frames_per_second
    }

    /// Returns the Theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Returns the Layout.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    fn try_from_raw(raw: RawRenderConfiguration) -> Result<Self, RenderConfigurationError> {
        let mut errors = RenderConfigurationError::new();

        let frame_size = match FrameSize::new(raw.frame_width, raw.frame_height) {
            Ok(frame_size) => Some(frame_size),
            Err(ConfigValueError) => {
                if raw.frame_width == 0 {
                    errors.push("frame_width", "positive integer");
                }
                if raw.frame_height == 0 {
                    errors.push("frame_height", "positive integer");
                }
                None
            }
        };

        let frames_per_second = match FramesPerSecond::new(raw.frames_per_second) {
            Ok(frames_per_second) => Some(frames_per_second),
            Err(ConfigValueError) => {
                errors.push("frames_per_second", "positive integer");
                None
            }
        };

        let theme = Theme::try_from_raw(raw.theme, &mut errors);
        let layout = Layout::try_from_raw(raw.layout, &mut errors);

        if errors.is_empty() {
            Ok(Self {
                visual_metaphor: VisualMetaphor::new("repository-replay"),
                frame_size: frame_size.expect("validated frame size"),
                frames_per_second: frames_per_second.expect("validated FPS"),
                theme: theme.expect("validated Theme"),
                layout: layout.expect("validated Layout"),
            })
        } else {
            Err(errors)
        }
    }
}

impl Default for RenderConfiguration {
    fn default() -> Self {
        Self {
            visual_metaphor: VisualMetaphor::new("repository-replay"),
            frame_size: FrameSize::new(1920, 1080).expect("default frame size is valid"),
            frames_per_second: FramesPerSecond::new(60).expect("default FPS is valid"),
            theme: Theme::default(),
            layout: Layout::RepositoryGraphWithParameters(RepositoryGraphLayout::default()),
        }
    }
}

/// The presentation model used to depict repository entities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualMetaphor(String);

impl VisualMetaphor {
    /// Creates a Visual Metaphor name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the Visual Metaphor name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A reusable presentation profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    name: String,
    background_color: HexColor,
    entity_color: HexColor,
    contributor_color: HexColor,
}

impl Theme {
    /// Creates a Theme name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Returns the Theme name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.name
    }

    /// Returns the Theme name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the background color.
    #[must_use]
    pub fn background_color(&self) -> &HexColor {
        &self.background_color
    }

    /// Returns the Repository Entity color.
    #[must_use]
    pub fn entity_color(&self) -> &HexColor {
        &self.entity_color
    }

    /// Returns the Contributor color.
    #[must_use]
    pub fn contributor_color(&self) -> &HexColor {
        &self.contributor_color
    }

    fn try_from_raw(raw: RawTheme, errors: &mut RenderConfigurationError) -> Option<Self> {
        let background_color =
            parse_hex_color("theme.background_color", &raw.background_color, errors);
        let entity_color = parse_hex_color("theme.entity_color", &raw.entity_color, errors);
        let contributor_color =
            parse_hex_color("theme.contributor_color", &raw.contributor_color, errors);

        Some(Self {
            name: raw.name,
            background_color: background_color?,
            entity_color: entity_color?,
            contributor_color: contributor_color?,
        })
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "gitflux-dark".to_owned(),
            background_color: HexColor::new("#0b1020").expect("default color is valid"),
            entity_color: HexColor::new("#7dd3fc").expect("default color is valid"),
            contributor_color: HexColor::new("#facc15").expect("default color is valid"),
        }
    }
}

/// A reusable spatial behavior model for arranging repository entities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    /// The Repository Graph layout.
    RepositoryGraph,
    /// The Repository Graph layout with explicit parameters.
    RepositoryGraphWithParameters(RepositoryGraphLayout),
    /// A named future Layout extension.
    Named(String),
}

impl Layout {
    /// Returns true when this is the Repository Graph layout.
    #[must_use]
    pub fn is_repository_graph(&self) -> bool {
        matches!(
            self,
            Self::RepositoryGraph | Self::RepositoryGraphWithParameters(_)
        )
    }

    /// Returns the Repository Entity spacing.
    #[must_use]
    pub fn entity_spacing(&self) -> EntitySpacing {
        match self {
            Self::RepositoryGraph => RepositoryGraphLayout::default().entity_spacing(),
            Self::RepositoryGraphWithParameters(layout) => layout.entity_spacing(),
            Self::Named(_) => EntitySpacing::new(1).expect("fallback spacing is valid"),
        }
    }

    /// Returns the number of Repository Graph settle iterations.
    #[must_use]
    pub fn settle_iterations(&self) -> SettleIterations {
        match self {
            Self::RepositoryGraph => RepositoryGraphLayout::default().settle_iterations(),
            Self::RepositoryGraphWithParameters(layout) => layout.settle_iterations(),
            Self::Named(_) => SettleIterations::new(1).expect("fallback settle count is valid"),
        }
    }

    fn try_from_raw(raw: RawLayout, errors: &mut RenderConfigurationError) -> Option<Self> {
        let kind_is_repository_graph = raw.kind == "repository_graph";
        if !kind_is_repository_graph {
            errors.push("layout.kind", r#"repository_graph"#);
        }

        let entity_spacing = match EntitySpacing::new(raw.entity_spacing) {
            Ok(entity_spacing) => Some(entity_spacing),
            Err(ConfigValueError) => {
                errors.push("layout.entity_spacing", "positive integer");
                None
            }
        };
        let settle_iterations = match SettleIterations::new(raw.settle_iterations) {
            Ok(settle_iterations) => Some(settle_iterations),
            Err(ConfigValueError) => {
                errors.push("layout.settle_iterations", "positive integer");
                None
            }
        };

        if kind_is_repository_graph {
            Some(Self::RepositoryGraphWithParameters(RepositoryGraphLayout {
                entity_spacing: entity_spacing?,
                settle_iterations: settle_iterations?,
            }))
        } else {
            None
        }
    }
}

/// Output frame dimensions for a Render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSize {
    width: u32,
    height: u32,
}

impl FrameSize {
    /// Creates positive frame dimensions.
    pub fn new(width: u32, height: u32) -> Result<Self, ConfigValueError> {
        if width == 0 || height == 0 {
            Err(ConfigValueError)
        } else {
            Ok(Self { width, height })
        }
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

/// Render frames per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramesPerSecond(u32);

impl FramesPerSecond {
    /// Creates a positive frame rate.
    pub fn new(value: u32) -> Result<Self, ConfigValueError> {
        if value == 0 {
            Err(ConfigValueError)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the frame rate.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

/// A Theme color stored in canonical hexadecimal notation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexColor(String);

impl HexColor {
    /// Creates a `#RRGGBB` color.
    pub fn new(value: impl Into<String>) -> Result<Self, ConfigValueError> {
        let value = value.into();
        let bytes = value.as_bytes();
        let is_hex =
            bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit);

        if is_hex {
            Ok(Self(value))
        } else {
            Err(ConfigValueError)
        }
    }

    /// Returns the canonical hexadecimal color.
    #[must_use]
    pub fn as_hex(&self) -> &str {
        &self.0
    }
}

/// Repository Graph layout parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryGraphLayout {
    entity_spacing: EntitySpacing,
    settle_iterations: SettleIterations,
}

impl RepositoryGraphLayout {
    /// Returns spacing between Repository Entities.
    #[must_use]
    pub fn entity_spacing(self) -> EntitySpacing {
        self.entity_spacing
    }

    /// Returns layout settle iterations.
    #[must_use]
    pub fn settle_iterations(self) -> SettleIterations {
        self.settle_iterations
    }
}

impl Default for RepositoryGraphLayout {
    fn default() -> Self {
        Self {
            entity_spacing: EntitySpacing::new(120).expect("default spacing is valid"),
            settle_iterations: SettleIterations::new(60).expect("default settle count is valid"),
        }
    }
}

/// Spacing between Repository Entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntitySpacing(u32);

impl EntitySpacing {
    /// Creates a positive entity spacing.
    pub fn new(value: u32) -> Result<Self, ConfigValueError> {
        if value == 0 {
            Err(ConfigValueError)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the spacing value.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

/// Number of Repository Graph settle iterations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettleIterations(u32);

impl SettleIterations {
    /// Creates a positive settle iteration count.
    pub fn new(value: u32) -> Result<Self, ConfigValueError> {
        if value == 0 {
            Err(ConfigValueError)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the settle iteration count.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

/// Diagnostics for invalid Render Configuration input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderConfigurationError {
    diagnostics: Vec<RenderConfigurationDiagnostic>,
}

impl RenderConfigurationError {
    fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    fn single(field: impl Into<String>, expected: impl Into<String>) -> Self {
        let mut error = Self::new();
        error.push(field, expected);
        error
    }

    fn push(&mut self, field: impl Into<String>, expected: impl Into<String>) {
        self.diagnostics.push(RenderConfigurationDiagnostic {
            field: field.into(),
            expected: expected.into(),
        });
    }

    fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

impl std::fmt::Display for RenderConfigurationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(formatter, "invalid Render Configuration")?;

        for diagnostic in &self.diagnostics {
            writeln!(
                formatter,
                "- {}: expected {}",
                diagnostic.field, diagnostic.expected
            )?;
        }

        Ok(())
    }
}

impl std::error::Error for RenderConfigurationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderConfigurationDiagnostic {
    field: String,
    expected: String,
}

struct RawRenderConfiguration {
    frame_width: u32,
    frame_height: u32,
    frames_per_second: u32,
    theme: RawTheme,
    layout: RawLayout,
}

struct RawTheme {
    name: String,
    background_color: String,
    entity_color: String,
    contributor_color: String,
}

struct RawLayout {
    kind: String,
    entity_spacing: u32,
    settle_iterations: u32,
}

impl RawRenderConfiguration {
    fn try_from_value(value: Value) -> Result<Self, RenderConfigurationError> {
        let mut errors = RenderConfigurationError::new();
        let Some(table) = value.as_table() else {
            errors.push(
                "render_configuration",
                "TOML table with frame_width, frame_height, frames_per_second, theme, layout",
            );
            return Err(errors);
        };

        report_unknown_fields(
            table,
            "",
            &[
                "frame_width",
                "frame_height",
                "frames_per_second",
                "theme",
                "layout",
            ],
            &mut errors,
        );

        let frame_width = u32_field(table, "frame_width", "frame_width", &mut errors);
        let frame_height = u32_field(table, "frame_height", "frame_height", &mut errors);
        let frames_per_second =
            u32_field(table, "frames_per_second", "frames_per_second", &mut errors);
        let theme = RawTheme::try_from_field(table.get("theme"), &mut errors);
        let layout = RawLayout::try_from_field(table.get("layout"), &mut errors);

        if errors.is_empty() {
            Ok(Self {
                frame_width: frame_width.expect("validated frame width"),
                frame_height: frame_height.expect("validated frame height"),
                frames_per_second: frames_per_second.expect("validated FPS"),
                theme: theme.expect("validated Theme section"),
                layout: layout.expect("validated Layout section"),
            })
        } else {
            Err(errors)
        }
    }
}

impl RawTheme {
    fn try_from_field(
        value: Option<&Value>,
        errors: &mut RenderConfigurationError,
    ) -> Option<Self> {
        let Some(value) = value else {
            errors.push("theme", "table with Theme fields");
            return None;
        };
        let Some(table) = value.as_table() else {
            errors.push("theme", "table with Theme fields");
            return None;
        };

        report_unknown_fields(
            table,
            "theme",
            &[
                "name",
                "background_color",
                "entity_color",
                "contributor_color",
            ],
            errors,
        );

        let name = string_field(table, "name", "theme.name", "string", errors);
        let background_color = string_field(
            table,
            "background_color",
            "theme.background_color",
            "#RRGGBB",
            errors,
        );
        let entity_color = string_field(
            table,
            "entity_color",
            "theme.entity_color",
            "#RRGGBB",
            errors,
        );
        let contributor_color = string_field(
            table,
            "contributor_color",
            "theme.contributor_color",
            "#RRGGBB",
            errors,
        );

        Some(Self {
            name: name?,
            background_color: background_color?,
            entity_color: entity_color?,
            contributor_color: contributor_color?,
        })
    }
}

impl RawLayout {
    fn try_from_field(
        value: Option<&Value>,
        errors: &mut RenderConfigurationError,
    ) -> Option<Self> {
        let Some(value) = value else {
            errors.push("layout", "table with Layout fields");
            return None;
        };
        let Some(table) = value.as_table() else {
            errors.push("layout", "table with Layout fields");
            return None;
        };

        report_unknown_fields(
            table,
            "layout",
            &["kind", "entity_spacing", "settle_iterations"],
            errors,
        );

        let kind = string_field(table, "kind", "layout.kind", r#"repository_graph"#, errors);
        let entity_spacing = u32_field(table, "entity_spacing", "layout.entity_spacing", errors);
        let settle_iterations = u32_field(
            table,
            "settle_iterations",
            "layout.settle_iterations",
            errors,
        );

        Some(Self {
            kind: kind?,
            entity_spacing: entity_spacing?,
            settle_iterations: settle_iterations?,
        })
    }
}

fn report_unknown_fields(
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    known_fields: &[&str],
    errors: &mut RenderConfigurationError,
) {
    for key in table.keys() {
        if !known_fields.contains(&key.as_str()) {
            let field = if prefix.is_empty() {
                key.to_owned()
            } else {
                format!("{prefix}.{key}")
            };
            errors.push(field, format!("known fields: {}", known_fields.join(", ")));
        }
    }
}

fn u32_field(
    table: &toml::map::Map<String, Value>,
    key: &str,
    field: &'static str,
    errors: &mut RenderConfigurationError,
) -> Option<u32> {
    match table.get(key) {
        Some(Value::Integer(value)) => u32::try_from(*value).ok(),
        Some(_) | None => None,
    }
    .or_else(|| {
        errors.push(field, "positive integer");
        None
    })
}

fn string_field(
    table: &toml::map::Map<String, Value>,
    key: &str,
    field: &'static str,
    expected: &'static str,
    errors: &mut RenderConfigurationError,
) -> Option<String> {
    match table.get(key) {
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) | None => {
            errors.push(field, expected);
            None
        }
    }
}

fn parse_hex_color(
    field: &'static str,
    value: &str,
    errors: &mut RenderConfigurationError,
) -> Option<HexColor> {
    match HexColor::new(value) {
        Ok(color) => Some(color),
        Err(ConfigValueError) => {
            errors.push(field, "#RRGGBB");
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigValueError;

#[cfg(test)]
mod tests {
    use super::{
        CommitEvent, CommitId, Contributor, FileChange, FileChangeKind, Mainline,
        RenderConfiguration, RepositoryEntity, RepositoryReplay,
    };

    #[test]
    fn repository_replay_keeps_commit_events_in_order() {
        let mut replay = RepositoryReplay::new(Mainline::new("main"));
        let contributor = Contributor::human("Ada");
        let entity = RepositoryEntity::new("src/lib.rs");

        replay.push_commit_event(CommitEvent::new(
            CommitId::new("abc123"),
            contributor,
            vec![FileChange::new(entity, FileChangeKind::Added)],
        ));

        assert_eq!(replay.mainline().as_str(), "main");
        assert_eq!(replay.commit_events()[0].id().as_str(), "abc123");
    }

    #[test]
    fn default_render_configuration_is_valid() {
        let config = RenderConfiguration::default();

        assert_eq!(config.frame_size().width(), 1920);
        assert_eq!(config.frame_size().height(), 1080);
        assert_eq!(config.frames_per_second().get(), 60);
        assert_eq!(config.theme().name(), "gitflux-dark");
        assert_eq!(config.theme().background_color().as_hex(), "#0b1020");
        assert!(config.layout().is_repository_graph());
    }

    #[test]
    fn parses_explicit_toml_render_configuration() {
        let config = RenderConfiguration::from_toml_str(
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
        )
        .expect("explicit TOML should parse");

        assert_eq!(config.frame_size().width(), 1280);
        assert_eq!(config.frame_size().height(), 720);
        assert_eq!(config.frames_per_second().get(), 30);
        assert_eq!(config.theme().name(), "terminal");
        assert_eq!(config.theme().entity_color().as_hex(), "#32d583");
        assert!(config.layout().is_repository_graph());
        assert_eq!(config.layout().entity_spacing().get(), 140);
        assert_eq!(config.layout().settle_iterations().get(), 80);
    }

    #[test]
    fn invalid_toml_reports_field_and_expected_shape() {
        let error = RenderConfiguration::from_toml_str(
            r##"
frame_width = 0
frame_height = 720
frames_per_second = 60

[theme]
name = "bad"
background_color = "blue"
entity_color = "#32d583"
contributor_color = "#fdb022"

[layout]
kind = "tree"
entity_spacing = 0
settle_iterations = 0
"##,
        )
        .expect_err("invalid TOML should report diagnostics");

        let message = error.to_string();

        assert!(message.contains("frame_width"));
        assert!(message.contains("positive integer"));
        assert!(message.contains("theme.background_color"));
        assert!(message.contains("#RRGGBB"));
        assert!(message.contains("layout.kind"));
        assert!(message.contains("repository_graph"));
        assert!(message.contains("layout.entity_spacing"));
        assert!(message.contains("layout.settle_iterations"));
    }

    #[test]
    fn missing_required_field_reports_field_and_expected_shape() {
        let error = RenderConfiguration::from_toml_str(
            r##"
frame_width = 1280
frame_height = 720
frames_per_second = 30

[theme]
name = "terminal"
background_color = "#101010"
entity_color = "#32d583"

[layout]
kind = "repository_graph"
entity_spacing = 140
settle_iterations = 80
"##,
        )
        .expect_err("missing required field should report field diagnostics");

        let message = error.to_string();

        assert!(message.contains("theme.contributor_color"));
        assert!(message.contains("#RRGGBB"));
        assert!(!message.contains("- toml:"));
    }

    #[test]
    fn wrong_type_reports_field_and_expected_shape() {
        let error = RenderConfiguration::from_toml_str(
            r##"
frame_width = "wide"
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
        )
        .expect_err("wrong field type should report field diagnostics");

        let message = error.to_string();

        assert!(message.contains("frame_width"));
        assert!(message.contains("positive integer"));
        assert!(!message.contains("- toml:"));
    }

    #[test]
    fn unknown_field_reports_field_and_known_fields() {
        let error = RenderConfiguration::from_toml_str(
            r##"
frame_width = 1280
frame_height = 720
frames_per_second = 30

[theme]
name = "terminal"
background_color = "#101010"
entity_color = "#32d583"
contributor_color = "#fdb022"
accent_color = "#ffffff"

[layout]
kind = "repository_graph"
entity_spacing = 140
settle_iterations = 80
"##,
        )
        .expect_err("unknown field should report field diagnostics");

        let message = error.to_string();

        assert!(message.contains("theme.accent_color"));
        assert!(message.contains("known fields"));
        assert!(message.contains("name"));
        assert!(message.contains("background_color"));
        assert!(message.contains("entity_color"));
        assert!(message.contains("contributor_color"));
        assert!(!message.contains("- toml:"));
    }

    #[test]
    fn malformed_toml_reports_parse_level_diagnostic() {
        let error = RenderConfiguration::from_toml_str(
            r##"
frame_width = 1280
frame_height =
"##,
        )
        .expect_err("malformed TOML should report parse diagnostics");

        let message = error.to_string();

        assert!(message.contains("toml"));
        assert!(message.contains("valid TOML Render Configuration"));
    }
}
