use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationMethod {
    Password,
    PrivateKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConnectionProfile {
    pub id: Uuid,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub authentication: AuthenticationMethod,
    pub authentication_reference: String,
    pub host_key_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DataSource {
    pub label: String,
    pub url: Url,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResourceLimits {
    pub max_cpu_cores: u16,
    pub max_memory_gib: u32,
    pub max_disk_gib: u32,
    pub max_step_seconds: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_cpu_cores: 8,
            max_memory_gib: 32,
            max_disk_gib: 200,
            max_step_seconds: 86_400,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectSpec {
    pub id: Uuid,
    pub connection_id: Uuid,
    pub remote_root: String,
    pub plan_summary: String,
    pub data_sources: Vec<DataSource>,
    pub resource_limits: ResourceLimits,
    pub allowed_network_domains: Vec<String>,
}
