//! Best-effort approval routing for runtime code. Execution remains subject to
//! the existing frozen capabilities, host validation, and per-call approvals.

use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Word(String),
    String(String),
    Mark(u8),
}

pub(crate) fn ordinary_runtime_call_is_low_risk(arguments: &Value) -> bool {
    if arguments
        .get("background")
        .is_some_and(|value| value != false)
        || arguments.get("capture_paths").is_some_and(|value| {
            !value.is_array()
                || value
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p.as_str().is_none_or(unsafe_project_path))
        })
    {
        return false;
    }
    let Some(language) = arguments.get("language").and_then(Value::as_str) else {
        return false;
    };
    let Some(code) = arguments.get("code").and_then(Value::as_str) else {
        return false;
    };
    if code.trim().is_empty() || code.len() > 256 * 1024 {
        return false;
    }
    let Some(tokens) = lex(code, language == "r", 0) else {
        return false;
    };
    match language {
        "python" => python_is_low_risk(&tokens, code),
        "r" => r_is_low_risk(&tokens),
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ApprovalReason {
    UnsupportedInput,
    BackgroundOrCapture,
    UnparseableCode,
    DangerousOperation,
    OutsideProject,
    UnsupportedOsOperation,
    UnprovenProjectPath,
    UnclassifiedCode,
}

impl ApprovalReason {
    fn message(self) -> &'static str {
        match self {
            Self::UnsupportedInput => {
                "Runtime language or code is unsupported for automatic approval"
            }
            Self::BackgroundOrCapture => "Background execution or capture paths require approval",
            Self::UnparseableCode => "Code could not be classified safely",
            Self::DangerousOperation => "Code contains an operation that requires approval",
            Self::OutsideProject => "Code references an external or protected path",
            Self::UnsupportedOsOperation => "Unsupported OS operation requires approval",
            Self::UnprovenProjectPath => "Project path cannot be proven from this code",
            Self::UnclassifiedCode => "Runtime code is outside the ordinary approval pattern",
        }
    }
}

fn add_reason(reasons: &mut Vec<ApprovalReason>, reason: ApprovalReason) {
    if reasons.len() < 4 && !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

pub(crate) fn ordinary_runtime_call_approval_reason(arguments: &Value) -> Option<String> {
    if ordinary_runtime_call_is_low_risk(arguments) {
        return None;
    }
    let mut reasons = Vec::new();
    if arguments
        .get("background")
        .is_some_and(|value| value != false)
        || arguments.get("capture_paths").is_some_and(|value| {
            !value.is_array()
                || value
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|path| path.as_str().is_none_or(unsafe_project_path))
        })
    {
        add_reason(&mut reasons, ApprovalReason::BackgroundOrCapture);
    }
    let language = arguments.get("language").and_then(Value::as_str);
    let code = arguments.get("code").and_then(Value::as_str);
    if !matches!(language, Some("python" | "r"))
        || code.is_none_or(|code| code.trim().is_empty() || code.len() > 256 * 1024)
    {
        add_reason(&mut reasons, ApprovalReason::UnsupportedInput);
    } else if let (Some(language), Some(code)) = (language, code) {
        if let Some(tokens) = lex(code, language == "r", 0) {
            // The OS proof is global. When a distinct unsupported operation
            // exists, it cannot identify which path argument failed.
            let unsupported_os = tokens.iter().enumerate().any(|(i, token)| {
                is_word(Some(token), "os")
                    && is_mark(tokens.get(i + 1), b'.')
                    && !is_word(tokens.get(i + 2), "makedirs")
                    && !(is_word(tokens.get(i + 2), "path")
                        && is_mark(tokens.get(i + 3), b'.')
                        && ["join", "getsize", "exists"]
                            .iter()
                            .any(|method| is_word(tokens.get(i + 4), method)))
            });
            let os_proof_failed = language == "python"
                && !unsupported_os
                && tokens.iter().any(|token| is_word(Some(token), "os"))
                && !super::runtime_approval_ast::safe_os_path_uses(code);
            for (i, token) in tokens.iter().enumerate() {
                if let Token::String(value) = token {
                    let path_argument = is_mark(tokens.get(i.wrapping_sub(1)), b'/')
                        || (is_mark(tokens.get(i.wrapping_sub(1)), b'(')
                            && ["Path", "open"]
                                .iter()
                                .any(|name| is_word(tokens.get(i.wrapping_sub(2)), name)));
                    if unmistakable_outside_path(value)
                        || (path_argument && unsafe_project_path(value))
                    {
                        add_reason(&mut reasons, ApprovalReason::OutsideProject);
                    }
                }
                if let Token::Word(word) = token {
                    if language == "python"
                        && ([
                            "subprocess",
                            "socket",
                            "ctypes",
                            "shutil",
                            "keyring",
                            "winreg",
                            "eval",
                            "exec",
                        ]
                        .contains(&word.as_str())
                            || ([
                                "remove", "unlink", "rmtree", "rename", "chmod", "chown",
                                "symlink", "system", "popen",
                            ]
                            .contains(&word.as_str())
                                && is_mark(tokens.get(i + 1), b'(')))
                    {
                        add_reason(&mut reasons, ApprovalReason::DangerousOperation);
                    }
                    if language == "r"
                        && [
                            "system", "system2", "shell", "unlink", "eval", "parse", "source",
                        ]
                        .contains(&word.as_str())
                        && is_mark(tokens.get(i + 1), b'(')
                    {
                        add_reason(&mut reasons, ApprovalReason::DangerousOperation);
                    }
                    if language == "python" && word == "os" && is_mark(tokens.get(i + 1), b'.') {
                        if is_word(tokens.get(i + 2), "path") && is_mark(tokens.get(i + 3), b'.') {
                            if !["join", "getsize", "exists"]
                                .iter()
                                .any(|method| is_word(tokens.get(i + 4), method))
                            {
                                add_reason(&mut reasons, ApprovalReason::UnsupportedOsOperation);
                            } else if os_proof_failed {
                                if let Some(Token::String(path)) = tokens.get(i + 6) {
                                    if unsafe_project_path(path) {
                                        add_reason(&mut reasons, ApprovalReason::OutsideProject);
                                    }
                                } else if let Some(Token::Word(name)) = tokens.get(i + 6)
                                    && !tokens.windows(2).any(|pair| {
                                        is_word(pair.first(), name) && is_mark(pair.get(1), b'=')
                                    })
                                {
                                    add_reason(&mut reasons, ApprovalReason::UnprovenProjectPath);
                                }
                            }
                        } else if !is_word(tokens.get(i + 2), "makedirs") {
                            add_reason(&mut reasons, ApprovalReason::UnsupportedOsOperation);
                        }
                    }
                }
            }
            if os_proof_failed {
                add_reason(&mut reasons, ApprovalReason::UnprovenProjectPath);
            }
        } else {
            add_reason(&mut reasons, ApprovalReason::UnparseableCode);
        }
    }
    if reasons.is_empty() {
        add_reason(&mut reasons, ApprovalReason::UnclassifiedCode);
    }
    Some(
        reasons
            .iter()
            .map(|reason| reason.message())
            .collect::<Vec<_>>()
            .join("; "),
    )
}

pub(super) fn unsafe_project_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let trimmed = path.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with('~')
        || trimmed.starts_with("//")
        || (trimmed.len() >= 2 && trimmed.as_bytes()[1] == b':')
    {
        return true;
    }
    trimmed.split('/').any(|part| {
        part == ".."
            || matches!(
                part.to_ascii_lowercase().as_str(),
                ".git" | ".codex" | ".agents" | ".omicsops" | "credentials" | "secrets"
            )
    })
}

fn unmistakable_outside_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() > 1 && bytes[0] == b'/')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
        || value.starts_with("../")
        || value.starts_with("..\\")
        || value.contains("/../")
        || value.contains("\\..\\")
        || value.starts_with("\\\\") && bytes.get(2).is_some_and(u8::is_ascii_alphanumeric)
}

fn lex(code: &str, r: bool, depth: usize) -> Option<Vec<Token>> {
    if depth > 8 {
        return None;
    }
    let bytes = code.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut stack = Vec::new();
    let mut line_start = true;
    let mut indented = false;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if ch.is_ascii_whitespace() {
            if !r && ch == b'\n' && stack.is_empty() {
                tokens.push(Token::Mark(b'\n'));
                line_start = true;
                indented = false;
            } else if !r && line_start && stack.is_empty() && matches!(ch, b' ' | b'\t') {
                indented = true;
            }
            i += 1;
            continue;
        }
        if !r && line_start && indented && stack.is_empty() {
            tokens.push(Token::Mark(b'\t'));
        }
        line_start = false;
        indented = false;
        if ch.is_ascii_alphabetic() || ch == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &code[start..i];
            if !r
                && i < bytes.len()
                && matches!(bytes[i], b'\'' | b'"')
                && word.len() <= 3
                && word
                    .bytes()
                    .all(|b| matches!(b.to_ascii_lowercase(), b'r' | b'u' | b'b' | b'f'))
            {
                let (value, end, expressions) =
                    scan_string(code, i, word.contains(['f', 'F']), depth)?;
                tokens.push(Token::String(value));
                for expr in expressions {
                    tokens.extend(lex(&expr, false, depth + 1)?);
                }
                i = end;
                continue;
            }
            tokens.push(Token::Word(word.to_owned()));
            continue;
        }
        if matches!(ch, b'\'' | b'"') {
            let (value, end, _) = scan_string(code, i, false, depth)?;
            tokens.push(Token::String(value));
            i = end;
            continue;
        }
        if ch.is_ascii_digit() {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.' || bytes[i] == b'_')
            {
                i += 1;
            }
            continue;
        }
        if ch == b'\\' && bytes.get(i + 1) == Some(&b'\n') {
            i += 2;
            continue;
        }
        if matches!(ch, b'(' | b'[' | b'{') {
            stack.push(ch);
        }
        if matches!(ch, b')' | b']' | b'}') {
            let open = stack.pop()?;
            if !matches!((open, ch), (b'(', b')') | (b'[', b']') | (b'{', b'}')) {
                return None;
            }
        }
        let allowed = b".,:;+-*/%<>=!&|^~@?";
        if !matches!(ch, b'(' | b')' | b'[' | b']' | b'{' | b'}')
            && !allowed.contains(&ch)
            && !(r && matches!(ch, b'$' | b'\\'))
        {
            return None;
        }
        tokens.push(Token::Mark(ch));
        i += 1;
    }
    stack.is_empty().then_some(tokens)
}

fn scan_string(
    code: &str,
    start: usize,
    fstring: bool,
    depth: usize,
) -> Option<(String, usize, Vec<String>)> {
    let bytes = code.as_bytes();
    let quote = bytes[start];
    let triple = bytes.get(start + 1) == Some(&quote) && bytes.get(start + 2) == Some(&quote);
    let width = if triple { 3 } else { 1 };
    let mut i = start + width;
    let mut expressions = Vec::new();
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = i.checked_add(2)?;
            continue;
        }
        if fstring && bytes[i] == b'{' {
            if bytes.get(i + 1) == Some(&b'{') {
                i += 2;
                continue;
            }
            let begin = i + 1;
            let mut nesting = 1;
            i += 1;
            while i < bytes.len() && nesting > 0 {
                if matches!(bytes[i], b'\'' | b'"') {
                    let (_, end, _) = scan_string(code, i, false, depth + 1)?;
                    i = end;
                    continue;
                }
                match bytes[i] {
                    b'{' => nesting += 1,
                    b'}' => nesting -= 1,
                    _ => {}
                }
                i += 1;
            }
            if nesting != 0 {
                return None;
            }
            expressions.push(code[begin..i - 1].to_owned());
            continue;
        }
        if fstring && bytes[i] == b'}' && bytes.get(i + 1) == Some(&b'}') {
            i += 2;
            continue;
        }
        if fstring && bytes[i] == b'}' {
            return None;
        }
        if bytes[i] == quote
            && (!triple || (bytes.get(i + 1) == Some(&quote) && bytes.get(i + 2) == Some(&quote)))
        {
            return Some((code[start + width..i].to_owned(), i + width, expressions));
        }
        if !triple && bytes[i] == b'\n' {
            return None;
        }
        i += 1;
    }
    None
}

fn is_word(token: Option<&Token>, value: &str) -> bool {
    matches!(token, Some(Token::Word(word)) if word == value)
}

fn is_mark(token: Option<&Token>, value: u8) -> bool {
    matches!(token, Some(Token::Mark(mark)) if *mark == value)
}

fn call_named(tokens: &[Token], i: usize, name: &str) -> bool {
    is_word(tokens.get(i), name) && is_mark(tokens.get(i + 1), b'(')
}

fn python_is_low_risk(tokens: &[Token], code: &str) -> bool {
    const RISKY_IMPORTS: &[&str] = &[
        "subprocess",
        "socket",
        "ctypes",
        "importlib",
        "pip",
        "shutil",
        "keyring",
        "winreg",
        "multiprocessing",
        "tempfile",
    ];
    const RISKY_CALLS: &[&str] = &[
        "eval",
        "exec",
        "__import__",
        "getattr",
        "setattr",
        "delattr",
        "globals",
        "locals",
        "input",
        "system",
        "popen",
        "spawn",
        "fork",
        "remove",
        "unlink",
        "rmdir",
        "rmtree",
        "rename",
        "chmod",
        "chown",
        "symlink",
        "link",
        "expanduser",
        "resolve",
        "getenv",
        "putenv",
        "unsetenv",
        "install",
        "exit",
        "chdir",
    ];
    const MUTATING_HTTP: &[&str] = &["post", "put", "patch", "delete"];
    if tokens.iter().any(|token| is_word(Some(token), "os"))
        && !super::runtime_approval_ast::safe_os_path_uses(code) {
        return false;
    }
    let mut path_names = std::collections::HashSet::new();
    for pair in tokens.windows(4) {
        if let [
            Token::Word(name),
            Token::Mark(b'='),
            Token::Word(source),
            Token::Mark(next),
        ] = pair
        {
            if (source == "Path" && *next == b'(')
                || (path_names.contains(source.as_str()) && *next == b'/')
            {
                path_names.insert(name.as_str());
            }
        }
    }
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::String(value) => {
                if unmistakable_outside_path(value) {
                    return false;
                }
                let path_argument = is_mark(tokens.get(i.wrapping_sub(1)), b'/')
                    || (is_mark(tokens.get(i.wrapping_sub(1)), b'(')
                        && ["Path", "open"]
                            .iter()
                            .any(|name| is_word(tokens.get(i.wrapping_sub(2)), name)));
                if path_argument && unsafe_project_path(value) {
                    return false;
                }
            }
            Token::Word(word) => {
                if word == "environ"
                    || word == "__dict__"
                    || word == "__class__"
                    || word == "__builtins__"
                {
                    return false;
                }
                if RISKY_IMPORTS.contains(&word.as_str()) {
                    return false;
                }
                if [
                    "eval",
                    "exec",
                    "__import__",
                    "getattr",
                    "setattr",
                    "delattr",
                    "globals",
                    "locals",
                ]
                .contains(&word.as_str())
                {
                    return false;
                }
                if word == "compile"
                    && !(is_mark(tokens.get(i.wrapping_sub(1)), b'.')
                        && is_word(tokens.get(i.wrapping_sub(2)), "re"))
                {
                    return false;
                }
                if ["Path", "Request", "urlopen"].contains(&word.as_str())
                    && is_word(tokens.get(i + 1), "as")
                {
                    return false;
                }
                if (word == "import" || word == "from")
                    && tokens
                        .get(i + 1)
                        .is_some_and(|next| matches!(next, Token::Mark(b'*')))
                {
                    return false;
                }
                if word == "import" && is_mark(tokens.get(i + 1), b'*') {
                    return false;
                }
                if RISKY_CALLS.contains(&word.as_str()) && is_mark(tokens.get(i + 1), b'(') {
                    return false;
                }
                if MUTATING_HTTP.contains(&word.as_str())
                    && is_mark(tokens.get(i.wrapping_sub(1)), b'.')
                    && is_mark(tokens.get(i + 1), b'(')
                {
                    return false;
                }
                if word == "Request" && is_mark(tokens.get(i + 1), b'(') {
                    if request_is_mutating(tokens, i + 1, false)
                        || call_has_second_positional(tokens, i + 1)
                    {
                        return false;
                    }
                }
                if word == "urlopen" && is_mark(tokens.get(i + 1), b'(') {
                    if call_has_keyword(tokens, i + 1, "data")
                        || call_has_second_positional(tokens, i + 1)
                    {
                        return false;
                    }
                }
                if word == "replace"
                    && is_mark(tokens.get(i.wrapping_sub(1)), b'.')
                    && is_mark(tokens.get(i + 1), b'(')
                {
                    // str.replace is common when cleaning literature text. Path.replace
                    // is covered when the receiver is visibly path-shaped.
                    if matches!(tokens.get(i.wrapping_sub(2)), Some(Token::Word(receiver)) if path_names.contains(receiver.as_str()))
                        || path_constructor_before_method(tokens, i)
                    {
                        return false;
                    }
                }
                if word == "home"
                    && is_mark(tokens.get(i.wrapping_sub(1)), b'.')
                    && is_mark(tokens.get(i + 1), b'(')
                {
                    return false;
                }
                if word == "request"
                    && is_mark(tokens.get(i + 1), b'(')
                    && request_is_mutating(tokens, i + 1, true)
                {
                    return false;
                }
            }
            _ => {}
        }
    }
    !tokens
        .windows(2)
        .any(|pair| pair == [Token::Word("import".into()), Token::Mark(b'*')])
}

fn path_constructor_before_method(tokens: &[Token], method: usize) -> bool {
    if !is_mark(tokens.get(method.wrapping_sub(2)), b')') {
        return false;
    }
    let mut depth = 0;
    for i in (0..method - 2).rev() {
        match tokens[i] {
            Token::Mark(b')') => depth += 1,
            Token::Mark(b'(') => {
                if depth == 0 {
                    return is_word(tokens.get(i.wrapping_sub(1)), "Path");
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    false
}

fn call_has_second_positional(tokens: &[Token], open: usize) -> bool {
    let mut depth = 0;
    for i in open..tokens.len() {
        match tokens[i] {
            Token::Mark(b'(') | Token::Mark(b'[') | Token::Mark(b'{') => depth += 1,
            Token::Mark(b')') | Token::Mark(b']') | Token::Mark(b'}') => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Token::Mark(b',') if depth == 1 => {
                return !matches!(tokens.get(i + 2), Some(Token::Mark(b'=')));
            }
            _ => {}
        }
    }
    false
}

fn call_has_keyword(tokens: &[Token], open: usize, keyword: &str) -> bool {
    let mut depth = 0;
    for i in open..tokens.len() {
        match tokens[i] {
            Token::Mark(b'(') => depth += 1,
            Token::Mark(b')') => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        if depth == 1 && is_word(tokens.get(i), keyword) && is_mark(tokens.get(i + 1), b'=') {
            return true;
        }
    }
    false
}

fn request_is_mutating(tokens: &[Token], open: usize, first_is_method: bool) -> bool {
    if call_has_keyword(tokens, open, "data") {
        return true;
    }
    if first_is_method
        && !matches!(tokens.get(open + 1), Some(Token::String(method)) if method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD"))
    {
        return true;
    }
    if !call_has_keyword(tokens, open, "method") {
        return false;
    }
    let mut depth = 0;
    for i in open..tokens.len() {
        match tokens[i] {
            Token::Mark(b'(') => depth += 1,
            Token::Mark(b')') => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        if depth == 1 && is_word(tokens.get(i), "method") && is_mark(tokens.get(i + 1), b'=') {
            return !matches!(tokens.get(i + 2), Some(Token::String(value)) if value.eq_ignore_ascii_case("GET") || value.eq_ignore_ascii_case("HEAD"));
        }
    }
    true
}

fn r_is_low_risk(tokens: &[Token]) -> bool {
    const RISKY: &[&str] = &[
        "system",
        "system2",
        "shell",
        "unlink",
        "eval",
        "parse",
        "source",
        "setwd",
        "normalizePath",
    ];
    for (i, token) in tokens.iter().enumerate() {
        if let Token::String(value) = token {
            if unmistakable_outside_path(value) {
                return false;
            }
            if unsafe_project_path(value)
                && (value.contains('/') || value.contains('\\'))
                && is_mark(tokens.get(i.wrapping_sub(1)), b'(')
            {
                return false;
            }
        }
        if let Token::Word(name) = token {
            if [
                "POST",
                "PUT",
                "PATCH",
                "DELETE",
                "VERB",
                "req_method",
                "curl_upload",
            ]
            .contains(&name.as_str())
                && is_mark(tokens.get(i + 1), b'(')
            {
                return false;
            }
            if RISKY.contains(&name.as_str()) && is_mark(tokens.get(i + 1), b'(') {
                return false;
            }
            if name == "Sys"
                && is_mark(tokens.get(i + 1), b'.')
                && (call_named(tokens, i + 2, "getenv") || call_named(tokens, i + 2, "setenv"))
            {
                return false;
            }
            if name == "file"
                && is_mark(tokens.get(i + 1), b'.')
                && call_named(tokens, i + 2, "remove")
            {
                return false;
            }
            if name == "install"
                && is_mark(tokens.get(i + 1), b'.')
                && call_named(tokens, i + 2, "packages")
            {
                return false;
            }
            if name == "dyn"
                && is_mark(tokens.get(i + 1), b'.')
                && call_named(tokens, i + 2, "load")
            {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn python(code: &str) -> Value {
        json!({"language":"python","code":code,"environment":"system"})
    }

    #[test]
    fn public_literature_get_and_project_artifacts_are_ordinary() {
        let code = r#"
import json, urllib.request, urllib.parse, re, html, csv, time, sys
from pathlib import Path
from datetime import datetime, timezone
from collections import Counter
OUT = Path("results/literature")
OUT.mkdir(parents=True, exist_ok=True)
url = "https://www.ebi.ac.uk/europepmc/webservices/rest/search?query=PBMC"
req = urllib.request.Request(url, headers={"Accept": "application/json"})
with urllib.request.urlopen(req, timeout=60) as response:
    data = json.load(response)
out_tsv = OUT / "papers.tsv"
with out_tsv.open("w", encoding="utf-8", newline="") as handle:
    csv.writer(handle, delimiter="\t").writerow(["title", "doi"])
L = [f"| {r['title'].replace('|', '\\|')} |" for r in data.get("resultList", {}).get("result", [])]
(OUT / "papers.md").write_text("\n".join(L), encoding="utf-8")
(OUT / "raw.json").write_text(json.dumps(data), encoding="utf-8")
"#;
        let tokens = lex(code, false, 0).expect("representative Python tokenizes");
        assert!(python_is_low_risk(&tokens, code), "{tokens:?}");
        assert!(ordinary_runtime_call_is_low_risk(&python(code)));
    }

    #[test]
    fn project_relative_os_path_metadata_is_ordinary() {
        let code = r#"
import csv, json, os
root = "results/review"
table = os.path.join(root, "study-counts.csv")
with open(table, encoding="utf-8") as handle:
    rows = list(csv.DictReader(handle))
size = os.path.getsize(table)
print(json.dumps({"rows": len(rows), "bytes": size}))
"#;
        assert!(ordinary_runtime_call_is_low_risk(&python(code)));
        assert!(ordinary_runtime_call_is_low_risk(&python(
            "import os\nprint(os.path.getsize(os.path.join('results', 'report.tsv')))"
        )));
    }

    #[test]
    fn project_relative_exists_checks_are_ordinary() {
        for code in [
            "import os\nprint(os.path.exists('results/review.csv'))",
            "import os\np = 'results/review.csv'\nif os.path.exists(p):\n    print('present')",
            "import os\nprint(os.path.exists(os.path.join('results', 'review.csv')))",
        ] {
            assert!(ordinary_runtime_call_is_low_risk(&python(code)), "{code}");
        }
    }

    #[test]
    fn project_date_named_output_is_ordinary() {
        let code = r#"
import csv, datetime, json, os, urllib.request
now = datetime.datetime.now(datetime.timezone.utc)
today = now.strftime('%Y-%m-%d')
out_dir = 'results/literature'
os.makedirs(out_dir, exist_ok=True)
out_path = 'results/literature/review_%s.json' % today.replace('-', '')
request = urllib.request.Request('https://www.ebi.ac.uk/europepmc/webservices/rest/search?query=mirna', headers={'Accept': 'application/json'})
with urllib.request.urlopen(request, timeout=60) as response:
    papers = json.load(response)
with open(out_path, 'w', encoding='utf-8') as handle:
    json.dump(papers, handle)
print(os.path.getsize(out_path))
"#;
        let args = python(code);
        assert!(ordinary_runtime_call_is_low_risk(&args));
        assert_eq!(ordinary_runtime_call_approval_reason(&args), None);
    }

    #[test]
    fn proven_project_directories_and_date_parts_are_ordinary() {
        for code in [
            "import os\nos.makedirs('results/review')",
            "import os\nos.makedirs(os.path.join('.', 'results'), exist_ok=True)",
            "import os\nos.makedirs(os.path.join('results', 'review'), exist_ok=False)",
            "import os\nroot = 'results/review'\nos.makedirs(root, exist_ok=True)",
            "import os, datetime\nstamp = datetime.datetime.now().strftime('%Y%m%d')\np = 'results/review/report_%s.json' % stamp\nprint(os.path.exists(p))",
            "import datetime, os\nclock = datetime.datetime.now(datetime.timezone.utc)\npart = clock.strftime('batch_%Y-%m-%d').replace('-', '_')\np = 'results/review/%s.json' % part\nprint(os.path.getsize(p))",
            "import os, datetime\ninstant = datetime.datetime.now()\na = instant.strftime('%Y%m%d')\nb = instant.strftime('%H%M%S')\np = 'results/review/%s_%s.json' % (a, b)\nprint(os.path.getsize(p))",
            "import os, datetime\ninstant = datetime.datetime.now()\npart = instant.strftime('%Y%m%d')\np = os.path.join(os.path.join('results', 'review'), part)\nprint(os.path.exists(p))",
        ] {
            assert!(ordinary_runtime_call_is_low_risk(&python(code)), "{code}");
        }
    }

    #[test]
    fn unproved_date_paths_and_directory_options_require_approval() {
        for code in [
            "import os\nos.makedirs('../outside', exist_ok=True)",
            "import os\nos.makedirs('C:/outside', exist_ok=True)",
            "import os\nos.makedirs('.git/hooks', exist_ok=True)",
            "import os\nos.makedirs('results', mode=0o777)",
            "import os\nos.makedirs('results', extra=True)",
            "import os\nos.makedirs('results', exist_ok=flag)",
            "import os\nos.makedirs('results', True)",
            "import os\nos.makedirs('results', exist_ok=True, exist_ok=False)",
            "import os\nos.makedirs('results', *arguments)",
            "import os\nos.makedirs('results', **options)",
            "import os\nos.makedirs(os.path.getsize('results/a'))",
            "import os\nos.makedirs(os.path.exists('results/a'))",
            "import os\nos.remove('results/a')",
            "from os import makedirs as mk\nmk('../outside')",
            "import os, datetime\nnow = datetime.datetime.now(datetime.timezone.utc)\nd = now.strftime('%Y/%m/%d')\np = 'results/%s.json' % d\nprint(os.path.getsize(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('secrets')\np = 'results/%s/file' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y').replace('2026', 'secrets')\np = os.path.join('results', d, 'file')\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now(datetime.timezone.utc)\nd = now.strftime('%Y-%m-%d')\np = '%s.json' % d\nprint(os.path.getsize(p))",
            "import os, datetime\nnow = datetime.datetime.now(datetime.timezone.utc)\nd = now.strftime('%Y-%m-%d')\np = 'results/%s.json' % d\np = unknown\nprint(os.path.getsize(p))",
            "import os, datetime\nnow = datetime.datetime.now(datetime.timezone.utc)\nd = now.strftime('%Y-%m-%d')\np = 'results/%s.json' % d\nprint(os.path.getsize(p))\nos.remove(p)",
            "import os\npart = response['name']\np = 'results/%s.json' % part\nprint(os.path.getsize(p))",
            "import os\npart = config['path'].replace('/', '')\np = 'results/%s.json' % part\nprint(os.path.getsize(p))",
            "import os\np = f'results/{response[\"name\"]}.json'\nprint(os.path.getsize(p))",
            "open(f'/tmp/{response[\"name\"]}.json', 'w')",
            "import os\np = os.path.getsize('results/a.json')\nprint(os.path.getsize(p))",
            "import os, datetime\nnow = datetime.datetime.now(1)\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now(datetime.timezone.utc, 1)\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % (1,)\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y%m%d')\np = 'results/%s%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y%m%d')\np = 'results/%02s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % (d, unknown)\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\ndatetime = unknown\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\ndatetime.datetime = unknown\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\ndatetime.datetime = custom\nimport datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now(datetime.timezone.utc)\ndatetime.timezone.utc = unknown\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nnow.strftime = unknown\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y%m%d')\nd.replace = unknown\np = 'results/%s.json' % d.replace('-', '')\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y%m%d')\nif condition:\n    p = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nfor now in values:\n    pass\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\ndef f(now):\n    d = now.strftime('%Y%m%d')\n    p = 'results/%s.json' % d\n    return os.path.exists(p)",
            "import os, datetime\nnow = datetime.datetime.now()\ndef f(datetime):\n    d = datetime.datetime.now().strftime('%Y%m%d')\n    p = 'results/%s.json' % d\n    return os.path.exists(p)",
            "import os, datetime\nnow = datetime.datetime.now()\nwith source as datetime:\n    p = 'results/%s.json' % now.strftime('%Y%m%d')\n    print(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nsink(datetime)\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\nos = unknown\nprint(os.path.exists(p))",
            "import os, datetime\nnow = datetime.datetime.now()\nd = now.strftime('%Y%m%d')\np = 'results/%s.json' % d\nos.path.exists = unknown\nprint(os.path.exists(p))",
            "import os\nos.makedirs(p)\np = 'results/safe'",
            "os.makedirs('results/safe')\nimport os",
            "import os, datetime\nd = now.strftime('%Y%m%d')\nnow = datetime.datetime.now()\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os\nos.makedirs('.git /hooks')",
            "import os\nos.makedirs('credentials./x')",
            "import os, datetime\nd = datetime.datetime.now().strftime('%Y%m%d')\np = 'results/%s/credentials./x' % d\nprint(os.path.exists(p))",
            "import os, datetime\nd = datetime.datetime.now().strftime('%Y%m%d')\np = os.path.join('results', d, '.git /hooks')\nprint(os.path.exists(p))",
            "import os, datetime, sys\nsys.modules['datetime'].datetime = factory\nd = datetime.datetime.now().strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime, sys\nsys.modules['os'].makedirs = factory\nos.makedirs('results/safe')",
            "import os, datetime\nfrom sys import modules as m\nm['datetime'].datetime = factory\nd = datetime.datetime.now().strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os, datetime, sys\nvars(sys)['modules']['datetime'].datetime = factory\nd = datetime.datetime.now().strftime('%Y%m%d')\np = 'results/%s.json' % d\nprint(os.path.exists(p))",
            "import os\np = 'results/safe'\ndef f[p]():\n    return os.path.exists(p)",
            "import os\np = 'results/safe'\nimport p.child\nprint(os.path.exists(p))",
        ] {
            assert!(!ordinary_runtime_call_is_low_risk(&python(code)), "{code}");
        }
    }

    #[test]
    fn date_replace_expansion_stays_within_proof_budget() {
        let large = "a".repeat(1000);
        let code = format!(
            "import os, datetime\nd = datetime.datetime.now().strftime('%Ya').replace('a', '{large}')\ne = d.replace('a', '{large}')\np = 'results/%s.json' % e\nprint(os.path.exists(p))"
        );
        assert!(!ordinary_runtime_call_is_low_risk(&python(&code)));
    }

    #[test]
    fn ast_path_proof_rejects_oversized_or_deep_inputs() {
        let oversized = format!("import os\n{}os.makedirs('results')", "pass\n".repeat(6600));
        let nested = format!(
            "import os\nos.makedirs({}'results'{})",
            "(".repeat(65),
            ")".repeat(65)
        );
        let long_expression = format!(
            "import os\nx = 1{}\nos.makedirs('results')",
            "**1".repeat(270)
        );
        for code in [&oversized, &nested, &long_expression] {
            assert!(!ordinary_runtime_call_is_low_risk(&python(code)));
        }
    }



    #[test]
    fn approval_reason_for_unproven_path_keeps_source_private() {
        let code = "import os\nos.makedirs('results/PRIVATE_SENTINEL', exist_ok=True)\nout = 'results/PRIVATE_SENTINEL_%s.json' % unknown\nprint(os.path.getsize(out))";
        let args = python(code);
        assert!(!ordinary_runtime_call_is_low_risk(&args));
        let reason = ordinary_runtime_call_approval_reason(&args).unwrap();
        assert!(reason.contains("Project path cannot be proven"), "{reason}");
        assert!(
            !reason.contains("Unsupported OS operation"),
            "{reason}"
        );
        assert!(!reason.contains("PRIVATE_SENTINEL"));
        assert!(reason.split("; ").count() <= 4);
    }

    #[test]
    fn unknown_os_path_has_a_specific_fixed_reason() {
        let args = python("import os\nprint(os.path.getsize(unknown))");
        let reason = ordinary_runtime_call_approval_reason(&args).unwrap();
        assert!(reason.contains("Project path cannot be proven"), "{reason}");
    }

    #[test]
    fn approval_reason_is_absent_for_existing_low_risk_code() {
        let args = python("import os\nprint(os.path.exists('results/report.json'))");
        assert!(ordinary_runtime_call_is_low_risk(&args));
        assert_eq!(ordinary_runtime_call_approval_reason(&args), None);
    }

    #[test]
    fn unrelated_os_operation_does_not_mislabel_a_proven_path() {
        let args = python(
            "import os\np = 'results/a.txt'\nos.listdir('results')\nprint(os.path.getsize(p))",
        );
        let reason = ordinary_runtime_call_approval_reason(&args).unwrap();
        assert!(reason.contains("Unsupported OS operation"));
        assert!(
            !reason.contains("Project path cannot be proven"),
            "{reason}"
        );
    }

    #[test]
    fn ordinary_variable_name_is_not_reported_as_a_dangerous_call() {
        let args = python("rename = 1\nimport os\nos.listdir('results')");
        let reason = ordinary_runtime_call_approval_reason(&args).unwrap();
        assert!(reason.contains("Unsupported OS operation"));
        assert!(
            !reason.contains("operation that requires approval"),
            "{reason}"
        );
    }

    #[test]
    fn protected_open_path_has_a_specific_fixed_reason() {
        let args = python("open('.git/PRIVATE_SENTINEL', 'r')");
        let reason = ordinary_runtime_call_approval_reason(&args).unwrap();
        assert!(reason.contains("external or protected path"), "{reason}");
        assert!(!reason.contains("PRIVATE_SENTINEL"));
    }

    #[test]
    fn project_relative_exists_allows_literature_read_and_csv_review() {
        let code = r#"
import csv, json, os, urllib.request
name = 'results/review.csv'
if os.path.exists(name):
    with open(name, encoding='utf-8') as handle:
        rows = list(csv.DictReader(handle))
else:
    rows = []
request = urllib.request.Request('https://www.ebi.ac.uk/europepmc/webservices/rest/search?query=PBMC', headers={'Accept': 'application/json'})
with urllib.request.urlopen(request, timeout=60) as response:
    papers = json.load(response)
print(len(rows), len(papers.get('resultList', {}).get('result', [])))
"#;
        assert!(ordinary_runtime_call_is_low_risk(&python(code)));
    }

    #[test]
    fn unsafe_exists_checks_still_require_approval() {
        for code in [
            "import os\nprint(os.path.exists(unknown))",
            "import os\nprint(os.path.exists('/tmp/outside'))",
            "import os\nprint(os.path.exists('C:/outside'))",
            "import os\nprint(os.path.exists('../outside'))",
            "import os\nprint(os.path.exists('results/../outside'))",
            "import os\nprint(os.path.exists('.git/config'))",
            "import os\np = 'results/review.csv'\np = unknown\nprint(os.path.exists(p))",
            "import os\nif condition:\n    p = 'results/review.csv'\nprint(os.path.exists(p))",
            "import os as operating\nprint(operating.path.exists('results/review.csv'))",
            "from os import path\nprint(path.exists('results/review.csv'))",
            "import os\ncheck = os.path.exists\nprint(check('results/review.csv'))",
            "import os\nprint(os.path.exists)",
            "import os\nprint(os.path.exists())",
            "import os\nprint(os.path.exists('results/review.csv', 'results/other.csv'))",
            "import os\nprint(os.path.exists(path='results/review.csv'))",
            "import os\nexists_result = os.path.exists('results/review.csv')\nprint(os.path.getsize(exists_result))",
        ] {
            assert!(!ordinary_runtime_call_is_low_risk(&python(code)), "{code}");
        }
    }

    #[test]
    fn os_path_escape_and_unproven_metadata_paths_require_approval() {
        for code in [
            "import os as operating\noperating.path.join('results', 'a')",
            "from os import path\npath.join('results', 'a')",
            "import os\nprint(os.environ)",
            "import os\nprint(os.path)",
            "import os\nfn = os.path.getsize",
            "import os\nprint(os.path.__dict__)",
            "import os\nprint(os.path.join('results', '../private'))",
            "import os\nprint(os.path.getsize('../private'))",
            "import os\nsource = input()\nprint(os.path.getsize(source))",
            "import os\nroot = 'results'\nroot = input()\nprint(os.path.getsize(os.path.join(root, 'a')))",
            "import os\nprint(getattr(os.path, 'getsize')('results/a'))",
            "import os\nprint(os.path.join.__call__('results', 'a'))",
        ] {
            assert!(!ordinary_runtime_call_is_low_risk(&python(code)), "{code}");
        }
    }

    #[test]
    fn os_path_proof_rejects_escaped_and_rebound_names() {
        for code in [
            "import os\nprint(os.path.join('results', '\\x2e\\x2e/private'))",
            "import os\nprint(os.path.getsize('results\\\\..\\\\private'))",
            "import os\nprint(os.path.getsize('C:private'))",
            "import os\nroot = 'results'\nroot += suffix\nprint(os.path.getsize(root))",
            "import os\nroot = 'results'\nfor root in paths:\n    print(os.path.getsize(root))",
            "import os\nroot = 'results'\nfor (_, root) in records:\n    print(os.path.getsize(root))",
            "import os\nroot = 'results'\n[os.path.getsize(root) for i in values for (_, root) in records]",
            "import os\nroot = 'results'\nmatch value:\n    case root:\n        print(os.path.getsize(root))",
            "import os\nroot = 'results'\nclass root(metaclass=Meta):\n    pass\nprint(os.path.getsize(root))",
            "import os\nroot = 'results'\nimport root\nprint(os.path.getsize(root))",
            "import os\nroot = 'results'\nfrom package import root\nprint(os.path.getsize(root))",
            "import os\nroot = 'results'\ndel root\nprint(os.path.getsize(root))",
            "import os\nroot = 'results'\ndef size(root):\n    return os.path.getsize(root)",
            "import os\nroot = 'results'\ndef f():\n    def g(root):\n        return os.path.getsize(root)\n    return g(path)\nprint(f())",
            "import os\nroot = config['path']\nif False:\n    root = 'results'\nprint(os.path.getsize(root))",
            "import os\nroot = config['path']\nif False: n = 1; root = 'results'\nprint(os.path.getsize(root))",
            "import os\nroot = 'results'\n(root,) = paths\nprint(os.path.getsize(root))",
            "import os\nroot = 'results'\nwith source as root:\n    pass\nprint(os.path.getsize(root))",
            "import os\nroot = 'results'\nwith source as (root, other):\n    pass\nprint(os.path.getsize(root))",
            "import os\nroot = 'results'\nroot := unknown\nprint(os.path.getsize(root))",
        ] {
            assert!(!ordinary_runtime_call_is_low_risk(&python(code)), "{code}");
        }
    }

    #[test]
    fn clear_risks_and_opaque_code_ask_for_approval() {
        for code in [
            "import subprocess as sp\nsp.run(['echo', 'x'])",
            "from os import system as run\nrun('echo x')",
            "import os as operating\noperating.environ['TOKEN']",
            "__import__('subprocess').run(['x'])",
            "eval('1 + 1')",
            "Path('../outside').write_text('x')",
            "Path('C:/outside').write_text('x')",
            "requests.post('https://example.org', json={})",
            "urllib.request.Request(url, data=b'payload')",
            "urllib.request.Request(url, method='POST')",
            "urllib.request.Request(url, b'payload')",
            "urllib.request.urlopen(url, b'payload')",
            "import json, subprocess as sp\nsp.run(['x'])",
            "Path('results/a').replace('results/b')",
            "p = Path('results/a')\np.replace('results/b')",
            "dest = 'C:/outside.txt'\nopen(dest, 'w')",
            "dest = '/tmp/outside.txt'\nopen(dest, 'w').write('x')",
            "Path('results') / '..'",
            "from pathlib import Path as P\nP('/outside').write_text('x')",
            "from urllib.request import Request as R\nR(url, data=b'payload')",
            "run = eval\nrun('1+1')",
            "from pathlib import *",
            "print('unterminated)",
        ] {
            assert!(!ordinary_runtime_call_is_low_risk(&python(code)), "{code}");
        }
        assert!(!ordinary_runtime_call_is_low_risk(
            &json!({"language":"python"})
        ));
        assert!(!ordinary_runtime_call_is_low_risk(
            &json!({"language":"javascript","code":"1"})
        ));
        assert!(!ordinary_runtime_call_is_low_risk(
            &json!({"language":"python","code":"print(1)","background":true})
        ));
    }

    #[test]
    fn comments_and_literature_strings_do_not_count_as_actions() {
        assert!(ordinary_runtime_call_is_low_risk(&python(
            "# delete and subprocess in an article\nprint('delete subprocess install')"
        )));
        assert!(ordinary_runtime_call_is_low_risk(&python(
            "from urllib.request import Request\nRequest('https://example.org', headers={'Accept':'application/json'})"
        )));
        assert!(ordinary_runtime_call_is_low_risk(&python(
            "import re\nre.compile(r'\\s+')\nprint('\\\\|')"
        )));
        assert!(ordinary_runtime_call_is_low_risk(
            &json!({"language":"r","code":"x <- c(1, 2)\nprint(mean(x))"})
        ));
        assert!(!ordinary_runtime_call_is_low_risk(
            &json!({"language":"r","code":"system('echo x')"})
        ));
        assert!(!ordinary_runtime_call_is_low_risk(
            &json!({"language":"r","code":"dyn.load('plugin.dll')"})
        ));
        assert!(ordinary_runtime_call_is_low_risk(
            &json!({"language":"r","code":"dir.create('results', showWarnings=FALSE)\nwrite.csv(data.frame(x=1), 'results/data.csv')"})
        ));
        for code in [
            "write.csv(data.frame(x=1), 'C:/outside.csv')",
            "writeLines('x', '../outside')",
            "writeLines('x', '/tmp/outside')",
            "httr::POST('https://example.org', body='x')",
            "httr2::req_method(req, 'POST')",
        ] {
            assert!(
                !ordinary_runtime_call_is_low_risk(&json!({"language":"r","code":code})),
                "{code}"
            );
        }
    }
}
