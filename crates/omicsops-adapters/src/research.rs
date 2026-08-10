use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{AdapterError, AdapterResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchProvenance {
    pub source: String,
    pub query: String,
    pub identifier: String,
    pub retrieved_at: DateTime<Utc>,
}

pub trait ResearchSource: Send + Sync {
    fn source_id(&self) -> &'static str;
    fn search_url(&self, query: &str, limit: usize) -> AdapterResult<Url>;

    fn provenance(
        &self,
        query: &str,
        identifier: &str,
        retrieved_at: DateTime<Utc>,
    ) -> ResearchProvenance {
        ResearchProvenance {
            source: self.source_id().into(),
            query: query.into(),
            identifier: identifier.into(),
            retrieved_at,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PubMedSource;
#[derive(Debug, Clone, Copy)]
pub struct EuropePmcSource;
#[derive(Debug, Clone, Copy)]
pub struct CrossrefSource;

fn parsed(url: &str) -> AdapterResult<Url> {
    Url::parse(url).map_err(|error| AdapterError::InvalidInput(error.to_string()))
}

impl ResearchSource for PubMedSource {
    fn source_id(&self) -> &'static str {
        "pubmed"
    }
    fn search_url(&self, query: &str, limit: usize) -> AdapterResult<Url> {
        let mut url = parsed("https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi")?;
        url.query_pairs_mut()
            .append_pair("db", "pubmed")
            .append_pair("retmode", "json")
            .append_pair("retmax", &limit.to_string())
            .append_pair("term", query);
        Ok(url)
    }
}

impl ResearchSource for EuropePmcSource {
    fn source_id(&self) -> &'static str {
        "europe-pmc"
    }
    fn search_url(&self, query: &str, limit: usize) -> AdapterResult<Url> {
        let mut url = parsed("https://www.ebi.ac.uk/europepmc/webservices/rest/search")?;
        url.query_pairs_mut()
            .append_pair("format", "json")
            .append_pair("pageSize", &limit.to_string())
            .append_pair("query", query);
        Ok(url)
    }
}

impl ResearchSource for CrossrefSource {
    fn source_id(&self) -> &'static str {
        "crossref"
    }
    fn search_url(&self, query: &str, limit: usize) -> AdapterResult<Url> {
        let mut url = parsed("https://api.crossref.org/works")?;
        url.query_pairs_mut()
            .append_pair("rows", &limit.to_string())
            .append_pair("query.bibliographic", query);
        Ok(url)
    }
}
