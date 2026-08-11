use chrono::{TimeZone, Utc};
use omicsops_adapters::research::{
    CrossrefSource, EuropePmcSource, PubMedSource, ResearchSource, parse_crossref_response,
    parse_europe_pmc_response, parse_pubmed_search_response, parse_pubmed_summary_response,
};

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
fn fixed_research_responses_normalize_identifiers_citations_and_pagination() {
    let at = Utc.with_ymd_and_hms(2026, 8, 11, 3, 0, 0).unwrap();
    let pubmed_page = parse_pubmed_search_response(
        r#"{"esearchresult":{"count":"3","retstart":"0","idlist":["123","456"]}}"#,
    )
    .unwrap();
    assert_eq!(pubmed_page.identifiers, vec!["123", "456"]);
    assert_eq!(pubmed_page.next_cursor.as_deref(), Some("2"));
    let pubmed = parse_pubmed_summary_response(
        r#"{"result":{"uids":["123"],"123":{"uid":"123","title":"PBMC atlas","pubdate":"2025","authors":[{"name":"Li J"}],"articleids":[{"idtype":"doi","value":"10.1/pbmc"}]}}}"#,
        "PBMC",
        at,
    )
    .unwrap();
    assert_eq!(pubmed[0].identifier, "PMID:123");
    assert_eq!(pubmed[0].doi.as_deref(), Some("10.1/pbmc"));

    let europe = parse_europe_pmc_response(
        r#"{"nextCursorMark":"next-page","resultList":{"result":[{"id":"PMC1","source":"PMC","title":"Batch correction","authorString":"Doe A","pubYear":"2024","doi":"10.2/batch"}]}}"#,
        "batch",
        at,
    )
    .unwrap();
    assert_eq!(europe.next_cursor.as_deref(), Some("next-page"));
    assert_eq!(europe.items[0].identifier, "PMC:PMC1");

    let crossref = parse_crossref_response(
        r#"{"message":{"total-results":2,"items":[{"DOI":"10.3/crossref","title":["Single-cell methods"],"author":[{"given":"Ada","family":"Lovelace"}],"published":{"date-parts":[[2023]]},"URL":"https://doi.org/10.3/crossref"}]}}"#,
        "single cell",
        at,
        0,
    )
    .unwrap();
    assert_eq!(crossref.next_cursor.as_deref(), Some("1"));
    assert_eq!(crossref.items[0].authors, vec!["Ada Lovelace"]);
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
