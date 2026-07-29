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
