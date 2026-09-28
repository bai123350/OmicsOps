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
        "python" => python_is_low_risk(&tokens),
        "r" => r_is_low_risk(&tokens),
        _ => false,
    }
}

fn unsafe_project_path(path: &str) -> bool {
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

// This is deliberately a small proof for the common metadata workflow, not a
// Python evaluator. Unproved syntax keeps the existing approval route.
fn safe_os_path_uses(tokens: &[Token]) -> bool {
    let mut names = std::collections::HashSet::new();
    let mut imported = false;
    for line in tokens.split(|token| is_mark(Some(token), b'\n')) {
        // A semicolon can continue a conditional or indented suite after the
        // first segment. No segment on that line may establish a path proof.
        let simple_line = !line.iter().any(|token| is_mark(Some(token), b';'));
        for statement in line.split(|token| is_mark(Some(token), b';')) {
            if statement.is_empty() {
                continue;
            }
            let mut import_positions = std::collections::HashSet::new();
            if is_word(statement.first(), "import") {
                for i in 1..statement.len() {
                    if is_word(statement.get(i), "os") {
                        if !(is_word(statement.get(i.wrapping_sub(1)), "import")
                            || is_mark(statement.get(i.wrapping_sub(1)), b','))
                            || !(i + 1 == statement.len() || is_mark(statement.get(i + 1), b','))
                        {
                            return false;
                        }
                        import_positions.insert(i);
                        imported = true;
                    }
                }
            }

            // A later assignment invalidates a path name, even when its new value
            // cannot be proved. Only a whole simple statement can establish one.
            let complex_binding = statement.iter().any(|token| {
                ["def", "lambda", "for", "case", "class", "import", "del"]
                    .iter()
                    .any(|keyword| is_word(Some(token), keyword))
            });
            let binding_start = statement
                .iter()
                .position(|token| is_word(Some(token), "for") || is_word(Some(token), "as"));
            let binding_end = binding_start.and_then(|start| {
                if is_word(statement.get(start), "for") {
                    statement
                        .iter()
                        .enumerate()
                        .skip(start + 1)
                        .find(|(_, token)| is_word(Some(token), "in"))
                        .map(|(i, _)| i)
                } else {
                    statement
                        .iter()
                        .enumerate()
                        .skip(start + 1)
                        .find(|(_, token)| is_mark(Some(token), b':'))
                        .map(|(i, _)| i)
                        .or(Some(statement.len()))
                }
            });
            let mut nesting = 0;
            let mut first_assignment = None;
            for (i, token) in statement.iter().enumerate() {
                match token {
                    Token::Mark(b'(' | b'[' | b'{') => nesting += 1,
                    Token::Mark(b')' | b']' | b'}') => nesting -= 1,
                    Token::Mark(b'=') if nesting == 0 => {
                        first_assignment = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            for i in 0..statement.len() {
                if let Token::Word(name) = &statement[i] {
                    let next_is_assignment = is_mark(statement.get(i + 1), b'=')
                        || (matches!(
                            statement.get(i + 1),
                            Some(Token::Mark(
                                b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b':' | b'@'
                            ))
                        ) && is_mark(statement.get(i + 2), b'='));
                    if next_is_assignment
                        || is_word(statement.get(i.wrapping_sub(1)), "for")
                        || is_word(statement.get(i.wrapping_sub(1)), "as")
                        || binding_start
                            .zip(binding_end)
                            .is_some_and(|(start, end)| i > start && i < end)
                        || first_assignment.is_some_and(|assignment| i < assignment)
                        || complex_binding
                    {
                        names.remove(name);
                    }
                }
            }
            for (i, token) in statement.iter().enumerate() {
                if is_word(Some(token), "os") && !import_positions.contains(&i) {
                    if !imported || safe_os_path_call(statement, i, &names, 0).is_none() {
                        return false;
                    }
                }
            }
            if simple_line {
                if let (Some(Token::Word(name)), Some(Token::Mark(b'='))) =
                    (statement.first(), statement.get(1))
                {
                    if safe_relative_path_expr(statement, 2, &names, 0) == Some(statement.len()) {
                        names.insert(name.clone());
                    }
                }
            }
        }
    }
    true
}

fn safe_relative_path_expr(
    tokens: &[Token],
    start: usize,
    names: &std::collections::HashSet<String>,
    depth: usize,
) -> Option<usize> {
    if depth > 4 {
        return None;
    }
    match tokens.get(start)? {
        Token::String(value) if !value.contains('\\') && !unsafe_project_path(value) => {
            Some(start + 1)
        }
        Token::Word(name) if names.contains(name) => Some(start + 1),
        Token::Word(name) if name == "os" => {
            let (method, end) = safe_os_path_call(tokens, start, names, depth + 1)?;
            (method == "join").then_some(end)
        }
        _ => None,
    }
}

fn safe_os_path_call<'a>(
    tokens: &'a [Token],
    start: usize,
    names: &std::collections::HashSet<String>,
    depth: usize,
) -> Option<(&'a str, usize)> {
    if depth > 4
        || !is_word(tokens.get(start), "os")
        || !is_mark(tokens.get(start + 1), b'.')
        || !is_word(tokens.get(start + 2), "path")
        || !is_mark(tokens.get(start + 3), b'.')
        || !is_mark(tokens.get(start + 5), b'(')
    {
        return None;
    }
    let Token::Word(method) = tokens.get(start + 4)? else {
        return None;
    };
    if method != "join" && method != "getsize" && method != "exists" {
        return None;
    }
    let mut cursor = start + 6;
    let mut arguments = 0;
    loop {
        cursor = safe_relative_path_expr(tokens, cursor, names, depth + 1)?;
        arguments += 1;
        if is_mark(tokens.get(cursor), b')') {
            break;
        }
        if !is_mark(tokens.get(cursor), b',') {
            return None;
        }
        cursor += 1;
    }
    if ((method == "getsize" || method == "exists") && arguments != 1)
        || (method == "join" && arguments < 2)
    {
        return None;
    }
    Some((method, cursor + 1))
}

fn python_is_low_risk(tokens: &[Token]) -> bool {
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
    if !safe_os_path_uses(tokens) {
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
        assert!(python_is_low_risk(&tokens), "{tokens:?}");
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
