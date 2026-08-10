use chrono::{TimeZone, Utc};
use omicsops_adapters::research::{CrossrefSource, EuropePmcSource, PubMedSource, ResearchSource};

#[test]
fn native_research_sources_build_scoped_https_queries() {
    let sources: Vec<Box<dyn ResearchSource>> = vec![
        Box::new(PubMedSource),
        Box::new(EuropePmcSource),
        Box::new(CrossrefSource),
    ];
    for source in sources {
        let url = source.search_url("PBMC batch correction", 20).unwrap();
        assert_eq!(url.scheme(), "https");
        assert!(url.as_str().contains("PBMC"));
        assert!(url.as_str().contains("20"));
    }
}

#[test]
fn research_provenance_keeps_query_source_identifier_and_retrieval_time() {
    let at = Utc.with_ymd_and_hms(2026, 8, 11, 2, 0, 0).unwrap();
    let provenance = PubMedSource.provenance("single cell QC", "PMID:12345", at);
    assert_eq!(provenance.source, "pubmed");
    assert_eq!(provenance.query, "single cell QC");
    assert_eq!(provenance.identifier, "PMID:12345");
    assert_eq!(provenance.retrieved_at, at);
}
