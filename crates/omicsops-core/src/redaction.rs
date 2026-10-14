use regex::Regex;

pub fn redact_secrets(input: &str, secrets: &[impl AsRef<str>]) -> String {
    let mut values = secrets
        .iter()
        .map(AsRef::as_ref)
        .filter(|secret| !secret.is_empty())
        .collect::<Vec<_>>();
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));

    let mut redacted = input.to_owned();
    for secret in values {
        redacted = redacted.replace(secret, "[REDACTED]");
    }

    let patterns = [
        r"(?i)(api[_-]?key\s*[=:]\s*)[^\s,;]+",
        r"(?i)(authorization:\s*bearer\s+)[^\s]+",
    ];
    for pattern in patterns {
        let regex = Regex::new(pattern).expect("static redaction regex");
        redacted = regex.replace_all(&redacted, "${1}[REDACTED]").into_owned();
    }
    redacted
}

/// Common public text projection; applies the existing credential and PEM rules.
pub fn public_text(value: &str) -> String {
    let redacted = redact_secrets(value, &[] as &[&str]);
    let redacted = redact_sensitive_assignments(&redacted);
    redact_private_key_blocks(&redacted)
}

const SENSITIVE_ASSIGNMENT_KEYS: &[&str] = &[
    "authorization",
    "access_token",
    "refresh_token",
    "password",
    "passphrase",
    "token",
];

fn redact_sensitive_assignments(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut copied_until = 0;
    let mut search_from = 0;

    while let Some((key_start, key)) = find_sensitive_assignment_key(input, search_from) {
        if !is_assignment_key_boundary(input, key_start) {
            search_from = key_start + key.len();
            continue;
        }

        let key_end = key_start + key.len();
        let mut after_key = key_end;
        if matches!(input.as_bytes().get(after_key), Some(b'\'' | b'"'))
            && key_start > 0
            && input.as_bytes().get(key_start - 1) == input.as_bytes().get(after_key)
        {
            after_key += 1;
        }
        let delimiter_start = skip_horizontal_space(input, after_key);
        let Some(delimiter) = input.as_bytes().get(delimiter_start).copied() else {
            break;
        };
        if delimiter != b'=' && delimiter != b':' {
            search_from = key_end;
            continue;
        }

        let value_start = skip_horizontal_space(input, delimiter_start + 1);
        let Some(value_byte) = input.as_bytes().get(value_start).copied() else {
            break;
        };
        if value_byte == b'\r' || value_byte == b'\n' {
            search_from = key_end;
            continue;
        }

        if value_byte == b'\'' || value_byte == b'"' {
            let quote = value_byte as char;
            let (value_end, closed) = quoted_assignment_end(input, value_start, quote);
            output.push_str(&input[copied_until..value_start]);
            output.push_str("[REDACTED]");
            if closed {
                output.push(quote);
            }
            copied_until = value_end;
            search_from = value_end;
            continue;
        }

        let mut value_end = unquoted_assignment_end(input, value_start);
        if key.eq_ignore_ascii_case("authorization") {
            value_end = authorization_assignment_end(input, value_start, value_end);
        }
        if value_end == value_start {
            search_from = key_end;
            continue;
        }
        output.push_str(&input[copied_until..value_start]);
        output.push_str("[REDACTED]");
        copied_until = value_end;
        search_from = value_end;
    }

    output.push_str(&input[copied_until..]);
    output
}

fn find_sensitive_assignment_key(input: &str, search_from: usize) -> Option<(usize, &'static str)> {
    for (offset, _) in input[search_from..].char_indices() {
        let start = search_from + offset;
        let mut found: Option<&'static str> = None;
        for &key in SENSITIVE_ASSIGNMENT_KEYS {
            let Some(candidate) = input.as_bytes().get(start..start + key.len()) else {
                continue;
            };
            if !candidate.eq_ignore_ascii_case(key.as_bytes()) {
                continue;
            }
            if found.is_none_or(|found_key| key.len() > found_key.len()) {
                found = Some(key);
            }
        }
        if let Some(key) = found {
            return Some((start, key));
        }
    }
    None
}

fn is_assignment_key_boundary(input: &str, start: usize) -> bool {
    start == 0
        || !input.as_bytes()[start - 1].is_ascii_alphanumeric()
            && input.as_bytes()[start - 1] != b'_'
}

fn skip_horizontal_space(input: &str, mut index: usize) -> usize {
    while matches!(input.as_bytes().get(index), Some(b' ' | b'\t')) {
        index += 1;
    }
    index
}

fn quoted_assignment_end(input: &str, start: usize, quote: char) -> (usize, bool) {
    let mut index = start + quote.len_utf8();
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index always remains on a UTF-8 boundary");
        if character == '\\' {
            index += character.len_utf8();
            if let Some(escaped) = input[index..].chars().next() {
                index += escaped.len_utf8();
            }
        } else if character == quote {
            return (index + character.len_utf8(), true);
        } else {
            index += character.len_utf8();
        }
    }
    (input.len(), false)
}

fn unquoted_assignment_end(input: &str, start: usize) -> usize {
    let mut index = start;
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index always remains on a UTF-8 boundary");
        if character.is_whitespace() || matches!(character, ',' | ';' | '}' | ']' | ')') {
            break;
        }
        index += character.len_utf8();
    }
    index
}

fn authorization_assignment_end(input: &str, start: usize, first_end: usize) -> usize {
    let first = &input[start..first_end];
    if !first.eq_ignore_ascii_case("bearer") {
        return first_end;
    }
    let second_start = skip_horizontal_space(input, first_end);
    if second_start == first_end
        || matches!(input.as_bytes().get(second_start), Some(b'\r' | b'\n'))
    {
        return first_end;
    }
    unquoted_assignment_end(input, second_start)
}

fn redact_private_key_blocks(input: &str) -> String {
    const BEGIN_MARKER: &str = "-----BEGIN ";
    const PEM_DASHES: &str = "-----";
    let mut output = String::with_capacity(input.len());
    let mut copied_until = 0;
    let mut search_from = 0;

    while let Some(relative_start) = input[search_from..].find(BEGIN_MARKER) {
        let begin = search_from + relative_start;
        let label_start = begin + BEGIN_MARKER.len();
        let Some(relative_header_end) = input[label_start..].find(PEM_DASHES) else {
            break;
        };
        let header_end = label_start + relative_header_end;
        let label = &input[label_start..header_end];
        if !label.to_ascii_uppercase().ends_with("PRIVATE KEY") {
            search_from = header_end + PEM_DASHES.len();
            continue;
        }

        let end_marker = format!("-----END {label}-----");
        let body_start = header_end + PEM_DASHES.len();
        let block_end = input[body_start..]
            .find(&end_marker)
            .map(|relative_end| body_start + relative_end + end_marker.len())
            .unwrap_or(input.len());
        output.push_str(&input[copied_until..begin]);
        output.push_str("[REDACTED]");
        copied_until = block_end;
        search_from = block_end;
    }

    output.push_str(&input[copied_until..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_secrets_retains_its_existing_api_and_pattern_scope() {
        assert_eq!(
            redact_secrets("password=fixture secret", &["secret"]),
            "password=fixture [REDACTED]"
        );
        assert_eq!(
            redact_secrets(
                "api_key=fixture-key authorization: bearer fixture-auth",
                &[] as &[&str]
            ),
            "api_key=[REDACTED] authorization: bearer [REDACTED]"
        );
    }

    #[test]
    fn public_text_redacts_sensitive_assignments_and_preserves_nonassignments() {
        assert_eq!(
            public_text(
                "password=fixture-password access_token=fixture-access refresh_token=fixture-refresh passphrase=fixture-pass token=fixture-token authorization=fixture-auth"
            ),
            "password=[REDACTED] access_token=[REDACTED] refresh_token=[REDACTED] passphrase=[REDACTED] token=[REDACTED] authorization=[REDACTED]"
        );
        let ordinary = "Checking password, mypassword=ordinary access_token_count=ordinary";
        assert_eq!(public_text(ordinary), ordinary);
        assert_eq!(
            public_text("AUTHORIZATION=Bearer fixture-token followup"),
            "AUTHORIZATION=[REDACTED] followup"
        );
    }

    #[test]
    fn public_text_handles_quoted_utf8_and_escaped_credentials() {
        let input =
            r#"公开 "password": "fixture quoted 💩 \" secret", token='fixture \' value' followup"#;
        let result = public_text(input);
        assert!(!result.contains("fixture"));
        assert!(!result.contains("secret"));
        assert!(!result.contains("value"));
        assert!(result.contains("公开"));
        assert!(result.ends_with(" followup"));
        assert_eq!(result.matches("[REDACTED]").count(), 2);
    }

    #[test]
    fn public_text_removes_complete_and_partial_private_key_blocks() {
        assert_eq!(
            public_text(
                "before\n-----BEGIN PRIVATE KEY-----\nfixture material\n-----END PRIVATE KEY-----\nafter"
            ),
            "before\n[REDACTED]\nafter"
        );
        assert_eq!(
            public_text("before\n-----BEGIN OPENSSH PRIVATE KEY-----\nfixture partial"),
            "before\n[REDACTED]"
        );
        let certificate = "-----BEGIN CERTIFICATE-----\nordinary\n-----END CERTIFICATE-----";
        assert_eq!(public_text(certificate), certificate);
    }
}
