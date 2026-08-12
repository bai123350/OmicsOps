use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    CoreError, CoreResult,
    domain::{ResourceLimits, StepRisk},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolManifest {
    pub id: String,
    pub version: String,
    pub title: String,
    pub description: String,
    pub executable: String,
    pub argument_schema: Value,
    pub micromamba_dependencies: Vec<String>,
    pub allowed_domains: Vec<String>,
    pub default_risk: StepRisk,
    pub max_resources: ResourceLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolSummary {
    pub id: String,
    pub version: String,
    pub title: String,
    pub description: String,
    pub risk: StepRisk,
}

#[derive(Debug, Clone, Default)]
pub struct ToolCatalog {
    tools: BTreeMap<(String, String), ToolManifest>,
}

impl ToolCatalog {
    pub fn new(manifests: Vec<ToolManifest>) -> CoreResult<Self> {
        let mut tools = BTreeMap::new();
        for manifest in manifests {
            if manifest.id.trim().is_empty() || manifest.version.trim().is_empty() {
                return Err(CoreError::Validation(
                    "tool id and version must not be empty".into(),
                ));
            }
            if manifest.executable.trim().is_empty() {
                return Err(CoreError::Validation(format!(
                    "tool {}@{} has no fixed executable",
                    manifest.id, manifest.version
                )));
            }
            if manifest.argument_schema.get("type").and_then(Value::as_str) != Some("object")
                || !manifest
                    .argument_schema
                    .get("properties")
                    .is_some_and(Value::is_object)
            {
                return Err(CoreError::Validation(format!(
                    "tool {}@{} has an invalid argument schema",
                    manifest.id, manifest.version
                )));
            }
            let key = (manifest.id.clone(), manifest.version.clone());
            if tools.insert(key.clone(), manifest).is_some() {
                return Err(CoreError::Validation(format!(
                    "duplicate tool {}@{}",
                    key.0, key.1
                )));
            }
        }
        Ok(Self { tools })
    }

    pub fn get(&self, id: &str, version: &str) -> Option<&ToolManifest> {
        self.tools.get(&(id.to_owned(), version.to_owned()))
    }

    pub fn summaries(&self) -> Vec<ToolSummary> {
        self.tools
            .values()
            .map(|manifest| ToolSummary {
                id: manifest.id.clone(),
                version: manifest.version.clone(),
                title: manifest.title.clone(),
                description: manifest.description.clone(),
                risk: manifest.default_risk,
            })
            .collect()
    }

    pub fn manifests(&self) -> Vec<ToolManifest> {
        self.tools.values().cloned().collect()
    }
}

pub fn builtin_tool_catalog() -> CoreResult<ToolCatalog> {
    let definitions = [
        (
            "core.download",
            "Validated download",
            "curl",
            vec!["curl"],
            vec![],
        ),
        (
            "env.micromamba",
            "Micromamba environment",
            "micromamba",
            vec!["micromamba"],
            vec![],
        ),
        ("bio.fastqc", "FastQC", "fastqc", vec!["fastqc"], vec![]),
        ("bio.multiqc", "MultiQC", "multiqc", vec!["multiqc"], vec![]),
        (
            "bio.salmon",
            "Salmon quantification",
            "salmon",
            vec!["salmon"],
            vec![],
        ),
        (
            "bio.deseq2",
            "DESeq2 analysis",
            "Rscript",
            vec!["r-base", "bioconductor-deseq2", "bioconductor-tximport"],
            vec![],
        ),
        (
            "bio.scanpy",
            "Scanpy analysis",
            "python",
            vec!["python", "scanpy", "python-igraph", "leidenalg"],
            vec![],
        ),
        (
            "bio.h5ad_to_seurat",
            "h5ad to Seurat",
            "Rscript",
            vec!["r-base", "r-seurat", "r-hdf5r"],
            vec![],
        ),
        (
            "report.html",
            "HTML report",
            "python",
            vec!["python", "jinja2"],
            vec![],
        ),
        (
            "agent.remote_task",
            "Approved remote agent task",
            "omicsops-agent",
            Vec::new(),
            vec![],
        ),
    ];
    let limits = ResourceLimits::default();
    let manifests = definitions
        .into_iter()
        .map(|(id, title, executable, dependencies, domains)| ToolManifest {
            id: id.into(),
            version: "1.0.0".into(),
            title: title.into(),
            description: format!("Built-in OmicsOps tool contract for {title}."),
            executable: executable.into(),
            argument_schema: match id {
                "core.download" => json!({"type":"object","required":["url","output"],"properties":{"url":{"type":"string"},"output":{"type":"string"},"sha256":{"type":"string"}},"additionalProperties":false}),
                "env.micromamba" => json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"},"file":{"type":"string"}},"additionalProperties":false}),
                "bio.fastqc" => json!({"type":"object","required":["input","outdir"],"properties":{"input":{"type":"string"},"outdir":{"type":"string"},"threads":{"type":"integer"}},"additionalProperties":false}),
                "bio.multiqc" => json!({"type":"object","required":["input","outdir"],"properties":{"input":{"type":"string"},"outdir":{"type":"string"}},"additionalProperties":false}),
                "bio.salmon" => json!({"type":"object","required":["index","read1","read2","output"],"properties":{"index":{"type":"string"},"read1":{"type":"string"},"read2":{"type":"string"},"output":{"type":"string"},"threads":{"type":"integer"}},"additionalProperties":false}),
                "bio.scanpy" => json!({
                    "type": "object",
                    "required": ["input_directory", "outputs"],
                    "properties": {
                        "script": {"type":"string"},
                        "input_directory": {"type":"string"},
                        "input_format": {"type":"string"},
                        "matrix_path": {"type":"string"},
                        "genes_path": {"type":"string"},
                        "barcodes_path": {"type":"string"},
                        "gene_id_column": {"type":"integer"},
                        "gene_name_column": {"type":"integer"},
                        "make_var_names_unique": {"type":"boolean"},
                        "qc": {"type":"object"},
                        "normalization": {"type":"object"},
                        "embedding": {"type":"object"},
                        "clustering": {"type":"object"},
                        "annotation": {"type":"object"},
                        "outputs": {"type":"object"}
                    },
                    "additionalProperties": false
                }),
                "report.html" => json!({
                    "type":"object",
                    "required":["output_path"],
                    "properties":{
                        "script":{"type":"string"},
                        "inputs":{"type":"array"},
                        "output_path":{"type":"string"},
                        "sections":{"type":"array"},
                        "title":{"type":"string"}
                    },
                    "additionalProperties":false
                }),
                "agent.remote_task" => json!({
                    "type":"object",
                    "required":["goal","remote_observation","completion_criteria"],
                    "properties":{
                        "goal":{"type":"string"},
                        "remote_observation":{"type":"string"},
                        "completion_criteria":{"type":"array"},
                        "skill_context":{"type":"string"},
                        "max_iterations":{"type":"integer"}
                    },
                    "additionalProperties":false
                }),
                _ => json!({"type":"object","required":["script"],"properties":{"script":{"type":"string"},"input":{"type":"string"},"output":{"type":"string"}},"additionalProperties":false}),
            },
            micromamba_dependencies: dependencies.into_iter().map(str::to_owned).collect(),
            allowed_domains: domains.into_iter().map(str::to_owned).collect(),
            default_risk: StepRisk::Low,
            max_resources: limits.clone(),
        })
        .collect();
    ToolCatalog::new(manifests)
}
