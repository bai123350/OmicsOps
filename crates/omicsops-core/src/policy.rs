use regex::Regex;
use url::Url;

use crate::domain::StepRisk;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allowed,
    RequiresApproval { reason: String },
    Denied { reason: String },
}

#[derive(Debug, Clone)]
pub struct CommandPolicy {
    project_root: String,
    allowed_domains: Vec<String>,
}

impl CommandPolicy {
    pub fn new(project_root: impl Into<String>, allowed_domains: &[&str]) -> Self {
        Self {
            project_root: project_root.into().trim_end_matches('/').to_owned(),
            allowed_domains: allowed_domains
                .iter()
                .map(|domain| domain.to_ascii_lowercase())
                .collect(),
        }
    }

    pub fn evaluate(&self, command: &str, risk: StepRisk) -> PolicyDecision {
        let normalized = command.to_ascii_lowercase();

        if contains_command(&normalized, "sudo")
            || contains_command(&normalized, "su")
            || normalized.contains("/etc/sudoers")
            || normalized.contains("/root/")
        {
            return PolicyDecision::Denied {
                reason: "privilege escalation and privileged paths are forbidden".into(),
            };
        }

        if normalized.contains("~/.ssh")
            || normalized.contains("/.ssh/")
            || normalized.contains("/proc/self/environ")
            || normalized.contains("credentials")
        {
            return PolicyDecision::Denied {
                reason: "commands may not read credential stores or SSH secrets".into(),
            };
        }

        if let Some(path) = destructive_external_path(command, &self.project_root) {
            return PolicyDecision::Denied {
                reason: format!("destructive target is outside the project: {path}"),
            };
        }

        if let Some(domain) = unapproved_network_domain(command, &self.allowed_domains) {
            return PolicyDecision::Denied {
                reason: format!("network domain is not declared: {domain}"),
            };
        }

        if matches!(risk, StepRisk::Destructive)
            || normalized.contains("--overwrite")
            || normalized.contains("--force")
            || normalized.contains("rm -")
        {
            return PolicyDecision::RequiresApproval {
                reason: "the action may overwrite or delete project artifacts".into(),
            };
        }

        PolicyDecision::Allowed
    }
}

fn contains_command(command: &str, executable: &str) -> bool {
    let pattern = format!(r"(^|[;&|()\s]){}([;&|()\s]|$)", regex::escape(executable));
    Regex::new(&pattern)
        .expect("static command regex")
        .is_match(command)
}

fn destructive_external_path(command: &str, project_root: &str) -> Option<String> {
    let destructive = Regex::new(r"(?i)(^|[;&|]\s*)(rm|mv|truncate|shred)\s+").unwrap();
    if !destructive.is_match(command) {
        return None;
    }

    let absolute_path = Regex::new(r#"(?P<path>/[A-Za-z0-9._~+\-/]+)"#).unwrap();
    absolute_path
        .captures_iter(command)
        .filter_map(|capture| capture.name("path").map(|value| value.as_str()))
        .find(|path| {
            let normalized = path.trim_end_matches('/');
            normalized != project_root && !normalized.starts_with(&format!("{project_root}/"))
        })
        .map(str::to_owned)
}

fn unapproved_network_domain(command: &str, allowed_domains: &[String]) -> Option<String> {
    let url_pattern = Regex::new(r#"https?://[^\s"'<>]+"#).unwrap();
    for match_ in url_pattern.find_iter(command) {
        let Ok(url) = Url::parse(match_.as_str()) else {
            continue;
        };
        let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
            continue;
        };
        let allowed = allowed_domains
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));
        if !allowed {
            return Some(host);
        }
    }
    None
}
