//! Positive, bounded detection of visible local deletion operations.
//! Unknown code is intentionally outside this finite detector.

use std::collections::{HashMap, HashSet};

use omicsops_protocol::{ComputeBackendKindV4, KernelLanguageV4, ToolCallV4};
use rustpython_ast::Visitor;
use rustpython_parser::{Parse, ast};
use serde_json::Value;

const REASON: &str = "This call may delete a local file or directory.";

pub(crate) fn local_deletion_approval_reason(
    call: &ToolCallV4,
    backend: ComputeBackendKindV4,
) -> Option<String> {
    let detected = match call.tool_id.as_str() {
        "runtime.execute" if backend != ComputeBackendKindV4::Ssh => {
            let code = call.arguments.get("code").and_then(Value::as_str)?;
            if code.len() > 64 * 1024 { return None; }
            match crate::agent_v4::parse_language(call.arguments.get("language").and_then(Value::as_str)?).ok()? {
                KernelLanguageV4::Python => python_deletes(code),
                KernelLanguageV4::R => r_deletes(code),
            }
        }
        "use_mcp_tool" => mcp_deletes(&call.arguments),
        _ => false,
    };
    detected.then(|| REASON.into())
}

fn expr_name(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Name(value) => Some(value.id.to_string()),
        ast::Expr::Attribute(value) => Some(format!("{}.{}", expr_name(&value.value)?, value.attr)),
        _ => None,
    }
}

fn literal(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Constant(value) => match &value.value {
            ast::Constant::Str(value) => Some(value.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn literal_command(expr: &ast::Expr) -> Option<Vec<String>> {
    if let Some(value) = literal(expr) { return Some(vec![value]); }
    let elements = match expr {
        ast::Expr::List(value) => &value.elts,
        ast::Expr::Tuple(value) => &value.elts,
        _ => return None,
    };
    elements.iter().map(literal).collect()
}

struct PythonDeletionAudit {
    aliases: HashMap<String, String>,
    paths: HashSet<String>,
    detected: bool,
    nodes: usize,
}

impl Default for PythonDeletionAudit {
    fn default() -> Self {
        Self {
            // A persistent kernel may have imported these in an earlier call.
            aliases: [
                ("os", "os"), ("shutil", "shutil"),
                ("pathlib", "pathlib"), ("subprocess", "subprocess"),
                ("Path", "pathlib.Path"),
            ].into_iter().map(|(name, target)| (name.into(), target.into())).collect(),
            paths: HashSet::new(), detected: false, nodes: 0,
        }
    }
}

impl PythonDeletionAudit {
    fn qualified(&self, expr: &ast::Expr) -> Option<String> {
        let name = expr_name(expr)?;
        let mut parts = name.splitn(2, '.');
        let root = parts.next()?;
        let prefix = self.aliases.get(root)?;
        Some(match parts.next() {
            Some(rest) => format!("{prefix}.{rest}"),
            None => prefix.clone(),
        })
    }

    fn path_receiver(&self, expr: &ast::Expr) -> bool {
        match expr {
            ast::Expr::Name(value) => self.paths.contains(value.id.as_str()),
            ast::Expr::Call(call) => self.qualified(&call.func).as_deref() == Some("pathlib.Path"),
            _ => false,
        }
    }
}

impl Visitor for PythonDeletionAudit {
    fn visit_stmt_import(&mut self, node: ast::StmtImport) {
        for alias in node.names {
            let module = alias.name.as_str();
            if ["os", "shutil", "pathlib", "subprocess"].contains(&module) {
                let binding = alias.asname.as_ref().map_or(module, |name| name.as_str());
                self.aliases.insert(binding.into(), module.into());
            }
        }
    }

    fn visit_stmt_import_from(&mut self, node: ast::StmtImportFrom) {
        if let Some(module) = &node.module {
            if ["os", "shutil", "pathlib", "subprocess"].contains(&module.as_str()) {
                for alias in node.names {
                    let binding = alias.asname.as_ref().unwrap_or(&alias.name);
                    self.aliases.insert(binding.to_string(), format!("{module}.{}", alias.name));
                }
            }
        }
    }

    fn visit_stmt_assign(&mut self, node: ast::StmtAssign) {
        let is_path = matches!(&*node.value, ast::Expr::Call(call)
            if self.qualified(&call.func).as_deref() == Some("pathlib.Path"));
        for target in &node.targets {
            if let ast::Expr::Name(name) = target {
                if is_path { self.paths.insert(name.id.to_string()); }
                else { self.paths.remove(name.id.as_str()); }
                self.aliases.remove(name.id.as_str());
            }
        }
        self.generic_visit_stmt_assign(node);
    }

    fn visit_expr_call(&mut self, node: ast::ExprCall) {
        self.nodes += 1;
        if self.nodes > 4000 { return; }
        let qualified = self.qualified(&node.func);
        let direct_delete = matches!(qualified.as_deref(),
            Some("os.remove" | "os.unlink" | "os.rmdir" | "os.removedirs" | "shutil.rmtree"
                | "pathlib.Path.unlink" | "pathlib.Path.rmdir"));
        let path_delete = matches!(&*node.func, ast::Expr::Attribute(attr)
            if ["unlink", "rmdir"].contains(&attr.attr.as_str()) && self.path_receiver(&attr.value));
        if direct_delete || path_delete { self.detected = true; }
        if matches!(qualified.as_deref(), Some("os.system" | "subprocess.run" | "subprocess.call" | "subprocess.Popen" | "subprocess.check_call" | "subprocess.check_output")) {
            let parameter = if qualified.as_deref() == Some("os.system") { "command" } else { "args" };
            let argument = node.args.first().or_else(|| node.keywords.iter()
                .find(|keyword| keyword.arg.as_ref().is_some_and(|name| name.as_str() == parameter))
                .map(|keyword| &keyword.value));
            if let Some(command) = argument.and_then(literal_command) {
                self.detected |= command_deletes(&command);
            }
        }
        self.generic_visit_expr_call(node);
    }
}

fn python_deletes(code: &str) -> bool {
    let Ok(suite) = ast::Suite::parse(code, "<approval>") else { return false; };
    let mut audit = PythonDeletionAudit::default();
    for statement in suite { audit.visit_stmt(statement); }
    audit.detected
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellFlavor { General, Cmd, PowerShell, PowerShell7, Posix }

fn command_deletes(command: &[String]) -> bool {
    command_deletes_with_shell(command, ShellFlavor::General)
}

fn command_deletes_with_shell(command: &[String], shell: ShellFlavor) -> bool {
    if command.is_empty() { return false; }
    if command.len() == 1 {
        return shell_commands(&command[0], shell).iter().any(|words| command_words_delete(words));
    }
    command_words_delete(command)
}

fn command_words_delete(words: &[String]) -> bool {
    if words.is_empty() { return false; }
    let executable = words[0].rsplit(['/', '\\']).next().unwrap_or("").to_ascii_lowercase();
    let executable = executable.trim_end_matches(".exe");
    match executable {
        "rm" | "rmdir" | "unlink" | "del" | "erase" | "rd" | "remove-item" | "ri" | "rm-item" => words.len() > 1,
        "cmd" => {
            let mut next = 1;
            while words.get(next).is_some_and(|flag| ["/q", "/d", "/s"].contains(&flag.to_ascii_lowercase().as_str())) { next += 1; }
            if words.get(next).is_some_and(|flag| ["/c", "/k"].contains(&flag.to_ascii_lowercase().as_str())) {
                command_deletes_with_shell(&words[next + 1..], ShellFlavor::Cmd)
            } else { false }
        }
        "powershell" | "pwsh" => {
            let mut next = 1;
            while let Some(flag) = words.get(next) {
                let flag = flag.to_ascii_lowercase();
                if ["-noprofile", "-nologo", "-noninteractive", "-noexit"].contains(&flag.as_str()) { next += 1; }
                else if ["-executionpolicy", "-windowstyle", "-inputformat", "-outputformat"].contains(&flag.as_str()) { next += 2; }
                else { break; }
            }
            if words.get(next).is_some_and(|flag| ["-command", "-c"].contains(&flag.to_ascii_lowercase().as_str())) {
                let shell = if executable == "pwsh" { ShellFlavor::PowerShell7 } else { ShellFlavor::PowerShell };
                command_deletes_with_shell(&words[next + 1..], shell)
            } else { false }
        }
        "sh" | "bash" if words.len() > 2 && words[1] == "-c" => command_deletes_with_shell(&words[2..], ShellFlavor::Posix),
        _ => false,
    }
}

fn shell_commands(text: &str, shell: ShellFlavor) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if shell == ShellFlavor::Cmd && quote.is_none() && ch == '^' {
            if let Some(escaped) = chars.next() { current.push(escaped); }
            continue;
        }
        if quote == Some(ch) { quote = None; continue; }
        if quote.is_none() && (ch == '\'' || ch == '"') { quote = Some(ch); continue; }
        let separator = match shell {
            ShellFlavor::Cmd => matches!(ch, '&' | '|'),
            ShellFlavor::PowerShell => matches!(ch, ';' | '|'),
            ShellFlavor::PowerShell7 => matches!(ch, ';' | '|') || (ch == '&' && chars.peek().copied() == Some('&')),
            ShellFlavor::General | ShellFlavor::Posix => matches!(ch, '&' | ';' | '|'),
        };
        if quote.is_none() && separator {
            if shell == ShellFlavor::PowerShell7 && ch == '&' { chars.next(); }
            if !current.is_empty() { words.push(std::mem::take(&mut current)); }
            if !words.is_empty() { commands.push(std::mem::take(&mut words)); }
        } else if quote.is_none() && ch.is_whitespace() {
            if !current.is_empty() { words.push(std::mem::take(&mut current)); }
        } else { current.push(ch); }
        if commands.len() > 128 || words.len() > 512 { return Vec::new(); }
    }
    if !current.is_empty() { words.push(current); }
    if !words.is_empty() { commands.push(words); }
    commands
}

fn r_deletes(code: &str) -> bool {
    let tokens = r_tokens(code);
    for window in tokens.windows(2) {
        if matches!((&window[0], &window[1]), (RToken::Word(name), RToken::Mark('(')) if name == "unlink") {
            return true;
        }
    }
    for window in tokens.windows(4) {
        if matches!((&window[0], &window[1], &window[2], &window[3]),
            (RToken::Word(file), RToken::Mark('.'), RToken::Word(remove), RToken::Mark('('))
                if file == "file" && remove == "remove") { return true; }
    }
    for (i, token) in tokens.iter().enumerate() {
        if let RToken::Word(name) = token {
            if ["system", "system2", "shell"].contains(&name.as_str())
                && matches!(tokens.get(i + 1), Some(RToken::Mark('(')))
                && let Some(arguments) = r_call_arguments(&tokens, i + 1)
                && let Some(command) = r_argument(&arguments, if name == "shell" { "cmd" } else { "command" }, 0)
                    .and_then(r_literal_words)
            {
                let mut command = command;
                if name == "system2" {
                    if let Some(extra) = r_argument(&arguments, "args", 1).and_then(r_literal_words) {
                        command.extend(extra);
                    }
                }
                if command_deletes(&command) { return true; }
            }
        }
    }
    false
}

fn r_call_arguments(tokens: &[RToken], open: usize) -> Option<Vec<&[RToken]>> {
    let mut result = Vec::new();
    let mut depth = 1usize;
    let mut start = open + 1;
    for i in start..tokens.len() {
        match tokens[i] {
            RToken::Mark('(') => depth += 1,
            RToken::Mark(')') => {
                depth -= 1;
                if depth == 0 {
                    if i > start { result.push(&tokens[start..i]); }
                    return Some(result);
                }
            }
            RToken::Mark(',') if depth == 1 => {
                result.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        if depth > 64 { return None; }
    }
    None
}

fn r_argument<'a>(arguments: &'a [&'a [RToken]], name: &str, position: usize) -> Option<&'a [RToken]> {
    for argument in arguments {
        if matches!(argument, [RToken::Word(key), RToken::Mark('='), ..] if key == name) {
            return Some(&argument[2..]);
        }
    }
    let positional = arguments.iter().filter(|argument| !matches!(argument, [RToken::Word(_), RToken::Mark('='), ..])).nth(position)?;
    Some(positional)
}

fn r_literal_words(tokens: &[RToken]) -> Option<Vec<String>> {
    if let [RToken::String(value)] = tokens { return Some(vec![value.clone()]); }
    if !matches!(tokens.first(), Some(RToken::Word(name)) if name == "c")
        || !matches!(tokens.get(1), Some(RToken::Mark('(')))
        || !matches!(tokens.last(), Some(RToken::Mark(')'))) { return None; }
    let mut result = Vec::new();
    let mut index = 2;
    while index < tokens.len() - 1 {
        let RToken::String(value) = &tokens[index] else { return None; };
        result.push(value.clone());
        index += 1;
        if index < tokens.len() - 1 {
            if !matches!(tokens[index], RToken::Mark(',')) { return None; }
            index += 1;
        }
    }
    Some(result)
}

#[derive(Debug)]
enum RToken { Word(String), String(String), Mark(char) }

fn r_tokens(code: &str) -> Vec<RToken> {
    let mut result = Vec::new();
    let mut chars = code.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '#' { for next in chars.by_ref() { if next == '\n' { break; } } continue; }
        if ch == '\'' || ch == '"' {
            let mut value = String::new();
            while let Some(next) = chars.next() {
                if next == '\\' { if let Some(escaped) = chars.next() { value.push(escaped); } }
                else if next == ch { break; }
                else { value.push(next); }
            }
            result.push(RToken::String(value));
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            let mut value = ch.to_string();
            while chars.peek().is_some_and(|next| next.is_ascii_alphanumeric() || *next == '_') {
                value.push(chars.next().unwrap());
            }
            result.push(RToken::Word(value));
        } else if !ch.is_whitespace() { result.push(RToken::Mark(ch)); }
        if result.len() > 6000 { return Vec::new(); }
    }
    result
}

fn mcp_deletes(call_arguments: &Value) -> bool {
    let Some(tool) = call_arguments.get("tool").and_then(Value::as_str) else { return false; };
    let args = call_arguments.get("arguments").unwrap_or(&Value::Null);
    let name = tool.to_ascii_lowercase().replace(['.', '-', '/'], "_");
    let explicit = ["delete_file", "remove_file", "trash_file", "delete_directory", "remove_directory", "delete_folder", "remove_folder", "trash_directory", "trash_folder"]
        .iter().any(|word| name == *word || name.ends_with(&format!("_{word}")));
    if explicit { return true; }
    let action = args.get("action").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    let deletion_action = ["delete", "remove", "trash", "unlink", "rmdir"].contains(&action.as_str());
    let file_target = ["path", "file_path", "directory", "directory_path", "folder", "folder_path", "local_path"]
        .iter().any(|key| args.get(*key).and_then(Value::as_str).is_some_and(|value| !value.trim().is_empty()));
    deletion_action && file_target
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn runtime(language: &str, code: &str) -> ToolCallV4 {
        ToolCallV4 { call_id: "test".into(), tool_id: "runtime.execute".into(), arguments: json!({"language":language,"code":code}) }
    }

    #[test]
    fn python_direct_and_literal_command_deletions_ask_only_on_local_compute() {
        for code in [
            "import os as operating\noperating.remove('PRIVATE_PATH')",
            "os.remove('PRIVATE_PATH')",
            "from os import unlink as erase\nerase('PRIVATE_PATH')",
            "import shutil as sh\nsh.rmtree('PRIVATE_PATH')",
            "from pathlib import Path as P\np = P('PRIVATE_PATH')\np.unlink()",
            "Path('PRIVATE_PATH').rmdir()",
            "from pathlib import Path\nPath.unlink(Path('PRIVATE_PATH'))",
            "import subprocess as sp\nsp.run(['rm', '-rf', 'PRIVATE_PATH'])",
            "import os\nos.system('cmd /c del PRIVATE_PATH')",
            "import subprocess\nsubprocess.run(['powershell', '-Command', 'Remove-Item PRIVATE_PATH'])",
        ] {
            let call = runtime("python", code);
            let reason = local_deletion_approval_reason(&call, ComputeBackendKindV4::Local).unwrap();
            assert!(!reason.contains("PRIVATE_PATH"), "{code}");
            assert!(local_deletion_approval_reason(&call, ComputeBackendKindV4::Docker).is_some(), "{code}");
            assert!(local_deletion_approval_reason(&call, ComputeBackendKindV4::Ssh).is_none(), "{code}");
        }
    }

    #[test]
    fn supported_language_aliases_use_the_same_detection_as_execution() {
        for (language, code) in [
            ("Python", "os.remove('PRIVATE_PATH')"),
            ("py", "os.remove('PRIVATE_PATH')"),
            ("R", "unlink('PRIVATE_PATH')"),
        ] {
            assert!(local_deletion_approval_reason(&runtime(language, code), ComputeBackendKindV4::Local).is_some(), "{language}");
        }
    }

    #[test]
    fn literal_shell_boundaries_and_launcher_flags_detect_commands_without_echo_false_positives() {
        for code in [
            "import os\nos.system('echo cleanup & del output.txt')",
            "import os\nos.system('echo cleanup; rm output.txt')",
            "import os\nos.system('echo cleanup && rm output.txt')",
            "import subprocess\nsubprocess.run(['powershell', '-NoProfile', '-Command', 'Remove-Item output.txt'])",
            "import subprocess\nsubprocess.run(['pwsh', '-NoLogo', '-NonInteractive', '-c', 'ri output.txt'])",
        ] {
            assert!(local_deletion_approval_reason(&runtime("python", code), ComputeBackendKindV4::Local).is_some(), "{code}");
        }
        for code in [
            "import os\nos.system('echo \"del output.txt\"')",
            "import subprocess\nsubprocess.run(['ssh', 'host', 'rm', 'remote'])",
            "import os\nos.system('echo cleanup & echo \"rm output.txt\"')",
        ] {
            assert!(local_deletion_approval_reason(&runtime("python", code), ComputeBackendKindV4::Local).is_none(), "{code}");
        }
    }

    #[test]
    fn explicit_shell_uses_its_own_literal_separators_and_cmd_caret_escape() {
        for code in [
            "import subprocess\nsubprocess.run(['cmd', '/c', 'echo cleanup; del output.txt'])",
            "import subprocess\nsubprocess.run(['cmd', '/c', 'echo cleanup ^& del output.txt'])",
        ] {
            assert!(local_deletion_approval_reason(&runtime("python", code), ComputeBackendKindV4::Local).is_none(), "{code}");
        }
        for code in [
            "import subprocess\nsubprocess.run(['cmd', '/c', 'echo cleanup & del output.txt'])",
            "import subprocess\nsubprocess.run(['cmd', '/c', 'echo cleanup && del output.txt'])",
            "import subprocess\nsubprocess.run(['powershell', '-Command', 'echo cleanup; Remove-Item output.txt'])",
            "import subprocess\nsubprocess.run(['pwsh', '-Command', 'echo cleanup && Remove-Item output.txt'])",
            "import subprocess\nsubprocess.run(['sh', '-c', 'echo cleanup; rm output.txt'])",
        ] {
            assert!(local_deletion_approval_reason(&runtime("python", code), ComputeBackendKindV4::Local).is_some(), "{code}");
        }
    }

    #[test]
    fn named_python_and_r_literal_vector_command_arguments_are_visible() {
        for (language, code) in [
            ("python", "import subprocess\nsubprocess.run(args=['cmd', '/c', 'del', 'output.txt'])"),
            ("python", "import os\nos.system(command='rm output.txt')"),
            ("r", "system(command='cmd /c del output.txt')"),
            ("r", "system2('cmd', c('/c', 'del', 'output.txt'))"),
            ("r", "system2(command='powershell', args=c('-NoProfile', '-Command', 'Remove-Item output.txt'))"),
        ] {
            assert!(local_deletion_approval_reason(&runtime(language, code), ComputeBackendKindV4::Local).is_some(), "{code}");
        }
        for (language, code) in [
            ("python", "import subprocess\nsubprocess.run(args=['echo', 'rm output.txt'])"),
            ("r", "system2('ssh', c('host', 'rm', 'remote'))"),
            ("r", "system(command='echo \\\"del output.txt\\\"')"),
        ] {
            assert!(local_deletion_approval_reason(&runtime(language, code), ComputeBackendKindV4::Local).is_none(), "{code}");
        }
    }

    #[test]
    fn r_deletion_and_commands_ask_without_matching_text_or_list_mutation() {
        for code in [
            "unlink('PRIVATE_PATH')",
            "file.remove('PRIVATE_PATH')",
            "system2('rm', 'PRIVATE_PATH')",
            "shell('Remove-Item PRIVATE_PATH')",
        ] {
            assert!(local_deletion_approval_reason(&runtime("r", code), ComputeBackendKindV4::Local).is_some(), "{code}");
        }
        for (language, code) in [
            ("python", "print('os.remove(\\\"file\\\")')"),
            ("python", "# os.remove('file')\nprint('safe')"),
            ("python", "rows.drop(columns=['a'])"),
            ("python", "requests.delete('https://example.org/data')"),
            ("python", "import subprocess\nsubprocess.run(['ssh','host','rm','remote'])"),
            ("python", "import subprocess\nsubprocess.run(['echo','rm file'])"),
            ("r", "print('unlink(\\\"file\\\")')"),
            ("r", "# file.remove('file')\nprint(1)"),
        ] {
            assert!(local_deletion_approval_reason(&runtime(language, code), ComputeBackendKindV4::Local).is_none(), "{code}");
        }
    }

    #[test]
    fn explicit_mcp_file_deletion_asks_even_with_ssh_or_read_only_hint() {
        for (tool, args) in [
            ("delete_file", json!({"path":"PRIVATE_PATH"})),
            ("filesystem.remove_directory", json!({"directory":"PRIVATE_PATH"})),
            ("files", json!({"action":"trash","file_path":"PRIVATE_PATH"})),
        ] {
            let call = ToolCallV4 { call_id:"mcp".into(), tool_id:"use_mcp_tool".into(), arguments:json!({"tool":tool,"arguments":args,"readOnlyHint":true}) };
            assert!(local_deletion_approval_reason(&call, ComputeBackendKindV4::Ssh).is_some(), "{tool}");
        }
        for (tool, args) in [
            ("search_files", json!({"query":"delete_file PRIVATE_PATH"})),
            ("http_delete", json!({"url":"https://example.org/PRIVATE_PATH"})),
            ("files", json!({"action":"delete","record_id":"abc"})),
        ] {
            let call = ToolCallV4 { call_id:"mcp".into(), tool_id:"use_mcp_tool".into(), arguments:json!({"tool":tool,"arguments":args}) };
            assert!(local_deletion_approval_reason(&call, ComputeBackendKindV4::Local).is_none(), "{tool}");
        }
    }
}
