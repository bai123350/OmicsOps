use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInspection {
    pub os: String,
    pub cpu_cores: u16,
    pub memory_kib: u64,
    pub disk_available_kib: u64,
    pub home: String,
    pub micromamba: Option<String>,
    pub remote_user: Option<String>,
    pub project_real_path: Option<String>,
    pub project_exists: bool,
    pub project_empty: bool,
    pub owner_matches: bool,
}

pub fn parse_server_inspection(raw: &str) -> Result<ServerInspection, String> {
    let values = raw
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim(), value.trim()))
        .collect::<BTreeMap<_, _>>();
    let required = |key: &str| {
        values
            .get(key)
            .copied()
            .ok_or_else(|| format!("server inspection omitted {key}"))
    };
    Ok(ServerInspection {
        os: required("OS")?.into(),
        cpu_cores: required("CPU")?
            .parse()
            .map_err(|_| "invalid CPU count".to_string())?,
        memory_kib: required("MEM_KIB")?
            .parse()
            .map_err(|_| "invalid memory value".to_string())?,
        disk_available_kib: required("DISK_KIB")?
            .parse()
            .map_err(|_| "invalid disk value".to_string())?,
        home: required("HOME")?.into(),
        micromamba: values
            .get("MAMBA")
            .filter(|value| !value.is_empty())
            .map(|value| (*value).to_owned()),
        remote_user: values.get("USER").map(|value| (*value).to_owned()),
        project_real_path: values
            .get("PROJECT_REAL")
            .filter(|value| !value.is_empty())
            .map(|value| (*value).to_owned()),
        project_exists: values
            .get("PROJECT_EXISTS")
            .is_none_or(|value| *value == "1"),
        project_empty: values
            .get("PROJECT_EMPTY")
            .is_none_or(|value| *value == "1"),
        owner_matches: values
            .get("OWNER_MATCHES")
            .is_none_or(|value| *value == "1"),
    })
}

pub fn inspection_command(remote_root: &str) -> String {
    let root = omicsops_core::project::shell_quote(remote_root);
    format!(
        "root={root}; \
         printf 'OS=%s\\n' \"$(uname -sr)\"; \
         printf 'CPU=%s\\n' \"$(getconf _NPROCESSORS_ONLN)\"; \
         printf 'MEM_KIB=%s\\n' \"$(awk '/MemTotal/ {{print $2}}' /proc/meminfo)\"; \
         parent=$(dirname \"$root\"); \
         printf 'DISK_KIB=%s\\n' \"$(df -Pk \"$parent\" | awk 'NR==2 {{print $4}}')\"; \
         printf 'HOME=%s\\n' \"$HOME\"; \
         printf 'USER=%s\\n' \"$(id -un)\"; \
         printf 'MAMBA=%s\\n' \"$(command -v micromamba || command -v conda || true)\"; \
         if test -e \"$root\"; then \
           printf 'PROJECT_EXISTS=1\\n'; \
           printf 'PROJECT_REAL=%s\\n' \"$(realpath \"$root\")\"; \
           test -z \"$(find \"$root\" -mindepth 1 -maxdepth 1 -print -quit)\" \
             && printf 'PROJECT_EMPTY=1\\n' || printf 'PROJECT_EMPTY=0\\n'; \
           test \"$(stat -c %U \"$root\")\" = \"$(id -un)\" \
             && printf 'OWNER_MATCHES=1\\n' || printf 'OWNER_MATCHES=0\\n'; \
         else \
           printf 'PROJECT_EXISTS=0\\nPROJECT_EMPTY=1\\nOWNER_MATCHES=1\\n'; \
           printf 'PROJECT_REAL=%s/%s\\n' \"$(realpath \"$parent\")\" \"$(basename \"$root\")\"; \
         fi"
    )
}
