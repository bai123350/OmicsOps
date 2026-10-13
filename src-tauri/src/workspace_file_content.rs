#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SmallWorkspaceTextIssue {
    TooLarge,
    UnsupportedContent,
    SensitiveContent,
}
/// Pure shared W10/W11 policy. No path opening, Store, DTO or credential access.
pub(crate) fn inspect_small_workspace_text<'a>(
    relative_path: &str,
    bytes: &'a [u8],
) -> Result<&'a str, SmallWorkspaceTextIssue> {
    if sensitive_workspace_filename(relative_path) {
        return Err(SmallWorkspaceTextIssue::SensitiveContent);
    }
    if bytes.len() > 512 * 1024 {
        return Err(SmallWorkspaceTextIssue::TooLarge);
    }
    let extension = relative_path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !["py", "r", "rmd", "md", "txt", "json", "csv", "tsv"].contains(&extension.as_str()) {
        return Err(SmallWorkspaceTextIssue::UnsupportedContent);
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| SmallWorkspaceTextIssue::UnsupportedContent)?;
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(SmallWorkspaceTextIssue::UnsupportedContent);
    }
    if !safe_workspace_public_text(relative_path) || !safe_workspace_public_text(text) {
        return Err(SmallWorkspaceTextIssue::SensitiveContent);
    }
    Ok(text)
}
/// Filename policy only; W02 remains the sole filesystem/path validator.
/// Credential-shaped components are excluded even when their bytes look harmless.
pub(crate) fn sensitive_workspace_filename(relative_path: &str) -> bool {
    relative_path.split(['/', '\\']).any(|component| {
        let lower = component.to_ascii_lowercase();
        if lower == ".env"
            || lower.starts_with(".env.")
            || matches!(lower.as_str(), ".ssh" | ".aws")
            || [".pem", ".key", ".p12", ".pfx"]
                .iter()
                .any(|suffix| lower.ends_with(suffix))
        {
            return true;
        }
        lower
            .trim_start_matches('.')
            .split(['.', '-', '_'])
            .any(|token| {
                matches!(
                    token,
                    "credential"
                        | "credentials"
                        | "auth"
                        | "authentication"
                        | "authorization"
                        | "password"
                        | "passwords"
                        | "passphrase"
                        | "secret"
                        | "secrets"
                        | "token"
                        | "tokens"
                        | "key"
                        | "keys"
                        | "privatekey"
                        | "apikey"
                        | "keystore"
                        | "rsa"
                        | "dsa"
                        | "ecdsa"
                        | "ed25519"
                )
            })
    })
}
/// Equality is intentional: even a harmless-looking public projection change
/// disqualifies original text. Authentication URIs never enter saved bodies.
pub(crate) fn safe_workspace_public_text(value: &str) -> bool {
    static ASSIGNMENTS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let assignments=ASSIGNMENTS.get_or_init(||regex::Regex::new(r#"(?i)(?:^|[^a-z0-9_])[\"']?(?:api[_-]?key|client[_-]?secret|aws[_-]?secret[_-]?access[_-]?key|secret[_-]?key|access[_-]?token|refresh[_-]?token|password|passphrase|authorization|token)[\"']?\s*[:=]\s*\S"#).expect("static workspace text policy"));
    !assignments.is_match(value)
        && crate::composer_references::public_text(value) == value
        && !value.match_indices("://").any(|(start, _)| {
            value[start + 3..]
                .split(['/', '?', '#', '\\', '"', '\'', ' ', '\n', '\r', '\t'])
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_text_safe_utf8_and_extensions() {
        for p in [
            "a.py", "a.R", "a.Rmd", "a.md", "a.txt", "a.json", "a.csv", "a.tsv",
        ] {
            assert_eq!(inspect_small_workspace_text(p, b"hello\n"), Ok("hello\n"));
        }
    }
    #[test]
    fn small_text_size_and_binary() {
        assert!(inspect_small_workspace_text("a.txt", &vec![b'x'; 512 * 1024]).is_ok());
        assert_eq!(
            inspect_small_workspace_text("a.txt", &vec![b'x'; 512 * 1024 + 1]),
            Err(SmallWorkspaceTextIssue::TooLarge)
        );
        assert_eq!(
            inspect_small_workspace_text("a.bin", b"hello"),
            Err(SmallWorkspaceTextIssue::UnsupportedContent)
        );
        assert_eq!(
            inspect_small_workspace_text("a.txt", b"\xff"),
            Err(SmallWorkspaceTextIssue::UnsupportedContent)
        );
        assert_eq!(
            inspect_small_workspace_text("a.txt", b"x\0y"),
            Err(SmallWorkspaceTextIssue::UnsupportedContent)
        );
    }
    #[test]
    fn small_text_sensitive_assignments_pem_uri_and_path() {
        for secret in [
            "password = 'FAKE_SENTINEL'",
            "api_key=FAKE_SENTINEL",
            "{\"token\":\"FAKE_SENTINEL\"}",
            "-----BEGIN PRIVATE KEY-----\nFAKE_SENTINEL\n-----END PRIVATE KEY-----",
            "https://user:FAKE_SENTINEL@example.invalid/x",
        ] {
            assert_eq!(
                inspect_small_workspace_text("a.txt", secret.as_bytes()),
                Err(SmallWorkspaceTextIssue::SensitiveContent)
            );
        }
        assert_eq!(
            inspect_small_workspace_text("password=FAKE_SENTINEL.txt", b"safe"),
            Err(SmallWorkspaceTextIssue::SensitiveContent)
        );
    }
    #[test]
    fn small_text_quoted_api_and_known_secret_fields() {
        for text in [
            r#"{"api_key":"FAKE_SENTINEL"}"#,
            r#"{"client_secret":"FAKE_SENTINEL"}"#,
            r#"{"aws_secret_access_key":"FAKE_SENTINEL"}"#,
        ] {
            assert_eq!(
                inspect_small_workspace_text("a.json", text.as_bytes()),
                Err(SmallWorkspaceTextIssue::SensitiveContent)
            );
        }
    }
    #[test]
    fn small_text_sensitive_filenames_and_components_precede_body_policy() {
        for path in [
            "credentials.json",
            "CREDENTIALS.JSON",
            "PrivateKey.JSON",
            "APIKEY.txt",
            ".env",
            "private.pem",
            "config/CrEdEnTiAlS/data.txt",
            "auth.json",
            "AUTH/settings.py",
            "authentication.txt",
            "private_key.txt",
            "PRIVATE-KEY/key.R",
            "id_rsa.txt",
            "SSH/id_ED25519.json",
            "api_key.json",
            "secrets/value.txt",
            ".env.txt",
            "config/.ENV/local.json",
        ] {
            assert_eq!(
                inspect_small_workspace_text(path, b"harmless UTF8"),
                Err(SmallWorkspaceTextIssue::SensitiveContent),
                "{path}"
            );
        }
        assert_eq!(
            inspect_small_workspace_text("research/authorship.md", b"safe"),
            Ok("safe")
        );
        assert_eq!(
            inspect_small_workspace_text("data/keyboard.txt", b"safe"),
            Ok("safe")
        );
    }
}
