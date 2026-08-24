//! Native PubMed MCP server.
//!
//! The desktop executable can be launched with `--omicsops-pubmed-mcp`.  In
//! that mode this module owns stdin/stdout and exposes the small, read-only
//! PubMed tool surface used by OmicsOps.  All diagnostics are deliberately
//! kept on stderr by the MCP transport; stdout is reserved for JSON-RPC.

use std::{
    collections::{HashMap, HashSet},
    env,
    time::Duration,
};

use omicsops_adapters::research::{parse_pubmed_search_response, pubmed_summary_url};
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use rmcp::{
    ErrorData as McpError, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars::JsonSchema,
    tool, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use url::Url;

const NCBI_BASE_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
const NCBI_TOOL_NAME: &str = "OmicsOps";
const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_SEARCH_LIMIT: usize = 50;
const MAX_FETCH_RECORDS: usize = 20;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
pub enum PubMedClientError {
    #[error("invalid PubMed input: {0}")]
    InvalidInput(String),
    #[error("PubMed request failed: {0}")]
    Network(String),
    #[error("PubMed response could not be parsed: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Default)]
pub struct PubMedClientConfig {
    /// NCBI API key, resolved from the process environment immediately before
    /// launching the bundled MCP process.  It is never serialized.
    pub api_key: Option<String>,
    /// NCBI administrator email, used for responsible E-utilities requests.
    pub email: Option<String>,
    pub tool_name: String,
}

impl PubMedClientConfig {
    pub fn from_environment() -> Self {
        Self {
            api_key: env::var("NCBI_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            email: env::var("NCBI_ADMIN_EMAIL")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            tool_name: NCBI_TOOL_NAME.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PubMedClient {
    http: reqwest::Client,
    config: PubMedClientConfig,
}

impl PubMedClient {
    pub fn from_environment() -> Result<Self, PubMedClientError> {
        Self::new(PubMedClientConfig::from_environment())
    }

    pub fn new(config: PubMedClientConfig) -> Result<Self, PubMedClientError> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent("OmicsOps/0.1 desktop PubMed MCP")
            .build()
            .map_err(|error| PubMedClientError::Network(error.to_string()))?;
        Ok(Self { http, config })
    }

    /// Build the ESearch URL without performing a network request.  Keeping
    /// URL construction public makes it possible to test that credentials are
    /// represented as query parameters without ever logging their values.
    pub fn search_url(
        &self,
        query: &str,
        start: usize,
        limit: usize,
    ) -> Result<Url, PubMedClientError> {
        validate_query(query)?;
        if limit == 0 || limit > MAX_SEARCH_LIMIT {
            return Err(PubMedClientError::InvalidInput(format!(
                "search limit must be between 1 and {MAX_SEARCH_LIMIT}"
            )));
        }
        let mut url = parsed_url("esearch.fcgi")?;
        url.query_pairs_mut()
            .append_pair("db", "pubmed")
            .append_pair("retmode", "json")
            .append_pair("retstart", &start.to_string())
            .append_pair("retmax", &limit.to_string())
            .append_pair("term", query);
        self.append_common_parameters(&mut url);
        Ok(url)
    }

    pub fn summary_url(&self, identifiers: &[String]) -> Result<Url, PubMedClientError> {
        if identifiers.is_empty() || identifiers.len() > MAX_SEARCH_LIMIT {
            return Err(PubMedClientError::InvalidInput(format!(
                "summary batch must contain 1-{MAX_SEARCH_LIMIT} PMIDs"
            )));
        }
        validate_pmids(identifiers)?;
        let mut url = pubmed_summary_url(identifiers)
            .map_err(|error| PubMedClientError::InvalidInput(error.to_string()))?;
        self.append_common_parameters(&mut url);
        Ok(url)
    }

    pub fn fetch_url(&self, identifiers: &[String]) -> Result<Url, PubMedClientError> {
        if identifiers.is_empty() || identifiers.len() > MAX_FETCH_RECORDS {
            return Err(PubMedClientError::InvalidInput(format!(
                "fetch batch must contain 1-{MAX_FETCH_RECORDS} PMIDs"
            )));
        }
        validate_pmids(identifiers)?;
        let mut url = parsed_url("efetch.fcgi")?;
        url.query_pairs_mut()
            .append_pair("db", "pubmed")
            .append_pair("retmode", "xml")
            .append_pair("rettype", "abstract")
            .append_pair("id", &identifiers.join(","));
        self.append_common_parameters(&mut url);
        Ok(url)
    }

    pub async fn search(
        &self,
        query: &str,
        start: usize,
        limit: usize,
    ) -> Result<PubMedSearchResult, PubMedClientError> {
        let raw = self.get_text(self.search_url(query, start, limit)?).await?;
        let page = parse_pubmed_search_result(&raw)?;
        let retrieved_at = chrono::Utc::now();
        let items = if page.identifiers.is_empty() {
            Vec::new()
        } else {
            let summary = self.get_text(self.summary_url(&page.identifiers)?).await?;
            parse_pubmed_summary_records(&summary)?
        };
        Ok(PubMedSearchResult {
            query: query.trim().to_owned(),
            page: (start / limit).saturating_add(1),
            limit,
            total: page.total,
            next_page: page
                .next_cursor
                .as_deref()
                .and_then(|cursor| cursor.parse::<usize>().ok())
                .map(|next| (next / limit).saturating_add(1)),
            items,
            retrieved_at,
        })
    }

    pub async fn fetch_records(
        &self,
        identifiers: &[String],
    ) -> Result<PubMedFetchResult, PubMedClientError> {
        if identifiers.is_empty() || identifiers.len() > MAX_FETCH_RECORDS {
            return Err(PubMedClientError::InvalidInput(format!(
                "fetch batch must contain 1-{MAX_FETCH_RECORDS} PMIDs"
            )));
        }
        validate_pmids(identifiers)?;

        let mut unique = Vec::with_capacity(identifiers.len());
        let mut seen = HashSet::new();
        for identifier in identifiers {
            if seen.insert(identifier.as_str()) {
                unique.push(identifier.clone());
            }
        }
        let raw = self.get_text(self.fetch_url(&unique)?).await?;
        let parsed = parse_pubmed_efetch_response(&raw)?;
        let by_pmid = parsed
            .into_iter()
            .map(|record| (record.pmid.clone(), record))
            .collect::<HashMap<_, _>>();
        let mut seen_output = HashSet::new();
        let records = identifiers
            .iter()
            .map(|pmid| {
                if !seen_output.insert(pmid.as_str()) {
                    return PubMedRecordResult {
                        pmid: pmid.clone(),
                        status: PubMedRecordStatus::Duplicate,
                        record: None,
                        message: Some("PMID was repeated in the request".into()),
                    };
                }
                match by_pmid.get(pmid) {
                    None => PubMedRecordResult {
                        pmid: pmid.clone(),
                        status: PubMedRecordStatus::Missing,
                        record: None,
                        message: Some("NCBI returned no record for this PMID".into()),
                    },
                    Some(record) if record.abstract_text.is_none() => PubMedRecordResult {
                        pmid: pmid.clone(),
                        status: PubMedRecordStatus::NoAbstract,
                        record: Some(record.clone()),
                        message: Some("record exists but has no abstract".into()),
                    },
                    Some(record) => PubMedRecordResult {
                        pmid: pmid.clone(),
                        status: PubMedRecordStatus::Ok,
                        record: Some(record.clone()),
                        message: None,
                    },
                }
            })
            .collect();
        Ok(PubMedFetchResult { records })
    }

    async fn get_text(&self, url: Url) -> Result<String, PubMedClientError> {
        self.http
            .get(url)
            .send()
            .await
            .map_err(|error| PubMedClientError::Network(error.to_string()))?
            .error_for_status()
            .map_err(|error| PubMedClientError::Network(error.to_string()))?
            .text()
            .await
            .map_err(|error| PubMedClientError::Network(error.to_string()))
    }

    fn append_common_parameters(&self, url: &mut Url) {
        url.query_pairs_mut().append_pair(
            "tool",
            if self.config.tool_name.trim().is_empty() {
                NCBI_TOOL_NAME
            } else {
                self.config.tool_name.trim()
            },
        );
        if let Some(email) = self.config.email.as_deref() {
            url.query_pairs_mut().append_pair("email", email);
        }
        if let Some(api_key) = self.config.api_key.as_deref() {
            url.query_pairs_mut().append_pair("api_key", api_key);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PubMedSearchResult {
    pub query: String,
    pub page: usize,
    pub limit: usize,
    pub total: usize,
    pub next_page: Option<usize>,
    pub items: Vec<PubMedSearchItem>,
    pub retrieved_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PubMedSearchItem {
    pub pmid: String,
    pub title: String,
    pub journal: Option<String>,
    pub publication_date: Option<String>,
    pub authors: Vec<String>,
    pub doi: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PubMedRecord {
    pub pmid: String,
    pub title: String,
    pub abstract_text: Option<String>,
    pub journal: Option<String>,
    pub publication_date: Option<String>,
    pub authors: Vec<String>,
    pub doi: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PubMedFetchResult {
    pub records: Vec<PubMedRecordResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PubMedRecordStatus {
    Ok,
    Missing,
    Duplicate,
    NoAbstract,
}

#[derive(Debug, Clone, Serialize)]
pub struct PubMedRecordResult {
    pub pmid: String,
    pub status: PubMedRecordStatus,
    pub record: Option<PubMedRecord>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PubMedSearchParams {
    #[schemars(description = "PubMed query, for example: single-cell RNA-seq tumor")]
    pub query: String,
    #[schemars(description = "1-based result page; defaults to 1")]
    pub page: Option<usize>,
    #[schemars(description = "results per page, from 1 to 50; defaults to 20")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PubMedFetchParams {
    #[schemars(description = "one to twenty PubMed IDs")]
    pub pmids: Vec<String>,
}

#[derive(Clone)]
pub struct PubMedMcpServer {
    client: PubMedClient,
}

impl PubMedMcpServer {
    pub fn from_environment() -> Result<Self, PubMedClientError> {
        Ok(Self {
            client: PubMedClient::from_environment()?,
        })
    }

    pub fn new(client: PubMedClient) -> Self {
        Self { client }
    }
}

#[tool_router(server_handler)]
impl PubMedMcpServer {
    #[tool(
        description = "Search PubMed and return paginated PMID metadata. This read-only tool does not download full text."
    )]
    async fn pubmed_search(
        &self,
        Parameters(params): Parameters<PubMedSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let query = params.query.trim();
        let page = params.page.unwrap_or(1).max(1);
        let limit = params.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        if let Err(error) = validate_query(query).and_then(|_| {
            (limit <= MAX_SEARCH_LIMIT && limit > 0)
                .then_some(())
                .ok_or_else(|| {
                    PubMedClientError::InvalidInput(format!(
                        "search limit must be between 1 and {MAX_SEARCH_LIMIT}"
                    ))
                })
        }) {
            return Err(McpError::invalid_params(error.to_string(), None));
        }
        let start = page.saturating_sub(1).saturating_mul(limit);
        match self.client.search(query, start, limit).await {
            Ok(result) => json_tool_result(&result),
            Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "PubMed search failed: {error}"
            ))])),
        }
    }

    #[tool(
        description = "Fetch PubMed abstract records by PMID using NCBI EFetch. Returns one status per requested PMID."
    )]
    async fn pubmed_fetch_records(
        &self,
        Parameters(params): Parameters<PubMedFetchParams>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(error) = validate_fetch_params(&params.pmids) {
            return Err(McpError::invalid_params(error.to_string(), None));
        }
        match self.client.fetch_records(&params.pmids).await {
            Ok(result) => json_tool_result(&result),
            Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "PubMed fetch failed: {error}"
            ))])),
        }
    }
}

pub async fn run_stdio_from_env() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let service = PubMedMcpServer::from_environment()?.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

fn json_tool_result<T: Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|error| McpError::internal_error(error.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
}

fn validate_fetch_params(pmids: &[String]) -> Result<(), PubMedClientError> {
    if pmids.is_empty() || pmids.len() > MAX_FETCH_RECORDS {
        return Err(PubMedClientError::InvalidInput(format!(
            "pmids must contain 1-{MAX_FETCH_RECORDS} values"
        )));
    }
    validate_pmids(pmids)
}

fn validate_query(query: &str) -> Result<(), PubMedClientError> {
    if query.is_empty() || query.len() > 500 || query.chars().any(char::is_control) {
        return Err(PubMedClientError::InvalidInput(
            "query must contain 1-500 printable characters".into(),
        ));
    }
    Ok(())
}

fn validate_pmids(pmids: &[String]) -> Result<(), PubMedClientError> {
    if pmids
        .iter()
        .any(|pmid| pmid.is_empty() || !pmid.chars().all(|character| character.is_ascii_digit()))
    {
        return Err(PubMedClientError::InvalidInput(
            "PMIDs must contain ASCII digits only".into(),
        ));
    }
    Ok(())
}

fn parsed_url(endpoint: &str) -> Result<Url, PubMedClientError> {
    Url::parse(&format!("{NCBI_BASE_URL}/{endpoint}"))
        .map_err(|error| PubMedClientError::InvalidInput(error.to_string()))
}

#[derive(Debug, Clone)]
struct ParsedSearchPage {
    total: usize,
    identifiers: Vec<String>,
    next_cursor: Option<String>,
}

fn parse_pubmed_search_result(raw: &str) -> Result<ParsedSearchPage, PubMedClientError> {
    let value: Value =
        serde_json::from_str(raw).map_err(|error| PubMedClientError::Parse(error.to_string()))?;
    let result = value
        .get("esearchresult")
        .ok_or_else(|| PubMedClientError::Parse("response is missing esearchresult".into()))?;
    let page = parse_pubmed_search_response(raw)
        .map_err(|error| PubMedClientError::Parse(error.to_string()))?;
    let total = json_usize(result.get("count")).unwrap_or(0);
    Ok(ParsedSearchPage {
        total,
        identifiers: page.identifiers,
        next_cursor: page.next_cursor,
    })
}

fn parse_pubmed_summary_records(raw: &str) -> Result<Vec<PubMedSearchItem>, PubMedClientError> {
    let value: Value =
        serde_json::from_str(raw).map_err(|error| PubMedClientError::Parse(error.to_string()))?;
    let result = value
        .get("result")
        .and_then(Value::as_object)
        .ok_or_else(|| PubMedClientError::Parse("summary is missing result".into()))?;
    let uids = result
        .get("uids")
        .and_then(Value::as_array)
        .ok_or_else(|| PubMedClientError::Parse("summary is missing uids".into()))?;
    uids.iter()
        .filter_map(Value::as_str)
        .map(|pmid| {
            let record = result
                .get(pmid)
                .ok_or_else(|| PubMedClientError::Parse(format!("summary is missing {pmid}")))?;
            Ok(PubMedSearchItem {
                pmid: pmid.to_owned(),
                title: value_string(record.get("title")).unwrap_or_default(),
                journal: value_string(record.get("fulljournalname"))
                    .or_else(|| value_string(record.get("source"))),
                publication_date: value_string(record.get("pubdate")),
                authors: value_name_array(record.get("authors")),
                doi: value_article_id(record.get("articleids"), "doi"),
                url: format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/"),
            })
        })
        .collect()
}

#[derive(Debug, Default)]
struct RawArticle {
    pmid: Option<String>,
    title: Option<String>,
    abstract_parts: Vec<String>,
    journal: Option<String>,
    publication_date: Option<String>,
    authors: Vec<String>,
    doi: Option<String>,
}

#[derive(Debug, Default)]
struct RawAuthor {
    fore_name: Option<String>,
    last_name: Option<String>,
    collective_name: Option<String>,
}

#[derive(Debug)]
enum CaptureField {
    Pmid,
    Title,
    Abstract,
    Journal,
    PublicationDate,
    Doi,
    AuthorForeName,
    AuthorLastName,
    AuthorCollectiveName,
}

#[derive(Debug)]
struct Capture {
    field: CaptureField,
    depth: usize,
    text: String,
}

/// Parse the XML returned by NCBI EFetch.  The parser intentionally accepts
/// both `Year` and `MedlineDate`, multiple labelled `AbstractText` nodes, and
/// collective author names.  Articles missing a PMID are ignored because they
/// cannot be safely correlated to a request item.
pub fn parse_pubmed_efetch_response(raw: &str) -> Result<Vec<PubMedRecord>, PubMedClientError> {
    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(true);
    let mut path: Vec<Vec<u8>> = Vec::new();
    let mut current: Option<RawArticle> = None;
    let mut author: Option<RawAuthor> = None;
    let mut capture: Option<Capture> = None;
    let mut records = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let local = local_name(event.name().as_ref()).to_vec();
                path.push(local.clone());
                if local.as_slice() == b"PubmedArticle" {
                    current = Some(RawArticle::default());
                } else if local.as_slice() == b"Author" && current.is_some() {
                    author = Some(RawAuthor::default());
                } else if capture.is_none() {
                    let field = match local.as_slice() {
                        b"PMID" if current.is_some() => Some(CaptureField::Pmid),
                        b"ArticleTitle" if current.is_some() => Some(CaptureField::Title),
                        b"AbstractText" if current.is_some() => Some(CaptureField::Abstract),
                        b"Year" | b"MedlineDate" if current.is_some() => {
                            Some(CaptureField::PublicationDate)
                        }
                        b"Title" if current.is_some() && parent_is(&path, b"Journal") => {
                            Some(CaptureField::Journal)
                        }
                        b"LastName" if author.is_some() => Some(CaptureField::AuthorLastName),
                        b"ForeName" if author.is_some() => Some(CaptureField::AuthorForeName),
                        b"CollectiveName" if author.is_some() => {
                            Some(CaptureField::AuthorCollectiveName)
                        }
                        b"ArticleId" if current.is_some() && article_id_is_doi(&event) => {
                            Some(CaptureField::Doi)
                        }
                        _ => None,
                    };
                    if let Some(field) = field {
                        capture = Some(Capture {
                            field,
                            depth: path.len(),
                            text: String::new(),
                        });
                    }
                }
            }
            Ok(Event::Empty(event)) => {
                let local = local_name(event.name().as_ref()).to_vec();
                if local.as_slice() == b"ArticleId"
                    && current.is_some()
                    && article_id_is_doi(&event)
                {
                    // An empty DOI is valid XML but is not useful metadata.
                }
            }
            Ok(Event::Text(event)) => {
                if let Some(capture) = capture.as_mut() {
                    let text = event
                        .decode()
                        .map_err(|error| PubMedClientError::Parse(error.to_string()))?;
                    append_capture_text(&mut capture.text, &text);
                }
            }
            Ok(Event::CData(event)) => {
                if let Some(capture) = capture.as_mut() {
                    let text = String::from_utf8_lossy(event.as_ref());
                    append_capture_text(&mut capture.text, &text);
                }
            }
            Ok(Event::End(event)) => {
                let local = local_name(event.name().as_ref()).to_vec();
                if let Some(active) = capture.as_ref() {
                    if active.depth == path.len() {
                        let active = capture.take().expect("capture exists");
                        finish_capture(active, current.as_mut(), author.as_mut());
                    }
                }
                if local.as_slice() == b"Author" {
                    if let (Some(current), Some(author)) = (current.as_mut(), author.take()) {
                        if let Some(name) = author_name(&author) {
                            current.authors.push(name);
                        }
                    }
                } else if local.as_slice() == b"PubmedArticle" {
                    if let Some(article) = current.take() {
                        if let Some(record) = build_record(article) {
                            records.push(record);
                        }
                    }
                }
                path.pop();
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(PubMedClientError::Parse(error.to_string())),
        }
    }
    Ok(records)
}

fn build_record(article: RawArticle) -> Option<PubMedRecord> {
    let pmid = article.pmid?.trim().to_owned();
    if pmid.is_empty() || !pmid.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    let abstract_text = article
        .abstract_parts
        .into_iter()
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    Some(PubMedRecord {
        url: format!("https://pubmed.ncbi.nlm.nih.gov/{pmid}/"),
        pmid,
        title: article.title.unwrap_or_default().trim().to_owned(),
        abstract_text: (!abstract_text.is_empty()).then(|| abstract_text.join("\n")),
        journal: article
            .journal
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        publication_date: article
            .publication_date
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        authors: article.authors,
        doi: article
            .doi
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
    })
}

fn finish_capture(
    capture: Capture,
    article: Option<&mut RawArticle>,
    author: Option<&mut RawAuthor>,
) {
    let text = capture.text.trim().to_owned();
    if text.is_empty() {
        return;
    }
    match capture.field {
        CaptureField::Pmid => {
            if let Some(article) = article {
                article.pmid = Some(text);
            }
        }
        CaptureField::Title => {
            if let Some(article) = article {
                article.title = Some(text);
            }
        }
        CaptureField::Abstract => {
            if let Some(article) = article {
                article.abstract_parts.push(text);
            }
        }
        CaptureField::Journal => {
            if let Some(article) = article {
                article.journal = Some(text);
            }
        }
        CaptureField::PublicationDate => {
            if let Some(article) = article {
                article.publication_date = Some(text);
            }
        }
        CaptureField::Doi => {
            if let Some(article) = article {
                article.doi = Some(text);
            }
        }
        CaptureField::AuthorForeName => {
            if let Some(author) = author {
                author.fore_name = Some(text);
            }
        }
        CaptureField::AuthorLastName => {
            if let Some(author) = author {
                author.last_name = Some(text);
            }
        }
        CaptureField::AuthorCollectiveName => {
            if let Some(author) = author {
                author.collective_name = Some(text);
            }
        }
    };
}

fn author_name(author: &RawAuthor) -> Option<String> {
    if let Some(collective) = author
        .collective_name
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return Some(collective.to_owned());
    }
    let name = format!(
        "{} {}",
        author.fore_name.as_deref().unwrap_or_default(),
        author.last_name.as_deref().unwrap_or_default()
    );
    (!name.trim().is_empty()).then(|| name.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn append_capture_text(output: &mut String, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if !output.is_empty() {
        output.push(' ');
    }
    output.push_str(text);
}

fn article_id_is_doi(event: &BytesStart<'_>) -> bool {
    event
        .attributes()
        .with_checks(false)
        .filter_map(Result::ok)
        .any(|attribute| {
            attribute.key.as_ref() == b"IdType"
                && attribute
                    .unescape_value()
                    .map(|value| value.eq_ignore_ascii_case("doi"))
                    .unwrap_or(false)
        })
}

fn parent_is(path: &[Vec<u8>], expected: &[u8]) -> bool {
    path.len() >= 2 && path[path.len() - 2].as_slice() == expected
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn json_usize(value: Option<&Value>) -> Option<usize> {
    value.and_then(|value| {
        value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn value_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

fn value_name_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value_string(value.get("name")))
                .collect()
        })
        .unwrap_or_default()
}

fn value_article_id(value: Option<&Value>, expected: &str) -> Option<String> {
    value
        .and_then(Value::as_array)
        .and_then(|values| {
            values
                .iter()
                .find(|value| value_string(value.get("idtype")).as_deref() == Some(expected))
        })
        .and_then(|value| value_string(value.get("value")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EFETCH: &str = r#"
<PubmedArticleSet>
  <PubmedArticle>
    <MedlineCitation><PMID Version="1">12345</PMID>
      <Article><ArticleTitle>Single-cell &amp; spatial atlas</ArticleTitle>
        <Journal><Title>Nature Methods</Title></Journal>
        <Abstract><AbstractText Label="BACKGROUND">First part.</AbstractText><AbstractText>Second part.</AbstractText></Abstract>
        <AuthorList><Author><LastName>Doe</LastName><ForeName>Jane</ForeName></Author><Author><CollectiveName>Atlas Consortium</CollectiveName></Author></AuthorList>
        <ArticleDate><Year>2025</Year></ArticleDate>
      </Article>
    </MedlineCitation>
    <PubmedData><ArticleIdList><ArticleId IdType="doi">10.1000/example</ArticleId></ArticleIdList></PubmedData>
  </PubmedArticle>
  <PubmedArticle>
    <MedlineCitation><PMID>999</PMID><Article><ArticleTitle>No abstract</ArticleTitle><Journal><Title>Cell</Title></Journal></Article></MedlineCitation>
  </PubmedArticle>
</PubmedArticleSet>
"#;

    #[test]
    fn parses_efetch_records_and_metadata() {
        let records = parse_pubmed_efetch_response(EFETCH).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].pmid, "12345");
        assert_eq!(records[0].journal.as_deref(), Some("Nature Methods"));
        assert_eq!(records[0].authors, vec!["Jane Doe", "Atlas Consortium"]);
        assert_eq!(
            records[0].abstract_text.as_deref(),
            Some("First part.\nSecond part.")
        );
        assert_eq!(records[0].doi.as_deref(), Some("10.1000/example"));
        assert_eq!(records[1].abstract_text, None);
    }

    #[test]
    fn rejects_invalid_or_oversized_inputs() {
        let client = PubMedClient::new(PubMedClientConfig::default()).unwrap();
        assert!(client.search_url("", 0, 20).is_err());
        assert!(client.search_url("ok", 0, MAX_SEARCH_LIMIT + 1).is_err());
        assert!(client.fetch_url(&["abc".into()]).is_err());
        assert!(
            client
                .fetch_url(&vec!["1".into(); MAX_FETCH_RECORDS + 1])
                .is_err()
        );
    }

    #[test]
    fn builds_scoped_urls_with_ncbi_metadata() {
        let client = PubMedClient::new(PubMedClientConfig {
            api_key: Some("secret".into()),
            email: Some("research@example.org".into()),
            tool_name: NCBI_TOOL_NAME.into(),
        })
        .unwrap();
        let url = client.fetch_url(&["123".into()]).unwrap();
        assert_eq!(url.scheme(), "https");
        assert!(url.as_str().contains("tool=OmicsOps"));
        assert!(url.as_str().contains("email=research%40example.org"));
        assert!(url.as_str().contains("api_key=secret"));
    }

    #[test]
    fn returns_per_record_status_for_duplicates_and_missing_records() {
        let parsed = parse_pubmed_efetch_response(EFETCH).unwrap();
        let by_pmid = parsed
            .into_iter()
            .map(|record| (record.pmid.clone(), record))
            .collect::<HashMap<_, _>>();
        let input = vec![
            "12345".to_owned(),
            "12345".to_owned(),
            "777".to_owned(),
            "999".to_owned(),
        ];
        let mut seen = HashSet::new();
        let result = input
            .iter()
            .map(|pmid| {
                if !seen.insert(pmid.as_str()) {
                    return PubMedRecordStatus::Duplicate;
                }
                match by_pmid.get(pmid) {
                    None => PubMedRecordStatus::Missing,
                    Some(record) if record.abstract_text.is_none() => {
                        PubMedRecordStatus::NoAbstract
                    }
                    Some(_) => PubMedRecordStatus::Ok,
                }
            })
            .collect::<Vec<_>>();
        assert!(matches!(result[0], PubMedRecordStatus::Ok));
        assert!(matches!(result[1], PubMedRecordStatus::Duplicate));
        assert!(matches!(result[2], PubMedRecordStatus::Missing));
        assert!(matches!(result[3], PubMedRecordStatus::NoAbstract));
    }
}
