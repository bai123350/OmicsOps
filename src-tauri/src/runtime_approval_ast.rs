//! Narrow, fail-closed Python AST proof for project-local OS paths.
//! This does not replace the runtime approval scanner's dangerous-operation veto.

use std::collections::{HashMap, HashSet};

use rustpython_ast::Visitor;
use rustpython_parser::{Mode, Parse, Tok, ast, lexer};

use super::runtime_approval::unsafe_project_path;

#[derive(Clone)]
enum Fact {
    Path(String),
    Instant,
    Fragment(String),
}

fn name(expr: &ast::Expr) -> Option<&str> {
    match expr {
        ast::Expr::Name(value) => Some(value.id.as_str()),
        _ => None,
    }
}

fn string(expr: &ast::Expr) -> Option<&str> {
    match expr {
        ast::Expr::Constant(value) => match &value.value {
            ast::Constant::Str(value) => Some(value),
            _ => None,
        },
        _ => None,
    }
}

fn attr<'a>(expr: &'a ast::Expr, field: &str) -> Option<&'a ast::Expr> {
    match expr {
        ast::Expr::Attribute(value) if value.attr.as_str() == field => Some(&value.value),
        _ => None,
    }
}

fn root_name(expr: &ast::Expr) -> Option<&str> {
    match expr {
        ast::Expr::Name(value) => Some(value.id.as_str()),
        ast::Expr::Attribute(value) => root_name(&value.value),
        ast::Expr::Subscript(value) => root_name(&value.value),
        _ => None,
    }
}

fn safe_path(path: &str) -> bool {
    path.len() <= 4096
        && !unsafe_project_path(path)
        && !path
            .bytes()
            .any(|c| c < 32 || matches!(c, b'\\' | b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*'))
        && !path
            .split('/')
            .any(|part| part.ends_with(' ') || (part != "." && part.ends_with('.')))
}

fn preflight(code: &str) -> bool {
    if code.len() > 32 * 1024 || code.lines().any(|line| line.len() > 2048) {
        return false;
    }
    let mut count = 0;
    let mut line_tokens = 0;
    let mut nesting = 0usize;
    let mut line_ops = 0;
    for token in lexer::lex(code, Mode::Module) {
        let Ok((token, _)) = token else { return false };
        count += 1;
        line_tokens += 1;
        if count > 6000 || line_tokens > 512 {
            return false;
        }
        match token {
            Tok::Lpar | Tok::Lsqb | Tok::Lbrace => {
                nesting += 1;
                if nesting > 64 {
                    return false;
                }
            }
            Tok::Rpar | Tok::Rsqb | Tok::Rbrace => nesting = nesting.saturating_sub(1),
            Tok::Plus | Tok::Minus | Tok::Star | Tok::Slash | Tok::Percent | Tok::Vbar => {
                line_ops += 1;
                if line_ops > 128 {
                    return false;
                }
            }
            Tok::Newline => {
                line_ops = 0;
                line_tokens = 0;
            }
            _ => {}
        }
    }
    true
}

#[derive(Default)]
struct Audit {
    stores: HashMap<String, usize>,
    tainted: HashSet<String>,
    mutated: HashSet<String>,
    module_loads: HashSet<(String, u32)>,
    calls: Vec<ast::ExprCall>,
    depth: usize,
    nodes: usize,
    invalid: bool,
    module_imports: HashMap<String, u32>,
}

impl Audit {
    fn bind(&mut self, name: &str) {
        self.tainted.insert(name.to_owned());
    }

    fn clean(&self, name: &str) -> bool {
        self.stores.get(name) == Some(&1)
            && !self.tainted.contains(name)
            && !self.mutated.contains(name)
    }
}

impl Visitor for Audit {
    fn visit_stmt(&mut self, node: ast::Stmt) {
        self.nodes += 1;
        self.depth += 1;
        if self.nodes > 6000 || self.depth > 128 {
            self.invalid = true;
        }
        if !self.invalid {
            self.generic_visit_stmt(node);
        }
        self.depth -= 1;
    }

    fn visit_expr(&mut self, node: ast::Expr) {
        self.nodes += 1;
        self.depth += 1;
        if self.nodes > 6000 || self.depth > 128 {
            self.invalid = true;
        }
        if !self.invalid {
            self.generic_visit_expr(node);
        }
        self.depth -= 1;
    }

    fn visit_expr_name(&mut self, node: ast::ExprName) {
        match node.ctx {
            ast::ExprContext::Store | ast::ExprContext::Del => {
                *self.stores.entry(node.id.to_string()).or_default() += 1;
            }
            ast::ExprContext::Load
                if node.id.as_str() == "os" || node.id.as_str() == "datetime" =>
            {
                self.module_loads
                    .insert((node.id.to_string(), node.range.start().to_u32()));
            }
            _ => {}
        }
    }

    fn visit_expr_attribute(&mut self, node: ast::ExprAttribute) {
        if node.attr.as_str() == "modules" {
            self.invalid = true;
        }
        if node.ctx != ast::ExprContext::Load {
            if let Some(root) = root_name(&node.value) {
                self.mutated.insert(root.to_owned());
            }
        }
        self.generic_visit_expr_attribute(node);
    }

    fn visit_expr_subscript(&mut self, node: ast::ExprSubscript) {
        if node.ctx != ast::ExprContext::Load {
            if let Some(root) = root_name(&node.value) {
                self.mutated.insert(root.to_owned());
            }
        }
        self.generic_visit_expr_subscript(node);
    }

    fn visit_expr_call(&mut self, node: ast::ExprCall) {
        if name(&node.func) == Some("vars") {
            self.invalid = true;
        }
        self.calls.push(node.clone());
        self.generic_visit_expr_call(node);
    }

    fn visit_stmt_import(&mut self, node: ast::StmtImport) {
        let import_end = node.range.end().to_u32();
        for alias in node.names {
            let imported = alias.name.as_str();
            let binding = alias.asname.as_ref().map_or_else(
                || imported.split('.').next().unwrap_or(imported),
                |name| name.as_str(),
            );
            if (imported == "os" || imported == "datetime")
                && alias.asname.is_none()
                && self.depth == 1
            {
                self.module_imports
                    .entry(imported.to_owned())
                    .or_insert(import_end);
            } else {
                self.bind(binding);
                if imported.starts_with("os.")
                    || imported.starts_with("datetime.")
                    || imported == "os"
                    || imported == "datetime"
                {
                    self.invalid = true;
                }
            }
        }
    }

    fn visit_stmt_import_from(&mut self, node: ast::StmtImportFrom) {
        if node.module.as_ref().is_some_and(|module| {
            module.as_str().starts_with("os") || module.as_str().starts_with("datetime")
        }) {
            self.invalid = true;
        }
        if node.module.as_ref().is_some_and(|module| module == "sys")
            && node
                .names
                .iter()
                .any(|alias| alias.name.as_str() == "modules")
        {
            self.invalid = true;
        }
        if node
            .module
            .as_ref()
            .is_some_and(|module| module == "os" || module == "datetime")
        {
            self.bind(node.module.as_ref().unwrap().as_str());
        }
        for alias in node.names {
            if alias.name.as_str() == "*" {
                self.invalid = true;
            }
            self.bind(alias.asname.as_ref().unwrap_or(&alias.name).as_str());
        }
    }

    fn visit_stmt_function_def(&mut self, node: ast::StmtFunctionDef) {
        if !node.type_params.is_empty() {
            self.invalid = true;
        }
        self.bind(node.name.as_str());
        self.generic_visit_stmt_function_def(node);
    }
    fn visit_stmt_async_function_def(&mut self, node: ast::StmtAsyncFunctionDef) {
        if !node.type_params.is_empty() {
            self.invalid = true;
        }
        self.bind(node.name.as_str());
        self.generic_visit_stmt_async_function_def(node);
    }
    fn visit_stmt_class_def(&mut self, node: ast::StmtClassDef) {
        if !node.type_params.is_empty() {
            self.invalid = true;
        }
        self.bind(node.name.as_str());
        self.generic_visit_stmt_class_def(node);
    }
    fn visit_stmt_type_alias(&mut self, _node: ast::StmtTypeAlias) {
        self.invalid = true;
    }
    fn visit_arg(&mut self, node: ast::Arg) {
        self.bind(node.arg.as_str());
        if let Some(annotation) = node.annotation {
            self.visit_expr(*annotation);
        }
    }
    fn visit_arguments(&mut self, node: ast::Arguments) {
        for arg in node
            .posonlyargs
            .into_iter()
            .chain(node.args)
            .chain(node.kwonlyargs)
        {
            self.visit_arg(arg.def);
            if let Some(default) = arg.default {
                self.visit_expr(*default);
            }
        }
        if let Some(arg) = node.vararg {
            self.visit_arg(*arg);
        }
        if let Some(arg) = node.kwarg {
            self.visit_arg(*arg);
        }
    }
    fn visit_keyword(&mut self, node: ast::Keyword) {
        self.visit_expr(node.value);
    }
    fn visit_withitem(&mut self, node: ast::WithItem) {
        self.visit_expr(node.context_expr);
        if let Some(target) = node.optional_vars {
            self.visit_expr(*target);
        }
    }
    fn visit_comprehension(&mut self, node: ast::Comprehension) {
        self.visit_expr(node.target);
        self.visit_expr(node.iter);
        for condition in node.ifs {
            self.visit_expr(condition);
        }
    }
    fn visit_match_case(&mut self, node: ast::MatchCase) {
        self.visit_pattern(node.pattern);
        if let Some(guard) = node.guard {
            self.visit_expr(*guard);
        }
        for statement in node.body {
            self.visit_stmt(statement);
        }
    }
    fn visit_excepthandler_except_handler(&mut self, node: ast::ExceptHandlerExceptHandler) {
        if let Some(name) = &node.name {
            self.bind(name.as_str());
        }
        self.generic_visit_excepthandler_except_handler(node);
    }
    fn visit_pattern_match_as(&mut self, node: ast::PatternMatchAs) {
        if let Some(name) = &node.name {
            self.bind(name.as_str());
        }
        self.generic_visit_pattern_match_as(node);
    }
    fn visit_pattern_match_star(&mut self, node: ast::PatternMatchStar) {
        if let Some(name) = &node.name {
            self.bind(name.as_str());
        }
        self.generic_visit_pattern_match_star(node);
    }
    fn visit_pattern_match_mapping(&mut self, node: ast::PatternMatchMapping) {
        if let Some(name) = &node.rest {
            self.bind(name.as_str());
        }
        self.generic_visit_pattern_match_mapping(node);
    }
    fn visit_stmt_global(&mut self, node: ast::StmtGlobal) {
        for name in node.names {
            self.bind(name.as_str());
        }
    }
    fn visit_stmt_nonlocal(&mut self, node: ast::StmtNonlocal) {
        for name in node.names {
            self.bind(name.as_str());
        }
    }
}

struct Proof<'a> {
    audit: &'a Audit,
    facts: HashMap<String, (Fact, u32)>,
    allowed_modules: HashSet<(String, u32)>,
}

impl Proof<'_> {
    fn allow_module(&mut self, expr: &ast::Expr, module: &str) -> bool {
        match expr {
            ast::Expr::Name(value)
                if value.id.as_str() == module
                    && self
                        .audit
                        .module_imports
                        .get(module)
                        .is_some_and(|end| *end <= value.range.start().to_u32())
                    && !self.audit.tainted.contains(module)
                    && !self.audit.mutated.contains(module)
                    && !self.audit.stores.contains_key(module) =>
            {
                self.allowed_modules
                    .insert((module.to_owned(), value.range.start().to_u32()));
                true
            }
            _ => false,
        }
    }

    fn instant(&mut self, expr: &ast::Expr) -> bool {
        let ast::Expr::Call(call) = expr else {
            return false;
        };
        if !call.keywords.is_empty() || call.args.len() > 1 {
            return false;
        }
        let Some(datetime_class) = attr(&call.func, "now").and_then(|expr| attr(expr, "datetime"))
        else {
            return false;
        };
        let saved = self.allowed_modules.clone();
        if !self.allow_module(datetime_class, "datetime") {
            return false;
        }
        if let Some(arg) = call.args.first() {
            let Some(timezone) = attr(arg, "utc").and_then(|expr| attr(expr, "timezone")) else {
                self.allowed_modules = saved;
                return false;
            };
            if self.allow_module(timezone, "datetime") {
                true
            } else {
                self.allowed_modules = saved;
                false
            }
        } else {
            true
        }
    }

    fn fragment(&mut self, expr: &ast::Expr, depth: usize) -> Option<String> {
        if depth > 8 {
            return None;
        }
        if let ast::Expr::Name(value) = expr {
            if let Some((Fact::Fragment(fragment), end)) = self.facts.get(value.id.as_str()) {
                if *end <= value.range.start().to_u32() {
                    return Some(fragment.clone());
                }
            }
        }
        let ast::Expr::Call(call) = expr else {
            return None;
        };
        if !call.keywords.is_empty() {
            return None;
        }
        if let Some(receiver) = attr(&call.func, "strftime") {
            if call.args.len() != 1 {
                return None;
            }
            let is_instant = match receiver {
                ast::Expr::Name(value) => {
                    matches!(self.facts.get(value.id.as_str()), Some((Fact::Instant, end)) if *end <= value.range.start().to_u32())
                }
                _ => self.instant(receiver),
            };
            if !is_instant {
                return None;
            }
            return date_fragment(string(&call.args[0])?);
        }
        let receiver = attr(&call.func, "replace")?;
        if call.args.len() != 2 {
            return None;
        }
        let old = string(&call.args[0])?;
        let new = string(&call.args[1])?;
        if old.is_empty()
            || old.bytes().any(|c| c.is_ascii_digit() || !safe_piece(c))
            || new.bytes().any(|c| !safe_piece(c))
        {
            return None;
        }
        let base = self.fragment(receiver, depth + 1)?;
        let matches = base.matches(old).count();
        let shrink = matches.checked_mul(old.len())?;
        let growth = matches.checked_mul(new.len())?;
        let length = base.len().checked_sub(shrink)?.checked_add(growth)?;
        if length > 4096 {
            return None;
        }
        let result = base.replace(old, new);
        result.bytes().any(|c| c.is_ascii_digit()).then_some(result)
    }

    fn path(&mut self, expr: &ast::Expr, depth: usize) -> Option<String> {
        if depth > 8 {
            return None;
        }
        match expr {
            ast::Expr::Constant(_) => {
                let path = string(expr)?;
                safe_path(path).then(|| path.to_owned())
            }
            ast::Expr::Name(value) => match self.facts.get(value.id.as_str()) {
                Some((Fact::Path(path), end)) if *end <= value.range.start().to_u32() => {
                    Some(path.clone())
                }
                _ => None,
            },
            ast::Expr::BinOp(value) if value.op == ast::Operator::Mod => {
                let template = string(&value.left)?;
                let args: Vec<&ast::Expr> = match value.right.as_ref() {
                    ast::Expr::Tuple(tuple) => tuple.elts.iter().collect(),
                    one => vec![one],
                };
                if args.is_empty()
                    || template.len() > 4096
                    || !template.contains('/')
                    || template.contains('\\')
                {
                    return None;
                }
                let mut parts = template.split("%s");
                let prefix = parts.next()?;
                if !prefix.contains('/') || !safe_path(prefix) {
                    return None;
                }
                let mut path = prefix.to_owned();
                for (part, argument) in parts.zip(args.iter()) {
                    let fragment = self.fragment(argument, depth + 1)?;
                    if path
                        .len()
                        .checked_add(fragment.len())?
                        .checked_add(part.len())?
                        > 4096
                    {
                        return None;
                    }
                    path.push_str(&fragment);
                    path.push_str(part);
                }
                if template.matches("%s").count() != args.len()
                    || path.contains('%')
                    || !safe_path(&path)
                {
                    return None;
                }
                Some(path)
            }
            ast::Expr::Call(call) if os_method(&call.func) == Some("join") => {
                if call.args.len() < 2 || !call.keywords.is_empty() {
                    return None;
                }
                let module = os_root(&call.func)?;
                if !self.allow_module(module, "os") {
                    return None;
                }
                let mut path = self.path(&call.args[0], depth + 1)?;
                for arg in call.args.iter().skip(1) {
                    let part = self
                        .path(arg, depth + 1)
                        .or_else(|| self.fragment(arg, depth + 1))?;
                    if part.starts_with('/') || part.contains('\\') {
                        return None;
                    }
                    if path.len().checked_add(1)?.checked_add(part.len())? > 4096 {
                        return None;
                    }
                    path.push('/');
                    path.push_str(&part);
                }
                safe_path(&path).then_some(path)
            }
            _ => None,
        }
    }

    fn os_call(&mut self, call: &ast::ExprCall) -> bool {
        let Some(method) = os_method(&call.func) else {
            return false;
        };
        let Some(module) = os_root(&call.func) else {
            return false;
        };
        if !self.allow_module(module, "os") {
            return false;
        }
        match method {
            "join" => self.path(&ast::Expr::Call(call.clone()), 0).is_some(),
            "exists" | "getsize" => {
                call.args.len() == 1
                    && call.keywords.is_empty()
                    && self.path(&call.args[0], 0).is_some()
            }
            "makedirs" => {
                if call.args.len() != 1
                    || call.keywords.len() > 1
                    || self.path(&call.args[0], 0).is_none()
                {
                    return false;
                }
                call.keywords.first().is_none_or(|keyword| {
                    keyword.arg.as_ref().is_some_and(|arg| arg == "exist_ok")
                        && matches!(&keyword.value, ast::Expr::Constant(value) if matches!(value.value, ast::Constant::Bool(_)))
                })
            }
            _ => false,
        }
    }
}

fn safe_piece(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')
}

fn date_fragment(format: &str) -> Option<String> {
    let bytes = format.as_bytes();
    let mut result = String::new();
    let mut i = 0;
    let mut digits = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            i += 1;
            if !matches!(
                bytes.get(i),
                Some(b'Y' | b'm' | b'd' | b'H' | b'M' | b'S' | b'f')
            ) {
                return None;
            }
            result.push('0');
            digits += 1;
        } else if safe_piece(bytes[i]) {
            result.push(bytes[i] as char);
        } else {
            return None;
        }
        i += 1;
        if result.len() > 4096 {
            return None;
        }
    }
    (digits > 0).then_some(result)
}

fn os_root(expr: &ast::Expr) -> Option<&ast::Expr> {
    if let Some(path) = attr(expr, "path") {
        return Some(path);
    }
    match expr {
        ast::Expr::Attribute(value) => {
            if value.attr.as_str() == "makedirs" {
                Some(&value.value)
            } else {
                attr(&value.value, "path")
            }
        }
        _ => None,
    }
}

fn os_method(expr: &ast::Expr) -> Option<&str> {
    let ast::Expr::Attribute(value) = expr else {
        return None;
    };
    match value.attr.as_str() {
        "makedirs" if name(&value.value) == Some("os") => Some("makedirs"),
        "join" | "exists" | "getsize"
            if attr(&value.value, "path").and_then(name) == Some("os") =>
        {
            Some(value.attr.as_str())
        }
        _ => None,
    }
}

pub(super) fn safe_os_path_uses(code: &str) -> bool {
    if !preflight(code) {
        return false;
    }
    let Ok(suite) = ast::Suite::parse(code, "<runtime>") else {
        return false;
    };
    let mut audit = Audit::default();
    for statement in suite.iter().cloned() {
        audit.visit_stmt(statement);
    }
    if audit.invalid
        || ["os", "datetime"].iter().any(|name| {
            audit.tainted.contains(*name)
                || audit.stores.contains_key(*name)
                || audit.mutated.contains(*name)
        })
    {
        return false;
    }
    let mut proof = Proof {
        audit: &audit,
        facts: HashMap::new(),
        allowed_modules: HashSet::new(),
    };
    for statement in &suite {
        if let ast::Stmt::Assign(assignment) = statement {
            if assignment.targets.len() != 1 {
                continue;
            }
            let Some(target) = name(&assignment.targets[0]) else {
                continue;
            };
            if !audit.clean(target) {
                continue;
            }
            let value = &assignment.value;
            let allowed_before = proof.allowed_modules.clone();
            let fact = if let Some(path) = proof.path(value, 0) {
                Some(Fact::Path(path))
            } else if proof.instant(value) {
                Some(Fact::Instant)
            } else {
                proof.fragment(value, 0).map(Fact::Fragment)
            };
            if let Some(fact) = fact {
                proof
                    .facts
                    .insert(target.to_owned(), (fact, assignment.range.end().to_u32()));
            } else {
                proof.allowed_modules = allowed_before;
            }
        }
    }
    for call in &audit.calls {
        if root_name(&call.func) == Some("os") && !proof.os_call(call) {
            return false;
        }
        if name(&call.func).is_some_and(|name| name == "open" || name == "Path")
            && (call.args.is_empty() || proof.path(&call.args[0], 0).is_none())
        {
            return false;
        }
    }
    audit.module_loads.is_subset(&proof.allowed_modules)
}
