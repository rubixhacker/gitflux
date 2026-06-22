//! Repository Ingestion seam for Gitflux.
//!
//! This crate will own Rust-native preparation of Git history into the
//! Repository Replay timeline. The current scaffold defines the public request
//! and summary types without choosing the concrete Git library integration.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use git2::{BranchType, Delta, DiffFindOptions, DiffOptions, Oid, Repository, Sort};
use gitflux_scene::{
    BranchFlow, CommitEvent, CommitEvidence, CommitId, CommitSubject, CompetingChange,
    CompetingChangeConfidence, CompetingChangeEvidence, CompetingChangeSource, Contributor,
    ContributorEvidence, ContributorKind, FileChange, FileChangeKind, GitTimestamp, Mainline,
    MergeSettlement, RepositoryEntity, RepositoryReplay,
};

/// Input for preparing Git history into a Repository Replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryIngestionRequest {
    repository_path: PathBuf,
    mainline: MainlineSelection,
    contributor_normalization: ContributorNormalizationRules,
}

impl RepositoryIngestionRequest {
    /// Creates a Repository Ingestion request.
    #[must_use]
    pub fn new(repository_path: impl Into<PathBuf>, mainline: Mainline) -> Self {
        Self {
            repository_path: repository_path.into(),
            mainline: MainlineSelection::Explicit(mainline),
            contributor_normalization: ContributorNormalizationRules::default(),
        }
    }

    /// Creates a Repository Ingestion request that detects the Mainline from local refs.
    #[must_use]
    pub fn detect_mainline(repository_path: impl Into<PathBuf>) -> Self {
        Self {
            repository_path: repository_path.into(),
            mainline: MainlineSelection::DetectFromLocalRefs,
            contributor_normalization: ContributorNormalizationRules::default(),
        }
    }

    /// Returns the repository path to ingest.
    #[must_use]
    pub fn repository_path(&self) -> &Path {
        &self.repository_path
    }

    /// Returns the explicit Mainline override, if one was supplied.
    #[must_use]
    pub fn explicit_mainline(&self) -> Option<&Mainline> {
        match &self.mainline {
            MainlineSelection::Explicit(mainline) => Some(mainline),
            MainlineSelection::DetectFromLocalRefs => None,
        }
    }

    /// Sets Contributor normalization rules for Repository Ingestion.
    #[must_use]
    pub fn with_contributor_normalization(
        mut self,
        contributor_normalization: ContributorNormalizationRules,
    ) -> Self {
        self.contributor_normalization = contributor_normalization;
        self
    }

    /// Returns the Contributor normalization rules.
    #[must_use]
    pub fn contributor_normalization(&self) -> &ContributorNormalizationRules {
        &self.contributor_normalization
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MainlineSelection {
    Explicit(Mainline),
    DetectFromLocalRefs,
}

/// Typed rules used to normalize raw Git signatures into Contributors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributorNormalizationRules {
    aliases: Vec<ContributorAliasRule>,
    automation: Vec<AutomationContributorRule>,
}

impl ContributorNormalizationRules {
    /// Creates rules with conservative default Automation Contributor detection.
    #[must_use]
    pub fn new() -> Self {
        Self {
            aliases: Vec::new(),
            automation: vec![
                AutomationContributorRule::name_contains("[bot]"),
                AutomationContributorRule::email_prefix("bot@"),
                AutomationContributorRule::email_contains("[bot]@"),
            ],
        }
    }

    /// Adds an explicit alias from one raw email to another canonical email.
    #[must_use]
    pub fn with_email_alias(
        mut self,
        alias_email: impl Into<String>,
        canonical_email: impl Into<String>,
    ) -> Self {
        self.aliases.push(ContributorAliasRule::email(
            alias_email.into(),
            canonical_email.into(),
        ));
        self
    }

    /// Adds an explicit Automation Contributor detection rule.
    #[must_use]
    pub fn with_automation_rule(mut self, rule: AutomationContributorRule) -> Self {
        self.automation.push(rule);
        self
    }

    /// Replaces Automation Contributor detection with exactly the supplied rules.
    #[must_use]
    pub fn with_automation_rules(
        mut self,
        rules: impl IntoIterator<Item = AutomationContributorRule>,
    ) -> Self {
        self.automation = rules.into_iter().collect();
        self
    }

    fn canonical_key(&self, evidence: &ContributorEvidence) -> String {
        let normalized_email = normalize_token(evidence.email());
        if !normalized_email.is_empty() {
            if let Some(alias) = self.aliases.iter().find_map(|alias| {
                alias
                    .matches_email(&normalized_email)
                    .then(|| alias.canonical_key())
            }) {
                return alias;
            }
            return format!("email:{normalized_email}");
        }

        format!("name:{}", normalize_token(evidence.name()))
    }

    fn contributor_kind(&self, evidence: &ContributorEvidence) -> ContributorKind {
        if self.automation.iter().any(|rule| rule.matches(evidence)) {
            ContributorKind::Automation
        } else {
            ContributorKind::Human
        }
    }
}

impl Default for ContributorNormalizationRules {
    fn default() -> Self {
        Self::new()
    }
}

/// Explicit evidence-based Contributor aliasing rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributorAliasRule {
    alias_email: String,
    canonical_email: String,
}

impl ContributorAliasRule {
    /// Creates an explicit email alias.
    #[must_use]
    pub fn email(alias_email: impl Into<String>, canonical_email: impl Into<String>) -> Self {
        Self {
            alias_email: normalize_token(alias_email.into()),
            canonical_email: normalize_token(canonical_email.into()),
        }
    }

    fn matches_email(&self, normalized_email: &str) -> bool {
        self.alias_email == normalized_email
    }

    fn canonical_key(&self) -> String {
        format!("email:{}", self.canonical_email)
    }
}

/// A configurable rule for detecting an Automation Contributor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationContributorRule {
    field: ContributorEvidenceField,
    pattern: ContributorEvidencePattern,
}

impl AutomationContributorRule {
    /// Matches when the normalized raw name contains the pattern.
    #[must_use]
    pub fn name_contains(value: impl Into<String>) -> Self {
        Self::new(
            ContributorEvidenceField::Name,
            ContributorEvidencePattern::Contains(normalize_token(value.into())),
        )
    }

    /// Matches when the normalized raw email starts with the pattern.
    #[must_use]
    pub fn email_prefix(value: impl Into<String>) -> Self {
        Self::new(
            ContributorEvidenceField::Email,
            ContributorEvidencePattern::Prefix(normalize_token(value.into())),
        )
    }

    /// Matches when the normalized raw email contains the pattern.
    #[must_use]
    pub fn email_contains(value: impl Into<String>) -> Self {
        Self::new(
            ContributorEvidenceField::Email,
            ContributorEvidencePattern::Contains(normalize_token(value.into())),
        )
    }

    /// Matches when the normalized raw email ends with the pattern.
    #[must_use]
    pub fn email_suffix(value: impl Into<String>) -> Self {
        Self::new(
            ContributorEvidenceField::Email,
            ContributorEvidencePattern::Suffix(normalize_token(value.into())),
        )
    }

    fn new(field: ContributorEvidenceField, pattern: ContributorEvidencePattern) -> Self {
        Self { field, pattern }
    }

    fn matches(&self, evidence: &ContributorEvidence) -> bool {
        let value = match self.field {
            ContributorEvidenceField::Name => evidence.name(),
            ContributorEvidenceField::Email => evidence.email(),
        };
        self.pattern.matches(&normalize_token(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContributorEvidenceField {
    Name,
    Email,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ContributorEvidencePattern {
    Prefix(String),
    Contains(String),
    Suffix(String),
}

impl ContributorEvidencePattern {
    fn matches(&self, value: &str) -> bool {
        match self {
            Self::Prefix(pattern) => value.starts_with(pattern),
            Self::Contains(pattern) => value.contains(pattern),
            Self::Suffix(pattern) => value.ends_with(pattern),
        }
    }
}

/// Minimal Repository Ingestion result used by later adapter work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryIngestionSummary {
    replay: RepositoryReplay,
}

impl RepositoryIngestionSummary {
    /// Creates a Repository Ingestion summary.
    #[must_use]
    pub fn new(replay: RepositoryReplay) -> Self {
        Self { replay }
    }

    /// Returns the prepared Repository Replay.
    #[must_use]
    pub fn replay(&self) -> &RepositoryReplay {
        &self.replay
    }
}

/// Builds an empty Repository Replay shell for an explicit Mainline.
pub fn scaffold_repository_replay(
    request: &RepositoryIngestionRequest,
) -> Result<RepositoryIngestionSummary, RepositoryIngestionError> {
    let mainline = request.explicit_mainline().ok_or_else(|| {
        RepositoryIngestionError::new(
            "Mainline must be explicit before scaffolding Repository Replay",
        )
    })?;

    Ok(RepositoryIngestionSummary::new(RepositoryReplay::new(
        mainline.clone(),
    )))
}

/// Ingests a local Git repository into Repository Replay events.
pub fn ingest_repository(
    request: &RepositoryIngestionRequest,
) -> Result<RepositoryIngestionSummary, RepositoryIngestionError> {
    let git_repository = NativeGitRepository::open(request.repository_path())?;
    let mainline = git_repository.resolve_mainline(&request.mainline)?;
    let commit_plan = git_repository.commit_plan(&mainline)?;
    let mut replay = RepositoryReplay::new(mainline);
    let mut contributor_normalizer =
        ContributorNormalizer::new(request.contributor_normalization());

    for planned_commit in commit_plan {
        replay.push_commit_event(
            git_repository.commit_event(planned_commit, &mut contributor_normalizer)?,
        );
    }
    add_file_level_competing_changes(&mut replay);

    Ok(RepositoryIngestionSummary::new(replay))
}

/// Error produced while preparing Repository Replay data.
#[derive(Debug)]
pub struct RepositoryIngestionError {
    message: String,
}

impl RepositoryIngestionError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RepositoryIngestionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RepositoryIngestionError {}

impl From<git2::Error> for RepositoryIngestionError {
    fn from(error: git2::Error) -> Self {
        Self::new(error.message())
    }
}

struct NativeGitRepository {
    repository: Repository,
}

#[derive(Debug, Clone)]
struct PlannedCommit {
    id: Oid,
    branch_flow: BranchFlow,
}

#[derive(Debug, Clone)]
struct LocalBranchTip {
    name: String,
    id: Oid,
}

impl NativeGitRepository {
    fn open(path: &Path) -> Result<Self, RepositoryIngestionError> {
        Ok(Self {
            repository: Repository::open(path)?,
        })
    }

    fn resolve_mainline(
        &self,
        selection: &MainlineSelection,
    ) -> Result<Mainline, RepositoryIngestionError> {
        match selection {
            MainlineSelection::Explicit(mainline) => {
                self.mainline_tip(mainline)?;
                Ok(mainline.clone())
            }
            MainlineSelection::DetectFromLocalRefs => self.detect_mainline(),
        }
    }

    fn detect_mainline(&self) -> Result<Mainline, RepositoryIngestionError> {
        for candidate in ["main", "master"] {
            let mainline = Mainline::new(candidate);
            if self.mainline_tip(&mainline).is_ok() {
                return Ok(mainline);
            }
        }

        let head = self.repository.head()?;
        let branch = head.shorthand().ok_or_else(|| {
            RepositoryIngestionError::new("could not detect Mainline from detached HEAD")
        })?;

        Ok(Mainline::new(branch))
    }

    fn mainline_tip(&self, mainline: &Mainline) -> Result<Oid, RepositoryIngestionError> {
        let mainline_ref = format!("refs/heads/{}", mainline.as_str());
        self.repository
            .revparse_single(&mainline_ref)
            .map(|object| object.id())
            .map_err(|_| {
                RepositoryIngestionError::new(format!(
                    "requested Mainline '{}' was not found in local repository",
                    mainline.as_str()
                ))
            })
    }

    fn mainline_commit_ids(
        &self,
        mainline: &Mainline,
    ) -> Result<Vec<Oid>, RepositoryIngestionError> {
        let mainline_tip = self.mainline_tip(mainline)?;
        let mut walk = self.repository.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        walk.push(mainline_tip)?;

        let mut commit_ids = walk.collect::<Result<Vec<_>, _>>()?;
        commit_ids.reverse();
        Ok(commit_ids)
    }

    fn commit_plan(
        &self,
        mainline: &Mainline,
    ) -> Result<Vec<PlannedCommit>, RepositoryIngestionError> {
        let mainline_tip = self.mainline_tip(mainline)?;
        let mainline_commit_ids = self.mainline_commit_ids(mainline)?;
        let first_parent_ids = self.first_parent_commit_ids(mainline_tip)?;
        let branch_tips = self.local_branch_tips_except(mainline)?;
        let mut branch_flow_by_commit_id = HashMap::new();

        self.add_merge_settlements(
            mainline,
            &mainline_commit_ids,
            &branch_tips,
            &mut branch_flow_by_commit_id,
        )?;
        self.add_unmerged_branch_superpositions(
            mainline,
            mainline_tip,
            &first_parent_ids,
            &branch_tips,
            &mut branch_flow_by_commit_id,
        )?;

        let mut seen_commit_ids = HashSet::new();
        let mut planned_commits = Vec::new();
        for commit_id in mainline_commit_ids {
            seen_commit_ids.insert(commit_id);
            planned_commits.push(PlannedCommit {
                id: commit_id,
                branch_flow: branch_flow_by_commit_id
                    .remove(&commit_id)
                    .unwrap_or(BranchFlow::Mainline),
            });
        }

        let mut remaining_commit_ids = branch_flow_by_commit_id
            .keys()
            .copied()
            .filter(|commit_id| !seen_commit_ids.contains(commit_id))
            .collect::<Vec<_>>();
        remaining_commit_ids.sort_by_key(|commit_id| {
            self.repository
                .find_commit(*commit_id)
                .map(|commit| commit.time().seconds())
                .unwrap_or_default()
        });

        for commit_id in remaining_commit_ids {
            if let Some(branch_flow) = branch_flow_by_commit_id.remove(&commit_id) {
                planned_commits.push(PlannedCommit {
                    id: commit_id,
                    branch_flow,
                });
            }
        }

        Ok(planned_commits)
    }

    fn first_parent_commit_ids(&self, tip: Oid) -> Result<HashSet<Oid>, RepositoryIngestionError> {
        let mut commit_ids = HashSet::new();
        let mut next_commit_id = Some(tip);

        while let Some(commit_id) = next_commit_id {
            let commit = self.repository.find_commit(commit_id)?;
            commit_ids.insert(commit_id);
            next_commit_id = if commit.parent_count() == 0 {
                None
            } else {
                Some(commit.parent_id(0)?)
            };
        }

        Ok(commit_ids)
    }

    fn local_branch_tips_except(
        &self,
        mainline: &Mainline,
    ) -> Result<Vec<LocalBranchTip>, RepositoryIngestionError> {
        let mut branch_tips = Vec::new();
        for branch_result in self.repository.branches(Some(BranchType::Local))? {
            let (branch, _) = branch_result?;
            let Some(name) = branch.name()?.map(str::to_owned) else {
                continue;
            };
            if name == mainline.as_str() {
                continue;
            }
            let Some(id) = branch.get().target() else {
                continue;
            };
            branch_tips.push(LocalBranchTip { name, id });
        }
        branch_tips.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(branch_tips)
    }

    fn add_merge_settlements(
        &self,
        mainline: &Mainline,
        mainline_commit_ids: &[Oid],
        branch_tips: &[LocalBranchTip],
        branch_flow_by_commit_id: &mut HashMap<Oid, BranchFlow>,
    ) -> Result<(), RepositoryIngestionError> {
        for commit_id in mainline_commit_ids {
            let commit = self.repository.find_commit(*commit_id)?;
            if commit.parent_count() < 2 {
                continue;
            }

            let first_parent_id = commit.parent_id(0)?;
            for parent_index in 1..commit.parent_count() {
                let side_parent_id = commit.parent_id(parent_index)?;
                let settled_commit_ids =
                    self.commits_reachable_from_hiding(side_parent_id, first_parent_id)?;
                if settled_commit_ids.is_empty() {
                    continue;
                }

                let branch = self
                    .branch_name_for_side_parent(side_parent_id, branch_tips)?
                    .unwrap_or_else(|| format!("unresolved/{}", short_commit_id(side_parent_id)));
                let branch_flow = BranchFlow::BranchSuperposition {
                    branch: branch.clone(),
                    mainline: mainline.clone(),
                };
                for settled_commit_id in &settled_commit_ids {
                    branch_flow_by_commit_id
                        .entry(*settled_commit_id)
                        .or_insert_with(|| branch_flow.clone());
                }
                let settlement = MergeSettlement::new(
                    branch,
                    mainline.clone(),
                    settled_commit_ids
                        .iter()
                        .map(|commit_id| CommitId::new(commit_id.to_string()))
                        .collect(),
                );
                append_merge_settlement(branch_flow_by_commit_id, *commit_id, settlement);
            }
        }

        Ok(())
    }

    fn add_unmerged_branch_superpositions(
        &self,
        mainline: &Mainline,
        mainline_tip: Oid,
        first_parent_ids: &HashSet<Oid>,
        branch_tips: &[LocalBranchTip],
        branch_flow_by_commit_id: &mut HashMap<Oid, BranchFlow>,
    ) -> Result<(), RepositoryIngestionError> {
        for branch_tip in branch_tips {
            if self
                .repository
                .graph_descendant_of(mainline_tip, branch_tip.id)?
            {
                continue;
            }

            let branch_commit_ids =
                self.commits_reachable_from_hiding(branch_tip.id, mainline_tip)?;
            let branch_flow = BranchFlow::BranchSuperposition {
                branch: branch_tip.name.clone(),
                mainline: mainline.clone(),
            };
            for commit_id in branch_commit_ids {
                if first_parent_ids.contains(&commit_id) {
                    continue;
                }
                branch_flow_by_commit_id
                    .entry(commit_id)
                    .or_insert_with(|| branch_flow.clone());
            }
        }

        Ok(())
    }

    fn commits_reachable_from_hiding(
        &self,
        start: Oid,
        hide: Oid,
    ) -> Result<Vec<Oid>, RepositoryIngestionError> {
        let mut walk = self.repository.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        walk.push(start)?;
        walk.hide(hide)?;

        let mut commit_ids = walk.collect::<Result<Vec<_>, _>>()?;
        commit_ids.reverse();
        Ok(commit_ids)
    }

    fn branch_name_for_side_parent(
        &self,
        side_parent_id: Oid,
        branch_tips: &[LocalBranchTip],
    ) -> Result<Option<String>, RepositoryIngestionError> {
        for branch_tip in branch_tips {
            if branch_tip.id == side_parent_id
                || self
                    .repository
                    .graph_descendant_of(branch_tip.id, side_parent_id)?
            {
                return Ok(Some(branch_tip.name.clone()));
            }
        }

        Ok(None)
    }

    fn commit_event(
        &self,
        planned_commit: PlannedCommit,
        contributor_normalizer: &mut ContributorNormalizer<'_>,
    ) -> Result<CommitEvent, RepositoryIngestionError> {
        let commit = self.repository.find_commit(planned_commit.id)?;
        let parent_ids = commit
            .parents()
            .map(|parent| CommitId::new(parent.id().to_string()))
            .collect();
        let author = evidence_from_signature(&commit.author());
        let committer = evidence_from_signature(&commit.committer());
        let contributor = contributor_normalizer.contributor_for(&author, &committer);
        let file_changes = self.file_changes_for_commit(&commit)?;

        let evidence = CommitEvidence::new(
            CommitId::new(commit.id().to_string()),
            CommitSubject::new(commit.summary().unwrap_or_default()),
            author,
            committer,
            contributor,
        )
        .with_parent_ids(parent_ids)
        .with_branch_flow(planned_commit.branch_flow)
        .with_file_changes(file_changes);

        Ok(CommitEvent::from_evidence(evidence))
    }

    fn file_changes_for_commit(
        &self,
        commit: &git2::Commit<'_>,
    ) -> Result<Vec<FileChange>, RepositoryIngestionError> {
        let new_tree = commit.tree()?;
        // Merge commits currently report File Changes relative to the first parent.
        let old_tree = if commit.parent_count() == 0 {
            None
        } else {
            Some(commit.parent(0)?.tree()?)
        };
        let mut options = DiffOptions::new();
        let mut diff = self.repository.diff_tree_to_tree(
            old_tree.as_ref(),
            Some(&new_tree),
            Some(&mut options),
        )?;
        let mut find_options = DiffFindOptions::new();
        find_options.renames(true);
        diff.find_similar(Some(&mut find_options))?;

        diff.deltas().map(file_change_from_delta).collect()
    }
}

fn short_commit_id(commit_id: Oid) -> String {
    commit_id.to_string().chars().take(12).collect()
}

fn append_merge_settlement(
    branch_flow_by_commit_id: &mut HashMap<Oid, BranchFlow>,
    commit_id: Oid,
    settlement: MergeSettlement,
) {
    match branch_flow_by_commit_id.get_mut(&commit_id) {
        Some(BranchFlow::MergeSettlements(settlements)) => settlements.push(settlement),
        _ => {
            branch_flow_by_commit_id
                .insert(commit_id, BranchFlow::MergeSettlements(vec![settlement]));
        }
    }
}

fn add_file_level_competing_changes(replay: &mut RepositoryReplay) {
    let mut evidence_by_path: BTreeMap<PathBuf, Vec<CompetingChangeEvidence>> = BTreeMap::new();
    let mut branches_by_path: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();

    for event in replay.commit_events() {
        let BranchFlow::BranchSuperposition { branch, .. } = event.branch_flow() else {
            continue;
        };

        for file_change in event.file_changes() {
            let path = file_change.entity().path().clone();
            branches_by_path
                .entry(path.clone())
                .or_default()
                .insert(branch.clone());
            evidence_by_path
                .entry(path)
                .or_default()
                .push(CompetingChangeEvidence::new(
                    branch.clone(),
                    event.id().clone(),
                ));
        }
    }

    for (path, branch_names) in branches_by_path {
        if branch_names.len() < 2 {
            continue;
        }
        if let Some(evidence) = evidence_by_path.remove(&path) {
            replay.push_competing_change(CompetingChange::new(
                RepositoryEntity::new(path),
                CompetingChangeSource::FileLevelOverlap,
                CompetingChangeConfidence::Medium,
                evidence,
            ));
        }
    }
}

struct ContributorNormalizer<'a> {
    rules: &'a ContributorNormalizationRules,
    contributors_by_key: HashMap<String, Contributor>,
}

impl<'a> ContributorNormalizer<'a> {
    fn new(rules: &'a ContributorNormalizationRules) -> Self {
        Self {
            rules,
            contributors_by_key: HashMap::new(),
        }
    }

    fn contributor_for(
        &mut self,
        author: &ContributorEvidence,
        committer: &ContributorEvidence,
    ) -> Contributor {
        let canonical_evidence =
            if self.rules.contributor_kind(committer) == ContributorKind::Automation {
                committer
            } else {
                author
            };
        let identity_key = self.rules.canonical_key(canonical_evidence);
        if let Some(contributor) = self.contributors_by_key.get(&identity_key) {
            return contributor.clone();
        }

        let display_name = display_name_from_evidence(canonical_evidence);
        let contributor = match self.rules.contributor_kind(canonical_evidence) {
            ContributorKind::Human => Contributor::normalized_human(display_name, &identity_key),
            ContributorKind::Automation => {
                Contributor::normalized_automation(display_name, &identity_key)
            }
        };
        self.contributors_by_key
            .insert(identity_key, contributor.clone());
        contributor
    }
}

fn display_name_from_evidence(evidence: &ContributorEvidence) -> String {
    if evidence.name().trim().is_empty() {
        evidence.email().trim().to_owned()
    } else {
        evidence.name().trim().to_owned()
    }
}

fn normalize_token(value: impl AsRef<str>) -> String {
    value.as_ref().trim().to_lowercase()
}

fn evidence_from_signature(signature: &git2::Signature<'_>) -> ContributorEvidence {
    let time = signature.when();
    ContributorEvidence::new(
        signature.name().unwrap_or_default(),
        signature.email().unwrap_or_default(),
        GitTimestamp::new(time.seconds(), time.offset_minutes()),
    )
}

fn file_change_from_delta(
    delta: git2::DiffDelta<'_>,
) -> Result<FileChange, RepositoryIngestionError> {
    if delta.status() == Delta::Renamed {
        let from = delta
            .old_file()
            .path()
            .ok_or_else(|| RepositoryIngestionError::new("rename is missing source path"))?;
        let to = delta
            .new_file()
            .path()
            .ok_or_else(|| RepositoryIngestionError::new("rename is missing destination path"))?;
        return Ok(FileChange::moved(
            RepositoryEntity::new(from),
            RepositoryEntity::new(to),
        ));
    }

    let kind = match delta.status() {
        Delta::Added | Delta::Untracked | Delta::Copied => FileChangeKind::Added,
        Delta::Deleted => FileChangeKind::Deleted,
        Delta::Modified | Delta::Typechange => FileChangeKind::Modified,
        Delta::Renamed => unreachable!("renames are handled with source and destination paths"),
        Delta::Unmodified | Delta::Ignored | Delta::Unreadable | Delta::Conflicted => {
            return Err(RepositoryIngestionError::new(format!(
                "unsupported file change status {:?}",
                delta.status()
            )));
        }
    };

    let path = match kind {
        FileChangeKind::Deleted => delta.old_file().path(),
        _ => delta.new_file().path(),
    }
    .ok_or_else(|| RepositoryIngestionError::new("file change is missing path evidence"))?;

    Ok(FileChange::new(RepositoryEntity::new(path), kind))
}

#[cfg(test)]
mod tests {
    use super::{ingest_repository, scaffold_repository_replay, RepositoryIngestionRequest};
    use super::{AutomationContributorRule, ContributorNormalizationRules};
    use gitflux_scene::{
        BranchFlow, CommitId, CompetingChangeConfidence, CompetingChangeSource, ContributorKind,
        FileChangeKind, Mainline,
    };
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn scaffold_uses_requested_mainline() {
        let request = RepositoryIngestionRequest::new(".", Mainline::new("trunk"));

        let summary =
            scaffold_repository_replay(&request).expect("explicit Mainline can scaffold replay");

        assert_eq!(summary.replay().mainline().as_str(), "trunk");
    }

    #[test]
    fn detected_mainline_request_does_not_expose_auto_sentinel() {
        let request = RepositoryIngestionRequest::detect_mainline(".");

        let error = scaffold_repository_replay(&request)
            .expect_err("detected Mainline needs repository resolution");

        assert_eq!(request.explicit_mainline(), None);
        assert!(!error.to_string().contains("auto"));
        assert_eq!(
            error.to_string(),
            "Mainline must be explicit before scaffolding Repository Replay"
        );
    }

    #[test]
    fn ingests_linear_create_modify_history_into_commit_events() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("src/lib.rs", "pub fn answer() -> u8 { 41 }\n");
        let first_commit = fixture.commit("Add library", "Ada Lovelace", "ada@example.test");
        fixture.write_file("src/lib.rs", "pub fn answer() -> u8 { 42 }\n");
        let second_commit = fixture.commit("Update answer", "Grace Hopper", "grace@example.test");

        let request = RepositoryIngestionRequest::new(fixture.path(), Mainline::new("main"));

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let events = summary.replay().commit_events();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id().as_str(), first_commit);
        assert!(events[0].parent_ids().is_empty());
        assert_eq!(events[0].subject().as_str(), "Add library");
        assert_eq!(events[0].author().name(), "Ada Lovelace");
        assert_eq!(events[0].author().email(), "ada@example.test");
        assert_eq!(events[0].committer().name(), "Ada Lovelace");
        assert_eq!(events[0].committer().email(), "ada@example.test");
        assert_eq!(events[0].authored_at().seconds(), 1_704_164_645);
        assert_eq!(events[0].authored_at().offset_minutes(), 0);
        assert_eq!(events[0].committed_at().seconds(), 1_704_164_645);
        assert_eq!(events[0].committed_at().offset_minutes(), 0);
        assert_eq!(events[0].file_changes().len(), 1);
        assert_eq!(
            events[0].file_changes()[0].entity().path(),
            Path::new("src/lib.rs")
        );
        assert_eq!(*events[0].file_changes()[0].kind(), FileChangeKind::Added);

        assert_eq!(events[1].id().as_str(), second_commit);
        assert_eq!(events[1].parent_ids()[0].as_str(), first_commit);
        assert_eq!(events[1].subject().as_str(), "Update answer");
        assert_eq!(events[1].author().name(), "Grace Hopper");
        assert_eq!(events[1].author().email(), "grace@example.test");
        assert_eq!(events[1].file_changes().len(), 1);
        assert_eq!(
            events[1].file_changes()[0].entity().path(),
            Path::new("src/lib.rs")
        );
        assert_eq!(
            *events[1].file_changes()[0].kind(),
            FileChangeKind::Modified
        );
    }

    #[test]
    fn ingests_rename_and_delete_file_changes_with_path_evidence() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("README.md", "Gitflux\n");
        fixture.commit("Add readme", "Ada Lovelace", "ada@example.test");
        fixture.rename_file("README.md", "docs/README.md");
        let rename_commit = fixture.commit("Move readme", "Ada Lovelace", "ada@example.test");
        fixture.remove_file("docs/README.md");
        let delete_commit = fixture.commit("Delete readme", "Ada Lovelace", "ada@example.test");

        let request = RepositoryIngestionRequest::new(fixture.path(), Mainline::new("main"));

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let events = summary.replay().commit_events();

        assert_eq!(events.len(), 3);
        assert_eq!(events[1].id().as_str(), rename_commit);
        assert_eq!(events[1].file_changes().len(), 1);
        let rename = &events[1].file_changes()[0];
        assert_eq!(*rename.kind(), FileChangeKind::Moved);
        assert_eq!(
            rename
                .previous_entity()
                .expect("rename should preserve source path")
                .path(),
            Path::new("README.md")
        );
        assert_eq!(rename.entity().path(), Path::new("docs/README.md"));

        assert_eq!(events[2].id().as_str(), delete_commit);
        assert_eq!(events[2].file_changes().len(), 1);
        assert_eq!(*events[2].file_changes()[0].kind(), FileChangeKind::Deleted);
        assert_eq!(
            events[2].file_changes()[0].entity().path(),
            Path::new("docs/README.md")
        );
    }

    #[test]
    fn missing_requested_mainline_returns_clear_error_without_falling_back_to_head() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("README.md", "Gitflux\n");
        fixture.commit("Add readme", "Ada Lovelace", "ada@example.test");
        let request = RepositoryIngestionRequest::new(fixture.path(), Mainline::new("release"));

        let error = ingest_repository(&request)
            .expect_err("missing requested Mainline must not ingest HEAD");

        assert_eq!(
            error.to_string(),
            "requested Mainline 'release' was not found in local repository"
        );
    }

    #[test]
    fn detects_mainline_from_local_refs_and_keeps_explicit_override() {
        let fixture = GeneratedGitRepository::new_on_branch("trunk");
        fixture.write_file("README.md", "Gitflux\n");
        fixture.commit("Add readme", "Ada Lovelace", "ada@example.test");
        fixture.create_branch("main");
        fixture.create_branch("release");

        let detected_request = RepositoryIngestionRequest::detect_mainline(fixture.path());
        let detected_summary =
            ingest_repository(&detected_request).expect("local main ref should be detected");

        assert_eq!(detected_summary.replay().mainline().as_str(), "main");

        let override_request =
            RepositoryIngestionRequest::new(fixture.path(), Mainline::new("release"));
        let override_summary =
            ingest_repository(&override_request).expect("explicit Mainline should win");

        assert_eq!(override_summary.replay().mainline().as_str(), "release");
    }

    #[test]
    fn represents_unmerged_branch_work_as_branch_superposition() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("README.md", "Gitflux\n");
        fixture.commit("Add readme", "Ada Lovelace", "ada@example.test");
        fixture.create_and_checkout_branch("feature/search");
        fixture.write_file("src/search.rs", "pub fn search() {}\n");
        let branch_commit = fixture.commit("Add search", "Grace Hopper", "grace@example.test");
        fixture.checkout_branch("main");

        let request = RepositoryIngestionRequest::detect_mainline(fixture.path());

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let branch_event = summary
            .replay()
            .commit_events()
            .iter()
            .find(|event| event.id().as_str() == branch_commit)
            .expect("unmerged branch Commit Event should be present");

        assert_eq!(
            branch_event.branch_flow(),
            &BranchFlow::BranchSuperposition {
                branch: "feature/search".to_owned(),
                mainline: Mainline::new("main")
            }
        );
        assert_eq!(branch_event.file_changes().len(), 1);
        assert_eq!(
            branch_event.file_changes()[0].entity().path(),
            Path::new("src/search.rs")
        );
    }

    #[test]
    fn detects_file_level_competing_change_across_branch_superpositions() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("README.md", "Gitflux\n");
        fixture.commit("Add readme", "Ada Lovelace", "ada@example.test");
        fixture.create_and_checkout_branch("feature/search");
        fixture.write_file("README.md", "Gitflux\nSearch branch\n");
        let search_commit = fixture.commit("Add search note", "Grace Hopper", "grace@example.test");
        fixture.checkout_branch("main");
        fixture.create_and_checkout_branch("feature/export");
        fixture.write_file("README.md", "Gitflux\nExport branch\n");
        let export_commit = fixture.commit("Add export note", "Alan Turing", "alan@example.test");
        fixture.checkout_branch("main");

        let request = RepositoryIngestionRequest::detect_mainline(fixture.path());

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let competing_changes = summary.replay().competing_changes();

        assert_eq!(competing_changes.len(), 1);
        let competing_change = &competing_changes[0];
        assert_eq!(competing_change.entity().path(), Path::new("README.md"));
        assert_eq!(
            competing_change.source(),
            CompetingChangeSource::FileLevelOverlap
        );
        assert_eq!(
            competing_change.confidence(),
            CompetingChangeConfidence::Medium
        );
        assert_eq!(competing_change.evidence().len(), 2);
        assert!(competing_change.evidence().iter().any(|evidence| {
            evidence.branch() == "feature/search" && evidence.commit_id().as_str() == search_commit
        }));
        assert!(competing_change.evidence().iter().any(|evidence| {
            evidence.branch() == "feature/export" && evidence.commit_id().as_str() == export_commit
        }));
    }

    #[test]
    fn omits_competing_change_for_non_overlapping_branch_superpositions() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("README.md", "Gitflux\n");
        fixture.commit("Add readme", "Ada Lovelace", "ada@example.test");
        fixture.create_and_checkout_branch("feature/search");
        fixture.write_file("src/search.rs", "pub fn search() {}\n");
        fixture.commit("Add search", "Grace Hopper", "grace@example.test");
        fixture.checkout_branch("main");
        fixture.create_and_checkout_branch("feature/export");
        fixture.write_file("src/export.rs", "pub fn export() {}\n");
        fixture.commit("Add export", "Alan Turing", "alan@example.test");
        fixture.checkout_branch("main");

        let request = RepositoryIngestionRequest::detect_mainline(fixture.path());

        let summary = ingest_repository(&request).expect("fixture repository should ingest");

        assert!(summary.replay().competing_changes().is_empty());
    }

    #[test]
    fn marks_metadata_only_merge_commit_as_merge_settlement_without_file_changes() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("README.md", "Gitflux\n");
        fixture.commit("Add readme", "Ada Lovelace", "ada@example.test");
        fixture.create_and_checkout_branch("feature/search");
        let branch_commit =
            fixture.empty_commit("Record search branch", "Grace Hopper", "grace@example.test");
        fixture.checkout_branch("main");
        fixture.merge_no_ff("feature/search", "Merge search");
        let merge_commit = fixture.git(["rev-parse", "HEAD"]).trim().to_owned();

        let request = RepositoryIngestionRequest::detect_mainline(fixture.path());

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let events = summary.replay().commit_events();
        let branch_event = events
            .iter()
            .find(|event| event.id().as_str() == branch_commit)
            .expect("merged branch Commit Event should be present");
        let settlement_event = events
            .iter()
            .find(|event| event.id().as_str() == merge_commit)
            .expect("merge Commit Event should be present");

        assert_eq!(
            branch_event.branch_flow(),
            &BranchFlow::BranchSuperposition {
                branch: "feature/search".to_owned(),
                mainline: Mainline::new("main")
            }
        );
        assert_eq!(
            settlement_event.branch_flow(),
            &BranchFlow::MergeSettlements(vec![gitflux_scene::MergeSettlement::new(
                "feature/search",
                Mainline::new("main"),
                vec![CommitId::new(branch_commit)]
            )])
        );
        assert!(
            settlement_event.file_changes().is_empty(),
            "metadata-only Merge Settlement should not duplicate File Changes"
        );
    }

    #[test]
    fn keeps_file_changes_on_merge_settlement_with_manual_resolution() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("README.md", "Gitflux\n");
        fixture.commit("Add readme", "Ada Lovelace", "ada@example.test");
        fixture.create_and_checkout_branch("feature/search");
        fixture.write_file("README.md", "Gitflux\nSearch branch\n");
        let branch_commit = fixture.commit("Add search note", "Grace Hopper", "grace@example.test");
        fixture.checkout_branch("main");
        fixture.write_file("README.md", "Gitflux\nMainline note\n");
        fixture.commit("Add mainline note", "Ada Lovelace", "ada@example.test");
        fixture.merge_no_commit("feature/search");
        fixture.write_file(
            "README.md",
            "Gitflux\nMainline note\nSearch branch\nResolved\n",
        );
        let merge_commit = fixture.commit(
            "Merge search with resolution",
            "Merge Bot",
            "merge@example.test",
        );

        let request = RepositoryIngestionRequest::detect_mainline(fixture.path());

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let settlement_event = summary
            .replay()
            .commit_events()
            .iter()
            .find(|event| event.id().as_str() == merge_commit)
            .expect("merge Commit Event should be present");

        assert_eq!(
            settlement_event.branch_flow(),
            &BranchFlow::MergeSettlements(vec![gitflux_scene::MergeSettlement::new(
                "feature/search",
                Mainline::new("main"),
                vec![CommitId::new(branch_commit)]
            )])
        );
        assert_eq!(settlement_event.file_changes().len(), 1);
        assert_eq!(
            settlement_event.file_changes()[0].entity().path(),
            Path::new("README.md")
        );
        assert_eq!(
            *settlement_event.file_changes()[0].kind(),
            FileChangeKind::Modified
        );
    }

    #[test]
    fn preserves_all_settlements_on_octopus_merge() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("README.md", "Gitflux\n");
        fixture.commit("Add readme", "Ada Lovelace", "ada@example.test");
        fixture.create_and_checkout_branch("feature/search");
        fixture.write_file("src/search.rs", "pub fn search() {}\n");
        let search_commit = fixture.commit("Add search", "Grace Hopper", "grace@example.test");
        fixture.checkout_branch("main");
        fixture.create_and_checkout_branch("feature/export");
        fixture.write_file("src/export.rs", "pub fn export() {}\n");
        let export_commit = fixture.commit("Add export", "Alan Turing", "alan@example.test");
        fixture.checkout_branch("main");
        fixture.merge_octopus(["feature/search", "feature/export"], "Merge features");
        let merge_commit = fixture.git(["rev-parse", "HEAD"]).trim().to_owned();

        let request = RepositoryIngestionRequest::detect_mainline(fixture.path());

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let settlement_event = summary
            .replay()
            .commit_events()
            .iter()
            .find(|event| event.id().as_str() == merge_commit)
            .expect("octopus merge Commit Event should be present");

        assert_eq!(
            settlement_event.branch_flow(),
            &BranchFlow::MergeSettlements(vec![
                gitflux_scene::MergeSettlement::new(
                    "feature/search",
                    Mainline::new("main"),
                    vec![CommitId::new(search_commit)]
                ),
                gitflux_scene::MergeSettlement::new(
                    "feature/export",
                    Mainline::new("main"),
                    vec![CommitId::new(export_commit)]
                )
            ])
        );
    }

    #[test]
    fn aliases_author_variants_with_same_normalized_email_into_one_contributor() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("src/lib.rs", "pub fn answer() -> u8 { 41 }\n");
        fixture.commit("Add library", "Stewart Boling", "stewart@example.test");
        fixture.write_file("src/lib.rs", "pub fn answer() -> u8 { 42 }\n");
        fixture.commit("Update answer", "Stewart B.", "STEWART@EXAMPLE.TEST");

        let request = RepositoryIngestionRequest::new(fixture.path(), Mainline::new("main"));

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let events = summary.replay().commit_events();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].contributor(), events[1].contributor());
        assert_eq!(events[0].contributor().display_name(), "Stewart Boling");
        assert_eq!(events[0].contributor().kind(), ContributorKind::Human);
        assert_eq!(events[1].author().name(), "Stewart B.");
        assert_eq!(events[1].author().email(), "STEWART@EXAMPLE.TEST");
    }

    #[test]
    fn explicit_email_alias_rule_merges_different_author_emails_conservatively() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("src/lib.rs", "pub fn answer() -> u8 { 41 }\n");
        fixture.commit(
            "Add library",
            "Stewart Boling",
            "stewart@users.noreply.github.com",
        );
        fixture.write_file("src/lib.rs", "pub fn answer() -> u8 { 42 }\n");
        fixture.commit("Update answer", "Stewart Boling", "stewart@example.test");
        let normalization = ContributorNormalizationRules::default()
            .with_email_alias("stewart@example.test", "stewart@users.noreply.github.com");

        let request = RepositoryIngestionRequest::new(fixture.path(), Mainline::new("main"))
            .with_contributor_normalization(normalization);

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let events = summary.replay().commit_events();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].contributor(), events[1].contributor());
        assert_eq!(events[1].author().email(), "stewart@example.test");
    }

    #[test]
    fn keeps_same_named_authors_with_different_emails_as_split_contributors() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("src/lib.rs", "pub fn ada() -> u8 { 1 }\n");
        fixture.commit("Add Ada work", "Alex Kim", "alex@personal.example");
        fixture.write_file("src/grace.rs", "pub fn grace() -> u8 { 2 }\n");
        fixture.commit("Add Grace work", "Alex Kim", "alex@work.example");

        let request = RepositoryIngestionRequest::new(fixture.path(), Mainline::new("main"));

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let events = summary.replay().commit_events();

        assert_eq!(events.len(), 2);
        assert_ne!(events[0].contributor(), events[1].contributor());
        assert_eq!(events[0].contributor().display_name(), "Alex Kim");
        assert_eq!(events[1].contributor().display_name(), "Alex Kim");
        assert_eq!(events[0].contributor().kind(), ContributorKind::Human);
        assert_eq!(events[1].contributor().kind(), ContributorKind::Human);
    }

    #[test]
    fn detects_automation_contributors_from_service_account_evidence() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("Cargo.toml", "[package]\nname = \"fixture\"\n");
        fixture.commit(
            "Update dependency",
            "dependabot[bot]",
            "49699333+dependabot[bot]@users.noreply.github.com",
        );

        let request = RepositoryIngestionRequest::new(fixture.path(), Mainline::new("main"));

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let event = &summary.replay().commit_events()[0];

        assert_eq!(event.contributor().display_name(), "dependabot[bot]");
        assert_eq!(event.contributor().kind(), ContributorKind::Automation);
        assert_eq!(event.author().name(), "dependabot[bot]");
        assert_eq!(
            event.author().email(),
            "49699333+dependabot[bot]@users.noreply.github.com"
        );
    }

    #[test]
    fn automation_committer_evidence_classifies_commit_event_as_automation_contributor() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("src/lib.rs", "pub fn generated() -> bool { true }\n");
        fixture.commit_with_author_and_committer(
            "Regenerate bindings",
            ("Ada Lovelace", "ada@example.test"),
            (
                "github-actions[bot]",
                "41898282+github-actions[bot]@users.noreply.github.com",
            ),
        );

        let request = RepositoryIngestionRequest::new(fixture.path(), Mainline::new("main"));

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let event = &summary.replay().commit_events()[0];

        assert_eq!(event.contributor().display_name(), "github-actions[bot]");
        assert_eq!(event.contributor().kind(), ContributorKind::Automation);
        assert_eq!(event.author().name(), "Ada Lovelace");
        assert_eq!(event.author().email(), "ada@example.test");
        assert_eq!(event.committer().name(), "github-actions[bot]");
        assert_eq!(
            event.committer().email(),
            "41898282+github-actions[bot]@users.noreply.github.com"
        );
    }

    #[test]
    fn supplied_empty_automation_rules_disable_default_automation_detection() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("Cargo.toml", "[package]\nname = \"fixture\"\n");
        fixture.commit(
            "Update dependency",
            "dependabot[bot]",
            "49699333+dependabot[bot]@users.noreply.github.com",
        );
        let normalization = ContributorNormalizationRules::default().with_automation_rules([]);

        let request = RepositoryIngestionRequest::new(fixture.path(), Mainline::new("main"))
            .with_contributor_normalization(normalization);

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let event = &summary.replay().commit_events()[0];

        assert_eq!(event.contributor().display_name(), "dependabot[bot]");
        assert_eq!(event.contributor().kind(), ContributorKind::Human);
    }

    #[test]
    fn supplied_automation_rules_replace_defaults_and_match_only_custom_rules() {
        let fixture = GeneratedGitRepository::new();
        fixture.write_file("Cargo.toml", "[package]\nname = \"fixture\"\n");
        fixture.commit("Run release task", "Release Worker", "release@ci.example");
        let normalization = ContributorNormalizationRules::default()
            .with_automation_rules([AutomationContributorRule::email_suffix("@ci.example")]);

        let request = RepositoryIngestionRequest::new(fixture.path(), Mainline::new("main"))
            .with_contributor_normalization(normalization);

        let summary = ingest_repository(&request).expect("fixture repository should ingest");
        let event = &summary.replay().commit_events()[0];

        assert_eq!(event.contributor().display_name(), "Release Worker");
        assert_eq!(event.contributor().kind(), ContributorKind::Automation);
    }

    struct GeneratedGitRepository {
        temp_dir: TempDir,
    }

    impl GeneratedGitRepository {
        fn new() -> Self {
            Self::new_on_branch("main")
        }

        fn new_on_branch(initial_branch: &str) -> Self {
            let temp_dir = tempfile::tempdir().expect("fixture tempdir should be created");
            let fixture = Self { temp_dir };
            fixture.git(["init", "--initial-branch", initial_branch]);
            fixture
        }

        fn path(&self) -> &Path {
            self.temp_dir.path()
        }

        fn write_file(&self, path: &str, contents: &str) {
            let path = self.path().join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("fixture parent directory should be created");
            }
            fs::write(path, contents).expect("fixture file should be written");
        }

        fn rename_file(&self, from: &str, to: &str) {
            if let Some(parent) = self.path().join(to).parent() {
                fs::create_dir_all(parent).expect("fixture parent directory should be created");
            }
            self.git(["mv", from, to]);
        }

        fn remove_file(&self, path: &str) {
            self.git(["rm", path]);
        }

        fn create_branch(&self, branch: &str) {
            self.git(["branch", branch]);
        }

        fn create_and_checkout_branch(&self, branch: &str) {
            self.git(["checkout", "-b", branch]);
        }

        fn checkout_branch(&self, branch: &str) {
            self.git(["checkout", branch]);
        }

        fn merge_no_ff(&self, branch: &str, subject: &str) {
            self.git_with_identity(
                ["merge", "--no-ff", branch, "-m", subject],
                "Merge Bot",
                "merge@example.test",
            );
        }

        fn merge_no_commit(&self, branch: &str) {
            let output = self.run_git_allow_failure(
                ["merge", "--no-ff", "--no-commit", branch],
                Some(GitIdentity {
                    author_name: "Merge Bot",
                    author_email: "merge@example.test",
                    committer_name: "Merge Bot",
                    committer_email: "merge@example.test",
                }),
            );
            assert!(
                !output.status.success(),
                "fixture merge should stop for manual resolution"
            );
        }

        fn merge_octopus<const N: usize>(&self, branches: [&str; N], subject: &str) {
            let mut args = vec!["merge", "--no-ff"];
            args.extend(branches);
            args.extend(["-m", subject]);
            self.git_vec_with_identity(args, "Merge Bot", "merge@example.test");
        }

        fn commit(&self, subject: &str, name: &str, email: &str) -> String {
            self.git(["add", "."]);
            self.git_with_identity(["commit", "-m", subject], name, email);
            self.git(["rev-parse", "HEAD"]).trim().to_owned()
        }

        fn commit_with_author_and_committer(
            &self,
            subject: &str,
            author: (&str, &str),
            committer: (&str, &str),
        ) -> String {
            self.git(["add", "."]);
            self.run_git(
                ["commit", "-m", subject],
                Some(GitIdentity {
                    author_name: author.0,
                    author_email: author.1,
                    committer_name: committer.0,
                    committer_email: committer.1,
                }),
            );
            self.git(["rev-parse", "HEAD"]).trim().to_owned()
        }

        fn empty_commit(&self, subject: &str, name: &str, email: &str) -> String {
            self.git_with_identity(["commit", "--allow-empty", "-m", subject], name, email);
            self.git(["rev-parse", "HEAD"]).trim().to_owned()
        }

        fn git<const N: usize>(&self, args: [&str; N]) -> String {
            self.run_git(args, None)
        }

        fn git_with_identity<const N: usize>(
            &self,
            args: [&str; N],
            name: &str,
            email: &str,
        ) -> String {
            self.run_git(
                args,
                Some(GitIdentity {
                    author_name: name,
                    author_email: email,
                    committer_name: name,
                    committer_email: email,
                }),
            )
        }

        fn git_vec_with_identity(&self, args: Vec<&str>, name: &str, email: &str) -> String {
            self.run_git_vec(
                args,
                Some(GitIdentity {
                    author_name: name,
                    author_email: email,
                    committer_name: name,
                    committer_email: email,
                }),
            )
        }

        fn run_git<const N: usize>(
            &self,
            args: [&str; N],
            identity: Option<GitIdentity<'_>>,
        ) -> String {
            self.run_git_vec(args.to_vec(), identity)
        }

        fn run_git_vec(&self, args: Vec<&str>, identity: Option<GitIdentity<'_>>) -> String {
            let mut command = Command::new("git");
            command
                .args(&args)
                .current_dir(self.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_DATE", "2024-01-02T03:04:05Z")
                .env("GIT_COMMITTER_DATE", "2024-01-02T03:04:05Z");

            if let Some(identity) = identity {
                command
                    .env("GIT_AUTHOR_NAME", identity.author_name)
                    .env("GIT_AUTHOR_EMAIL", identity.author_email)
                    .env("GIT_COMMITTER_NAME", identity.committer_name)
                    .env("GIT_COMMITTER_EMAIL", identity.committer_email);
            }

            let output = command.output().expect("git fixture command should run");
            assert!(
                output.status.success(),
                "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
                args,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).expect("git fixture output should be utf-8")
        }

        fn run_git_allow_failure<const N: usize>(
            &self,
            args: [&str; N],
            identity: Option<GitIdentity<'_>>,
        ) -> std::process::Output {
            let mut command = Command::new("git");
            command
                .args(args)
                .current_dir(self.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_DATE", "2024-01-02T03:04:05Z")
                .env("GIT_COMMITTER_DATE", "2024-01-02T03:04:05Z");

            if let Some(identity) = identity {
                command
                    .env("GIT_AUTHOR_NAME", identity.author_name)
                    .env("GIT_AUTHOR_EMAIL", identity.author_email)
                    .env("GIT_COMMITTER_NAME", identity.committer_name)
                    .env("GIT_COMMITTER_EMAIL", identity.committer_email);
            }

            command.output().expect("git fixture command should run")
        }
    }

    struct GitIdentity<'a> {
        author_name: &'a str,
        author_email: &'a str,
        committer_name: &'a str,
        committer_email: &'a str,
    }
}
