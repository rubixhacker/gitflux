//! Scene and layout data for Repository Replay rendering.
//!
//! This crate owns the deterministic core data shared by Repository Ingestion,
//! GPU rendering, and Video Export orchestration. It names the Repository
//! Replay timeline, Repository Graph layout, repository entities, contributors,
//! and Render Configuration without depending on Git, wgpu, or FFmpeg adapters.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use toml::Value;

/// A deterministic playback model for a repository's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryReplay {
    mainline: Mainline,
    commit_events: Vec<CommitEvent>,
    competing_changes: Vec<CompetingChange>,
}

impl RepositoryReplay {
    /// Creates a Repository Replay for the given Mainline.
    #[must_use]
    pub fn new(mainline: Mainline) -> Self {
        Self {
            mainline,
            commit_events: Vec::new(),
            competing_changes: Vec::new(),
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

    /// Returns provisional Competing Changes detected before Merge Settlement.
    #[must_use]
    pub fn competing_changes(&self) -> &[CompetingChange] {
        &self.competing_changes
    }

    /// Appends a Commit Event to the Repository Replay timeline.
    pub fn push_commit_event(&mut self, commit_event: CommitEvent) {
        self.commit_events.push(commit_event);
    }

    /// Appends a Competing Change to the Repository Replay.
    pub fn push_competing_change(&mut self, competing_change: CompetingChange) {
        self.competing_changes.push(competing_change);
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
    parent_ids: Vec<CommitId>,
    branch_flow: BranchFlow,
    subject: CommitSubject,
    authored_at: GitTimestamp,
    committed_at: GitTimestamp,
    author: ContributorEvidence,
    committer: ContributorEvidence,
    contributor: Contributor,
    file_changes: Vec<FileChange>,
}

/// Semantic evidence used to build a Commit Event from Repository Ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitEvidence {
    id: CommitId,
    parent_ids: Vec<CommitId>,
    branch_flow: BranchFlow,
    subject: CommitSubject,
    authored_at: GitTimestamp,
    committed_at: GitTimestamp,
    author: ContributorEvidence,
    committer: ContributorEvidence,
    contributor: Contributor,
    file_changes: Vec<FileChange>,
}

impl CommitEvidence {
    /// Creates semantic Commit Event evidence.
    #[must_use]
    pub fn new(
        id: CommitId,
        subject: CommitSubject,
        author: ContributorEvidence,
        committer: ContributorEvidence,
        contributor: Contributor,
    ) -> Self {
        let authored_at = author.timestamp();
        let committed_at = committer.timestamp();
        Self {
            id,
            parent_ids: Vec::new(),
            branch_flow: BranchFlow::Mainline,
            subject,
            authored_at,
            committed_at,
            author,
            committer,
            contributor,
            file_changes: Vec::new(),
        }
    }

    /// Sets parent Commit Event identifiers.
    #[must_use]
    pub fn with_parent_ids(mut self, parent_ids: Vec<CommitId>) -> Self {
        self.parent_ids = parent_ids;
        self
    }

    /// Sets the branch-flow state for this Commit Event.
    #[must_use]
    pub fn with_branch_flow(mut self, branch_flow: BranchFlow) -> Self {
        self.branch_flow = branch_flow;
        self
    }

    /// Sets visible File Changes.
    #[must_use]
    pub fn with_file_changes(mut self, file_changes: Vec<FileChange>) -> Self {
        self.file_changes = file_changes;
        self
    }
}

impl CommitEvent {
    /// Creates a Commit Event.
    #[must_use]
    pub fn new(id: CommitId, contributor: Contributor, file_changes: Vec<FileChange>) -> Self {
        Self {
            id,
            parent_ids: Vec::new(),
            branch_flow: BranchFlow::Mainline,
            subject: CommitSubject::new(""),
            authored_at: GitTimestamp::new(0, 0),
            committed_at: GitTimestamp::new(0, 0),
            author: ContributorEvidence::new(
                contributor.display_name(),
                "",
                GitTimestamp::new(0, 0),
            ),
            committer: ContributorEvidence::new(
                contributor.display_name(),
                "",
                GitTimestamp::new(0, 0),
            ),
            contributor,
            file_changes,
        }
    }

    /// Creates a Commit Event from Repository Ingestion evidence.
    #[must_use]
    pub fn from_evidence(evidence: CommitEvidence) -> Self {
        Self {
            id: evidence.id,
            parent_ids: evidence.parent_ids,
            branch_flow: evidence.branch_flow,
            subject: evidence.subject,
            authored_at: evidence.authored_at,
            committed_at: evidence.committed_at,
            author: evidence.author,
            committer: evidence.committer,
            contributor: evidence.contributor,
            file_changes: evidence.file_changes,
        }
    }

    /// Returns the Commit Event identifier.
    #[must_use]
    pub fn id(&self) -> &CommitId {
        &self.id
    }

    /// Returns the parent Commit Event identifiers.
    #[must_use]
    pub fn parent_ids(&self) -> &[CommitId] {
        &self.parent_ids
    }

    /// Returns the branch-flow state for this Commit Event.
    #[must_use]
    pub fn branch_flow(&self) -> &BranchFlow {
        &self.branch_flow
    }

    /// Returns the commit subject.
    #[must_use]
    pub fn subject(&self) -> &CommitSubject {
        &self.subject
    }

    /// Returns the author timestamp.
    #[must_use]
    pub fn authored_at(&self) -> GitTimestamp {
        self.authored_at
    }

    /// Returns the committer timestamp.
    #[must_use]
    pub fn committed_at(&self) -> GitTimestamp {
        self.committed_at
    }

    /// Returns raw author evidence.
    #[must_use]
    pub fn author(&self) -> &ContributorEvidence {
        &self.author
    }

    /// Returns raw committer evidence.
    #[must_use]
    pub fn committer(&self) -> &ContributorEvidence {
        &self.committer
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

/// Branch-flow classification for a Commit Event in Repository Replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchFlow {
    /// The Commit Event is on the selected Mainline.
    Mainline,
    /// The Commit Event is provisional work from a branch relative to the Mainline.
    BranchSuperposition {
        /// Local branch name carrying the provisional work.
        branch: String,
        /// Mainline the branch is provisional against.
        mainline: Mainline,
    },
    /// One or more branch settlements attached to a merge Commit Event.
    MergeSettlements(Vec<MergeSettlement>),
}

/// Branch work settled into the Mainline by a merge Commit Event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeSettlement {
    branch: String,
    mainline: Mainline,
    settled_commit_ids: Vec<CommitId>,
}

impl MergeSettlement {
    /// Creates Merge Settlement evidence.
    #[must_use]
    pub fn new(
        branch: impl Into<String>,
        mainline: Mainline,
        settled_commit_ids: Vec<CommitId>,
    ) -> Self {
        Self {
            branch: branch.into(),
            mainline,
            settled_commit_ids,
        }
    }

    /// Returns the local branch whose provisional work is settled.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Returns the Mainline receiving the settled work.
    #[must_use]
    pub fn mainline(&self) -> &Mainline {
        &self.mainline
    }

    /// Returns the Commit Events settled by this merge.
    #[must_use]
    pub fn settled_commit_ids(&self) -> &[CommitId] {
        &self.settled_commit_ids
    }
}

/// Provisional overlap between branch changes before Merge Settlement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompetingChange {
    entity: RepositoryEntity,
    source: CompetingChangeSource,
    confidence: CompetingChangeConfidence,
    evidence: Vec<CompetingChangeEvidence>,
}

impl CompetingChange {
    /// Creates a Competing Change with typed source and confidence.
    #[must_use]
    pub fn new(
        entity: RepositoryEntity,
        source: CompetingChangeSource,
        confidence: CompetingChangeConfidence,
        evidence: Vec<CompetingChangeEvidence>,
    ) -> Self {
        Self {
            entity,
            source,
            confidence,
            evidence,
        }
    }

    /// Returns the Repository Entity with overlapping provisional changes.
    #[must_use]
    pub fn entity(&self) -> &RepositoryEntity {
        &self.entity
    }

    /// Returns the detection source for this Competing Change.
    #[must_use]
    pub fn source(&self) -> CompetingChangeSource {
        self.source
    }

    /// Returns the confidence level for this Competing Change.
    #[must_use]
    pub fn confidence(&self) -> CompetingChangeConfidence {
        self.confidence
    }

    /// Returns the branch and commit evidence behind this Competing Change.
    #[must_use]
    pub fn evidence(&self) -> &[CompetingChangeEvidence] {
        &self.evidence
    }
}

/// Evidence source used to detect a Competing Change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompetingChangeSource {
    /// Repository-relative file path overlap across Branch Superpositions.
    FileLevelOverlap,
}

/// Confidence assigned to a Competing Change source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompetingChangeConfidence {
    /// Medium confidence: overlapping files, without line-range or symbol proof.
    Medium,
}

/// Branch Superposition evidence for a Competing Change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompetingChangeEvidence {
    branch: String,
    commit_id: CommitId,
}

impl CompetingChangeEvidence {
    /// Creates branch and Commit Event evidence for a Competing Change.
    #[must_use]
    pub fn new(branch: impl Into<String>, commit_id: CommitId) -> Self {
        Self {
            branch: branch.into(),
            commit_id,
        }
    }

    /// Returns the branch carrying provisional work.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Returns the Commit Event identifier for this evidence.
    #[must_use]
    pub fn commit_id(&self) -> &CommitId {
        &self.commit_id
    }
}

/// A commit subject as recorded by Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSubject(String);

impl CommitSubject {
    /// Creates a commit subject.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the subject text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
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
    previous_entity: Option<RepositoryEntity>,
    kind: FileChangeKind,
}

impl FileChange {
    /// Creates a File Change.
    #[must_use]
    pub fn new(entity: RepositoryEntity, kind: FileChangeKind) -> Self {
        Self {
            entity,
            previous_entity: None,
            kind,
        }
    }

    /// Creates a moved or renamed File Change with source and destination evidence.
    #[must_use]
    pub fn moved(from: RepositoryEntity, to: RepositoryEntity) -> Self {
        Self {
            entity: to,
            previous_entity: Some(from),
            kind: FileChangeKind::Moved,
        }
    }

    /// Returns the Repository Entity affected by the change.
    #[must_use]
    pub fn entity(&self) -> &RepositoryEntity {
        &self.entity
    }

    /// Returns the previous Repository Entity for moves or renames.
    #[must_use]
    pub fn previous_entity(&self) -> Option<&RepositoryEntity> {
        self.previous_entity.as_ref()
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
    identity_key: String,
    kind: ContributorKind,
}

impl Contributor {
    /// Creates a human Contributor.
    #[must_use]
    pub fn human(display_name: impl Into<String>) -> Self {
        let display_name = display_name.into();
        Self {
            identity_key: display_name.clone(),
            display_name,
            kind: ContributorKind::Human,
        }
    }

    /// Creates a human Contributor with a stable normalized identity key.
    #[must_use]
    pub fn normalized_human(
        display_name: impl Into<String>,
        identity_key: impl Into<String>,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            identity_key: identity_key.into(),
            kind: ContributorKind::Human,
        }
    }

    /// Creates an Automation Contributor.
    #[must_use]
    pub fn automation(display_name: impl Into<String>) -> Self {
        let display_name = display_name.into();
        Self {
            identity_key: display_name.clone(),
            display_name,
            kind: ContributorKind::Automation,
        }
    }

    /// Creates an Automation Contributor with a stable normalized identity key.
    #[must_use]
    pub fn normalized_automation(
        display_name: impl Into<String>,
        identity_key: impl Into<String>,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            identity_key: identity_key.into(),
            kind: ContributorKind::Automation,
        }
    }

    /// Returns the display name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the stable normalized identity key.
    #[must_use]
    pub fn identity_key(&self) -> &str {
        &self.identity_key
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

/// Raw Contributor evidence recorded on a Git commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributorEvidence {
    name: String,
    email: String,
    timestamp: GitTimestamp,
}

impl ContributorEvidence {
    /// Creates raw Contributor evidence from Git signature data.
    #[must_use]
    pub fn new(name: impl Into<String>, email: impl Into<String>, timestamp: GitTimestamp) -> Self {
        Self {
            name: name.into(),
            email: email.into(),
            timestamp,
        }
    }

    /// Returns the raw Git signature name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the raw Git signature email.
    #[must_use]
    pub fn email(&self) -> &str {
        &self.email
    }

    /// Returns the Git signature timestamp.
    #[must_use]
    pub fn timestamp(&self) -> GitTimestamp {
        self.timestamp
    }
}

/// A Git timestamp with UTC seconds and timezone offset evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitTimestamp {
    seconds: i64,
    offset_minutes: i32,
}

impl GitTimestamp {
    /// Creates a Git timestamp.
    #[must_use]
    pub fn new(seconds: i64, offset_minutes: i32) -> Self {
        Self {
            seconds,
            offset_minutes,
        }
    }

    /// Returns seconds since the Unix epoch.
    #[must_use]
    pub fn seconds(self) -> i64 {
        self.seconds
    }

    /// Returns the timezone offset in minutes.
    #[must_use]
    pub fn offset_minutes(self) -> i32 {
        self.offset_minutes
    }
}

/// Render-ready scene data for the Repository Graph layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryGraphScene {
    mainline: String,
    frame_size: SceneFrameSize,
    frames_per_second: u32,
    explicit_path_filter: Option<SceneExplicitPathFilter>,
    contributors: Vec<SceneContributor>,
    directories: Vec<SceneDirectory>,
    files: Vec<SceneFile>,
    visual_summaries: Vec<VisualSummary>,
    activities: Vec<SceneActivity>,
    competing_changes: Vec<SceneCompetingChange>,
}

impl RepositoryGraphScene {
    /// Builds a deterministic Repository Graph scene from Repository Replay events.
    #[must_use]
    pub fn from_replay(replay: &RepositoryReplay, configuration: &RenderConfiguration) -> Self {
        let spacing = i32::try_from(configuration.layout().entity_spacing().get())
            .expect("entity spacing fits scene coordinates");
        let mut contributor_by_id = BTreeMap::<ContributorSceneId, SceneContributorSeed>::new();
        let mut directory_paths = BTreeSet::<String>::new();
        let mut file_paths = BTreeSet::<String>::new();
        let path_filter = configuration.explicit_path_filter();

        for commit_event in replay.commit_events() {
            let visible_file_changes: Vec<&FileChange> = commit_event
                .file_changes()
                .iter()
                .filter(|file_change| path_filter.includes_file_change(file_change))
                .collect();
            if visible_file_changes.is_empty() {
                continue;
            }

            let contributor = commit_event.contributor();
            contributor_by_id
                .entry(ContributorSceneId(contributor.identity_key().to_owned()))
                .or_insert_with(|| SceneContributorSeed {
                    display_name: contributor.display_name().to_owned(),
                    kind: contributor.kind(),
                });

            for file_change in visible_file_changes {
                collect_repository_entity_paths(
                    file_change.entity(),
                    &mut directory_paths,
                    &mut file_paths,
                );
                if let Some(previous_entity) = file_change.previous_entity() {
                    collect_repository_entity_paths(
                        previous_entity,
                        &mut directory_paths,
                        &mut file_paths,
                    );
                }
            }
        }
        for competing_change in replay.competing_changes() {
            if !path_filter.includes_entity(competing_change.entity()) {
                continue;
            }
            collect_repository_entity_paths(
                competing_change.entity(),
                &mut directory_paths,
                &mut file_paths,
            );
        }

        let contributors = contributor_by_id
            .into_iter()
            .enumerate()
            .map(|(index, (id, seed))| SceneContributor {
                id,
                display_name: seed.display_name,
                kind: seed.kind,
                position: ScenePosition::new(index, 0, spacing),
            })
            .collect();

        let directories = directory_paths
            .into_iter()
            .enumerate()
            .map(|(index, path)| SceneDirectory {
                id: DirectorySceneId(path.clone()),
                path,
                position: ScenePosition::new(index, 1, spacing),
            })
            .collect();

        let files = file_paths
            .clone()
            .into_iter()
            .enumerate()
            .map(|(index, path)| SceneFile {
                id: FileSceneId(path.clone()),
                parent_directory_id: parent_directory_id(Path::new(&path)),
                emphasis: SceneEmphasis::from_path(Path::new(&path)),
                path,
                position: ScenePosition::new(index, 2, spacing),
                motion: MotionState::Settled,
            })
            .collect();
        let visual_summaries = build_visual_summaries(
            &file_paths,
            replay,
            path_filter,
            configuration.level_of_detail(),
            spacing,
        );

        let activities = replay
            .commit_events()
            .iter()
            .filter_map(|commit_event| {
                let file_changes: Vec<SceneFileChange> = commit_event
                    .file_changes()
                    .iter()
                    .filter(|file_change| path_filter.includes_file_change(file_change))
                    .map(|file_change| {
                        SceneFileChange::from_file_change(
                            file_change,
                            MotionState::from(commit_event.branch_flow()),
                        )
                    })
                    .collect();
                (!file_changes.is_empty()).then_some((commit_event, file_changes))
            })
            .enumerate()
            .map(|(index, (commit_event, file_changes))| {
                let playback_frame = u64::from(configuration.frames_per_second().get())
                    * u64::try_from(index).expect("commit index fits playback frame");
                SceneActivity {
                    commit_id: CommitSceneId(commit_event.id().as_str().to_owned()),
                    playback_frame,
                    contributor_id: ContributorSceneId(
                        commit_event.contributor().identity_key().to_owned(),
                    ),
                    branch_activity: SceneBranchActivity::from(commit_event.branch_flow()),
                    file_changes,
                }
            })
            .collect();
        let competing_changes = replay
            .competing_changes()
            .iter()
            .filter(|competing_change| path_filter.includes_entity(competing_change.entity()))
            .map(SceneCompetingChange::from)
            .collect();

        Self {
            mainline: replay.mainline().as_str().to_owned(),
            frame_size: SceneFrameSize {
                width: configuration.frame_size().width(),
                height: configuration.frame_size().height(),
            },
            frames_per_second: configuration.frames_per_second().get(),
            explicit_path_filter: path_filter.as_scene_filter(),
            contributors,
            directories,
            files,
            visual_summaries,
            activities,
            competing_changes,
        }
    }

    /// Returns the Mainline for this Repository Graph scene.
    #[must_use]
    pub fn mainline(&self) -> &str {
        &self.mainline
    }

    /// Returns the configured scene frame size.
    #[must_use]
    pub fn frame_size(&self) -> SceneFrameSize {
        self.frame_size
    }

    /// Returns the configured render frame rate.
    #[must_use]
    pub fn frames_per_second(&self) -> u32 {
        self.frames_per_second
    }

    /// Returns scene Contributors in deterministic layout order.
    #[must_use]
    pub fn contributors(&self) -> &[SceneContributor] {
        &self.contributors
    }

    /// Returns an explicit path filter applied before scene Level of Detail.
    #[must_use]
    pub fn explicit_path_filter(&self) -> Option<&SceneExplicitPathFilter> {
        self.explicit_path_filter.as_ref()
    }

    /// Returns scene directories in deterministic layout order.
    #[must_use]
    pub fn directories(&self) -> &[SceneDirectory] {
        &self.directories
    }

    /// Returns scene files in deterministic layout order.
    #[must_use]
    pub fn files(&self) -> &[SceneFile] {
        &self.files
    }

    /// Returns Visual Summaries produced by Level of Detail policy.
    #[must_use]
    pub fn visual_summaries(&self) -> &[VisualSummary] {
        &self.visual_summaries
    }

    /// Returns timed Commit Event activity in replay order.
    #[must_use]
    pub fn activities(&self) -> &[SceneActivity] {
        &self.activities
    }

    /// Returns provisional Competing Changes preserved for scene rendering.
    #[must_use]
    pub fn competing_changes(&self) -> &[SceneCompetingChange] {
        &self.competing_changes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SceneContributorSeed {
    display_name: String,
    kind: ContributorKind,
}

/// Explicit path scope applied before Level of Detail summarization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneExplicitPathFilter {
    included_paths: Vec<String>,
}

impl SceneExplicitPathFilter {
    /// Returns repository-relative path prefixes included in this scene.
    #[must_use]
    pub fn included_paths(&self) -> &[String] {
        &self.included_paths
    }
}

/// A Repository Entity that stands in for multiple underlying entities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualSummary {
    id: VisualSummarySceneId,
    path: String,
    represented_file_ids: Vec<FileSceneId>,
    represented_entity_count: usize,
    activity_count: usize,
    weight: VisualSummaryWeight,
    position: ScenePosition,
    emphasis: SceneEmphasis,
}

impl VisualSummary {
    /// Returns the stable Visual Summary scene identifier.
    #[must_use]
    pub fn id(&self) -> &VisualSummarySceneId {
        &self.id
    }

    /// Returns the repository-relative directory path represented by this summary.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns file scene identifiers represented by this Visual Summary.
    #[must_use]
    pub fn represented_file_ids(&self) -> &[FileSceneId] {
        &self.represented_file_ids
    }

    /// Returns how many Repository Entities are represented.
    #[must_use]
    pub fn represented_entity_count(&self) -> usize {
        self.represented_entity_count
    }

    /// Returns how many File Changes contributed to this Visual Summary.
    #[must_use]
    pub fn activity_count(&self) -> usize {
        self.activity_count
    }

    /// Returns the visual weight preserved from represented activity.
    #[must_use]
    pub fn weight(&self) -> VisualSummaryWeight {
        self.weight
    }

    /// Returns the deterministic scene position.
    #[must_use]
    pub fn position(&self) -> ScenePosition {
        self.position
    }

    /// Returns whether this Visual Summary should be visually de-emphasized.
    #[must_use]
    pub fn emphasis(&self) -> SceneEmphasis {
        self.emphasis
    }
}

/// A stable scene identifier for a Visual Summary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VisualSummarySceneId(String);

impl VisualSummarySceneId {
    /// Returns the stable Visual Summary scene identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Visual weight for a Visual Summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualSummaryWeight(usize);

impl VisualSummaryWeight {
    /// Returns the preserved activity weight.
    #[must_use]
    pub fn get(self) -> usize {
        self.0
    }
}

/// Output frame dimensions copied into render-ready scene data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneFrameSize {
    width: u32,
    height: u32,
}

impl SceneFrameSize {
    /// Returns the scene frame width in pixels.
    #[must_use]
    pub fn width(self) -> u32 {
        self.width
    }

    /// Returns the scene frame height in pixels.
    #[must_use]
    pub fn height(self) -> u32 {
        self.height
    }
}

/// A stable scene identifier for a Contributor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContributorSceneId(String);

impl ContributorSceneId {
    /// Returns the stable Contributor scene identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stable scene identifier for a directory Repository Entity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DirectorySceneId(String);

impl DirectorySceneId {
    /// Returns the stable directory scene identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stable scene identifier for a file Repository Entity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileSceneId(String);

impl FileSceneId {
    /// Returns the stable file scene identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stable scene identifier for a Commit Event.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CommitSceneId(String);

impl CommitSceneId {
    /// Returns the stable Commit Event scene identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Deterministic scene-space position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenePosition {
    x: i32,
    y: i32,
}

impl ScenePosition {
    fn new(index: usize, row: i32, spacing: i32) -> Self {
        Self {
            x: i32::try_from(index).expect("scene index fits coordinates") * spacing,
            y: row * spacing,
        }
    }

    /// Returns the deterministic x coordinate.
    #[must_use]
    pub fn x(self) -> i32 {
        self.x
    }

    /// Returns the deterministic y coordinate.
    #[must_use]
    pub fn y(self) -> i32 {
        self.y
    }
}

/// Render-ready Contributor node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneContributor {
    id: ContributorSceneId,
    display_name: String,
    kind: ContributorKind,
    position: ScenePosition,
}

impl SceneContributor {
    /// Returns the Contributor scene identifier.
    #[must_use]
    pub fn id(&self) -> &ContributorSceneId {
        &self.id
    }

    /// Returns the display name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns whether this Contributor is human or automation.
    #[must_use]
    pub fn kind(&self) -> ContributorKind {
        self.kind
    }

    /// Returns the deterministic scene position.
    #[must_use]
    pub fn position(&self) -> ScenePosition {
        self.position
    }
}

/// Render-ready directory node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneDirectory {
    id: DirectorySceneId,
    path: String,
    position: ScenePosition,
}

impl SceneDirectory {
    /// Returns the directory scene identifier.
    #[must_use]
    pub fn id(&self) -> &DirectorySceneId {
        &self.id
    }

    /// Returns the repository-relative directory path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the deterministic scene position.
    #[must_use]
    pub fn position(&self) -> ScenePosition {
        self.position
    }
}

/// Render-ready file node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneFile {
    id: FileSceneId,
    path: String,
    parent_directory_id: Option<DirectorySceneId>,
    position: ScenePosition,
    motion: MotionState,
    emphasis: SceneEmphasis,
}

impl SceneFile {
    /// Returns the file scene identifier.
    #[must_use]
    pub fn id(&self) -> &FileSceneId {
        &self.id
    }

    /// Returns the repository-relative file path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the containing directory scene identifier, if any.
    #[must_use]
    pub fn parent_directory_id(&self) -> Option<&DirectorySceneId> {
        self.parent_directory_id.as_ref()
    }

    /// Returns the deterministic scene position.
    #[must_use]
    pub fn position(&self) -> ScenePosition {
        self.position
    }

    /// Returns the basic motion state.
    #[must_use]
    pub fn motion(&self) -> MotionState {
        self.motion
    }

    /// Returns whether this file should be visually de-emphasized.
    #[must_use]
    pub fn emphasis(&self) -> SceneEmphasis {
        self.emphasis
    }
}

/// Scene-level visual emphasis for Repository Entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneEmphasis {
    /// Normal visual weight.
    Normal,
    /// Lower visual weight while preserving the Repository Entity in scene data.
    DeEmphasized,
}

impl SceneEmphasis {
    fn from_path(path: &Path) -> Self {
        let path_text = path.to_string_lossy();
        let lockfile = matches!(path_text.as_ref(), "Cargo.lock" | "package-lock.json");
        let generated_directory = path.components().any(|component| {
            matches!(
                component.as_os_str().to_string_lossy().as_ref(),
                "generated" | "target" | "vendor" | "node_modules"
            )
        });

        if lockfile || generated_directory {
            Self::DeEmphasized
        } else {
            Self::Normal
        }
    }
}

/// Basic scene motion state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionState {
    /// Repository Entity has settled into the main Repository Graph.
    Settled,
    /// Repository Entity is provisional branch work before Merge Settlement.
    Provisional,
    /// Repository Entity is settling from branch work into the Mainline.
    Settling,
}

impl From<&BranchFlow> for MotionState {
    fn from(branch_flow: &BranchFlow) -> Self {
        match branch_flow {
            BranchFlow::Mainline => Self::Settled,
            BranchFlow::BranchSuperposition { .. } => Self::Provisional,
            BranchFlow::MergeSettlements(_) => Self::Settling,
        }
    }
}

/// Timed activity derived from one Commit Event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneActivity {
    commit_id: CommitSceneId,
    playback_frame: u64,
    contributor_id: ContributorSceneId,
    branch_activity: SceneBranchActivity,
    file_changes: Vec<SceneFileChange>,
}

impl SceneActivity {
    /// Returns the Commit Event scene identifier.
    #[must_use]
    pub fn commit_id(&self) -> &CommitSceneId {
        &self.commit_id
    }

    /// Returns the playback frame where this activity begins.
    #[must_use]
    pub fn playback_frame(&self) -> u64 {
        self.playback_frame
    }

    /// Returns the Contributor responsible for the activity.
    #[must_use]
    pub fn contributor_id(&self) -> &ContributorSceneId {
        &self.contributor_id
    }

    /// Returns the branch-flow scene activity.
    #[must_use]
    pub fn branch_activity(&self) -> &SceneBranchActivity {
        &self.branch_activity
    }

    /// Returns File Changes for this activity.
    #[must_use]
    pub fn file_changes(&self) -> &[SceneFileChange] {
        &self.file_changes
    }
}

/// Branch-flow state preserved for scene activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneBranchActivity {
    /// Activity belongs to the Mainline.
    Mainline,
    /// Activity is provisional branch work relative to the Mainline.
    BranchSuperposition { branch: String, mainline: String },
    /// Activity settles provisional branch work into the Mainline.
    MergeSettlements(Vec<SceneMergeSettlement>),
}

/// Render-ready Merge Settlement activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMergeSettlement {
    branch: String,
    mainline: String,
    settled_commit_ids: Vec<CommitSceneId>,
}

impl SceneMergeSettlement {
    /// Returns the branch being settled.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Returns the Mainline receiving the settled branch work.
    #[must_use]
    pub fn mainline(&self) -> &str {
        &self.mainline
    }

    /// Returns settled Commit Event identifiers.
    #[must_use]
    pub fn settled_commit_ids(&self) -> &[CommitSceneId] {
        &self.settled_commit_ids
    }
}

impl From<&BranchFlow> for SceneBranchActivity {
    fn from(branch_flow: &BranchFlow) -> Self {
        match branch_flow {
            BranchFlow::Mainline => Self::Mainline,
            BranchFlow::BranchSuperposition { branch, mainline } => Self::BranchSuperposition {
                branch: branch.clone(),
                mainline: mainline.as_str().to_owned(),
            },
            BranchFlow::MergeSettlements(settlements) => Self::MergeSettlements(
                settlements
                    .iter()
                    .map(|settlement| SceneMergeSettlement {
                        branch: settlement.branch().to_owned(),
                        mainline: settlement.mainline().as_str().to_owned(),
                        settled_commit_ids: settlement
                            .settled_commit_ids()
                            .iter()
                            .map(|commit_id| CommitSceneId(commit_id.as_str().to_owned()))
                            .collect(),
                    })
                    .collect(),
            ),
        }
    }
}

/// Render-ready File Change activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneFileChange {
    file_id: FileSceneId,
    previous_file_id: Option<FileSceneId>,
    kind: FileChangeKind,
    motion: MotionState,
}

impl SceneFileChange {
    /// Returns the file affected by this File Change.
    #[must_use]
    pub fn file_id(&self) -> &FileSceneId {
        &self.file_id
    }

    /// Returns the previous file identifier for moved File Changes.
    #[must_use]
    pub fn previous_file_id(&self) -> Option<&FileSceneId> {
        self.previous_file_id.as_ref()
    }

    /// Returns the File Change kind.
    #[must_use]
    pub fn kind(&self) -> FileChangeKind {
        self.kind
    }

    /// Returns the motion state for this File Change.
    #[must_use]
    pub fn motion(&self) -> MotionState {
        self.motion
    }
}

impl From<&FileChange> for SceneFileChange {
    fn from(file_change: &FileChange) -> Self {
        Self::from_file_change(file_change, MotionState::Settled)
    }
}

impl SceneFileChange {
    fn from_file_change(file_change: &FileChange, motion: MotionState) -> Self {
        Self {
            file_id: FileSceneId(repository_entity_path(file_change.entity())),
            previous_file_id: file_change
                .previous_entity()
                .map(|entity| FileSceneId(repository_entity_path(entity))),
            kind: *file_change.kind(),
            motion,
        }
    }
}

/// Render-ready Competing Change activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneCompetingChange {
    file_id: FileSceneId,
    source: CompetingChangeSource,
    confidence: CompetingChangeConfidence,
    evidence: Vec<SceneCompetingChangeEvidence>,
}

impl SceneCompetingChange {
    /// Returns the file with overlapping provisional changes.
    #[must_use]
    pub fn file_id(&self) -> &FileSceneId {
        &self.file_id
    }

    /// Returns how the Competing Change was detected.
    #[must_use]
    pub fn source(&self) -> CompetingChangeSource {
        self.source
    }

    /// Returns the confidence level for this Competing Change.
    #[must_use]
    pub fn confidence(&self) -> CompetingChangeConfidence {
        self.confidence
    }

    /// Returns branch evidence for this Competing Change.
    #[must_use]
    pub fn evidence(&self) -> &[SceneCompetingChangeEvidence] {
        &self.evidence
    }
}

impl From<&CompetingChange> for SceneCompetingChange {
    fn from(competing_change: &CompetingChange) -> Self {
        Self {
            file_id: FileSceneId(repository_entity_path(competing_change.entity())),
            source: competing_change.source(),
            confidence: competing_change.confidence(),
            evidence: competing_change
                .evidence()
                .iter()
                .map(|evidence| SceneCompetingChangeEvidence {
                    branch: evidence.branch().to_owned(),
                    commit_id: CommitSceneId(evidence.commit_id().as_str().to_owned()),
                })
                .collect(),
        }
    }
}

/// Branch evidence for a render-ready Competing Change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneCompetingChangeEvidence {
    branch: String,
    commit_id: CommitSceneId,
}

impl SceneCompetingChangeEvidence {
    /// Returns the branch carrying provisional work.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Returns the Commit Event carrying provisional work.
    #[must_use]
    pub fn commit_id(&self) -> &CommitSceneId {
        &self.commit_id
    }
}

fn collect_repository_entity_paths(
    entity: &RepositoryEntity,
    directory_paths: &mut BTreeSet<String>,
    file_paths: &mut BTreeSet<String>,
) {
    let path = repository_entity_path(entity);
    file_paths.insert(path.clone());

    let mut current = Path::new(&path).parent();
    while let Some(parent) = current {
        if parent.as_os_str().is_empty() {
            break;
        }
        directory_paths.insert(parent.to_string_lossy().into_owned());
        current = parent.parent();
    }
}

fn build_visual_summaries(
    file_paths: &BTreeSet<String>,
    replay: &RepositoryReplay,
    path_filter: &ExplicitPathFilter,
    level_of_detail: LevelOfDetailPolicy,
    spacing: i32,
) -> Vec<VisualSummary> {
    let mut files_by_parent = BTreeMap::<String, Vec<String>>::new();
    for file_path in file_paths {
        if let Some(parent) = parent_directory_path(Path::new(file_path)) {
            files_by_parent
                .entry(parent)
                .or_default()
                .push(file_path.clone());
        }
    }

    files_by_parent
        .into_iter()
        .filter(|(_, represented_paths)| {
            represented_paths.len() >= level_of_detail.dense_directory_threshold().get()
        })
        .enumerate()
        .map(|(index, (path, represented_paths))| {
            let activity_count = replay
                .commit_events()
                .iter()
                .flat_map(CommitEvent::file_changes)
                .filter(|file_change| path_filter.includes_file_change(file_change))
                .filter(|file_change| {
                    file_change_references_any_path(file_change, &represented_paths)
                })
                .count();
            let emphasis = if represented_paths.iter().all(|path| {
                SceneEmphasis::from_path(Path::new(path)) == SceneEmphasis::DeEmphasized
            }) {
                SceneEmphasis::DeEmphasized
            } else {
                SceneEmphasis::Normal
            };

            VisualSummary {
                id: VisualSummarySceneId(format!("summary:{path}")),
                path,
                represented_entity_count: represented_paths.len(),
                activity_count,
                weight: VisualSummaryWeight(activity_count),
                represented_file_ids: represented_paths.into_iter().map(FileSceneId).collect(),
                position: ScenePosition::new(index, 3, spacing),
                emphasis,
            }
        })
        .collect()
}

fn file_change_references_any_path(file_change: &FileChange, paths: &[String]) -> bool {
    let current_path = repository_entity_path(file_change.entity());
    paths.iter().any(|path| path == &current_path)
        || file_change.previous_entity().is_some_and(|entity| {
            let previous_path = repository_entity_path(entity);
            paths.iter().any(|path| path == &previous_path)
        })
}

fn parent_directory_path(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    (!parent.as_os_str().is_empty()).then(|| parent.to_string_lossy().into_owned())
}

fn repository_entity_path(entity: &RepositoryEntity) -> String {
    entity.path().to_string_lossy().into_owned()
}

fn parent_directory_id(path: &Path) -> Option<DirectorySceneId> {
    let parent = path.parent()?;
    (!parent.as_os_str().is_empty())
        .then(|| DirectorySceneId(parent.to_string_lossy().into_owned()))
}

/// A reusable set of parameters for rendering a Repository Replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderConfiguration {
    visual_metaphor: VisualMetaphor,
    frame_size: FrameSize,
    frames_per_second: FramesPerSecond,
    theme: Theme,
    layout: Layout,
    level_of_detail: LevelOfDetailPolicy,
    explicit_path_filter: ExplicitPathFilter,
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
            level_of_detail: LevelOfDetailPolicy::default(),
            explicit_path_filter: ExplicitPathFilter::default(),
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

    /// Returns the Level of Detail policy for scene summarization.
    #[must_use]
    pub fn level_of_detail(&self) -> LevelOfDetailPolicy {
        self.level_of_detail
    }

    /// Returns explicit repository path filters applied before Level of Detail.
    #[must_use]
    pub fn explicit_path_filter(&self) -> &ExplicitPathFilter {
        &self.explicit_path_filter
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
        let level_of_detail = LevelOfDetailPolicy::try_from_raw(raw.level_of_detail, &mut errors);
        let explicit_path_filter = ExplicitPathFilter::try_from_raw(raw.filters, &mut errors);

        if errors.is_empty() {
            Ok(Self {
                visual_metaphor: VisualMetaphor::new("repository-replay"),
                frame_size: frame_size.expect("validated frame size"),
                frames_per_second: frames_per_second.expect("validated FPS"),
                theme: theme.expect("validated Theme"),
                layout: layout.expect("validated Layout"),
                level_of_detail: level_of_detail.expect("validated Level of Detail"),
                explicit_path_filter: explicit_path_filter.expect("validated explicit filters"),
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
            level_of_detail: LevelOfDetailPolicy::default(),
            explicit_path_filter: ExplicitPathFilter::default(),
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

/// A render policy that summarizes dense Repository Entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelOfDetailPolicy {
    dense_directory_threshold: EntityCountThreshold,
}

impl LevelOfDetailPolicy {
    /// Returns the file count at which a directory receives a Visual Summary.
    #[must_use]
    pub fn dense_directory_threshold(self) -> EntityCountThreshold {
        self.dense_directory_threshold
    }

    fn try_from_raw(
        raw: Option<RawLevelOfDetailPolicy>,
        errors: &mut RenderConfigurationError,
    ) -> Option<Self> {
        let Some(raw) = raw else {
            return Some(Self::default());
        };
        let dense_directory_threshold =
            match EntityCountThreshold::new(raw.dense_directory_threshold) {
                Ok(threshold) => Some(threshold),
                Err(ConfigValueError) => {
                    errors.push(
                        "level_of_detail.dense_directory_threshold",
                        "positive integer",
                    );
                    None
                }
            };

        Some(Self {
            dense_directory_threshold: dense_directory_threshold?,
        })
    }
}

impl Default for LevelOfDetailPolicy {
    fn default() -> Self {
        Self {
            dense_directory_threshold: EntityCountThreshold::new(5)
                .expect("default dense directory threshold is valid"),
        }
    }
}

/// Positive Repository Entity count threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityCountThreshold(usize);

impl EntityCountThreshold {
    /// Creates a positive entity count threshold.
    pub fn new(value: usize) -> Result<Self, ConfigValueError> {
        if value == 0 {
            Err(ConfigValueError)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the threshold value.
    #[must_use]
    pub fn get(self) -> usize {
        self.0
    }
}

/// Explicit path filters that scope the Repository Replay before summarization.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExplicitPathFilter {
    included_paths: Vec<PathBuf>,
}

impl ExplicitPathFilter {
    /// Returns true when no explicit path scope is configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.included_paths.is_empty()
    }

    /// Returns configured repository-relative included paths.
    #[must_use]
    pub fn included_paths(&self) -> &[PathBuf] {
        &self.included_paths
    }

    fn includes_file_change(&self, file_change: &FileChange) -> bool {
        self.includes_entity(file_change.entity())
            || file_change
                .previous_entity()
                .is_some_and(|entity| self.includes_entity(entity))
    }

    fn includes_entity(&self, entity: &RepositoryEntity) -> bool {
        self.is_empty()
            || self
                .included_paths
                .iter()
                .any(|included_path| entity.path().starts_with(included_path))
    }

    fn as_scene_filter(&self) -> Option<SceneExplicitPathFilter> {
        (!self.is_empty()).then(|| SceneExplicitPathFilter {
            included_paths: self
                .included_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        })
    }

    fn try_from_raw(
        raw: Option<RawExplicitPathFilter>,
        _errors: &mut RenderConfigurationError,
    ) -> Option<Self> {
        let Some(raw) = raw else {
            return Some(Self::default());
        };

        Some(Self {
            included_paths: raw.included_paths.into_iter().map(PathBuf::from).collect(),
        })
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
    level_of_detail: Option<RawLevelOfDetailPolicy>,
    filters: Option<RawExplicitPathFilter>,
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

struct RawLevelOfDetailPolicy {
    dense_directory_threshold: usize,
}

struct RawExplicitPathFilter {
    included_paths: Vec<String>,
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
                "level_of_detail",
                "filters",
            ],
            &mut errors,
        );

        let frame_width = u32_field(table, "frame_width", "frame_width", &mut errors);
        let frame_height = u32_field(table, "frame_height", "frame_height", &mut errors);
        let frames_per_second =
            u32_field(table, "frames_per_second", "frames_per_second", &mut errors);
        let theme = RawTheme::try_from_field(table.get("theme"), &mut errors);
        let layout = RawLayout::try_from_field(table.get("layout"), &mut errors);
        let level_of_detail =
            RawLevelOfDetailPolicy::try_from_field(table.get("level_of_detail"), &mut errors);
        let filters = RawExplicitPathFilter::try_from_field(table.get("filters"), &mut errors);

        if errors.is_empty() {
            Ok(Self {
                frame_width: frame_width.expect("validated frame width"),
                frame_height: frame_height.expect("validated frame height"),
                frames_per_second: frames_per_second.expect("validated FPS"),
                theme: theme.expect("validated Theme section"),
                layout: layout.expect("validated Layout section"),
                level_of_detail,
                filters,
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

impl RawLevelOfDetailPolicy {
    fn try_from_field(
        value: Option<&Value>,
        errors: &mut RenderConfigurationError,
    ) -> Option<Self> {
        let value = value?;
        let Some(table) = value.as_table() else {
            errors.push("level_of_detail", "table with Level of Detail fields");
            return None;
        };

        report_unknown_fields(
            table,
            "level_of_detail",
            &["dense_directory_threshold"],
            errors,
        );

        let dense_directory_threshold = usize_field(
            table,
            "dense_directory_threshold",
            "level_of_detail.dense_directory_threshold",
            errors,
        );

        Some(Self {
            dense_directory_threshold: dense_directory_threshold?,
        })
    }
}

impl RawExplicitPathFilter {
    fn try_from_field(
        value: Option<&Value>,
        errors: &mut RenderConfigurationError,
    ) -> Option<Self> {
        let value = value?;
        let Some(table) = value.as_table() else {
            errors.push("filters", "table with filter fields");
            return None;
        };

        report_unknown_fields(table, "filters", &["included_paths"], errors);

        let included_paths =
            string_array_field(table, "included_paths", "filters.included_paths", errors);

        Some(Self {
            included_paths: included_paths?,
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

fn usize_field(
    table: &toml::map::Map<String, Value>,
    key: &str,
    field: &'static str,
    errors: &mut RenderConfigurationError,
) -> Option<usize> {
    match table.get(key) {
        Some(Value::Integer(value)) => usize::try_from(*value).ok(),
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

fn string_array_field(
    table: &toml::map::Map<String, Value>,
    key: &str,
    field: &'static str,
    errors: &mut RenderConfigurationError,
) -> Option<Vec<String>> {
    match table.get(key) {
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| match value {
                Value::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        Some(_) | None => None,
    }
    .or_else(|| {
        errors.push(field, "array of repository-relative path strings");
        None
    })
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
        BranchFlow, CommitEvent, CommitEvidence, CommitId, CommitSubject, CompetingChange,
        CompetingChangeConfidence, CompetingChangeEvidence, CompetingChangeSource, Contributor,
        ContributorEvidence, FileChange, FileChangeKind, GitTimestamp, Mainline, MergeSettlement,
        RenderConfiguration, RepositoryEntity, RepositoryGraphScene, RepositoryReplay,
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
        assert_eq!(
            replay.commit_events()[0].branch_flow(),
            &BranchFlow::Mainline
        );
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

    #[test]
    fn repository_graph_scene_snapshots_linear_replay() {
        let mut replay = RepositoryReplay::new(Mainline::new("main"));

        replay.push_commit_event(CommitEvent::new(
            CommitId::new("a1"),
            Contributor::human("Ada"),
            vec![FileChange::new(
                RepositoryEntity::new("src/lib.rs"),
                FileChangeKind::Added,
            )],
        ));
        replay.push_commit_event(CommitEvent::new(
            CommitId::new("b2"),
            Contributor::human("Grace"),
            vec![
                FileChange::new(RepositoryEntity::new("README.md"), FileChangeKind::Added),
                FileChange::new(
                    RepositoryEntity::new("src/lib.rs"),
                    FileChangeKind::Modified,
                ),
            ],
        ));

        let scene = RepositoryGraphScene::from_replay(&replay, &RenderConfiguration::default());

        assert_eq!(
            format!("{scene:#?}"),
            r#"RepositoryGraphScene {
    mainline: "main",
    frame_size: SceneFrameSize {
        width: 1920,
        height: 1080,
    },
    frames_per_second: 60,
    explicit_path_filter: None,
    contributors: [
        SceneContributor {
            id: ContributorSceneId(
                "Ada",
            ),
            display_name: "Ada",
            kind: Human,
            position: ScenePosition {
                x: 0,
                y: 0,
            },
        },
        SceneContributor {
            id: ContributorSceneId(
                "Grace",
            ),
            display_name: "Grace",
            kind: Human,
            position: ScenePosition {
                x: 120,
                y: 0,
            },
        },
    ],
    directories: [
        SceneDirectory {
            id: DirectorySceneId(
                "src",
            ),
            path: "src",
            position: ScenePosition {
                x: 0,
                y: 120,
            },
        },
    ],
    files: [
        SceneFile {
            id: FileSceneId(
                "README.md",
            ),
            path: "README.md",
            parent_directory_id: None,
            position: ScenePosition {
                x: 0,
                y: 240,
            },
            motion: Settled,
            emphasis: Normal,
        },
        SceneFile {
            id: FileSceneId(
                "src/lib.rs",
            ),
            path: "src/lib.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src",
                ),
            ),
            position: ScenePosition {
                x: 120,
                y: 240,
            },
            motion: Settled,
            emphasis: Normal,
        },
    ],
    visual_summaries: [],
    activities: [
        SceneActivity {
            commit_id: CommitSceneId(
                "a1",
            ),
            playback_frame: 0,
            contributor_id: ContributorSceneId(
                "Ada",
            ),
            branch_activity: Mainline,
            file_changes: [
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/lib.rs",
                    ),
                    previous_file_id: None,
                    kind: Added,
                    motion: Settled,
                },
            ],
        },
        SceneActivity {
            commit_id: CommitSceneId(
                "b2",
            ),
            playback_frame: 60,
            contributor_id: ContributorSceneId(
                "Grace",
            ),
            branch_activity: Mainline,
            file_changes: [
                SceneFileChange {
                    file_id: FileSceneId(
                        "README.md",
                    ),
                    previous_file_id: None,
                    kind: Added,
                    motion: Settled,
                },
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/lib.rs",
                    ),
                    previous_file_id: None,
                    kind: Modified,
                    motion: Settled,
                },
            ],
        },
    ],
    competing_changes: [],
}"#
        );
    }

    #[test]
    fn repository_graph_scene_preserves_branch_and_merge_activity() {
        let mainline = Mainline::new("main");
        let mut replay = RepositoryReplay::new(mainline.clone());
        let contributor = Contributor::human("Ada");
        let evidence = ContributorEvidence::new("Ada", "ada@example.com", GitTimestamp::new(1, 0));

        replay.push_commit_event(CommitEvent::from_evidence(
            CommitEvidence::new(
                CommitId::new("a1"),
                CommitSubject::new("base"),
                evidence.clone(),
                evidence.clone(),
                contributor.clone(),
            )
            .with_file_changes(vec![FileChange::new(
                RepositoryEntity::new("src/lib.rs"),
                FileChangeKind::Added,
            )]),
        ));
        replay.push_commit_event(CommitEvent::from_evidence(
            CommitEvidence::new(
                CommitId::new("b1"),
                CommitSubject::new("feature work"),
                evidence.clone(),
                evidence.clone(),
                contributor.clone(),
            )
            .with_parent_ids(vec![CommitId::new("a1")])
            .with_branch_flow(BranchFlow::BranchSuperposition {
                branch: "feature".to_owned(),
                mainline: mainline.clone(),
            })
            .with_file_changes(vec![FileChange::new(
                RepositoryEntity::new("src/lib.rs"),
                FileChangeKind::Modified,
            )]),
        ));
        replay.push_commit_event(CommitEvent::from_evidence(
            CommitEvidence::new(
                CommitId::new("m1"),
                CommitSubject::new("merge feature"),
                evidence.clone(),
                evidence,
                contributor,
            )
            .with_parent_ids(vec![CommitId::new("a1"), CommitId::new("b1")])
            .with_branch_flow(BranchFlow::MergeSettlements(vec![MergeSettlement::new(
                "feature",
                mainline,
                vec![CommitId::new("b1")],
            )]))
            .with_file_changes(vec![FileChange::new(
                RepositoryEntity::new("src/lib.rs"),
                FileChangeKind::Modified,
            )]),
        ));
        replay.push_competing_change(CompetingChange::new(
            RepositoryEntity::new("src/lib.rs"),
            CompetingChangeSource::FileLevelOverlap,
            CompetingChangeConfidence::Medium,
            vec![
                CompetingChangeEvidence::new("feature", CommitId::new("b1")),
                CompetingChangeEvidence::new("main", CommitId::new("a1")),
            ],
        ));

        let scene = RepositoryGraphScene::from_replay(&replay, &RenderConfiguration::default());

        assert_eq!(
            format!("{scene:#?}"),
            r#"RepositoryGraphScene {
    mainline: "main",
    frame_size: SceneFrameSize {
        width: 1920,
        height: 1080,
    },
    frames_per_second: 60,
    explicit_path_filter: None,
    contributors: [
        SceneContributor {
            id: ContributorSceneId(
                "Ada",
            ),
            display_name: "Ada",
            kind: Human,
            position: ScenePosition {
                x: 0,
                y: 0,
            },
        },
    ],
    directories: [
        SceneDirectory {
            id: DirectorySceneId(
                "src",
            ),
            path: "src",
            position: ScenePosition {
                x: 0,
                y: 120,
            },
        },
    ],
    files: [
        SceneFile {
            id: FileSceneId(
                "src/lib.rs",
            ),
            path: "src/lib.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src",
                ),
            ),
            position: ScenePosition {
                x: 0,
                y: 240,
            },
            motion: Settled,
            emphasis: Normal,
        },
    ],
    visual_summaries: [],
    activities: [
        SceneActivity {
            commit_id: CommitSceneId(
                "a1",
            ),
            playback_frame: 0,
            contributor_id: ContributorSceneId(
                "Ada",
            ),
            branch_activity: Mainline,
            file_changes: [
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/lib.rs",
                    ),
                    previous_file_id: None,
                    kind: Added,
                    motion: Settled,
                },
            ],
        },
        SceneActivity {
            commit_id: CommitSceneId(
                "b1",
            ),
            playback_frame: 60,
            contributor_id: ContributorSceneId(
                "Ada",
            ),
            branch_activity: BranchSuperposition {
                branch: "feature",
                mainline: "main",
            },
            file_changes: [
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/lib.rs",
                    ),
                    previous_file_id: None,
                    kind: Modified,
                    motion: Provisional,
                },
            ],
        },
        SceneActivity {
            commit_id: CommitSceneId(
                "m1",
            ),
            playback_frame: 120,
            contributor_id: ContributorSceneId(
                "Ada",
            ),
            branch_activity: MergeSettlements(
                [
                    SceneMergeSettlement {
                        branch: "feature",
                        mainline: "main",
                        settled_commit_ids: [
                            CommitSceneId(
                                "b1",
                            ),
                        ],
                    },
                ],
            ),
            file_changes: [
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/lib.rs",
                    ),
                    previous_file_id: None,
                    kind: Modified,
                    motion: Settling,
                },
            ],
        },
    ],
    competing_changes: [
        SceneCompetingChange {
            file_id: FileSceneId(
                "src/lib.rs",
            ),
            source: FileLevelOverlap,
            confidence: Medium,
            evidence: [
                SceneCompetingChangeEvidence {
                    branch: "feature",
                    commit_id: CommitSceneId(
                        "b1",
                    ),
                },
                SceneCompetingChangeEvidence {
                    branch: "main",
                    commit_id: CommitSceneId(
                        "a1",
                    ),
                },
            ],
        },
    ],
}"#
        );
    }

    #[test]
    fn repository_graph_scene_summarizes_dense_directory_with_activity_weight() {
        let mut replay = RepositoryReplay::new(Mainline::new("main"));
        let dense_changes = (0..5)
            .map(|index| {
                FileChange::new(
                    RepositoryEntity::new(format!("src/generated/file_{index}.rs")),
                    FileChangeKind::Modified,
                )
            })
            .collect();

        replay.push_commit_event(CommitEvent::new(
            CommitId::new("dense"),
            Contributor::automation("Generator"),
            dense_changes,
        ));

        let scene = RepositoryGraphScene::from_replay(&replay, &RenderConfiguration::default());

        assert_eq!(
            format!("{scene:#?}"),
            r#"RepositoryGraphScene {
    mainline: "main",
    frame_size: SceneFrameSize {
        width: 1920,
        height: 1080,
    },
    frames_per_second: 60,
    explicit_path_filter: None,
    contributors: [
        SceneContributor {
            id: ContributorSceneId(
                "Generator",
            ),
            display_name: "Generator",
            kind: Automation,
            position: ScenePosition {
                x: 0,
                y: 0,
            },
        },
    ],
    directories: [
        SceneDirectory {
            id: DirectorySceneId(
                "src",
            ),
            path: "src",
            position: ScenePosition {
                x: 0,
                y: 120,
            },
        },
        SceneDirectory {
            id: DirectorySceneId(
                "src/generated",
            ),
            path: "src/generated",
            position: ScenePosition {
                x: 120,
                y: 120,
            },
        },
    ],
    files: [
        SceneFile {
            id: FileSceneId(
                "src/generated/file_0.rs",
            ),
            path: "src/generated/file_0.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src/generated",
                ),
            ),
            position: ScenePosition {
                x: 0,
                y: 240,
            },
            motion: Settled,
            emphasis: DeEmphasized,
        },
        SceneFile {
            id: FileSceneId(
                "src/generated/file_1.rs",
            ),
            path: "src/generated/file_1.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src/generated",
                ),
            ),
            position: ScenePosition {
                x: 120,
                y: 240,
            },
            motion: Settled,
            emphasis: DeEmphasized,
        },
        SceneFile {
            id: FileSceneId(
                "src/generated/file_2.rs",
            ),
            path: "src/generated/file_2.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src/generated",
                ),
            ),
            position: ScenePosition {
                x: 240,
                y: 240,
            },
            motion: Settled,
            emphasis: DeEmphasized,
        },
        SceneFile {
            id: FileSceneId(
                "src/generated/file_3.rs",
            ),
            path: "src/generated/file_3.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src/generated",
                ),
            ),
            position: ScenePosition {
                x: 360,
                y: 240,
            },
            motion: Settled,
            emphasis: DeEmphasized,
        },
        SceneFile {
            id: FileSceneId(
                "src/generated/file_4.rs",
            ),
            path: "src/generated/file_4.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src/generated",
                ),
            ),
            position: ScenePosition {
                x: 480,
                y: 240,
            },
            motion: Settled,
            emphasis: DeEmphasized,
        },
    ],
    visual_summaries: [
        VisualSummary {
            id: VisualSummarySceneId(
                "summary:src/generated",
            ),
            path: "src/generated",
            represented_file_ids: [
                FileSceneId(
                    "src/generated/file_0.rs",
                ),
                FileSceneId(
                    "src/generated/file_1.rs",
                ),
                FileSceneId(
                    "src/generated/file_2.rs",
                ),
                FileSceneId(
                    "src/generated/file_3.rs",
                ),
                FileSceneId(
                    "src/generated/file_4.rs",
                ),
            ],
            represented_entity_count: 5,
            activity_count: 5,
            weight: VisualSummaryWeight(
                5,
            ),
            position: ScenePosition {
                x: 0,
                y: 360,
            },
            emphasis: DeEmphasized,
        },
    ],
    activities: [
        SceneActivity {
            commit_id: CommitSceneId(
                "dense",
            ),
            playback_frame: 0,
            contributor_id: ContributorSceneId(
                "Generator",
            ),
            branch_activity: Mainline,
            file_changes: [
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/generated/file_0.rs",
                    ),
                    previous_file_id: None,
                    kind: Modified,
                    motion: Settled,
                },
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/generated/file_1.rs",
                    ),
                    previous_file_id: None,
                    kind: Modified,
                    motion: Settled,
                },
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/generated/file_2.rs",
                    ),
                    previous_file_id: None,
                    kind: Modified,
                    motion: Settled,
                },
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/generated/file_3.rs",
                    ),
                    previous_file_id: None,
                    kind: Modified,
                    motion: Settled,
                },
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/generated/file_4.rs",
                    ),
                    previous_file_id: None,
                    kind: Modified,
                    motion: Settled,
                },
            ],
        },
    ],
    competing_changes: [],
}"#
        );
    }

    #[test]
    fn explicit_path_filter_scopes_scene_before_visual_summary() {
        let mut replay = RepositoryReplay::new(Mainline::new("main"));
        let mut file_changes = (0..5)
            .map(|index| {
                FileChange::new(
                    RepositoryEntity::new(format!("src/app/file_{index}.rs")),
                    FileChangeKind::Added,
                )
            })
            .collect::<Vec<_>>();
        file_changes.push(FileChange::new(
            RepositoryEntity::new("docs/guide.md"),
            FileChangeKind::Added,
        ));
        file_changes.push(FileChange::new(
            RepositoryEntity::new("tests/app_test.rs"),
            FileChangeKind::Added,
        ));

        replay.push_commit_event(CommitEvent::new(
            CommitId::new("mixed"),
            Contributor::human("Ada"),
            file_changes,
        ));

        let config = RenderConfiguration::from_toml_str(
            r##"
frame_width = 1920
frame_height = 1080
frames_per_second = 60

[theme]
name = "gitflux-dark"
background_color = "#0b1020"
entity_color = "#7dd3fc"
contributor_color = "#facc15"

[layout]
kind = "repository_graph"
entity_spacing = 120
settle_iterations = 60

[filters]
included_paths = ["src/app"]
"##,
        )
        .expect("explicit path filter should parse");

        let scene = RepositoryGraphScene::from_replay(&replay, &config);

        assert_eq!(
            format!("{scene:#?}"),
            r#"RepositoryGraphScene {
    mainline: "main",
    frame_size: SceneFrameSize {
        width: 1920,
        height: 1080,
    },
    frames_per_second: 60,
    explicit_path_filter: Some(
        SceneExplicitPathFilter {
            included_paths: [
                "src/app",
            ],
        },
    ),
    contributors: [
        SceneContributor {
            id: ContributorSceneId(
                "Ada",
            ),
            display_name: "Ada",
            kind: Human,
            position: ScenePosition {
                x: 0,
                y: 0,
            },
        },
    ],
    directories: [
        SceneDirectory {
            id: DirectorySceneId(
                "src",
            ),
            path: "src",
            position: ScenePosition {
                x: 0,
                y: 120,
            },
        },
        SceneDirectory {
            id: DirectorySceneId(
                "src/app",
            ),
            path: "src/app",
            position: ScenePosition {
                x: 120,
                y: 120,
            },
        },
    ],
    files: [
        SceneFile {
            id: FileSceneId(
                "src/app/file_0.rs",
            ),
            path: "src/app/file_0.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src/app",
                ),
            ),
            position: ScenePosition {
                x: 0,
                y: 240,
            },
            motion: Settled,
            emphasis: Normal,
        },
        SceneFile {
            id: FileSceneId(
                "src/app/file_1.rs",
            ),
            path: "src/app/file_1.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src/app",
                ),
            ),
            position: ScenePosition {
                x: 120,
                y: 240,
            },
            motion: Settled,
            emphasis: Normal,
        },
        SceneFile {
            id: FileSceneId(
                "src/app/file_2.rs",
            ),
            path: "src/app/file_2.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src/app",
                ),
            ),
            position: ScenePosition {
                x: 240,
                y: 240,
            },
            motion: Settled,
            emphasis: Normal,
        },
        SceneFile {
            id: FileSceneId(
                "src/app/file_3.rs",
            ),
            path: "src/app/file_3.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src/app",
                ),
            ),
            position: ScenePosition {
                x: 360,
                y: 240,
            },
            motion: Settled,
            emphasis: Normal,
        },
        SceneFile {
            id: FileSceneId(
                "src/app/file_4.rs",
            ),
            path: "src/app/file_4.rs",
            parent_directory_id: Some(
                DirectorySceneId(
                    "src/app",
                ),
            ),
            position: ScenePosition {
                x: 480,
                y: 240,
            },
            motion: Settled,
            emphasis: Normal,
        },
    ],
    visual_summaries: [
        VisualSummary {
            id: VisualSummarySceneId(
                "summary:src/app",
            ),
            path: "src/app",
            represented_file_ids: [
                FileSceneId(
                    "src/app/file_0.rs",
                ),
                FileSceneId(
                    "src/app/file_1.rs",
                ),
                FileSceneId(
                    "src/app/file_2.rs",
                ),
                FileSceneId(
                    "src/app/file_3.rs",
                ),
                FileSceneId(
                    "src/app/file_4.rs",
                ),
            ],
            represented_entity_count: 5,
            activity_count: 5,
            weight: VisualSummaryWeight(
                5,
            ),
            position: ScenePosition {
                x: 0,
                y: 360,
            },
            emphasis: Normal,
        },
    ],
    activities: [
        SceneActivity {
            commit_id: CommitSceneId(
                "mixed",
            ),
            playback_frame: 0,
            contributor_id: ContributorSceneId(
                "Ada",
            ),
            branch_activity: Mainline,
            file_changes: [
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/app/file_0.rs",
                    ),
                    previous_file_id: None,
                    kind: Added,
                    motion: Settled,
                },
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/app/file_1.rs",
                    ),
                    previous_file_id: None,
                    kind: Added,
                    motion: Settled,
                },
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/app/file_2.rs",
                    ),
                    previous_file_id: None,
                    kind: Added,
                    motion: Settled,
                },
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/app/file_3.rs",
                    ),
                    previous_file_id: None,
                    kind: Added,
                    motion: Settled,
                },
                SceneFileChange {
                    file_id: FileSceneId(
                        "src/app/file_4.rs",
                    ),
                    previous_file_id: None,
                    kind: Added,
                    motion: Settled,
                },
            ],
        },
    ],
    competing_changes: [],
}"#
        );
    }

    #[test]
    fn generated_vendor_and_lockfile_paths_are_de_emphasized_without_exclusion() {
        let mut replay = RepositoryReplay::new(Mainline::new("main"));
        replay.push_commit_event(CommitEvent::new(
            CommitId::new("deps"),
            Contributor::automation("Dependency update"),
            vec![
                FileChange::new(
                    RepositoryEntity::new("target/cache.bin"),
                    FileChangeKind::Added,
                ),
                FileChange::new(
                    RepositoryEntity::new("vendor/lib.rs"),
                    FileChangeKind::Added,
                ),
                FileChange::new(
                    RepositoryEntity::new("node_modules/pkg/index.js"),
                    FileChangeKind::Added,
                ),
                FileChange::new(
                    RepositoryEntity::new("Cargo.lock"),
                    FileChangeKind::Modified,
                ),
                FileChange::new(
                    RepositoryEntity::new("package-lock.json"),
                    FileChangeKind::Modified,
                ),
                FileChange::new(
                    RepositoryEntity::new("src/generated/file.rs"),
                    FileChangeKind::Modified,
                ),
                FileChange::new(
                    RepositoryEntity::new("src/lib.rs"),
                    FileChangeKind::Modified,
                ),
            ],
        ));

        let scene = RepositoryGraphScene::from_replay(&replay, &RenderConfiguration::default());
        let snapshot = format!("{scene:#?}");

        assert!(snapshot.contains("target/cache.bin"));
        assert!(snapshot.contains("vendor/lib.rs"));
        assert!(snapshot.contains("node_modules/pkg/index.js"));
        assert!(snapshot.contains("Cargo.lock"));
        assert!(snapshot.contains("package-lock.json"));
        assert!(snapshot.contains("src/generated/file.rs"));
        assert!(snapshot.contains("src/lib.rs"));
        assert_eq!(snapshot.matches("emphasis: DeEmphasized").count(), 6);
        assert!(snapshot.contains("emphasis: Normal"));
    }

    #[test]
    fn visual_summary_counts_move_activity_from_previous_entity() {
        let mut replay = RepositoryReplay::new(Mainline::new("main"));
        let dense_changes = (0..5)
            .map(|index| {
                FileChange::new(
                    RepositoryEntity::new(format!("src/old/file_{index}.rs")),
                    FileChangeKind::Added,
                )
            })
            .collect();

        replay.push_commit_event(CommitEvent::new(
            CommitId::new("base"),
            Contributor::human("Ada"),
            dense_changes,
        ));
        replay.push_commit_event(CommitEvent::new(
            CommitId::new("move"),
            Contributor::human("Ada"),
            vec![FileChange::moved(
                RepositoryEntity::new("src/old/file_0.rs"),
                RepositoryEntity::new("src/new/file_0.rs"),
            )],
        ));

        let scene = RepositoryGraphScene::from_replay(&replay, &RenderConfiguration::default());
        let old_summary = scene
            .visual_summaries()
            .iter()
            .find(|summary| summary.path() == "src/old")
            .expect("source directory should remain summarized");

        assert_eq!(old_summary.represented_entity_count(), 5);
        assert_eq!(old_summary.activity_count(), 6);
        assert_eq!(old_summary.weight().get(), 6);
    }
}
