//! Repository Ingestion seam for Gitflux.
//!
//! This crate will own Rust-native preparation of Git history into the
//! Repository Replay timeline. The current scaffold defines the public request
//! and summary types without choosing the concrete Git library integration.

use std::path::{Path, PathBuf};

use git2::{Delta, DiffFindOptions, DiffOptions, Oid, Repository, Sort};
use gitflux_scene::{
    CommitEvent, CommitEvidence, CommitId, CommitSubject, Contributor, ContributorEvidence,
    FileChange, FileChangeKind, GitTimestamp, Mainline, RepositoryEntity, RepositoryReplay,
};

/// Input for preparing Git history into a Repository Replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryIngestionRequest {
    repository_path: PathBuf,
    mainline: Mainline,
}

impl RepositoryIngestionRequest {
    /// Creates a Repository Ingestion request.
    #[must_use]
    pub fn new(repository_path: impl Into<PathBuf>, mainline: Mainline) -> Self {
        Self {
            repository_path: repository_path.into(),
            mainline,
        }
    }

    /// Returns the repository path to ingest.
    #[must_use]
    pub fn repository_path(&self) -> &Path {
        &self.repository_path
    }

    /// Returns the Mainline for replay settlement.
    #[must_use]
    pub fn mainline(&self) -> &Mainline {
        &self.mainline
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

/// Builds an empty Repository Replay shell for the requested Mainline.
#[must_use]
pub fn scaffold_repository_replay(
    request: &RepositoryIngestionRequest,
) -> RepositoryIngestionSummary {
    RepositoryIngestionSummary::new(RepositoryReplay::new(request.mainline().clone()))
}

/// Ingests a local Git repository into Repository Replay events.
pub fn ingest_repository(
    request: &RepositoryIngestionRequest,
) -> Result<RepositoryIngestionSummary, RepositoryIngestionError> {
    let git_repository = NativeGitRepository::open(request.repository_path())?;
    let commit_ids = git_repository.mainline_commit_ids(request.mainline())?;
    let mut replay = RepositoryReplay::new(request.mainline().clone());

    for commit_id in commit_ids {
        replay.push_commit_event(git_repository.commit_event(commit_id)?);
    }

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

impl NativeGitRepository {
    fn open(path: &Path) -> Result<Self, RepositoryIngestionError> {
        Ok(Self {
            repository: Repository::open(path)?,
        })
    }

    fn mainline_commit_ids(
        &self,
        mainline: &Mainline,
    ) -> Result<Vec<Oid>, RepositoryIngestionError> {
        let mainline_ref = format!("refs/heads/{}", mainline.as_str());
        let mainline_object = self
            .repository
            .revparse_single(&mainline_ref)
            .map_err(|_| {
                RepositoryIngestionError::new(format!(
                    "requested Mainline '{}' was not found in local repository",
                    mainline.as_str()
                ))
            })?;
        let mut walk = self.repository.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        walk.push(mainline_object.id())?;

        let mut commit_ids = walk.collect::<Result<Vec<_>, _>>()?;
        commit_ids.reverse();
        Ok(commit_ids)
    }

    fn commit_event(&self, commit_id: Oid) -> Result<CommitEvent, RepositoryIngestionError> {
        let commit = self.repository.find_commit(commit_id)?;
        let parent_ids = commit
            .parents()
            .map(|parent| CommitId::new(parent.id().to_string()))
            .collect();
        let author = evidence_from_signature(&commit.author());
        let committer = evidence_from_signature(&commit.committer());
        let contributor = Contributor::human(author.name());
        let file_changes = self.file_changes_for_commit(&commit)?;

        let evidence = CommitEvidence::new(
            CommitId::new(commit.id().to_string()),
            CommitSubject::new(commit.summary().unwrap_or_default()),
            author,
            committer,
            contributor,
        )
        .with_parent_ids(parent_ids)
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
    use gitflux_scene::{FileChangeKind, Mainline};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn scaffold_uses_requested_mainline() {
        let request = RepositoryIngestionRequest::new(".", Mainline::new("trunk"));

        let summary = scaffold_repository_replay(&request);

        assert_eq!(summary.replay().mainline().as_str(), "trunk");
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

    struct GeneratedGitRepository {
        temp_dir: TempDir,
    }

    impl GeneratedGitRepository {
        fn new() -> Self {
            let temp_dir = tempfile::tempdir().expect("fixture tempdir should be created");
            let fixture = Self { temp_dir };
            fixture.git(["init", "--initial-branch=main"]);
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

        fn commit(&self, subject: &str, name: &str, email: &str) -> String {
            self.git(["add", "."]);
            self.git_with_identity(["commit", "-m", subject], name, email);
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
            self.run_git(args, Some((name, email)))
        }

        fn run_git<const N: usize>(
            &self,
            args: [&str; N],
            identity: Option<(&str, &str)>,
        ) -> String {
            let mut command = Command::new("git");
            command
                .args(args)
                .current_dir(self.path())
                .env("GIT_AUTHOR_DATE", "2024-01-02T03:04:05Z")
                .env("GIT_COMMITTER_DATE", "2024-01-02T03:04:05Z");

            if let Some((name, email)) = identity {
                command
                    .env("GIT_AUTHOR_NAME", name)
                    .env("GIT_AUTHOR_EMAIL", email)
                    .env("GIT_COMMITTER_NAME", name)
                    .env("GIT_COMMITTER_EMAIL", email);
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
    }
}
