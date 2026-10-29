//! Frozen SSH environment identifiers. Project environments name only direct
//! children of `.envs`; they never accept an arbitrary executable or prefix.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshEnvironmentLanguage {
    Python,
    R,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshEnvironment<'a> {
    System,
    Managed(&'a str),
    Project {
        python: Option<&'a str>,
        r: Option<&'a str>,
    },
}

const INVALID: &str = "environment must be system, a safe managed name, or an envs project binding";

pub fn safe_environment_basename(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
}

/// Protocol records retain their original 128-byte managed-name allowance.
/// Runtime managed names remain capped at 64 bytes in `parse`.
pub fn valid_compute_environment(value: &str) -> bool {
    if value.starts_with("envs:") {
        value.len() <= 136 && SshEnvironment::parse(value).is_ok()
    } else {
        safe_environment_basename(value, 128)
    }
}

impl<'a> SshEnvironment<'a> {
    pub fn parse(value: &'a str) -> Result<Self, &'static str> {
        if value == "system" {
            return Ok(Self::System);
        }
        if let Some(binding) = value.strip_prefix("envs:") {
            let mut parts = binding.split(':');
            let first = parts.next().ok_or(INVALID)?;
            let second = parts.next();
            if parts.next().is_some() {
                return Err(INVALID);
            }
            let name = |part: &'a str| -> Result<Option<&'a str>, &'static str> {
                if part.is_empty() {
                    Ok(None)
                } else if safe_environment_basename(part, 64) {
                    Ok(Some(part))
                } else {
                    Err(INVALID)
                }
            };
            let python = name(first)?;
            let r = match second {
                Some(part) => name(part)?,
                None => python,
            };
            if python.is_none() && r.is_none() {
                return Err(INVALID);
            }
            return Ok(Self::Project { python, r });
        }
        if safe_environment_basename(value, 64) {
            Ok(Self::Managed(value))
        } else {
            Err(INVALID)
        }
    }

    pub fn is_project(self) -> bool {
        matches!(self, Self::Project { .. })
    }

    pub fn prefix(
        self,
        root: &str,
        language: SshEnvironmentLanguage,
    ) -> Result<Option<String>, &'static str> {
        let root = root.trim_end_matches('/');
        match self {
            Self::System => Ok(None),
            Self::Managed(name) => Ok(Some(format!("{root}/.omicsops/environments/{name}"))),
            Self::Project { python, r } => {
                let name = match language {
                    SshEnvironmentLanguage::Python => python,
                    SshEnvironmentLanguage::R => r,
                }
                .ok_or("the frozen project environment has no binding for this language")?;
                Ok(Some(format!("{root}/.envs/{name}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_environment_bindings_resolve_each_language_without_fallback() {
        let pair = SshEnvironment::parse("envs:python311:r443").unwrap();
        assert_eq!(
            pair.prefix("/project/", SshEnvironmentLanguage::Python)
                .unwrap()
                .as_deref(),
            Some("/project/.envs/python311")
        );
        assert_eq!(
            pair.prefix("/project", SshEnvironmentLanguage::R)
                .unwrap()
                .as_deref(),
            Some("/project/.envs/r443")
        );
        let python = SshEnvironment::parse("envs:python311:").unwrap();
        assert!(
            python
                .prefix("/project", SshEnvironmentLanguage::R)
                .is_err()
        );
        let r = SshEnvironment::parse("envs::r443").unwrap();
        assert!(
            r.prefix("/project", SshEnvironmentLanguage::Python)
                .is_err()
        );
        let single = SshEnvironment::parse("envs:shared").unwrap();
        assert_eq!(
            single.prefix("/project", SshEnvironmentLanguage::Python),
            single.prefix("/project", SshEnvironmentLanguage::R)
        );
    }

    #[test]
    fn ssh_environment_legacy_names_keep_their_existing_scope() {
        assert_eq!(
            SshEnvironment::parse("system")
                .unwrap()
                .prefix("/project", SshEnvironmentLanguage::Python)
                .unwrap(),
            None
        );
        assert_eq!(
            SshEnvironment::parse("analysis-r")
                .unwrap()
                .prefix("/project", SshEnvironmentLanguage::R)
                .unwrap()
                .as_deref(),
            Some("/project/.omicsops/environments/analysis-r")
        );
        assert!(valid_compute_environment(&"x".repeat(128)));
        assert!(!valid_compute_environment(&"x".repeat(129)));
        assert!(SshEnvironment::parse(&"x".repeat(65)).is_err());
    }

    #[test]
    fn ssh_environment_tokens_reject_traversal_controls_and_unbounded_names() {
        for token in [
            "",
            ".",
            "..",
            "../outside",
            "/absolute",
            "bad/name",
            "envs:",
            "envs::",
            "envs:..",
            "envs:.",
            "envs:/outside",
            "envs:a:b:c",
            "envs:a:bad\\name",
            "envs:a:r\n",
            "envs:a:r\0",
            "envs:a:r;id",
        ] {
            assert!(SshEnvironment::parse(token).is_err(), "{token:?}");
            assert!(!valid_compute_environment(token), "{token:?}");
        }
        assert!(
            SshEnvironment::parse(&format!("envs:{}:{}", "a".repeat(64), "b".repeat(64))).is_ok()
        );
        assert!(SshEnvironment::parse(&format!("envs:{}", "a".repeat(65))).is_err());
    }
}
