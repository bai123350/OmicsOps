use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub const KNOWLEDGE_RUNTIME_V4: &str = "omicsops.knowledge@4.0.0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SkillSectionV4 {
    pub heading: String,
    pub content: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SkillDocumentV4 {
    pub skill_id: Uuid,
    pub name: String,
    pub version: String,
    pub package_sha256: String,
    pub enabled: bool,
    pub sections: Vec<SkillSectionV4>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillSearchHitV4 {
    pub skill_id: Uuid,
    pub name: String,
    pub version: String,
    pub package_sha256: String,
    pub matching_sections: Vec<String>,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FrozenSkillUseV4 {
    pub skill_id: Uuid,
    pub name: String,
    pub version: String,
    pub package_sha256: String,
    pub sections: Vec<SkillSectionV4>,
    pub frozen_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryDocumentV4 {
    pub id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub dimension: String,
    pub key: String,
    pub statement: String,
    pub source_kind: String,
    pub source_id: String,
    pub conflicted_with: BTreeSet<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemorySearchHitV4 {
    pub document: MemoryDocumentV4,
    pub lexical_rank: usize,
    pub recency_rank: usize,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpToolIndexV4 {
    pub server_id: Uuid,
    pub server_name: String,
    pub tool_name: String,
    pub description: String,
    pub input_schema: Value,
    pub schema_sha256: String,
    pub configured: bool,
    pub enabled: bool,
    pub launch_approved: bool,
    pub tool_approved: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpToolSearchHitV4 {
    pub tool: McpToolIndexV4,
    pub score: f64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum KnowledgeErrorV4 {
    #[error("skill is disabled or missing")]
    SkillUnavailable,
    #[error("requested skill section was not found: {0}")]
    SectionMissing(String),
    #[error("MCP server is not configured")]
    McpNotConfigured,
    #[error("MCP server is disabled")]
    McpDisabled,
    #[error("MCP launch is not approved")]
    McpLaunchNotApproved,
    #[error("MCP tool is not approved")]
    McpToolNotApproved,
    #[error("MCP schema changed after search")]
    McpSchemaChanged,
}

pub fn authorize_mcp_use(
    tool: &McpToolIndexV4,
    expected_schema_sha256: &str,
) -> Result<(), KnowledgeErrorV4> {
    if !tool.configured {
        return Err(KnowledgeErrorV4::McpNotConfigured);
    }
    if !tool.enabled {
        return Err(KnowledgeErrorV4::McpDisabled);
    }
    if !tool.launch_approved {
        return Err(KnowledgeErrorV4::McpLaunchNotApproved);
    }
    if !tool.tool_approved {
        return Err(KnowledgeErrorV4::McpToolNotApproved);
    }
    if tool.schema_sha256 != expected_schema_sha256 {
        return Err(KnowledgeErrorV4::McpSchemaChanged);
    }
    Ok(())
}

pub fn markdown_sections(markdown: &str) -> Vec<SkillSectionV4> {
    let mut sections = Vec::new();
    let mut heading = "Introduction".to_string();
    let mut content = String::new();
    for line in markdown.lines() {
        if line.starts_with('#') {
            push_section(&mut sections, &heading, &content);
            heading = line.trim_start_matches('#').trim().to_owned();
            content.clear();
        } else {
            content.push_str(line);
            content.push('\n');
        }
    }
    push_section(&mut sections, &heading, &content);
    sections
}

fn push_section(sections: &mut Vec<SkillSectionV4>, heading: &str, content: &str) {
    let content = content.trim();
    if content.is_empty() {
        return;
    }
    sections.push(SkillSectionV4 {
        heading: heading.to_owned(),
        content: content.to_owned(),
        sha256: digest(content.as_bytes()),
    });
}

pub fn search_skills(
    query: &str,
    documents: &[SkillDocumentV4],
    limit: usize,
) -> Vec<SkillSearchHitV4> {
    let query_terms = lexical_terms(query);
    if query_terms.is_empty() {
        return Vec::new();
    }
    let mut hits = documents
        .iter()
        .filter(|document| document.enabled)
        .filter_map(|document| {
            let mut matching_sections = Vec::new();
            let mut score = weighted_overlap(&query_terms, &lexical_terms(&document.name)) * 3.0;
            for section in &document.sections {
                let section_score = weighted_overlap(
                    &query_terms,
                    &lexical_terms(&format!("{} {}", section.heading, section.content)),
                );
                if section_score > 0.0 {
                    matching_sections.push(section.heading.clone());
                    score += section_score;
                }
            }
            (score > 0.0).then(|| SkillSearchHitV4 {
                skill_id: document.skill_id,
                name: document.name.clone(),
                version: document.version.clone(),
                package_sha256: document.package_sha256.clone(),
                matching_sections,
                score,
            })
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.name.cmp(&right.name))
    });
    hits.truncate(limit.min(20));
    hits
}

pub fn freeze_skill(
    document: &SkillDocumentV4,
    headings: &[String],
) -> Result<FrozenSkillUseV4, KnowledgeErrorV4> {
    if !document.enabled {
        return Err(KnowledgeErrorV4::SkillUnavailable);
    }
    let selected = if headings.is_empty() {
        document.sections.clone()
    } else {
        let mut selected = Vec::new();
        for heading in headings {
            let section = document
                .sections
                .iter()
                .find(|section| section.heading == *heading)
                .ok_or_else(|| KnowledgeErrorV4::SectionMissing(heading.clone()))?;
            selected.push(section.clone());
        }
        selected
    };
    let frozen_sha256 = digest(
        &serde_json::to_vec(&(
            document.skill_id,
            &document.version,
            &document.package_sha256,
            &selected,
        ))
        .expect("skill freeze is serializable"),
    );
    Ok(FrozenSkillUseV4 {
        skill_id: document.skill_id,
        name: document.name.clone(),
        version: document.version.clone(),
        package_sha256: document.package_sha256.clone(),
        sections: selected,
        frozen_sha256,
    })
}

pub fn search_memory(
    query: &str,
    documents: &[MemoryDocumentV4],
    now: DateTime<Utc>,
    limit: usize,
) -> Vec<MemorySearchHitV4> {
    let query_terms = lexical_terms(query);
    if query_terms.is_empty() {
        return Vec::new();
    }
    let mut lexical = documents
        .iter()
        .filter_map(|document| {
            let terms = lexical_terms(&format!(
                "{} {} {} {}",
                document.dimension, document.key, document.statement, document.source_kind
            ));
            let score = weighted_overlap(&query_terms, &terms);
            (score > 0.0).then_some((document, score))
        })
        .collect::<Vec<_>>();
    lexical.sort_by(|(left_document, left), (right_document, right)| {
        right
            .total_cmp(left)
            .then_with(|| right_document.created_at.cmp(&left_document.created_at))
    });
    let lexical_rank = lexical
        .iter()
        .enumerate()
        .map(|(rank, (document, _))| (document.id, rank + 1))
        .collect::<BTreeMap<_, _>>();
    let mut recent = lexical
        .iter()
        .map(|(document, _)| *document)
        .collect::<Vec<_>>();
    recent.sort_by_key(|document| std::cmp::Reverse(document.created_at));
    let recency_rank = recent
        .iter()
        .enumerate()
        .map(|(rank, document)| (document.id, rank + 1))
        .collect::<BTreeMap<_, _>>();
    let mut hits = lexical
        .into_iter()
        .map(|(document, lexical_score)| {
            let lexical_rank = lexical_rank[&document.id];
            let recency_rank = recency_rank[&document.id];
            let age_days = (now - document.created_at).num_days().max(0) as f64;
            let recency_bonus = 1.0 / (1.0 + age_days / 30.0);
            let score = 1.0 / (60.0 + lexical_rank as f64)
                + 0.35 / (60.0 + recency_rank as f64)
                + 0.01 * lexical_score
                + 0.005 * recency_bonus;
            MemorySearchHitV4 {
                document: document.clone(),
                lexical_rank,
                recency_rank,
                score,
            }
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| right.score.total_cmp(&left.score));
    hits.truncate(limit.min(50));
    hits
}

pub fn search_mcp_tools(
    query: &str,
    tools: &[McpToolIndexV4],
    limit: usize,
) -> Vec<McpToolSearchHitV4> {
    let query_terms = lexical_terms(query);
    if query_terms.is_empty() {
        return Vec::new();
    }
    let mut hits = tools
        .iter()
        .filter(|tool| tool.configured && tool.enabled)
        .filter_map(|tool| {
            let score = weighted_overlap(
                &query_terms,
                &lexical_terms(&format!(
                    "{} {} {}",
                    tool.server_name, tool.tool_name, tool.description
                )),
            );
            (score > 0.0).then(|| McpToolSearchHitV4 {
                tool: tool.clone(),
                score,
            })
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| right.score.total_cmp(&left.score));
    hits.truncate(limit.min(20));
    hits
}

pub fn schema_digest(value: &Value) -> String {
    digest(&serde_json::to_vec(value).expect("schema is serializable"))
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn lexical_terms(value: &str) -> BTreeSet<String> {
    let normalized = value.to_lowercase();
    let mut terms = BTreeSet::new();
    let mut word = String::new();
    let mut cjk = String::new();
    let flush_word = |word: &mut String, terms: &mut BTreeSet<String>| {
        if !word.is_empty() {
            terms.insert(std::mem::take(word));
        }
    };
    let flush_cjk = |cjk: &mut String, terms: &mut BTreeSet<String>| {
        let chars = cjk.chars().collect::<Vec<_>>();
        for width in [2, 3] {
            for window in chars.windows(width) {
                terms.insert(window.iter().collect());
            }
        }
        if chars.len() == 1 {
            terms.insert(chars[0].to_string());
        }
        cjk.clear();
    };
    for character in normalized.chars() {
        if is_cjk(character) {
            flush_word(&mut word, &mut terms);
            cjk.push(character);
        } else {
            flush_cjk(&mut cjk, &mut terms);
            if character.is_alphanumeric() || character == '_' || character == '-' {
                word.push(character);
            } else {
                flush_word(&mut word, &mut terms);
            }
        }
    }
    flush_word(&mut word, &mut terms);
    flush_cjk(&mut cjk, &mut terms);
    terms
}

fn is_cjk(character: char) -> bool {
    matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn weighted_overlap(query: &BTreeSet<String>, document: &BTreeSet<String>) -> f64 {
    query
        .iter()
        .filter(|term| document.contains(*term))
        .map(|term| if term.chars().count() >= 3 { 2.0 } else { 1.0 })
        .sum::<f64>()
        / query.len().max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn unmatched_skills_are_not_returned_and_selected_sections_are_frozen() {
        let skill = SkillDocumentV4 {
            skill_id: Uuid::new_v4(),
            name: "single-cell-qc".into(),
            version: "1.2.0".into(),
            package_sha256: "package".into(),
            enabled: true,
            sections: markdown_sections(
                "# Quality control\nFilter low quality cells\n# Clustering\nRun Leiden",
            ),
        };
        assert!(search_skills("protein docking", &[skill.clone()], 5).is_empty());
        let hits = search_skills("quality cells", &[skill.clone()], 5);
        assert_eq!(hits.len(), 1);
        let frozen = freeze_skill(&skill, &["Quality control".into()]).unwrap();
        assert_eq!(frozen.sections.len(), 1);
        assert_eq!(frozen.package_sha256, "package");
        assert_eq!(frozen.frozen_sha256.len(), 64);
    }

    #[test]
    fn chinese_bigrams_trigrams_and_recency_rrf_find_project_memory() {
        let project_id = Uuid::new_v4();
        let now = Utc::now();
        let documents = vec![
            MemoryDocumentV4 {
                id: Uuid::new_v4(),
                project_id,
                conversation_id: None,
                dimension: "decision".into(),
                key: "细胞过滤".into(),
                statement: "单细胞质控使用线粒体比例阈值".into(),
                source_kind: "notebook".into(),
                source_id: "old".into(),
                conflicted_with: BTreeSet::new(),
                created_at: now - Duration::days(100),
            },
            MemoryDocumentV4 {
                id: Uuid::new_v4(),
                project_id,
                conversation_id: None,
                dimension: "decision".into(),
                key: "质控阈值".into(),
                statement: "最新单细胞质控阈值来自复核记录".into(),
                source_kind: "notebook".into(),
                source_id: "new".into(),
                conflicted_with: BTreeSet::new(),
                created_at: now,
            },
        ];
        let hits = search_memory("单细胞质控", &documents, now, 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].document.source_id, "new");
    }

    #[test]
    fn mcp_search_does_not_make_unmatched_or_disabled_schemas_visible() {
        let base = McpToolIndexV4 {
            server_id: Uuid::new_v4(),
            server_name: "literature".into(),
            tool_name: "search_papers".into(),
            description: "Search biomedical literature".into(),
            input_schema: serde_json::json!({"type":"object"}),
            schema_sha256: "schema".into(),
            configured: true,
            enabled: true,
            launch_approved: false,
            tool_approved: false,
            updated_at: Utc::now(),
        };
        assert_eq!(search_mcp_tools("literature", &[base.clone()], 5).len(), 1);
        assert!(search_mcp_tools("weather", &[base.clone()], 5).is_empty());
        assert!(
            search_mcp_tools(
                "literature",
                &[McpToolIndexV4 {
                    enabled: false,
                    ..base
                }],
                5
            )
            .is_empty()
        );
    }

    #[test]
    fn mcp_cannot_launch_until_all_four_states_and_schema_match() {
        let mut tool = McpToolIndexV4 {
            server_id: Uuid::new_v4(),
            server_name: "papers".into(),
            tool_name: "search".into(),
            description: "search".into(),
            input_schema: serde_json::json!({}),
            schema_sha256: "expected".into(),
            configured: true,
            enabled: true,
            launch_approved: false,
            tool_approved: true,
            updated_at: Utc::now(),
        };
        assert_eq!(
            authorize_mcp_use(&tool, "expected").unwrap_err(),
            KnowledgeErrorV4::McpLaunchNotApproved
        );
        tool.launch_approved = true;
        assert_eq!(
            authorize_mcp_use(&tool, "changed").unwrap_err(),
            KnowledgeErrorV4::McpSchemaChanged
        );
        assert!(authorize_mcp_use(&tool, "expected").is_ok());
    }
}
