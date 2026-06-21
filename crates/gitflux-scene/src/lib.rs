//! Scene and layout data for Repository Replay rendering.
//!
//! This crate owns the deterministic core data shared by Repository Ingestion,
//! GPU rendering, and Video Export orchestration. It names the Repository
//! Replay timeline, Repository Graph layout, repository entities, contributors,
//! and Render Configuration without depending on Git, wgpu, or FFmpeg adapters.

use std::path::PathBuf;

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
    theme: Theme,
    layout: Layout,
}

impl RenderConfiguration {
    /// Creates a Render Configuration.
    #[must_use]
    pub fn new(visual_metaphor: VisualMetaphor, theme: Theme, layout: Layout) -> Self {
        Self {
            visual_metaphor,
            theme,
            layout,
        }
    }

    /// Returns the Visual Metaphor.
    #[must_use]
    pub fn visual_metaphor(&self) -> &VisualMetaphor {
        &self.visual_metaphor
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
pub struct Theme(String);

impl Theme {
    /// Creates a Theme name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the Theme name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A reusable spatial behavior model for arranging repository entities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    /// The Repository Graph layout.
    RepositoryGraph,
    /// A named future Layout extension.
    Named(String),
}

#[cfg(test)]
mod tests {
    use super::{
        CommitEvent, CommitId, Contributor, FileChange, FileChangeKind, Mainline, RepositoryEntity,
        RepositoryReplay,
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
}
