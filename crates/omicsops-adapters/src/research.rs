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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchItem {
    pub identifier: String,
    pub title: String,
    pub abstract_text: Option<String>,
    pub authors: Vec<String>,
    pub year: Option<u32>,
    pub doi: Option<String>,
    pub url: Option<String>,
    pub provenance: ResearchProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchPage {
    pub items: Vec<ResearchItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubMedSearchPage {
    pub identifiers: Vec<String>,
    pub next_cursor: Option<String>,
}

pub trait ResearchSource: Send + Sync {
    fn source_id(&self) -> &'static str;
    fn search_url(&self, query: &str, limit: usize) -> AdapterResult<Url>;
    fn search_page_url(
        &self,
        query: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> AdapterResult<Url> {
        let _ = cursor;
        self.search_url(query, limit)
    }

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
    fn search_page_url(
        &self,
        query: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> AdapterResult<Url> {
        let mut url = self.search_url(query, limit)?;
        if let Some(cursor) = cursor {
            let offset = parse_numeric_cursor(cursor)?;
            url.query_pairs_mut()
                .append_pair("retstart", &offset.to_string());
        }
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
    fn search_page_url(
        &self,
        query: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> AdapterResult<Url> {
        let mut url = self.search_url(query, limit)?;
        if let Some(cursor) = cursor {
            if cursor.len() > 512 || cursor.chars().any(char::is_control) {
                return Err(AdapterError::InvalidInput(
                    "invalid Europe PMC cursor".into(),
                ));
            }
            url.query_pairs_mut().append_pair("cursorMark", cursor);
        }
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
    fn search_page_url(
        &self,
        query: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> AdapterResult<Url> {
        let mut url = self.search_url(query, limit)?;
        if let Some(cursor) = cursor {
            let offset = parse_numeric_cursor(cursor)?;
            url.query_pairs_mut()
                .append_pair("offset", &offset.to_string());
        }
        Ok(url)
    }
}

pub fn pubmed_summary_url(identifiers: &[String]) -> AdapterResult<Url> {
    if identifiers.is_empty() || identifiers.len() > 100 {
        return Err(AdapterError::InvalidInput(
            "invalid PubMed identifier batch".into(),
        ));
    }
    if identifiers.iter().any(|identifier| {
        !identifier
            .chars()
            .all(|character| character.is_ascii_digit())
    }) {
        return Err(AdapterError::InvalidInput(
            "invalid PubMed identifier".into(),
        ));
    }
    let mut url = parsed("https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi")?;
    url.query_pairs_mut()
        .append_pair("db", "pubmed")
        .append_pair("retmode", "json")
        .append_pair("id", &identifiers.join(","));
    Ok(url)
}

pub fn parse_pubmed_search_response(raw: &str) -> AdapterResult<PubMedSearchPage> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    let result = value.get("esearchresult").ok_or_else(|| {
        AdapterError::InvalidInput("PubMed response is missing esearchresult".into())
    })?;
    let identifiers = result
        .get("idlist")
        .and_then(|value| value.as_array())
        .ok_or_else(|| AdapterError::InvalidInput("PubMed response is missing idlist".into()))?
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let count = json_usize(result.get("count"));
    let start = json_usize(result.get("retstart")).unwrap_or(0);
    let next = start.saturating_add(identifiers.len());
    Ok(PubMedSearchPage {
        identifiers,
        next_cursor: count
            .filter(|count| next < *count)
            .map(|_| next.to_string()),
    })
}

pub fn parse_pubmed_summary_response(
    raw: &str,
    query: &str,
    retrieved_at: DateTime<Utc>,
) -> AdapterResult<Vec<ResearchItem>> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    let result = value
        .get("result")
        .and_then(|value| value.as_object())
        .ok_or_else(|| AdapterError::InvalidInput("PubMed summary is missing result".into()))?;
    let identifiers = result
        .get("uids")
        .and_then(|value| value.as_array())
        .ok_or_else(|| AdapterError::InvalidInput("PubMed summary is missing uids".into()))?;
    identifiers
        .iter()
        .filter_map(|value| value.as_str())
        .map(|uid| {
            let record = result.get(uid).ok_or_else(|| {
                AdapterError::InvalidInput(format!("PubMed summary is missing {uid}"))
            })?;
            let doi = record
                .get("articleids")
                .and_then(|value| value.as_array())
                .and_then(|identifiers| {
                    identifiers.iter().find(|identifier| {
                        identifier.get("idtype").and_then(|value| value.as_str()) == Some("doi")
                    })
                })
                .and_then(|identifier| identifier.get("value"))
                .and_then(|value| value.as_str())
                .map(str::to_owned);
            let identifier = format!("PMID:{uid}");
            Ok(ResearchItem {
                identifier: identifier.clone(),
                title: json_string(record.get("title")).unwrap_or_default(),
                abstract_text: None,
                authors: json_name_array(record.get("authors")),
                year: json_string(record.get("pubdate")).and_then(|value| first_year(&value)),
                url: Some(format!("https://pubmed.ncbi.nlm.nih.gov/{uid}/")),
                doi,
                provenance: PubMedSource.provenance(query, &identifier, retrieved_at),
            })
        })
        .collect()
}

pub fn parse_europe_pmc_response(
    raw: &str,
    query: &str,
    retrieved_at: DateTime<Utc>,
) -> AdapterResult<ResearchPage> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    let records = value
        .pointer("/resultList/result")
        .and_then(|value| value.as_array())
        .ok_or_else(|| {
            AdapterError::InvalidInput("Europe PMC response is missing results".into())
        })?;
    let items = records
        .iter()
        .filter_map(|record| {
            let id = json_string(record.get("id"))?;
            let source = json_string(record.get("source")).unwrap_or_else(|| "MED".into());
            let identifier = format!("{source}:{id}");
            Some(ResearchItem {
                identifier: identifier.clone(),
                title: json_string(record.get("title")).unwrap_or_default(),
                abstract_text: json_string(record.get("abstractText")),
                authors: json_string(record.get("authorString"))
                    .map(|value| vec![value])
                    .unwrap_or_default(),
                year: json_string(record.get("pubYear")).and_then(|value| value.parse().ok()),
                doi: json_string(record.get("doi")),
                url: json_string(record.get("pmcid"))
                    .map(|pmcid| format!("https://europepmc.org/article/PMC/{pmcid}")),
                provenance: EuropePmcSource.provenance(query, &identifier, retrieved_at),
            })
        })
        .collect();
    Ok(ResearchPage {
        items,
        next_cursor: json_string(value.get("nextCursorMark")).filter(|cursor| !cursor.is_empty()),
    })
}

pub fn parse_crossref_response(
    raw: &str,
    query: &str,
    retrieved_at: DateTime<Utc>,
    offset: usize,
) -> AdapterResult<ResearchPage> {
    let value: serde_json::Value = serde_json::from_str(raw)?;
    let records = value
        .pointer("/message/items")
        .and_then(|value| value.as_array())
        .ok_or_else(|| AdapterError::InvalidInput("Crossref response is missing items".into()))?;
    let items = records
        .iter()
        .filter_map(|record| {
            let doi = json_string(record.get("DOI"))?;
            let identifier = format!("DOI:{doi}");
            let title = record
                .get("title")
                .and_then(|value| value.as_array())
                .and_then(|titles| titles.first())
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_owned();
            let authors = record
                .get("author")
                .and_then(|value| value.as_array())
                .map(|authors| authors.iter().filter_map(crossref_author).collect())
                .unwrap_or_default();
            let year = record
                .pointer("/published/date-parts/0/0")
                .and_then(|value| value.as_u64())
                .and_then(|value| u32::try_from(value).ok());
            Some(ResearchItem {
                identifier: identifier.clone(),
                title,
                abstract_text: json_string(record.get("abstract")),
                authors,
                year,
                doi: Some(doi),
                url: json_string(record.get("URL")),
                provenance: CrossrefSource.provenance(query, &identifier, retrieved_at),
            })
        })
        .collect::<Vec<_>>();
    let next = offset.saturating_add(records.len());
    let total = value
        .pointer("/message/total-results")
        .and_then(|value| value.as_u64())
        .and_then(|value| usize::try_from(value).ok());
    Ok(ResearchPage {
        items,
        next_cursor: total
            .filter(|total| next < *total)
            .map(|_| next.to_string()),
    })
}

fn parse_numeric_cursor(cursor: &str) -> AdapterResult<usize> {
    cursor
        .parse::<usize>()
        .map_err(|_| AdapterError::InvalidInput("invalid numeric research cursor".into()))
}

fn json_string(value: Option<&serde_json::Value>) -> Option<String> {
    value.and_then(|value| value.as_str()).map(str::to_owned)
}

fn json_usize(value: Option<&serde_json::Value>) -> Option<usize> {
    value.and_then(|value| {
        value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn json_name_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| json_string(value.get("name")))
                .collect()
        })
        .unwrap_or_default()
}

fn first_year(value: &str) -> Option<u32> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .find(|part| part.len() == 4)
        .and_then(|part| part.parse().ok())
}

fn crossref_author(value: &serde_json::Value) -> Option<String> {
    let given = value
        .get("given")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let family = value
        .get("family")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let combined = format!("{given} {family}").trim().to_owned();
    (!combined.is_empty()).then_some(combined)
}
