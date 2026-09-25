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
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }
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

fn python_is_low_risk(tokens: &[Token]) -> bool {
    const RISKY_IMPORTS: &[&str] = &[
        "os",
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
