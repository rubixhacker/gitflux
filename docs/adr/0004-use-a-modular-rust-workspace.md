# Use a modular Rust workspace

Gitflux will start as a Rust workspace with separate crates for the CLI, Git ingestion, scene/layout data, GPU rendering, and video export orchestration. This keeps the repository replay pipeline testable without requiring the renderer, and preserves clear boundaries between history preparation, visual scene construction, frame rendering, and FFmpeg-driven export.
