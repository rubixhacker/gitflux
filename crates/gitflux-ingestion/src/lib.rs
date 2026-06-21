//! Repository Ingestion seam for Gitflux.
//!
//! This crate will own Rust-native preparation of Git history into the
//! Repository Replay timeline. The current scaffold defines the public request
//! and summary types without choosing the concrete Git library integration.

use std::path::{Path, PathBuf};

use gitflux_scene::{Mainline, RepositoryReplay};

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

#[cfg(test)]
mod tests {
    use super::{scaffold_repository_replay, RepositoryIngestionRequest};
    use gitflux_scene::Mainline;

    #[test]
    fn scaffold_uses_requested_mainline() {
        let request = RepositoryIngestionRequest::new(".", Mainline::new("trunk"));

        let summary = scaffold_repository_replay(&request);

        assert_eq!(summary.replay().mainline().as_str(), "trunk");
    }
}
