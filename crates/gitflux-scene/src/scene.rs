use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{
    BranchFlow, CommitEvent, CompetingChange, CompetingChangeConfidence, CompetingChangeSource,
    ContributorKind, ExplicitPathFilter, FileChange, FileChangeKind, LevelOfDetailPolicy,
    RenderConfiguration, RepositoryEntity, RepositoryReplay,
};

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
    pub(crate) included_paths: Vec<String>,
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
