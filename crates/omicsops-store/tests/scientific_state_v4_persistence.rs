use std::collections::BTreeSet;

use chrono::Utc;
use omicsops_core::workspace::{Project, ProjectTemplate};
use omicsops_science::{DatasetStageV4, ScientificStateV4, VerifiedDatasetFactV4};
use omicsops_store::Store;
use uuid::Uuid;

#[tokio::test]
async fn scientific_state_and_normalized_registries_survive_restart() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("state.sqlite");
    let project = Project::new(
        Uuid::new_v4(),
        "scientific fixture",
        "C:\\data\\scientific-fixture",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let dataset_id;
    {
        let store = Store::open(&database).await.unwrap();
        store.save_project(&project).await.unwrap();
        let mut state = ScientificStateV4::new(project.id);
        dataset_id = state
            .register_dataset(
                VerifiedDatasetFactV4 {
                    modality: "single_cell_rna".into(),
                    species: "human".into(),
                    sample_ids: BTreeSet::from(["sample-a".into()]),
                    matrix_shape: vec![3_000, 20_000],
                    stage: DatasetStageV4::Raw,
                    relative_path: "data/input.h5ad".into(),
                    size_bytes: 123,
                    sha256: "abc".into(),
                },
                Utc::now(),
            )
            .id;
        store.save_scientific_state_v4(&state).await.unwrap();
    }
    let reopened = Store::open(&database).await.unwrap();
    let restored = reopened
        .scientific_state_v4(project.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.revision, 1);
    assert_eq!(restored.datasets[&dataset_id].sha256, "abc");
    assert!(restored.datasets[&dataset_id].active);
}
