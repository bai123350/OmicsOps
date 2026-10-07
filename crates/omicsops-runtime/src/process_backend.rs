use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use omicsops_process::{background_command, configure_background_command};
use omicsops_protocol::{
    ComputeBackendDescriptorV4, ComputeBackendKindV4, ExecutionContextKeyV4, IsolationStrengthV4,
    KERNEL_INSPECTION_MAX_RESPONSE_BYTES, KERNEL_INSPECTION_TIMEOUT_SECONDS,
    KernelInspectionPayloadV4, KernelLanguageV4, OutputCaptureV4, RuntimeArtifactV4,
    RuntimeResultV4, validate_inspection_payload,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, Lines,
    },
    process::{Child, ChildStdout, Command},
    sync::Mutex,
};
use uuid::Uuid;

use crate::{KernelBackendV4, KernelInspectionErrorV4, KernelProcessV4};

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
                let mut command = background_command(&self.python_program);
                command.arg("-u");
                command
            }
            KernelLanguageV4::R => {
                let mut command = background_command(&self.r_program);
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
        let mut command = background_command(&self.engine_program);
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
    healthy: AtomicBool,
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

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }

    async fn inspect(&self) -> Result<KernelInspectionPayloadV4, KernelInspectionErrorV4> {
        let mut guard = self
            .handle
            .try_lock()
            .map_err(|_| KernelInspectionErrorV4::Busy)?;
        if !self.is_healthy() {
            return Err(KernelInspectionErrorV4::Unavailable);
        }
        let handle = guard.as_mut().ok_or(KernelInspectionErrorV4::Unavailable)?;
        inspect_jsonl(&mut handle.stdin, handle.lines.get_mut(), &self.healthy).await
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
        let result = async {
            let request_id = Uuid::new_v4();
            let request_id_text = request_id.to_string();
            let mut guard = self.handle.lock().await;
            // acquire may have returned this healthy Arc while an inspection
            // still held the handle. Recheck before sending after that wait.
            if !self.is_healthy() {
                return Err("kernel process is unavailable".into());
            }
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
                if event.get("request_id").and_then(Value::as_str) != Some(request_id_text.as_str())
                {
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
        .await;
        if result.is_err() {
            // An Err means the host did not consume a confirmed terminal
            // response. Unread or malformed JSONL may remain on the wire, so a
            // later cell must replace this process rather than reuse it.
            self.healthy.store(false, Ordering::SeqCst);
        }
        result
    }

    async fn interrupt(&self) -> Result<(), String> {
        if let Some(mut handle) = self.handle.lock().await.take() {
            let _ = handle.child.kill().await;
            let _ = handle.child.wait().await;
        }
        if let Some((engine, name)) = &self.container_cleanup {
            let _ = background_command(engine)
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

/// Once a write may have begun, cancellation is as uncertain as an I/O error.
/// Keep this guard outside the timed future so either deadline drops it safely.
struct InspectionHealthGuard<'a> {
    healthy: &'a AtomicBool,
    verified: bool,
}

impl Drop for InspectionHealthGuard<'_> {
    fn drop(&mut self) {
        if !self.verified {
            self.healthy.store(false, Ordering::SeqCst);
        }
    }
}

async fn inspect_jsonl(
    writer: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncBufRead + Unpin),
    healthy: &AtomicBool,
) -> Result<KernelInspectionPayloadV4, KernelInspectionErrorV4> {
    let request_id = Uuid::new_v4().to_string();
    let request = format!("{}\n", json!({"action":"inspect","request_id":request_id}));
    let mut health = InspectionHealthGuard {
        healthy,
        verified: false,
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(KERNEL_INSPECTION_TIMEOUT_SECONDS),
        async {
            writer
                .write_all(request.as_bytes())
                .await
                .map_err(|_| KernelInspectionErrorV4::Unavailable)?;
            writer
                .flush()
                .await
                .map_err(|_| KernelInspectionErrorV4::Unavailable)?;
            let mut frame = Vec::new();
            loop {
                let available = reader
                    .fill_buf()
                    .await
                    .map_err(|_| KernelInspectionErrorV4::Unavailable)?;
                if available.is_empty() {
                    return Err(KernelInspectionErrorV4::Protocol);
                }
                let newline = available.iter().position(|byte| *byte == b'\n');
                let count = newline.map_or(available.len(), |index| index + 1);
                if frame.len() + count > KERNEL_INSPECTION_MAX_RESPONSE_BYTES
                    || newline.is_some_and(|_| count != available.len())
                {
                    return Err(KernelInspectionErrorV4::Protocol);
                }
                frame.extend_from_slice(&available[..count]);
                reader.consume(count);
                if newline.is_some() {
                    break;
                }
            }
            let mut event: Value =
                serde_json::from_slice(&frame).map_err(|_| KernelInspectionErrorV4::Protocol)?;
            let fields = event
                .as_object_mut()
                .ok_or(KernelInspectionErrorV4::Protocol)?;
            if fields.len() != 4
                || fields.get("kind").and_then(Value::as_str) != Some("inspection")
                || fields.get("request_id").and_then(Value::as_str) != Some(request_id.as_str())
            {
                return Err(KernelInspectionErrorV4::Protocol);
            }
            fields.remove("kind");
            fields.remove("request_id");
            let payload: KernelInspectionPayloadV4 =
                serde_json::from_value(event).map_err(|_| KernelInspectionErrorV4::Protocol)?;
            validate_inspection_payload(&payload).map_err(|_| KernelInspectionErrorV4::Protocol)?;
            if !healthy.load(Ordering::SeqCst) {
                return Err(KernelInspectionErrorV4::Unavailable);
            }
            Ok(payload)
        },
    )
    .await
    .map_err(|_| KernelInspectionErrorV4::Unavailable)?;
    if result.is_ok() {
        health.verified = true;
    }
    result
}

async fn launch_process(
    mut command: Command,
    session_id: Uuid,
    key: &ExecutionContextKeyV4,
    project_root: PathBuf,
    backend: &str,
    container_cleanup: Option<(String, String)>,
) -> Result<Arc<dyn KernelProcessV4>, String> {
    // Keep the spawn boundary defensive even if a future backend constructs
    // its command without using `background_command`.
    configure_background_command(&mut command);
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
        healthy: AtomicBool::new(true),
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
sys.stdin.reconfigure(encoding="utf-8")
sys.stdout.reconfigure(encoding="utf-8")
project = pathlib.Path(sys.argv[1]).resolve()
scope = {"__name__": "__omicsops_kernel__"}
_INSPECT_TYPE = type
_INSPECT_DICT = dict
_INSPECT_STR = str
_INSPECT_LIST = list
_INSPECT_TUPLE = tuple
_INSPECT_SET = set
_INSPECT_FROZENSET = frozenset
_INSPECT_BYTES = bytes
_INSPECT_BYTEARRAY = bytearray
_INSPECT_INT = int
_INSPECT_FLOAT = float
_INSPECT_BOOL = bool
_INSPECT_COMPLEX = complex
_INSPECT_NONE = type(None)
_INSPECT_ORD = ord
_INSPECT_DUMPS = json.dumps
_INSPECT_MAX_BINDINGS = 4096
_INSPECT_MAX_OBJECTS = 64
_INSPECT_MAX_NAME_BYTES = 256
_INSPECT_PAYLOAD_BYTES = 12288
def _inspect_name(name):
    if _INSPECT_TYPE(name) is not _INSPECT_STR:
        return None
    if not name or _INSPECT_STR.__len__(name) > _INSPECT_MAX_NAME_BYTES:
        return None
    for char in name:
        number = _INSPECT_ORD(char)
        if number < 32 or 127 <= number <= 159 or 0xD800 <= number <= 0xDFFF:
            return None
    if _INSPECT_BYTES.__len__(_INSPECT_STR.encode(name, "utf-8")) > _INSPECT_MAX_NAME_BYTES:
        return None
    lower = _INSPECT_STR.lower(name)
    if _INSPECT_STR.startswith(name, "_") or any(part in lower for part in (
            "password", "passwd", "secret", "token", "credential", "private_key",
            "api_key", "apikey", "access_key")):
        return None
    return name
def _inspect_metadata(name, value):
    kind = _INSPECT_TYPE(value)
    label, count = "opaque", None
    if kind is _INSPECT_LIST:
        label, count = "list", _INSPECT_LIST.__len__(value)
    elif kind is _INSPECT_TUPLE:
        label, count = "tuple", _INSPECT_TUPLE.__len__(value)
    elif kind is _INSPECT_DICT:
        label, count = "dict", _INSPECT_DICT.__len__(value)
    elif kind is _INSPECT_SET:
        label, count = "set", _INSPECT_SET.__len__(value)
    elif kind is _INSPECT_FROZENSET:
        label, count = "frozenset", _INSPECT_FROZENSET.__len__(value)
    elif kind is _INSPECT_STR:
        label, count = "str", _INSPECT_STR.__len__(value)
    elif kind is _INSPECT_BYTES:
        label, count = "bytes", _INSPECT_BYTES.__len__(value)
    elif kind is _INSPECT_BYTEARRAY:
        label, count = "bytearray", _INSPECT_BYTEARRAY.__len__(value)
    elif kind is _INSPECT_INT:
        label = "int"
    elif kind is _INSPECT_FLOAT:
        label = "float"
    elif kind is _INSPECT_BOOL:
        label = "bool"
    elif kind is _INSPECT_COMPLEX:
        label = "complex"
    elif kind is _INSPECT_NONE:
        label = "NoneType"
    return {"name": name, "type_label": label, "dimensions": None,
            "element_count": count, "summary": label + ", metadata only"}
def inspect_objects(scope: dict) -> dict:
    objects, truncated, checked, budget = [], False, 0, 64
    if _INSPECT_TYPE(scope) is not _INSPECT_DICT:
        return {"objects": objects, "truncated": True}
    for name, value in _INSPECT_DICT.items(scope):
        if checked >= _INSPECT_MAX_BINDINGS or _INSPECT_LIST.__len__(objects) >= _INSPECT_MAX_OBJECTS:
            truncated = True
            break
        checked += 1
        safe_name = _inspect_name(name)
        if safe_name is None:
            truncated = True
            continue
        item = _inspect_metadata(safe_name, value)
        size = _INSPECT_BYTES.__len__(_INSPECT_STR.encode(_INSPECT_DUMPS(item, ensure_ascii=False), "utf-8")) + 2
        if budget + size > _INSPECT_PAYLOAD_BYTES:
            truncated = True
            break
        objects.append(item)
        budget += size
    return {"objects": objects, "truncated": truncated}
def emit(request_id, kind, **values):
    event = {"request_id": request_id, "kind": kind}
    event.update(values)
    print(json.dumps(event, ensure_ascii=False), flush=True)
for raw in sys.stdin:
    request_id = "protocol"
    try:
        request = json.loads(raw); request_id = request.get("request_id", "protocol")
        if request.get("action") == "inspect":
            try:
                emit(request_id, "inspection", **inspect_objects(scope))
            except Exception:
                emit(request_id, "failed", message="inspection unavailable")
            continue
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
        print(json.dumps({"request_id":request_id,"kind":"failed","message":str(error)}), flush=True)
"#;

const R_DRIVER: &str = r#"suppressPackageStartupMessages(library(jsonlite))
args <- commandArgs(trailingOnly=TRUE); project <- normalizePath(args[[1]], mustWork=TRUE); scope <- new.env(parent=globalenv())
inspect_bindings <- function(scope) {
    objects <- list(); truncated <- FALSE; budget <- 64L
    if (!is.environment(scope) || is.object(scope) || isS4(scope) || length(scope) > 4096L) return(list(objects=objects,truncated=TRUE))
    for (name in ls(envir=scope,all.names=TRUE,sorted=FALSE)) {
        if (length(objects) >= 64L) { truncated <- TRUE; break }
        if (nchar(name,type="chars") > 256L) { truncated <- TRUE; next }
        name <- enc2utf8(name)
        if (nchar(name,type="bytes") > 256L || grepl("[[:cntrl:]]",name) || startsWith(name,"_") ||
            grepl("password|passwd|secret|token|credential|private_key|api_key|apikey|access_key",tolower(name))) {
            truncated <- TRUE; next
        }
        if (bindingIsActive(name,scope)) { truncated <- TRUE; next }
        # substitute returns the promise expression or already bound object;
        # it never evaluates the expression or calls an active binding.
        value <- do.call(substitute,list(as.name(name),scope))
        label <- "opaque"; count <- NULL; dimensions <- NULL
        kind <- typeof(value)
        if (!is.object(value) && !isS4(value) &&
            kind %in% c("NULL","logical","integer","double","complex","character","raw","list")) {
            label <- kind; count <- length(value)
            raw_dims <- attr(value,"dim",exact=TRUE)
            if (!is.null(raw_dims) && !is.object(raw_dims) && !isS4(raw_dims) &&
                typeof(raw_dims) %in% c("integer","double") && length(raw_dims) <= 8L &&
                all(is.finite(raw_dims)) && all(raw_dims >= 0) && all(raw_dims == floor(raw_dims))) {
                dimensions <- as.list(raw_dims)
            }
        }
        item <- list(name=name,type_label=label,dimensions=dimensions,element_count=count,summary=paste0(label,", metadata only"))
        size <- nchar(enc2utf8(toJSON(item,auto_unbox=TRUE,null="null")),type="bytes") + 2L
        if (budget + size > 12288L) { truncated <- TRUE; break }
        objects[[length(objects)+1L]] <- item; budget <- budget + size
    }
    list(objects=objects,truncated=truncated)
}
emit <- function(request_id, kind, values=list()) { event <- c(list(request_id=request_id,kind=kind),values); cat(toJSON(event,auto_unbox=TRUE,null="null"),"\n",sep=""); flush.console() }
input <- file("stdin","r")
repeat { raw <- readLines(input,n=1,warn=FALSE); if (!length(raw)) break; request <- fromJSON(raw,simplifyVector=FALSE); request_id <- request$request_id
    if (identical(request$action,"inspect")) {
        tryCatch(emit(request_id,"inspection",inspect_bindings(scope)),error=function(e) emit(request_id,"failed",list(message="inspection unavailable")))
        next
    }
    emit(request_id,"started"); output <- character(); tryCatch({ output <- capture.output(eval(parse(text=request$code),envir=scope)); if(length(output)) emit(request_id,"stdout",list(payload=paste0(output,collapse="\n"))); if(length(request$capture_paths)) for(relative in request$capture_paths) { path <- normalizePath(file.path(project,relative),mustWork=TRUE); if(!startsWith(path,paste0(project,.Platform$file.sep)) || dir.exists(path)) stop("artifact escaped project root"); emit(request_id,"artifact",list(relative_path=relative)) }; emit(request_id,"completed") },error=function(e) emit(request_id,"failed",list(message=conditionMessage(e)))) }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn inspection_kernel() -> ProcessKernelV4 {
        ProcessKernelV4 {
            id: Uuid::new_v4(),
            identity: "fixture".into(),
            project_root: PathBuf::new(),
            output_dir: PathBuf::new(),
            handle: Mutex::new(None),
            healthy: AtomicBool::new(true),
            container_cleanup: None,
        }
    }

    #[tokio::test]
    async fn inspect_busy_preserves_health() {
        let kernel = inspection_kernel();
        let _held = kernel.handle.lock().await;
        assert_eq!(
            kernel.inspect().await,
            Err(crate::KernelInspectionErrorV4::Busy)
        );
        assert!(kernel.is_healthy());
    }

    #[tokio::test]
    async fn inspect_stopped_before_send_preserves_health() {
        let kernel = inspection_kernel();
        assert_eq!(
            kernel.inspect().await,
            Err(crate::KernelInspectionErrorV4::Unavailable)
        );
        assert!(kernel.is_healthy());
    }

    async fn inspect_fixture_response(
        frame: Vec<u8>,
    ) -> (
        Result<omicsops_protocol::KernelInspectionPayloadV4, crate::KernelInspectionErrorV4>,
        bool,
    ) {
        let (host, worker) = tokio::io::duplex(32768);
        let (output, mut input) = tokio::io::split(host);
        let mut reader = BufReader::new(output);
        let healthy = AtomicBool::new(true);
        let worker = tokio::spawn(async move {
            let (input, mut output) = tokio::io::split(worker);
            let mut lines = BufReader::new(input).lines();
            let request: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request["action"], "inspect");
            assert_eq!(request.as_object().unwrap().len(), 2);
            let frame = String::from_utf8(frame)
                .unwrap()
                .replace("REQUEST_ID", request["request_id"].as_str().unwrap());
            output.write_all(frame.as_bytes()).await.unwrap();
        });
        let result = inspect_jsonl(&mut input, &mut reader, &healthy).await;
        worker.await.unwrap();
        (result, healthy.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn inspect_frame_identity_and_budget() {
        let (result, healthy) = inspect_fixture_response(
            br#"{"kind":"inspection","request_id":"REQUEST_ID","objects":[],"truncated":false}
"#
            .to_vec(),
        )
        .await;
        assert_eq!(result.unwrap().objects.len(), 0);
        assert!(healthy);
        for frame in [
            br#"{"kind":"inspection","request_id":"wrong","objects":[],"truncated":false}
"#.to_vec(),
            br#"{"kind":"started","request_id":"REQUEST_ID","objects":[],"truncated":false}
"#.to_vec(),
            br#"{"kind":"inspection","request_id":"REQUEST_ID","objects":[],"truncated":false,"value":"forbidden"}
"#.to_vec(),
            br#"{"kind":"inspection","request_id":"REQUEST_ID","objects":[],"truncated":false}"#.to_vec(),
            vec![b'x'; 16385],
            br#"{"kind":"inspection","request_id":"REQUEST_ID","objects":[],"truncated":false}
{"kind":"inspection"}
"#.to_vec(),
            format!("{{\"kind\":\"inspection\",\"request_id\":\"REQUEST_ID\",\"objects\":[{{\"name\":\"{}\",\"type_label\":\"list\",\"dimensions\":null,\"element_count\":0,\"summary\":\"\"}}],\"truncated\":false}}\n", "a".repeat(257)).into_bytes(),
        ] {
            let (result, healthy) = inspect_fixture_response(frame).await;
            assert!(result.is_err());
            assert!(!healthy);
        }
    }

    #[tokio::test]
    async fn inspect_frame_budget_counts_newline() {
        let base =
            br#"{"kind":"inspection","request_id":"REQUEST_ID","objects":[],"truncated":false}"#;
        for (bytes, accepted) in [(16384, true), (16385, false)] {
            let mut frame = base.to_vec();
            // A UUID is 36 bytes; the placeholder replaced by the fixture is 10.
            frame.resize(bytes - 26 - 1, b' ');
            frame.push(b'\n');
            let (result, healthy) = inspect_fixture_response(frame).await;
            assert_eq!(result.is_ok(), accepted);
            assert_eq!(healthy, accepted);
        }
    }

    #[tokio::test]
    async fn inspect_protocol_timeout_marks_unhealthy_without_interrupt() {
        let (host, worker) = tokio::io::duplex(1024);
        let (output, mut input) = tokio::io::split(host);
        let mut reader = BufReader::new(output);
        let healthy = AtomicBool::new(true);
        let (sent, observed) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(async move {
            let mut lines = BufReader::new(worker).lines();
            let request: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request["action"], "inspect");
            sent.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let result = inspect_jsonl(&mut input, &mut reader, &healthy).await;
        observed.await.unwrap();
        assert_eq!(result, Err(crate::KernelInspectionErrorV4::Unavailable));
        assert!(!healthy.load(Ordering::SeqCst));
        assert!(!worker.is_finished()); // observing did not close/interrupt the transport
        worker.abort();
    }

    #[tokio::test]
    async fn inspect_cancelled_sent_future_marks_unhealthy() {
        let (host, worker) = tokio::io::duplex(1024);
        let (output, mut input) = tokio::io::split(host);
        let mut reader = BufReader::new(output);
        let healthy = AtomicBool::new(true);
        let mut worker = BufReader::new(worker).lines();
        {
            let future = inspect_jsonl(&mut input, &mut reader, &healthy);
            tokio::pin!(future);
            tokio::select! {
                result = &mut future => panic!("inspection unexpectedly completed: {result:?}"),
                request = worker.next_line() => {
                    assert_eq!(serde_json::from_str::<Value>(&request.unwrap().unwrap()).unwrap()["action"], "inspect");
                    assert!(healthy.load(Ordering::SeqCst));
                }
            }
        }
        assert!(!healthy.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn inspect_cancelled_partial_write_marks_unhealthy() {
        let (host, mut worker) = tokio::io::duplex(1);
        let (output, mut input) = tokio::io::split(host);
        let mut reader = BufReader::new(output);
        let healthy = AtomicBool::new(true);
        {
            let future = inspect_jsonl(&mut input, &mut reader, &healthy);
            tokio::pin!(future);
            let mut byte = [0];
            tokio::select! {
                result = &mut future => panic!("inspection unexpectedly completed: {result:?}"),
                read = worker.read_exact(&mut byte) => { read.unwrap(); assert_eq!(byte[0], b'{'); }
            }
        }
        assert!(!healthy.load(Ordering::SeqCst));
    }

    struct InspectionTransportFixture {
        id: Uuid,
        transport: Mutex<tokio::io::DuplexStream>,
        healthy: AtomicBool,
        executes: std::sync::atomic::AtomicUsize,
        interrupts: std::sync::atomic::AtomicUsize,
    }

    struct InspectionBackendFixture {
        local: LocalKernelBackendV4,
        launches: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl KernelBackendV4 for InspectionBackendFixture {
        fn descriptor(&self) -> ComputeBackendDescriptorV4 {
            self.local.descriptor()
        }
        async fn launch(
            &self,
            _: &ExecutionContextKeyV4,
        ) -> Result<Arc<dyn KernelProcessV4>, String> {
            self.launches.fetch_add(1, Ordering::SeqCst);
            Err("unexpected inspection launch".into())
        }
    }

    #[async_trait]
    impl KernelProcessV4 for InspectionTransportFixture {
        fn session_id(&self) -> Uuid {
            self.id
        }
        fn process_identity(&self) -> &str {
            "inspection-duplex"
        }
        fn is_healthy(&self) -> bool {
            self.healthy.load(Ordering::SeqCst)
        }
        async fn inspect(
            &self,
        ) -> Result<omicsops_protocol::KernelInspectionPayloadV4, crate::KernelInspectionErrorV4>
        {
            let mut transport = self.transport.lock().await;
            let (reader, mut writer) = tokio::io::split(&mut *transport);
            inspect_jsonl(&mut writer, &mut BufReader::new(reader), &self.healthy).await
        }
        async fn execute(&self, _: String, _: Vec<String>) -> Result<RuntimeResultV4, String> {
            self.executes.fetch_add(1, Ordering::SeqCst);
            Err("unexpected fixture execute".into())
        }
        async fn interrupt(&self) -> Result<(), String> {
            self.interrupts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn inspect_manager_outer_deadline_cancels_sent_transport_without_interrupt() {
        let project = tempfile::tempdir().unwrap();
        let backend = Arc::new(InspectionBackendFixture {
            local: LocalKernelBackendV4::new(project.path()).unwrap(),
            launches: 0.into(),
        });
        let manager = Arc::new(crate::RuntimeManagerV4::new(backend.clone()));
        let key = key("local", KernelLanguageV4::Python);
        let (host, worker) = tokio::io::duplex(1024);
        let process = Arc::new(InspectionTransportFixture {
            id: Uuid::new_v4(),
            transport: Mutex::new(host),
            healthy: AtomicBool::new(true),
            executes: 0.into(),
            interrupts: 0.into(),
        });
        manager
            .sessions
            .lock()
            .await
            .insert(key.clone(), process.clone());
        let lookup = manager.sessions.lock().await;
        let inspection = tokio::spawn({
            let manager = manager.clone();
            let key = key.clone();
            async move { manager.inspect_existing(&key, None).await }
        });
        // Consume part of the outer budget before the worker starts. Its inner
        // timer is therefore still pending when the manager drops the future.
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        drop(lookup);
        let mut worker = BufReader::new(worker).lines();
        let request: Value =
            serde_json::from_str(&worker.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(request["action"], "inspect");
        assert!(process.is_healthy());
        let result = inspection.await.unwrap();
        assert_eq!(
            result.status,
            omicsops_protocol::KernelInspectionStatusV4::Unavailable
        );
        assert!(!process.is_healthy());
        assert_eq!(process.executes.load(Ordering::SeqCst), 0);
        assert_eq!(process.interrupts.load(Ordering::SeqCst), 0);
        assert_eq!(backend.launches.load(Ordering::SeqCst), 0);
        assert!(!project.path().join(".omicsops").exists()); // no launch/archive
        assert!(manager.sessions.lock().await.contains_key(&key));
    }

    #[tokio::test]
    async fn inspect_queued_execute_rejects_cancelled_inspection_transport() {
        // Temporary protocol fixture only: no real scientific worker or user code.
        let project = tempfile::tempdir().unwrap();
        let mut command = background_command("python");
        command.arg("-u").arg("-c").arg(r#"import sys, json
for raw in sys.stdin:
    request = json.loads(raw)
    if request['action'] == 'inspect':
        print('inspect sent', file=sys.stderr, flush=True)
        continue
    print('execute sent', file=sys.stderr, flush=True)
    for kind, values in [('started', {}), ('stdout', {'payload': 'fixture executed'}), ('completed', {})]:
        print(json.dumps(dict(request_id=request['request_id'], kind=kind, **values)), flush=True)
"#).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut observed = BufReader::new(child.stderr.take().unwrap()).lines();
        let kernel = Arc::new(ProcessKernelV4 {
            id: Uuid::new_v4(),
            identity: "temporary-protocol-fixture".into(),
            project_root: project.path().to_path_buf(),
            output_dir: project.path().join("outputs"),
            handle: Mutex::new(Some(ProcessHandle {
                child,
                stdin,
                lines: BufReader::new(stdout).lines(),
            })),
            healthy: AtomicBool::new(true),
            container_cleanup: None,
        });
        let backend = Arc::new(InspectionBackendFixture {
            local: LocalKernelBackendV4::new(project.path()).unwrap(),
            launches: 0.into(),
        });
        let manager = crate::RuntimeManagerV4::new(backend.clone());
        let key = key("local", KernelLanguageV4::Python);
        manager
            .sessions
            .lock()
            .await
            .insert(key.clone(), kernel.clone());
        let inspection = tokio::spawn({
            let kernel = kernel.clone();
            async move { kernel.inspect().await }
        });
        assert_eq!(
            observed.next_line().await.unwrap().as_deref(),
            Some("inspect sent")
        );
        assert!(kernel.is_healthy());
        assert_eq!(kernel.inspect().await, Err(KernelInspectionErrorV4::Busy));
        assert!(kernel.is_healthy());
        let acquired = manager.acquire(&key).await.unwrap();
        let expected: Arc<dyn KernelProcessV4> = kernel.clone();
        assert!(Arc::ptr_eq(&acquired, &expected));
        let execution = acquired.execute("fixed fixture request".into(), vec![]);
        tokio::pin!(execution);
        // Poll execute once while inspection owns the real process handle.
        tokio::select! {
            biased;
            result = &mut execution => panic!("queued execute completed early: {result:?}"),
            _ = std::future::ready(()) => {}
        }
        inspection.abort();
        assert!(inspection.await.unwrap_err().is_cancelled());
        assert!(!kernel.is_healthy());
        let result = execution.await;
        assert_eq!(result.unwrap_err(), "kernel process is unavailable");
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), observed.next_line())
                .await
                .is_err()
        );
        assert!(!project.path().join("outputs").exists());
        assert_eq!(backend.launches.load(Ordering::SeqCst), 0);
        // Fixture cleanup occurs only after the no-send/no-launch assertions.
        let mut handle = kernel.handle.lock().await.take().unwrap();
        handle.child.kill().await.unwrap();
        handle.child.wait().await.unwrap();
    }

    async fn run_python_driver(project: &Path, code: &str) -> Option<Vec<Value>> {
        if background_command("python")
            .arg("--version")
            .output()
            .await
            .is_err()
        {
            return None;
        }
        let session_id = Uuid::new_v4();
        let request_id = Uuid::new_v4();
        let driver = write_driver(project, KernelLanguageV4::Python, session_id)
            .await
            .unwrap();
        let mut command = background_command("python");
        command
            .arg("-u")
            .arg(driver)
            .arg(project)
            .arg(Uuid::new_v4().to_string())
            .arg(session_id.to_string())
            .env("PYTHONIOENCODING", "ascii")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        stdin
            .write_all(
                format!(
                    "{}\n",
                    json!({"action":"execute","request_id":request_id,"code":code,"capture_paths":[]})
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stdin.flush().await.unwrap();
        let mut lines = BufReader::new(stdout).lines();
        let mut events = Vec::new();
        loop {
            let line = tokio::time::timeout(std::time::Duration::from_secs(3), lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let event: Value = serde_json::from_str(&line).unwrap();
            let terminal = matches!(
                event.get("kind").and_then(Value::as_str),
                Some("completed" | "failed")
            );
            events.push(event);
            if terminal {
                break;
            }
        }
        child.kill().await.unwrap();
        Some(events)
    }

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
        if background_command("python")
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

    #[tokio::test]
    async fn python_driver_uses_utf8_for_unicode_jsonl_on_ascii_hosts() {
        let project = tempfile::tempdir().unwrap();
        let Some(events) = run_python_driver(project.path(), "print('中文 🧬')").await else {
            return;
        };
        let request_id = events[0]["request_id"].clone();

        assert!(events.iter().all(|event| event["request_id"] == request_id));
        assert!(events.iter().any(|event| {
            event["kind"] == "stdout"
                && event["payload"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("中文 🧬")
        }));
        assert_eq!(events.last().unwrap()["kind"], "completed");
    }

    #[tokio::test]
    async fn python_driver_fallback_keeps_the_active_request_identity() {
        let project = tempfile::tempdir().unwrap();
        let Some(events) = run_python_driver(
            project.path(),
            "import sys\nsys.__stdout__.reconfigure(encoding='ascii')\nprint('🧬')",
        )
        .await
        else {
            return;
        };

        assert_eq!(
            events
                .iter()
                .map(|event| event["kind"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["started", "failed"]
        );
        assert_eq!(events.last().unwrap()["kind"], "failed");
        assert_eq!(
            events.last().unwrap()["request_id"],
            events[0]["request_id"]
        );
        assert_ne!(events.last().unwrap()["request_id"], "protocol");
        assert!(
            events.last().unwrap()["message"]
                .as_str()
                .unwrap_or_default()
                .contains("ascii")
        );
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
