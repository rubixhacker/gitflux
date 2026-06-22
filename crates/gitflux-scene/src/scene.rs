use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{
    BranchFlow, CommitEvent, CompetingChange, CompetingChangeConfidence, CompetingChangeSource,
    ContributorKind, ExplicitPathFilter, FileChange, FileChangeKind, LevelOfDetailPolicy,
    RenderConfiguration, ReplayPacingDuration, RepositoryEntity, RepositoryReplay,
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

        let visible_activities: Vec<(&CommitEvent, Vec<SceneFileChange>)> = replay
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
            .collect();
        let paced_activities = order_commit_events_by_parent_ids(visible_activities);
        let pacing_decisions = ReplayPacingDecisions::from_commit_events(
            &paced_activities,
            configuration.frames_per_second().get(),
            configuration.replay_pacing(),
        );
        let activities = paced_activities
            .into_iter()
            .enumerate()
            .map(|(index, (commit_event, mut file_changes))| {
                let pacing_decision = pacing_decisions
                    .get(index)
                    .expect("Replay Pacing decision exists for visible Commit Event");
                apply_file_change_offsets(&mut file_changes, pacing_decision.file_change_offsets());
                SceneActivity {
                    commit_id: CommitSceneId(commit_event.id().as_str().to_owned()),
                    playback_frame: pacing_decision.playback_frame(),
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
#[derive(Clone, PartialEq, Eq)]
pub struct SceneFileChange {
    file_id: FileSceneId,
    previous_file_id: Option<FileSceneId>,
    kind: FileChangeKind,
    motion: MotionState,
    playback_frame_offset: u64,
}

impl std::fmt::Debug for SceneFileChange {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = formatter.debug_struct("SceneFileChange");
        debug
            .field("file_id", &self.file_id)
            .field("previous_file_id", &self.previous_file_id)
            .field("kind", &self.kind)
            .field("motion", &self.motion);
        if self.playback_frame_offset != 0 {
            debug.field("playback_frame_offset", &self.playback_frame_offset);
        }
        debug.finish()
    }
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

    /// Returns this File Change's offset from its Commit Event activity frame.
    #[must_use]
    pub fn playback_frame_offset(&self) -> u64 {
        self.playback_frame_offset
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
            playback_frame_offset: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplayPacingDecision {
    playback_frame: u64,
    file_change_offsets: Vec<u64>,
}

impl ReplayPacingDecision {
    fn playback_frame(&self) -> u64 {
        self.playback_frame
    }

    fn file_change_offsets(&self) -> &[u64] {
        &self.file_change_offsets
    }
}

struct ReplayPacingDecisions;

impl ReplayPacingDecisions {
    fn from_commit_events(
        commit_events: &[(&CommitEvent, Vec<SceneFileChange>)],
        frames_per_second: u32,
        replay_pacing: crate::ReplayPacing,
    ) -> Vec<ReplayPacingDecision> {
        let timeline = ReplayTimeline::from_commit_events(
            commit_events,
            frames_per_second,
            replay_pacing.duration(),
        );
        let playback_frames = timeline.playback_frames();
        let large_commit_window_frames =
            u64::from(frames_per_second) * u64::from(replay_pacing.large_commit_spread_seconds());

        playback_frames
            .iter()
            .enumerate()
            .zip(commit_events)
            .map(
                |((index, playback_frame), (_, file_changes))| ReplayPacingDecision {
                    playback_frame: *playback_frame,
                    file_change_offsets: file_change_offsets(file_changes.len(), {
                        let available_window_frames = timeline.available_spread_frames_after(index);
                        large_commit_window_frames.min(available_window_frames)
                    }),
                },
            )
            .collect()
    }
}

struct ReplayTimeline {
    playback_frames: Vec<u64>,
    target_end_frame: Option<u64>,
}

impl ReplayTimeline {
    fn from_commit_events(
        commit_events: &[(&CommitEvent, Vec<SceneFileChange>)],
        frames_per_second: u32,
        duration: ReplayPacingDuration,
    ) -> Self {
        let playback_frames = paced_playback_frames(commit_events, frames_per_second, duration);
        let target_end_frame = explicit_target_end_frame(duration, frames_per_second);

        Self {
            playback_frames,
            target_end_frame,
        }
    }

    fn playback_frames(&self) -> &[u64] {
        &self.playback_frames
    }

    fn available_spread_frames_after(&self, index: usize) -> u64 {
        let playback_frame = self
            .playback_frames
            .get(index)
            .copied()
            .expect("Replay Timeline has frame for Commit Event index");
        if let Some(next_playback_frame) = self.playback_frames.get(index + 1) {
            return exclusive_spread_window(playback_frame, *next_playback_frame);
        }

        self.target_end_frame
            .map(|target_end_frame| exclusive_spread_window(playback_frame, target_end_frame))
            .unwrap_or(u64::MAX)
    }
}

fn exclusive_spread_window(playback_frame: u64, boundary_frame: u64) -> u64 {
    boundary_frame
        .saturating_sub(playback_frame)
        .saturating_sub(1)
}

fn paced_playback_frames(
    commit_events: &[(&CommitEvent, Vec<SceneFileChange>)],
    frames_per_second: u32,
    duration: ReplayPacingDuration,
) -> Vec<u64> {
    match commit_events.len() {
        0 => return Vec::new(),
        1 => return vec![0],
        _ => {}
    }

    let interval_count = commit_events.len() - 1;
    let target_frames = target_frames(duration, frames_per_second, interval_count);
    let weights: Vec<f64> = commit_events
        .windows(2)
        .map(|pair| {
            let previous = pair[0].0.committed_at().seconds();
            let current = pair[1].0.committed_at().seconds();
            let quiet_gap_seconds = current.saturating_sub(previous).max(0) as f64;

            quiet_gap_seconds.ln_1p().max(1.0)
        })
        .collect();
    let total_weight = weights.iter().sum::<f64>();
    let mut frames = Vec::with_capacity(commit_events.len());
    let mut cumulative_weight = 0.0;

    frames.push(0);
    for (index, weight) in weights.iter().enumerate() {
        cumulative_weight += weight;
        let remaining_intervals = interval_count - index - 1;
        let mut frame = ((cumulative_weight / total_weight) * target_frames as f64).round() as u64;
        let previous_frame = *frames.last().expect("at least first frame exists");
        frame = frame.max(previous_frame + 1);
        if remaining_intervals == 0 {
            frame = target_frames;
        } else {
            frame = frame.min(target_frames - remaining_intervals as u64);
        }
        frames.push(frame);
    }

    frames
}

fn explicit_target_end_frame(
    duration: ReplayPacingDuration,
    frames_per_second: u32,
) -> Option<u64> {
    match duration {
        ReplayPacingDuration::Auto => None,
        ReplayPacingDuration::Target { duration_seconds } => {
            Some(u64::from(duration_seconds) * u64::from(frames_per_second))
        }
    }
}

fn target_frames(
    duration: ReplayPacingDuration,
    frames_per_second: u32,
    interval_count: usize,
) -> u64 {
    match duration {
        ReplayPacingDuration::Auto => {
            u64::try_from(interval_count).expect("interval count fits u64")
                * u64::from(frames_per_second)
        }
        ReplayPacingDuration::Target { duration_seconds } => {
            let configured_target = u64::from(duration_seconds) * u64::from(frames_per_second);
            configured_target.max(u64::try_from(interval_count).expect("interval count fits u64"))
        }
    }
}

fn file_change_offsets(file_change_count: usize, large_commit_window_frames: u64) -> Vec<u64> {
    const LARGE_COMMIT_FILE_CHANGE_THRESHOLD: usize = 6;

    if file_change_count < LARGE_COMMIT_FILE_CHANGE_THRESHOLD || large_commit_window_frames == 0 {
        return vec![0; file_change_count];
    }

    let spread_steps = u64::try_from(file_change_count - 1).expect("file change count fits u64");
    (0..file_change_count)
        .map(|index| {
            let index = u64::try_from(index).expect("file change index fits u64");
            (index * large_commit_window_frames + spread_steps / 2) / spread_steps
        })
        .collect()
}

fn apply_file_change_offsets(file_changes: &mut [SceneFileChange], offsets: &[u64]) {
    for (file_change, offset) in file_changes.iter_mut().zip(offsets) {
        file_change.playback_frame_offset = *offset;
    }
}

fn order_commit_events_by_parent_ids(
    commit_events: Vec<(&CommitEvent, Vec<SceneFileChange>)>,
) -> Vec<(&CommitEvent, Vec<SceneFileChange>)> {
    let mut index_by_commit_id = BTreeMap::<String, usize>::new();
    for (index, (commit_event, _)) in commit_events.iter().enumerate() {
        index_by_commit_id.insert(commit_event.id().as_str().to_owned(), index);
    }

    let mut child_indexes_by_parent_index = BTreeMap::<usize, Vec<usize>>::new();
    let mut visible_parent_counts = vec![0_usize; commit_events.len()];
    for (child_index, (commit_event, _)) in commit_events.iter().enumerate() {
        for parent_id in commit_event.parent_ids() {
            if let Some(parent_index) = index_by_commit_id.get(parent_id.as_str()) {
                visible_parent_counts[child_index] += 1;
                child_indexes_by_parent_index
                    .entry(*parent_index)
                    .or_default()
                    .push(child_index);
            }
        }
    }

    let mut ready_indexes = BTreeSet::<usize>::new();
    for (index, visible_parent_count) in visible_parent_counts.iter().enumerate() {
        if *visible_parent_count == 0 {
            ready_indexes.insert(index);
        }
    }

    let mut ordered_indexes = Vec::with_capacity(commit_events.len());
    let mut ordered_index_set = BTreeSet::<usize>::new();
    while let Some(index) = ready_indexes.pop_first() {
        ordered_indexes.push(index);
        ordered_index_set.insert(index);

        if let Some(child_indexes) = child_indexes_by_parent_index.get(&index) {
            for child_index in child_indexes {
                visible_parent_counts[*child_index] -= 1;
                if visible_parent_counts[*child_index] == 0 {
                    ready_indexes.insert(*child_index);
                }
            }
        }
    }

    if ordered_indexes.len() != commit_events.len() {
        for index in 0..commit_events.len() {
            if ordered_index_set.insert(index) {
                ordered_indexes.push(index);
            }
        }
    }

    let mut commit_events_by_index: Vec<Option<(&CommitEvent, Vec<SceneFileChange>)>> =
        commit_events.into_iter().map(Some).collect();
    ordered_indexes
        .into_iter()
        .map(|index| {
            commit_events_by_index[index]
                .take()
                .expect("ordered Commit Event index is unique")
        })
        .collect()
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
