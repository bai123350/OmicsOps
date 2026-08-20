use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use async_trait::async_trait;
use omicsops_protocol::{
    ComputeBackendDescriptorV4, ComputeBackendKindV4, ExecutionContextKeyV4, IsolationStrengthV4,
    KernelLanguageV4, OutputCaptureV4, RuntimeArtifactV4, RuntimeResultV4,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdout, Command},
    sync::Mutex,
};
use uuid::Uuid;

use crate::{KernelBackendV4, KernelProcessV4};

pub struct LocalKernelBackendV4 {
    project_root: PathBuf,
    python_program: String,
    r_program: String,
}

impl LocalKernelBackendV4 {
    pub fn new(project_root: impl AsRef<Path>) -> Result<Self, String> {
        let project_root = canonical_project_root(project_root.as_ref())?;
        Ok(Self {
            project_root,
            python_program: "python".into(),
            r_program: "Rscript".into(),
        })
    }

    pub fn with_programs(mut self, python: impl Into<String>, r: impl Into<String>) -> Self {
        self.python_program = python.into();
        self.r_program = r.into();
        self
    }
}

#[async_trait]
impl KernelBackendV4 for LocalKernelBackendV4 {
    fn descriptor(&self) -> ComputeBackendDescriptorV4 {
        ComputeBackendDescriptorV4 {
            schema_version: 4,
            backend_id: "local".into(),
            kind: ComputeBackendKindV4::Local,
            isolation: IsolationStrengthV4::Process,
            available: true,
            supports_python: true,
            supports_r: true,
            supports_network_policy: false,
        }
    }

    async fn launch(
        &self,
        key: &ExecutionContextKeyV4,
    ) -> Result<Arc<dyn KernelProcessV4>, String> {
        if key.backend_id != "local" {
            return Err("local backend received a mismatched backend id".into());
        }
        if key.environment != "system" {
            return Err("local V4 backend currently supports only the system environment".into());
        }
        let session_id = Uuid::new_v4();
        let driver = write_driver(&self.project_root, key.language, session_id).await?;
        let mut command = match key.language {
            KernelLanguageV4::Python => {
                let mut command = Command::new(&self.python_program);
                command.arg("-u");
                command
            }
            KernelLanguageV4::R => {
                let mut command = Command::new(&self.r_program);
                command.arg("--vanilla");
                command
            }
        };
        command
            .arg(&driver)
            .arg(&self.project_root)
            .arg(key.project_id.to_string())
            .arg(session_id.to_string())
            .current_dir(&self.project_root);
        launch_process(
            command,
            session_id,
            key,
            self.project_root.clone(),
            "local",
            None,
        )
        .await
    }
}

pub struct ContainerKernelBackendV4 {
    project_root: PathBuf,
    kind: ComputeBackendKindV4,
    engine_program: String,
    image: String,
    network_enabled: bool,
}

impl ContainerKernelBackendV4 {
    pub fn docker(
        project_root: impl AsRef<Path>,
        image: impl Into<String>,
    ) -> Result<Self, String> {
        Self::new(project_root, ComputeBackendKindV4::Docker, "docker", image)
    }

    pub fn podman(
        project_root: impl AsRef<Path>,
        image: impl Into<String>,
    ) -> Result<Self, String> {
        Self::new(project_root, ComputeBackendKindV4::Podman, "podman", image)
    }

    fn new(
        project_root: impl AsRef<Path>,
        kind: ComputeBackendKindV4,
        engine_program: impl Into<String>,
        image: impl Into<String>,
    ) -> Result<Self, String> {
        let image = image.into();
        if image.trim().is_empty() || image.chars().any(char::is_whitespace) {
            return Err("container image must be a non-empty argument-safe reference".into());
        }
        Ok(Self {
            project_root: canonical_project_root(project_root.as_ref())?,
            kind,
            engine_program: engine_program.into(),
            image,
            network_enabled: false,
        })
    }

    pub fn with_engine_program(mut self, program: impl Into<String>) -> Self {
        self.engine_program = program.into();
        self
    }

    pub fn with_network(mut self, enabled: bool) -> Self {
        self.network_enabled = enabled;
        self
    }

    fn backend_id(&self) -> &'static str {
        match self.kind {
            ComputeBackendKindV4::Docker => "docker",
            ComputeBackendKindV4::Podman => "podman",
            _ => unreachable!("container constructor fixes backend kind"),
        }
    }

    fn container_arguments(
        &self,
        key: &ExecutionContextKeyV4,
        relative_driver: &str,
        session_id: Uuid,
        container_name: &str,
    ) -> Vec<String> {
        let mount = format!("{}:/workspace:rw", self.project_root.display());
        let mut arguments = vec![
            "run".into(),
            "--rm".into(),
            "-i".into(),
            "--name".into(),
            container_name.into(),
            "--read-only".into(),
            "--cap-drop=ALL".into(),
            "--security-opt=no-new-privileges".into(),
            "--pids-limit=256".into(),
            "--network".into(),
            if self.network_enabled {
                "bridge"
            } else {
                "none"
            }
            .into(),
            "-v".into(),
            mount,
            "-w".into(),
            "/workspace".into(),
            self.image.clone(),
        ];
        match key.language {
            KernelLanguageV4::Python => arguments.extend(["python".into(), "-u".into()]),
            KernelLanguageV4::R => arguments.extend(["Rscript".into(), "--vanilla".into()]),
        }
        arguments.extend([
            format!("/workspace/{relative_driver}"),
            "/workspace".into(),
            key.project_id.to_string(),
            session_id.to_string(),
        ]);
        arguments
    }
}

#[async_trait]
impl KernelBackendV4 for ContainerKernelBackendV4 {
    fn descriptor(&self) -> ComputeBackendDescriptorV4 {
        ComputeBackendDescriptorV4 {
            schema_version: 4,
            backend_id: self.backend_id().into(),
            kind: self.kind,
            isolation: IsolationStrengthV4::Container,
            available: true,
            supports_python: true,
            supports_r: true,
            supports_network_policy: true,
        }
    }

    async fn launch(
        &self,
        key: &ExecutionContextKeyV4,
    ) -> Result<Arc<dyn KernelProcessV4>, String> {
        if key.backend_id != self.backend_id() {
            return Err("container backend received a mismatched backend id".into());
        }
        if key.environment != "system" {
            return Err(
                "container V4 backend freezes its environment in the selected image".into(),
            );
        }
        let session_id = Uuid::new_v4();
        let driver = write_driver(&self.project_root, key.language, session_id).await?;
        let relative_driver = driver
            .strip_prefix(&self.project_root)
            .map_err(|_| "kernel driver escaped project root".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let container_name = format!("omicsops-v4-{session_id}");
        let mut command = Command::new(&self.engine_program);
        command.args(self.container_arguments(key, &relative_driver, session_id, &container_name));
        launch_process(
            command,
            session_id,
            key,
            self.project_root.clone(),
            self.backend_id(),
            Some((self.engine_program.clone(), container_name)),
        )
        .await
    }
}

struct ProcessHandle {
    child: Child,
    stdin: tokio::process::ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
}

struct ProcessKernelV4 {
    id: Uuid,
    identity: String,
    project_root: PathBuf,
    output_dir: PathBuf,
    handle: Mutex<Option<ProcessHandle>>,
    container_cleanup: Option<(String, String)>,
}

#[async_trait]
impl KernelProcessV4 for ProcessKernelV4 {
    fn session_id(&self) -> Uuid {
        self.id
    }

    fn process_identity(&self) -> &str {
        &self.identity
    }

    async fn execute(
        &self,
        code: String,
        capture_paths: Vec<String>,
    ) -> Result<RuntimeResultV4, String> {
        if code.trim().is_empty() || code.len() > 1024 * 1024 {
            return Err("kernel code must contain 1..=1048576 bytes".into());
        }
        for path in &capture_paths {
            validate_capture_path(path)?;
        }
        let request_id = Uuid::new_v4();
        let request_id_text = request_id.to_string();
        let mut guard = self.handle.lock().await;
        let handle = guard.as_mut().ok_or("kernel process is stopped")?;
        let request = json!({
            "action":"execute",
            "request_id":request_id,
            "code":code,
            "capture_paths":capture_paths,
        });
        handle
            .stdin
            .write_all(format!("{request}\n").as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        handle
            .stdin
            .flush()
            .await
            .map_err(|error| error.to_string())?;
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut artifacts = Vec::new();
        let succeeded = loop {
            let line = handle
                .lines
                .next_line()
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "kernel process disconnected".to_string())?;
            let event: Value = serde_json::from_str(&line)
                .map_err(|error| format!("invalid kernel JSONL event: {error}"))?;
            if event.get("request_id").and_then(Value::as_str) != Some(request_id_text.as_str()) {
                return Err("kernel event request identity mismatch".into());
            }
            match event.get("kind").and_then(Value::as_str) {
                Some("stdout") => stdout.push_str(
                    event
                        .get("payload")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                ),
                Some("stderr") => stderr.push_str(
                    event
                        .get("payload")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                ),
                Some("artifact") => {
                    // The worker is not an authority for scientific file facts.
                    // The Rust Host re-reads the mounted project file and derives
                    // its canonical path, byte size, and digest itself.
                    artifacts.push(
                        verify_runtime_artifact(
                            &self.project_root,
                            &required_string(&event, "relative_path")?,
                        )
                        .await?,
                    );
                }
                Some("completed") => break true,
                Some("failed") => {
                    stderr.push_str(
                        event
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("kernel failed"),
                    );
                    break false;
                }
                Some("started") => {}
                _ => return Err("unknown kernel JSONL event".into()),
            }
        };
        fs::create_dir_all(&self.output_dir)
            .await
            .map_err(|error| error.to_string())?;
        let stdout_capture = archive_output(
            &self.project_root,
            &self.output_dir,
            request_id,
            "stdout",
            &stdout,
        )
        .await?;
        let stderr_capture = archive_output(
            &self.project_root,
            &self.output_dir,
            request_id,
            "stderr",
            &stderr,
        )
        .await?;
        Ok(RuntimeResultV4 {
            request_id,
            session_id: self.id,
            process_identity: self.identity.clone(),
            stdout: stdout_capture.excerpt.clone(),
            stderr: stderr_capture.excerpt.clone(),
            stdout_capture: Some(stdout_capture),
            stderr_capture: Some(stderr_capture),
            succeeded,
            artifacts,
            software_versions: BTreeMap::new(),
        })
    }

    async fn interrupt(&self) -> Result<(), String> {
        if let Some(mut handle) = self.handle.lock().await.take() {
            let _ = handle.child.kill().await;
            let _ = handle.child.wait().await;
        }
        if let Some((engine, name)) = &self.container_cleanup {
            let _ = Command::new(engine)
                .arg("rm")
                .arg("-f")
                .arg(name)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        Ok(())
    }
}

async fn launch_process(
    mut command: Command,
    session_id: Uuid,
    key: &ExecutionContextKeyV4,
    project_root: PathBuf,
    backend: &str,
    container_cleanup: Option<(String, String)>,
) -> Result<Arc<dyn KernelProcessV4>, String> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Kernel protocol output must only use stdout. Inheriting or piping an
        // unread engine stderr could deadlock a long-running container process.
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let process_id = child.id().ok_or("kernel process id is unavailable")?;
    let stdin = child.stdin.take().ok_or("kernel stdin is unavailable")?;
    let stdout = child.stdout.take().ok_or("kernel stdout is unavailable")?;
    Ok(Arc::new(ProcessKernelV4 {
        id: session_id,
        identity: format!("{backend}-jsonl:{process_id}:{session_id}"),
        output_dir: project_root
            .join(".omicsops")
            .join("runs")
            .join(key.run_id.to_string())
            .join("outputs"),
        project_root,
        handle: Mutex::new(Some(ProcessHandle {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
        })),
        container_cleanup,
    }))
}

async fn write_driver(
    project_root: &Path,
    language: KernelLanguageV4,
    session_id: Uuid,
) -> Result<PathBuf, String> {
    let directory = project_root.join(".omicsops").join("kernels");
    fs::create_dir_all(&directory)
        .await
        .map_err(|error| error.to_string())?;
    let (extension, contents) = match language {
        KernelLanguageV4::Python => ("py", PYTHON_DRIVER),
        KernelLanguageV4::R => ("R", R_DRIVER),
    };
    let path = directory.join(format!("v4-{session_id}.driver.{extension}"));
    fs::write(&path, contents)
        .await
        .map_err(|error| error.to_string())?;
    Ok(path)
}

async fn archive_output(
    project_root: &Path,
    output_dir: &Path,
    request_id: Uuid,
    stream: &str,
    contents: &str,
) -> Result<OutputCaptureV4, String> {
    let path = output_dir.join(format!("{request_id}.{stream}.txt"));
    fs::write(&path, contents)
        .await
        .map_err(|error| error.to_string())?;
    let archive_path = path
        .strip_prefix(project_root)
        .map_err(|_| "output archive escaped project root".to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    let (excerpt, truncated) = bounded_excerpt(contents, 16 * 1024);
    Ok(OutputCaptureV4 {
        excerpt,
        total_bytes: contents.len() as u64,
        sha256: hex::encode(Sha256::digest(contents.as_bytes())),
        archive_path,
        truncated,
    })
}

async fn verify_runtime_artifact(
    project_root: &Path,
    relative_path: &str,
) -> Result<RuntimeArtifactV4, String> {
    validate_capture_path(relative_path)?;
    let path = fs::canonicalize(project_root.join(relative_path))
        .await
        .map_err(|error| format!("artifact is unavailable: {error}"))?;
    if !path.starts_with(project_root) || path == project_root {
        return Err("artifact escaped the canonical project root".into());
    }
    let metadata = fs::metadata(&path)
        .await
        .map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("artifact is not a regular file".into());
    }
    let mut file = fs::File::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(RuntimeArtifactV4 {
        relative_path: path
            .strip_prefix(project_root)
            .map_err(|_| "artifact escaped project root".to_string())?
            .to_string_lossy()
            .replace('\\', "/"),
        size_bytes: metadata.len(),
        sha256: hex::encode(digest.finalize()),
    })
}

fn canonical_project_root(path: &Path) -> Result<PathBuf, String> {
    let root = path.canonicalize().map_err(|error| error.to_string())?;
    if !root.is_dir() {
        return Err("compute project root is not a directory".into());
    }
    Ok(root)
}

fn validate_capture_path(path: &str) -> Result<(), String> {
    let normalized = path.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".." | ".omicsops"))
    {
        return Err(format!("unsafe kernel capture path: {path}"));
    }
    Ok(())
}

fn required_string(value: &Value, field: &str) -> Result<String, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("kernel event field {field} is missing"))
}

fn bounded_excerpt(value: &str, limit: usize) -> (String, bool) {
    if value.len() <= limit {
        return (value.into(), false);
    }
    let half = limit / 2;
    let mut head = half.min(value.len());
    while !value.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = value.len().saturating_sub(half);
    while !value.is_char_boundary(tail) {
        tail += 1;
    }
    (
        format!(
            "{}\n... <output truncated; full stream archived> ...\n{}",
            &value[..head],
            &value[tail..]
        ),
        true,
    )
}

const PYTHON_DRIVER: &str = r#"import contextlib, hashlib, io, json, pathlib, sys, traceback
project = pathlib.Path(sys.argv[1]).resolve()
scope = {"__name__": "__omicsops_kernel__"}
def emit(request_id, kind, **values):
    event = {"request_id": request_id, "kind": kind}
    event.update(values)
    print(json.dumps(event, ensure_ascii=False), flush=True)
for raw in sys.stdin:
    try:
        request = json.loads(raw); request_id = request["request_id"]
        emit(request_id, "started")
        stdout, stderr = io.StringIO(), io.StringIO()
        try:
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                exec(compile(request["code"], "<omicsops-cell>", "exec"), scope, scope)
            if stdout.getvalue(): emit(request_id, "stdout", payload=stdout.getvalue())
            if stderr.getvalue(): emit(request_id, "stderr", payload=stderr.getvalue())
            for relative in request.get("capture_paths", []):
                path = (project / relative).resolve()
                if project not in path.parents or ".omicsops" in path.relative_to(project).parts or not path.is_file():
                    raise ValueError("artifact path is not a regular project file: " + relative)
                digest = hashlib.sha256()
                with path.open("rb") as stream:
                    for chunk in iter(lambda: stream.read(1024 * 1024), b""): digest.update(chunk)
                emit(request_id, "artifact", relative_path=path.relative_to(project).as_posix(), size_bytes=path.stat().st_size, sha256=digest.hexdigest())
            emit(request_id, "completed")
        except Exception as error:
            if stdout.getvalue(): emit(request_id, "stdout", payload=stdout.getvalue())
            emit(request_id, "stderr", payload=traceback.format_exc())
            emit(request_id, "failed", message=str(error))
    except Exception as error:
        print(json.dumps({"request_id":"protocol","kind":"failed","message":str(error)}), flush=True)
"#;

const R_DRIVER: &str = r#"suppressPackageStartupMessages(library(jsonlite))
args <- commandArgs(trailingOnly=TRUE); project <- normalizePath(args[[1]], mustWork=TRUE); scope <- new.env(parent=globalenv())
emit <- function(request_id, kind, values=list()) { event <- c(list(request_id=request_id,kind=kind),values); cat(toJSON(event,auto_unbox=TRUE,null="null"),"\n",sep=""); flush.console() }
input <- file("stdin","r")
repeat { raw <- readLines(input,n=1,warn=FALSE); if (!length(raw)) break; request <- fromJSON(raw,simplifyVector=FALSE); request_id <- request$request_id; emit(request_id,"started"); output <- character(); tryCatch({ output <- capture.output(eval(parse(text=request$code),envir=scope)); if(length(output)) emit(request_id,"stdout",list(payload=paste0(output,collapse="\n"))); if(length(request$capture_paths)) for(relative in request$capture_paths) { path <- normalizePath(file.path(project,relative),mustWork=TRUE); if(!startsWith(path,paste0(project,.Platform$file.sep)) || dir.exists(path)) stop("artifact escaped project root"); emit(request_id,"artifact",list(relative_path=relative)) }; emit(request_id,"completed") },error=function(e) emit(request_id,"failed",list(message=conditionMessage(e)))) }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn key(backend_id: &str, language: KernelLanguageV4) -> ExecutionContextKeyV4 {
        ExecutionContextKeyV4 {
            project_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            backend_id: backend_id.into(),
            language,
            environment: "system".into(),
        }
    }

    #[tokio::test]
    async fn local_python_kernel_preserves_namespace_and_process_identity() {
        if Command::new("python")
            .arg("--version")
            .output()
            .await
            .is_err()
        {
            return;
        }
        let project = tempfile::tempdir().unwrap();
        let backend = Arc::new(LocalKernelBackendV4::new(project.path()).unwrap());
        let manager = crate::RuntimeManagerV4::new(backend);
        let key = key("local", KernelLanguageV4::Python);
        let first = manager
            .execute(
                &key,
                "from pathlib import Path\nanswer = 42\nPath('result.txt').write_text('host-verified')"
                    .into(),
                vec!["result.txt".into()],
            )
            .await
            .unwrap();
        let second = manager
            .execute(&key, "print(answer)".into(), vec![])
            .await
            .unwrap();
        assert!(first.succeeded);
        assert_eq!(second.stdout.trim(), "42");
        assert_eq!(first.session_id, second.session_id);
        assert_eq!(first.process_identity, second.process_identity);
        assert_eq!(first.artifacts[0].size_bytes, 13);
        assert_eq!(first.artifacts[0].sha256.len(), 64);
        manager.interrupt(&key).await.unwrap();
    }

    #[test]
    fn container_command_defaults_to_hardened_offline_execution() {
        let project = tempfile::tempdir().unwrap();
        let backend = ContainerKernelBackendV4::docker(project.path(), "omicsops/test:1").unwrap();
        let arguments = backend.container_arguments(
            &key("docker", KernelLanguageV4::Python),
            ".omicsops/kernels/driver.py",
            Uuid::nil(),
            "omicsops-test",
        );
        for required in [
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--pids-limit=256",
            "none",
        ] {
            assert!(arguments.iter().any(|argument| argument == required));
        }
        assert_eq!(
            arguments
                .windows(2)
                .find(|pair| pair[0] == "--network")
                .map(|pair| pair[1].as_str()),
            Some("none")
        );
    }
}
