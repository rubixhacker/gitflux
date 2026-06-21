# Use Rust-native Git ingestion

Gitflux will ingest repository history through a Rust Git library by default instead of treating the `git` CLI as the primary data path. This preserves structured access to commits, paths, and file changes, and supports the performance goal of parallel repository ingestion without making text parsing of command output part of the architecture.
