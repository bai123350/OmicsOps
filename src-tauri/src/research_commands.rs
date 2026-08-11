use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use omicsops_adapters::research::{
    CrossrefSource, EuropePmcSource, PubMedSource, ResearchItem, ResearchSource,
    parse_crossref_response, parse_europe_pmc_response, parse_pubmed_search_response,
    parse_pubmed_summary_response, pubmed_summary_url,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;

use crate::commands::AppState;

const CACHE_TTL_HOURS: i64 = 24;
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(350);

#[derive(Debug, Clone, Deserialize)]
pub struct ResearchSearchRequest {
    pub source: String,
    pub query: String,
    pub limit: usize,
    pub cursor: Option<String>,
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSearchResult {
    pub source: String,
    pub query: String,
    pub retrieved_at: DateTime<Utc>,
    pub next_cursor: Option<String>,
    pub items: Vec<ResearchItem>,
    pub cached: bool,
}

#[tauri::command]
pub async fn search_research(
    state: State<'_, AppState>,
    request: ResearchSearchRequest,
) -> Result<ResearchSearchResult, String> {
    let query = request.query.trim();
    if query.is_empty() || query.len() > 500 || query.chars().any(char::is_control) {
        return Err("research query must contain 1-500 printable characters".into());
    }
    let limit = request.limit.clamp(1, 50);
    let source: Box<dyn ResearchSource> = match request.source.as_str() {
        "pubmed" => Box::new(PubMedSource),
        "europe-pmc" => Box::new(EuropePmcSource),
        "crossref" => Box::new(CrossrefSource),
        other => return Err(format!("unsupported research source: {other}")),
    };
    let cache_key = research_cache_key(source.source_id(), query, limit, request.cursor.as_deref());
    if !request.refresh {
        if let Some(mut cached) = state
            .repository
            .get_json::<ResearchSearchResult>("research_search_cache", &cache_key)
            .map_err(|error| error.to_string())?
            .filter(|cached| {
                Utc::now()
                    .signed_duration_since(cached.retrieved_at)
                    .num_hours()
                    < CACHE_TTL_HOURS
            })
        {
            cached.cached = true;
            return Ok(cached);
        }
    }

    let url = source
        .search_page_url(query, limit, request.cursor.as_deref())
        .map_err(|error| error.to_string())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("OmicsOps/0.1 desktop research workbench")
        .build()
        .map_err(|error| error.to_string())?;
    throttle(&state, source.source_id()).await;
    let raw = get_text(&client, url).await?;
    let retrieved_at = Utc::now();
    let (items, next_cursor) = match source.source_id() {
        "pubmed" => {
            let search = parse_pubmed_search_response(&raw).map_err(|error| error.to_string())?;
            let items = if search.identifiers.is_empty() {
                Vec::new()
            } else {
                throttle(&state, source.source_id()).await;
                let summary = get_text(
                    &client,
                    pubmed_summary_url(&search.identifiers).map_err(|error| error.to_string())?,
                )
                .await?;
                parse_pubmed_summary_response(&summary, query, retrieved_at)
                    .map_err(|error| error.to_string())?
            };
            (items, search.next_cursor)
        }
        "europe-pmc" => {
            let page = parse_europe_pmc_response(&raw, query, retrieved_at)
                .map_err(|error| error.to_string())?;
            (page.items, page.next_cursor)
        }
        "crossref" => {
            let offset = request
                .cursor
                .as_deref()
                .unwrap_or("0")
                .parse()
                .map_err(|_| "invalid Crossref cursor".to_string())?;
            let page = parse_crossref_response(&raw, query, retrieved_at, offset)
                .map_err(|error| error.to_string())?;
            (page.items, page.next_cursor)
        }
        _ => unreachable!(),
    };
    let result = ResearchSearchResult {
        source: source.source_id().into(),
        query: query.into(),
        retrieved_at,
        next_cursor,
        items,
        cached: false,
    };
    state
        .repository
        .put_json("research_search_cache", &cache_key, &result)
        .map_err(|error| error.to_string())?;
    Ok(result)
}

async fn get_text(client: &reqwest::Client, url: url::Url) -> Result<String, String> {
    client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .text()
        .await
        .map_err(|error| error.to_string())
}

async fn throttle(state: &State<'_, AppState>, source: &str) {
    let wait = {
        let requests = state.research_last_request.lock().await;
        requests
            .get(source)
            .map(|last| MIN_REQUEST_INTERVAL.saturating_sub(last.elapsed()))
            .unwrap_or_default()
    };
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
    state
        .research_last_request
        .lock()
        .await
        .insert(source.to_owned(), Instant::now());
}

pub fn research_cache_key(source: &str, query: &str, limit: usize, cursor: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    for part in [source, query, &limit.to_string(), cursor.unwrap_or("")] {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}
