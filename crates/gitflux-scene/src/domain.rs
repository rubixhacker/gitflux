use std::path::PathBuf;

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
