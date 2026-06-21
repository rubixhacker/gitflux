# Gitflux

Gitflux is a Git history visualization tool centered on replaying repository evolution and producing shareable visual outputs.

## Language

**Repository Replay**:
A deterministic playback of a Git repository's history over time.
_Avoid_: Live monitoring, activity feed

**Replay Pacing**:
The mapping from repository history time to playback time in a repository replay.
_Avoid_: Playback speed, timeline scale

**Repository Ingestion**:
The preparation of Git history into the timeline used by a repository replay.
_Avoid_: Import, indexing

**Commit Event**:
A timeline unit in a repository replay that groups related file changes under one Git commit.
_Avoid_: Frame, transaction

**File Change**:
A visible change to a repository entity within a commit event.
_Avoid_: Diff, patch

**Repository Entity**:
A visual participant in a repository replay, such as a contributor, file, or directory.
_Avoid_: Sprite, object

**Contributor**:
A normalized person or service identity whose activity appears in a repository replay.
_Avoid_: Git author, committer, actor

**Automation Contributor**:
A contributor identity representing bots, scripts, dependency services, or other non-human repository activity.
_Avoid_: Bot, service account

**Render**:
The production of visual frames from a repository replay, either for preview or exported media.
_Avoid_: Capture, recording

**Render Configuration**:
A reusable set of parameters that defines how a repository replay is rendered.
_Avoid_: App state, preferences

**Visual Metaphor**:
The presentation model used to depict repository entities and their changes over time.
_Avoid_: Skin, animation style

**Theme**:
A reusable presentation profile for the appearance of a repository replay.
_Avoid_: Layout, renderer

**Layout**:
A reusable spatial behavior model for arranging and moving repository entities during a repository replay.
_Avoid_: Theme, skin

**Repository Graph**:
A layout that presents repository entities as a connected graph shaped by directory structure, file relationships, and contributor activity.
_Avoid_: File tree, network map

**Branch Superposition**:
A visual state where unmerged branch changes appear as provisional variations of the parent branch.
_Avoid_: Branch overlay, hidden branch

**Mainline**:
The branch treated as the primary history path for merge settlement and repository replay.
_Avoid_: Default branch, base branch

**Merge Settlement**:
The moment when provisional branch changes become part of the main repository replay after a merge.
_Avoid_: Branch close, merge animation

**Conflict Resolution**:
The visible reconciliation of competing branch changes into the version that remains after merge settlement.
_Avoid_: Merge result, fixed conflict

**Competing Change**:
A provisional branch change that overlaps with another branch change before merge settlement.
_Avoid_: Git conflict, conflict marker

**Level of Detail**:
A render policy that summarizes dense repository entities until more detail is useful for the current replay moment or view.
_Avoid_: Filtering, hiding

**Visual Summary**:
A repository entity that stands in for multiple underlying entities while preserving their activity and scale.
_Avoid_: Hidden files, dropped entities

**Interactive Preview**:
A live, adjustable view of a repository replay used to tune presentation before or during rendering.
_Avoid_: Desktop app, viewer

**Video Export**:
A rendered media output produced from a repository replay for sharing or archival playback.
_Avoid_: Screen recording, capture

**Export Manifest**:
A sidecar document that records the replay, render, and encoding inputs used to produce a video export.
_Avoid_: Log file, metadata dump

**Export Throughput**:
The rate at which a video export can produce completed output relative to the replay duration.
_Avoid_: Playback speed, preview performance
