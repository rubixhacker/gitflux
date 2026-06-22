//! Scene and layout data for Repository Replay rendering.
//!
//! This crate owns the deterministic core data shared by Repository Ingestion,
//! GPU rendering, and Video Export orchestration. It names the Repository
//! Replay timeline, Repository Graph layout, repository entities, contributors,
//! and Render Configuration without depending on Git, wgpu, or FFmpeg adapters.

mod config;
mod domain;
mod scene;

pub use config::{
    ConfigValueError, EntityCountThreshold, EntitySpacing, ExplicitPathFilter, FrameSize,
    FramesPerSecond, HexColor, Layout, LevelOfDetailPolicy, RenderConfiguration,
    RenderConfigurationError, RepositoryGraphLayout, SettleIterations, Theme, VisualMetaphor,
};
pub use domain::{
    BranchFlow, CommitEvent, CommitEvidence, CommitId, CommitSubject, CompetingChange,
    CompetingChangeConfidence, CompetingChangeEvidence, CompetingChangeSource, Contributor,
    ContributorEvidence, ContributorKind, FileChange, FileChangeKind, GitTimestamp, Mainline,
    MergeSettlement, RepositoryEntity, RepositoryReplay,
};
pub use scene::{
    CommitSceneId, ContributorSceneId, DirectorySceneId, FileSceneId, MotionState,
    RepositoryGraphScene, SceneActivity, SceneBranchActivity, SceneCompetingChange,
    SceneCompetingChangeEvidence, SceneContributor, SceneDirectory, SceneEmphasis,
    SceneExplicitPathFilter, SceneFile, SceneFileChange, SceneFrameSize, SceneMergeSettlement,
    ScenePosition, VisualSummary, VisualSummarySceneId, VisualSummaryWeight,
};

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
