use omicsops_adapters::ssh::SshSession;
use omicsops_core::{
    CoreError, CoreResult,
    plan_v2::{StepAction, StepSpecV2, VerificationSpec, action_hash},
    project::shell_quote,
    tools::ToolCatalog,
};
use serde::{Deserialize, Serialize};

pub fn reconcile_manifest(contents: &str, expected_action_hash: &str) -> Result<(), String> {
    if contents.trim().is_empty() {
        return Err("remote manifest is missing".into());
    }
    let value: Value = serde_json::from_str(contents)
        .map_err(|_| "remote manifest is invalid or was tampered with".to_owned())?;
    if value.get("action_hash").and_then(Value::as_str) != Some(expected_action_hash) {
        return Err("remote manifest action hash does not match the checkpoint".into());
    }
    if value.get("status").and_then(Value::as_str) != Some("exited")
        || value.get("exit_code").and_then(Value::as_u64) != Some(0)
    {
        return Err("remote manifest does not record a successful exit".into());
    }
    Ok(())
}
use serde_json::Value;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionResultV2 {
    pub exit_status: u32,
    pub process_group_id: Option<u32>,
    pub verifications: Vec<omicsops_core::plan_v2::VerificationResult>,
}

pub struct SshV2Executor {
    session: Arc<SshSession>,
    project_root: String,
    catalog: ToolCatalog,
    poll_interval: Duration,
    cancel_requested: Arc<AtomicBool>,
}

impl SshV2Executor {
    pub fn new(
        session: Arc<SshSession>,
        project_root: impl Into<String>,
        catalog: ToolCatalog,
        cancel_requested: Arc<AtomicBool>,
    ) -> Self {
        Self {
            session,
            project_root: project_root.into().trim_end_matches('/').into(),
            catalog,
            poll_interval: Duration::from_secs(2),
            cancel_requested,
        }
    }

    pub async fn execute(
        &self,
        step: &StepSpecV2,
        attempt: u8,
    ) -> Result<ExecutionResultV2, String> {
        let id = safe_identifier(&step.id);
        let script_path = format!("{}/scripts/{id}.{attempt}.sh", self.project_root);
        let script = render_remote_step_script_v2(&self.project_root, step, attempt, &self.catalog)
            .map_err(|error| error.to_string())?;
        self.session
            .upload_text(&script_path, &script)
            .await
            .map_err(|error| error.to_string())?;
        self.session
            .execute_checked(&format!(
                "chmod 700 {} && {}",
                shell_quote(&script_path),
                crate::render_start_command(&self.project_root, &step.id, attempt)
            ))
            .await
            .map_err(|error| error.to_string())?;
        self.poll_and_verify(step, attempt).await
    }

    pub async fn recover(
        &self,
        step: &StepSpecV2,
        attempt: u8,
        expected_action_hash: &str,
    ) -> Result<ExecutionResultV2, String> {
        let id = safe_identifier(&step.id);
        let manifest_path = format!(
            "{}/.omicsops/state/{id}.{attempt}.manifest.json",
            self.project_root
        );
        let manifest = self
            .session
            .execute(&format!(
                "test -f {0} && cat {0}",
                shell_quote(&manifest_path)
            ))
            .await
            .map_err(|error| error.to_string())?;
        let manifest_value: Value = serde_json::from_str(&manifest.stdout)
            .map_err(|_| "remote manifest is missing, invalid, or was tampered with".to_owned())?;
        if manifest_value.get("action_hash").and_then(Value::as_str) != Some(expected_action_hash) {
            return Err(
                "remote manifest action hash does not match the approved recovery action".into(),
            );
        }
        let output = self
            .session
            .execute(&crate::render_status_command(
                &self.project_root,
                &step.id,
                attempt,
            ))
            .await
            .map_err(|error| error.to_string())?;
        if output.stdout.trim() == "LOST" {
            return Err(
                "remote process and exit state are missing; refusing a silent rerun".into(),
            );
        }
        self.poll_and_verify(step, attempt).await
    }

    async fn poll_and_verify(
        &self,
        step: &StepSpecV2,
        attempt: u8,
    ) -> Result<ExecutionResultV2, String> {
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(step.resources.max_step_seconds);
        let exit_status = loop {
            if self.cancel_requested.load(Ordering::SeqCst)
                || tokio::time::Instant::now() >= deadline
            {
                self.session
                    .execute(&crate::render_cancel_command(
                        &self.project_root,
                        &step.id,
                        attempt,
                        false,
                    ))
                    .await
                    .map_err(|error| error.to_string())?;
                break if self.cancel_requested.load(Ordering::SeqCst) {
                    130
                } else {
                    124
                };
            }
            let output = self
                .session
                .execute(&crate::render_status_command(
                    &self.project_root,
                    &step.id,
                    attempt,
                ))
                .await
                .map_err(|error| error.to_string())?;
            if let Some(value) = output.stdout.trim().strip_prefix("EXIT:") {
                break value.trim().parse().unwrap_or(255);
            }
            if output.stdout.trim() == "LOST" {
                break 255;
            }
            tokio::time::sleep(self.poll_interval).await;
        };
        let mut verifications = Vec::new();
        for specification in &step.verifications {
            let command =
                render_verification_command(specification).map_err(|error| error.to_string())?;
            let output = self
                .session
                .execute(&format!(
                    "cd {} && {command}",
                    shell_quote(&self.project_root)
                ))
                .await
                .map_err(|error| error.to_string())?;
            verifications.push(omicsops_core::plan_v2::VerificationResult {
                specification: specification.clone(),
                passed: output.status == 0,
                observed: if output.stderr.trim().is_empty() {
                    output.stdout
                } else {
                    output.stderr
                },
            });
        }
        let id = safe_identifier(&step.id);
        let pgid_path = format!("{}/.omicsops/state/{id}.{attempt}.pgid", self.project_root);
        let process_group_id = self
            .session
            .execute(&format!("test -f {0} && cat {0}", shell_quote(&pgid_path)))
            .await
            .ok()
            .and_then(|output| output.stdout.trim().parse().ok());
        Ok(ExecutionResultV2 {
            exit_status,
            process_group_id,
            verifications,
        })
    }

    pub async fn verify_completed(
        &self,
        step: &StepSpecV2,
        attempt: u8,
        expected_action_hash: &str,
    ) -> Result<(), String> {
        let id = safe_identifier(&step.id);
        let manifest_path = format!(
            "{}/.omicsops/state/{id}.{attempt}.manifest.json",
            self.project_root
        );
        let manifest = self
            .session
            .execute(&format!(
                "test -f {0} && cat {0}",
                shell_quote(&manifest_path)
            ))
            .await
            .map_err(|error| error.to_string())?;
        if manifest.status != 0 {
            return Err("remote manifest is missing".into());
        }
        reconcile_manifest(&manifest.stdout, expected_action_hash)?;
        for specification in &step.verifications {
            let command =
                render_verification_command(specification).map_err(|error| error.to_string())?;
            let output = self
                .session
                .execute(&format!(
                    "cd {} && {command}",
                    shell_quote(&self.project_root)
                ))
                .await
                .map_err(|error| error.to_string())?;
            if output.status != 0 {
                return Err(format!(
                    "completed artifact failed re-verification: {:?}",
                    specification
                ));
            }
        }
        Ok(())
    }

    pub async fn artifact_metadata(&self, relative_path: &str) -> Result<(u64, String), String> {
        let path = format!(
            "{}/{}",
            self.project_root,
            relative_path.trim_start_matches('/')
        );
        let output = self
            .session
            .execute_checked(&format!(
                "stat -c '%s' {0} && sha256sum {0}",
                shell_quote(&path)
            ))
            .await
            .map_err(|error| error.to_string())?;
        let mut lines = output.stdout.lines();
        let size = lines
            .next()
            .and_then(|value| value.trim().parse().ok())
            .ok_or_else(|| "artifact size was not returned".to_owned())?;
        let sha256 = lines
            .next()
            .and_then(|value| value.split_whitespace().next())
            .ok_or_else(|| "artifact hash was not returned".to_owned())?
            .to_owned();
        Ok((size, sha256))
    }
}

pub fn compile_step_action(action: &StepAction, catalog: &ToolCatalog) -> CoreResult<String> {
    match action {
        StepAction::LegacyShell { command } => Ok(command.clone()),
        StepAction::Tool {
            tool_id,
            version,
            arguments,
        } => {
            let manifest = catalog.get(tool_id, version).ok_or_else(|| {
                CoreError::Validation(format!("unknown tool {tool_id}@{version}"))
            })?;
            let object = arguments.as_object().ok_or_else(|| {
                CoreError::Validation("tool arguments must be a JSON object".into())
            })?;
            if tool_id == "core.download" {
                let url = required_string(object, "url")?;
                let output = required_string(object, "output")?;
                let mut command = format!(
                    "'curl' '--fail' '--location' '--proto' '=https' '--output' {} {}",
                    shell_quote(output),
                    shell_quote(url)
                );
                if let Some(hash) = object.get("sha256").and_then(Value::as_str) {
                    command.push_str(&format!(
                        " && printf '%s  %s\\n' {} {} | sha256sum --check --status",
                        shell_quote(hash),
                        shell_quote(output)
                    ));
                }
                return Ok(command);
            }
            if tool_id == "env.micromamba" {
                let mut command = vec![
                    shell_quote(&manifest.executable),
                    "'create'".into(),
                    "'--yes'".into(),
                    "'--name'".into(),
                    shell_quote(required_string(object, "name")?),
                ];
                if let Some(file) = object.get("file").and_then(Value::as_str) {
                    command.extend(["'--file'".into(), shell_quote(file)]);
                }
                return Ok(command.join(" "));
            }
            if tool_id == "bio.salmon" {
                let threads = object
                    .get("threads")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .to_string();
                return Ok([
                    "'salmon'",
                    "'quant'",
                    "'-i'",
                    &shell_quote(required_string(object, "index")?),
                    "'-l'",
                    "'A'",
                    "'-1'",
                    &shell_quote(required_string(object, "read1")?),
                    "'-2'",
                    &shell_quote(required_string(object, "read2")?),
                    "'-o'",
                    &shell_quote(required_string(object, "output")?),
                    "'-p'",
                    &shell_quote(&threads),
                ]
                .join(" "));
            }
            let positional = match tool_id.as_str() {
                "bio.fastqc" => vec!["input"],
                "bio.multiqc" => vec!["input"],
                "bio.deseq2" | "bio.scanpy" | "bio.h5ad_to_seurat" | "report.html" => {
                    vec!["script"]
                }
                _ => Vec::new(),
            };
            let mut command = vec![shell_quote(&manifest.executable)];
            for name in &positional {
                command.push(shell_quote(required_string(object, name)?));
            }
            for (name, value) in object {
                if positional.contains(&name.as_str()) {
                    continue;
                }
                let option = format!("--{}", name.replace('_', "-"));
                match value {
                    Value::Bool(true) => command.push(shell_quote(&option)),
                    Value::Bool(false) | Value::Null => {}
                    Value::Array(values) => {
                        for value in values {
                            command.push(shell_quote(&option));
                            command.push(shell_quote(&scalar(value)?));
                        }
                    }
                    value => {
                        command.push(shell_quote(&option));
                        command.push(shell_quote(&scalar(value)?));
                    }
                }
            }
            Ok(command.join(" "))
        }
    }
}

fn required_string<'a>(
    object: &'a serde_json::Map<String, Value>,
    name: &str,
) -> CoreResult<&'a str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| CoreError::Validation(format!("tool argument {name} must be a string")))
}

fn scalar(value: &Value) -> CoreResult<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        _ => Err(CoreError::Validation(
            "nested tool arguments are not supported by the shell compiler".into(),
        )),
    }
}

pub fn render_remote_step_script_v2(
    root: &str,
    step: &StepSpecV2,
    attempt: u8,
    catalog: &ToolCatalog,
) -> CoreResult<String> {
    let root = root.trim_end_matches('/');
    let id = safe_identifier(&step.id);
    let prefix = format!("{root}/.omicsops/state/{id}.{attempt}");
    let working = format!("{root}/{}", step.working_directory.trim_matches('/'));
    let log = format!("{root}/logs/{id}.{attempt}.log");
    let manifest = format!("{prefix}.manifest.json");
    let command = compile_step_action(&step.action, catalog)?;
    let action_hash = action_hash(&step.action)?;
    let memory_bytes = u64::from(step.resources.max_memory_gib) * 1024 * 1024 * 1024;
    let disk_kib = u64::from(step.resources.max_disk_gib) * 1024 * 1024;
    let threads = step.resources.max_cpu_cores.max(1);
    Ok(format!(
        "#!/usr/bin/env bash\n# launched with setsid to create an independent process group\nset -u -o pipefail\numask 077\n\
         printf '%s\\n' \"$$\" > {pgid_tmp}\nmv {pgid_tmp} {pgid}\n\
         printf '%s\\n' '{{\"action_hash\":\"{action_hash}\",\"attempt\":{attempt},\"status\":\"running\",\"pgid\":'\"$$\"'}}' > {manifest_tmp}\nmv {manifest_tmp} {manifest}\n\
         resolved_root=$(realpath -m {root})\nresolved_working=$(realpath -m {working})\ncase \"$resolved_working\" in \"$resolved_root\"|\"$resolved_root\"/*) ;; *) printf '%s\\n' 'path escapes project root' >&2; exit 126 ;; esac\n\
         usage_kib=$(du -sk {root} | awk '{{print $1}}')\nif test \"$usage_kib\" -ge {disk_kib}; then printf '%s\\n' 'disk budget exceeded before execution' >&2; exit 125; fi\n\
         cd {working}\nset +e\n\
         export OMP_NUM_THREADS={threads} OPENBLAS_NUM_THREADS={threads} MKL_NUM_THREADS={threads} NUMEXPR_NUM_THREADS={threads}\n\
         if command -v taskset >/dev/null 2>&1; then cpu_prefix=\"taskset -c 0-$(({threads}-1))\"; else cpu_prefix=; fi\n\
         $cpu_prefix prlimit --as={memory_bytes} -- bash -c {quoted_command} > {log} 2>&1\n\
         status=$?\nusage_kib=$(du -sk {root} | awk '{{print $1}}')\nif test \"$usage_kib\" -gt {disk_kib}; then printf '%s\\n' 'disk budget exceeded after execution' >> {log}; status=125; fi\nprintf '%s\\n' \"$status\" > {exit_tmp}\nmv {exit_tmp} {exit}\n\
         printf '%s\\n' '{{\"action_hash\":\"{action_hash}\",\"attempt\":{attempt},\"status\":\"exited\",\"exit_code\":'\"$status\"'}}' > {manifest_tmp}\nmv {manifest_tmp} {manifest}\nexit \"$status\"\n",
        pgid_tmp = shell_quote(&format!("{prefix}.pgid.tmp")),
        pgid = shell_quote(&format!("{prefix}.pgid")),
        manifest_tmp = shell_quote(&format!("{manifest}.tmp")),
        manifest = shell_quote(&manifest),
        root = shell_quote(root),
        disk_kib = disk_kib,
        working = shell_quote(&working),
        quoted_command = shell_quote(&command),
        log = shell_quote(&log),
        exit_tmp = shell_quote(&format!("{prefix}.exit.tmp")),
        exit = shell_quote(&format!("{prefix}.exit")),
    ))
}

pub fn render_verification_command(spec: &VerificationSpec) -> CoreResult<String> {
    match spec {
        VerificationSpec::ExitCode { expected } => Ok(format!("test \"$status\" -eq {expected}")),
        VerificationSpec::File {
            path,
            min_bytes,
            sha256,
        } => {
            let path = shell_quote(path);
            let mut command =
                format!("test -f {path} && test \"$(wc -c < {path})\" -ge {min_bytes}");
            if let Some(hash) = sha256 {
                command.push_str(&format!(
                    " && test \"$(sha256sum {path} | cut -d' ' -f1)\" = {}",
                    shell_quote(hash)
                ));
            }
            Ok(command)
        }
        VerificationSpec::JsonField {
            path,
            pointer,
            expected,
        } => python_check(
            "import functools,json,sys; v=json.load(open(sys.argv[1])); p=sys.argv[2]; parts=[x.replace('~1','/').replace('~0','~') for x in p.strip('/').split('/') if x]; v=functools.reduce(lambda a,k:a[int(k)] if isinstance(a,list) else a[k],parts,v); assert str(v).lower()==sys.argv[3].lower() # json_pointer",
            &[path, pointer, expected],
        ),
        VerificationSpec::Table {
            path,
            delimiter,
            required_columns,
            min_rows,
        } => python_check(
            "import csv,sys; rows=list(csv.reader(open(sys.argv[1],newline=''),delimiter=sys.argv[2])); assert rows; assert set(sys.argv[3].split('\\x1f')).issubset(rows[0]); assert len(rows)-1>=int(sys.argv[4])",
            &[
                path,
                &delimiter.to_string(),
                &required_columns.join("\u{1f}"),
                &min_rows.to_string(),
            ],
        ),
        VerificationSpec::DomainReport { path, validator } => match validator.as_str() {
            "h5ad" => python_check(
                "import anndata,sys; x=anndata.read_h5ad(sys.argv[1],backed='r'); assert x.n_obs>0 and x.n_vars>0",
                &[path],
            ),
            "seurat" | "rds" => Ok(format!(
                "Rscript -e {} {}",
                shell_quote(
                    "x<-readRDS(commandArgs(TRUE)[1]); stopifnot(inherits(x, 'Seurat'), ncol(x)>0, nrow(x)>0)"
                ),
                shell_quote(path)
            )),
            other => Err(CoreError::Validation(format!(
                "unknown domain validator {other}"
            ))),
        },
    }
}

fn python_check(program: &str, arguments: &[&str]) -> CoreResult<String> {
    let mut command = vec!["python".to_owned(), "-c".to_owned(), shell_quote(program)];
    command.extend(arguments.iter().map(|value| shell_quote(value)));
    Ok(command.join(" "))
}

fn safe_identifier(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
