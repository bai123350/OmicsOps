use serde_json::Value;

#[test]
fn bulk_rnaseq_fixture_is_commit_pinned_balanced_and_complete() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../acceptance/bulk-rnaseq.json")).unwrap();
    assert_eq!(
        fixture.pointer("/dataset/commit").and_then(Value::as_str),
        Some("72a702d346833d5523bc40d032323ea548603b00")
    );
    let samples = fixture["samples"].as_array().unwrap();
    assert_eq!(
        samples
            .iter()
            .filter(|sample| sample["condition"] == "WT")
            .count(),
        3
    );
    assert_eq!(
        samples
            .iter()
            .filter(|sample| sample["condition"] == "RAP1_IAA_30M")
            .count(),
        3
    );
    for accession in [
        "SRR6357070",
        "SRR6357071",
        "SRR6357072",
        "SRR6357076",
        "SRR6357077",
        "SRR6357078",
    ] {
        assert!(
            samples
                .iter()
                .any(|sample| sample["accession"] == accession)
        );
    }
    for tool in [
        "bio.fastqc",
        "bio.multiqc",
        "bio.salmon",
        "bio.deseq2",
        "report.html",
    ] {
        assert!(
            fixture["workflow"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == tool)
        );
    }
    for output in [
        "results/counts.tsv",
        "results/differential-expression.tsv",
        "results/pca.png",
        "results/volcano.png",
        "results/report.html",
        ".omicsops/environment.lock",
    ] {
        assert!(
            fixture["required_outputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == output)
        );
    }
}
