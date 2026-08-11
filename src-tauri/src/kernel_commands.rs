use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use omicsops_adapters::{
    kernel::{kernel_driver, validate_capture_paths, validate_kernel_code},
    ssh::{SshJsonlProcess, SshSession},
};
use omicsops_agent::{
    FormalStepProposal, KernelEvent, KernelEventDecoder, KernelEventKind, KernelLanguage,
    KernelRequest, KernelSession, KernelState,
};
use omicsops_core::{
    project::shell_quote,
    workspace::{Artifact, Project},
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Mutex, Semaphore};
use uuid::Uuid;

use crate::commands::{AppState, connect_profile, find_profile, require_trusted_host};
use omicsops_adapters::persistence::Repository;

pub type ActiveKernelMap = Arc<Mutex<HashMap<Uuid, Arc<ActiveKernel>>>>;

pub struct ActiveKernel {
    pub project_id: Uuid,
    inner: Mutex<RemoteKernel>,
}

struct RemoteKernel {
    metadata: KernelSession,
    remote_root: String,
    process: Option<SshJsonlProcess>,
    ssh: Option<SshSession>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartKernelRequest {
    pub project_id: Uuid,
    pub language: KernelLanguage,
    pub rebuild_session_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecuteKernelCellRequest {
    pub session_id: Uuid,
    pub code: String,
    #[serde(default)]
    pub save: bool,
    #[serde(default)]
    pub capture_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KernelCellResult {
    pub request_id: Uuid,
    pub saved_cell_index: Option<usize>,
    pub events: Vec<KernelEvent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromoteKernelCellRequest {
    pub session_id: Uuid,
    pub cell_index: usize,
    pub name: String,
    pub version: u32,
}

pub fn mark_orphaned_kernels_interrupted(repository: &Repository) -> Result<usize, String> {
    let mut changed = 0;
    for mut session in repository
        .list_json::<KernelSession>("kernel_session")
        .map_err(|error| error.to_string())?
    {
        if session.state == KernelState::Running {
            session.interrupt();
            repository
                .put_json("kernel_session", &session.id.to_string(), &session)
                .map_err(|error| error.to_string())?;
            changed += 1;
        }
    }
    Ok(changed)
}

#[tauri::command]
pub fn list_kernel_sessions(state: State<'_, AppState>) -> Result<Vec<KernelSession>, String> {
    state
        .repository
        .list_json("kernel_session")
        .map_err(|error| error.to_string())
}

pub fn promote_saved_kernel_cell(
    repository: &Repository,
    request: &PromoteKernelCellRequest,
) -> Result<FormalStepProposal, String> {
    if request.name.trim().is_empty() || request.version == 0 {
        return Err("formal step name must be non-empty and version must be positive".into());
    }
    let session = repository
        .get_json::<KernelSession>("kernel_session", &request.session_id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "kernel session was not found".to_string())?;
    session
        .promote_cell(request.cell_index, request.name.trim(), request.version)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn promote_kernel_cell(
    state: State<'_, AppState>,
    request: PromoteKernelCellRequest,
) -> Result<FormalStepProposal, String> {
    promote_saved_kernel_cell(&state.repository, &request)
}

#[tauri::command]
pub async fn start_kernel(
    app: AppHandle,
    state: State<'_, AppState>,
    request: StartKernelRequest,
) -> Result<KernelSession, String> {
    let project = project_for_kernel(&state, request.project_id)?;
    let active_count = state
        .active_kernels
        .lock()
        .await
        .values()
        .filter(|kernel| kernel.project_id == project.id)
        .count();
    if active_count >= 3 {
        return Err("a project may have at most three active kernel sessions".into());
    }
    let mut metadata = if let Some(session_id) = request.rebuild_session_id {
        let session = state
            .repository
            .get_json::<KernelSession>("kernel_session", &session_id.to_string())
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "interrupted kernel session was not found".to_string())?;
        if session.project_id != project.id || session.language != request.language {
            return Err("kernel rebuild identity does not match the project and language".into());
        }
        if session.state != KernelState::Interrupted {
            return Err("only interrupted kernel sessions can be rebuilt".into());
        }
        session
    } else {
        KernelSession::new(Uuid::new_v4(), project.id, request.language)
    };
    metadata.start().map_err(|error| error.to_string())?;
    let (ssh, mut process, remote_root) = launch_remote_kernel(&state, &project, &metadata).await?;
    for code in metadata.saved_cells_owned() {
        let request_id = Uuid::new_v4();
        let events = exchange_cell(
            &app,
            &mut process,
            project.id,
            metadata.id,
            request_id,
            code,
            vec![],
        )
        .await?;
        if !events
            .iter()
            .any(|event| matches!(event.event, KernelEventKind::Completed))
        {
            metadata.interrupt();
            return Err("saved kernel cell failed during rebuild".into());
        }
    }
    state
        .repository
        .put_json("kernel_session", &metadata.id.to_string(), &metadata)
        .map_err(|error| error.to_string())?;
    let active = Arc::new(ActiveKernel {
        project_id: project.id,
        inner: Mutex::new(RemoteKernel {
            metadata: metadata.clone(),
            remote_root,
            process: Some(process),
            ssh: Some(ssh),
        }),
    });
    state
        .active_kernels
        .lock()
        .await
        .insert(metadata.id, active);
    Ok(metadata)
}

#[tauri::command]
pub async fn execute_kernel_cell(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ExecuteKernelCellRequest,
) -> Result<KernelCellResult, String> {
    validate_kernel_code(&request.code).map_err(|error| error.to_string())?;
    validate_capture_paths(&request.capture_paths).map_err(|error| error.to_string())?;
    let active = state
        .active_kernels
        .lock()
        .await
        .get(&request.session_id)
        .cloned()
        .ok_or_else(|| "kernel session is not active".to_string())?;
    let queue = project_queue(&state, active.project_id).await;
    let _permit = queue
        .acquire()
        .await
        .map_err(|_| "project kernel queue closed".to_string())?;
    let mut kernel = active.inner.lock().await;
    if kernel.metadata.state != KernelState::Running {
        return Err("kernel session is not running".into());
    }
    let request_id = Uuid::new_v4();
    let project_id = kernel.metadata.project_id;
    let session_id = kernel.metadata.id;
    let process = kernel
        .process
        .as_mut()
        .ok_or_else(|| "kernel process is unavailable".to_string())?;
    let events = match exchange_cell(
        &app,
        process,
        project_id,
        session_id,
        request_id,
        request.code.clone(),
        request.capture_paths,
    )
    .await
    {
        Ok(events) => events,
        Err(error) => {
            kernel.metadata.interrupt();
            state
                .repository
                .put_json("kernel_session", &session_id.to_string(), &kernel.metadata)
                .map_err(|error| error.to_string())?;
            drop(kernel);
            state.active_kernels.lock().await.remove(&session_id);
            return Err(error);
        }
    };
    let succeeded = events
        .iter()
        .any(|event| matches!(event.event, KernelEventKind::Completed));
    let saved_cell_index = if succeeded {
        kernel
            .metadata
            .record_executed_cell(request.code, request.save)
            .map_err(|error| error.to_string())?
    } else {
        None
    };
    for event in &events {
        if let KernelEventKind::Artifact {
            relative_path,
            size_bytes,
            sha256,
        } = &event.event
        {
            let artifact = verify_remote_artifact(
                kernel
                    .ssh
                    .as_ref()
                    .ok_or_else(|| "kernel SSH session is unavailable".to_string())?,
                project_id,
                &kernel.remote_root,
                relative_path,
                *size_bytes,
                sha256,
            )
            .await?;
            state
                .repository
                .save_artifact_v3(&artifact)
                .map_err(|error| error.to_string())?;
            app.emit("artifact-event", &artifact)
                .map_err(|error| error.to_string())?;
        }
    }
    state
        .repository
        .put_json("kernel_session", &session_id.to_string(), &kernel.metadata)
        .map_err(|error| error.to_string())?;
    Ok(KernelCellResult {
        request_id,
        saved_cell_index,
        events,
    })
}

#[tauri::command]
pub async fn interrupt_kernel(
    state: State<'_, AppState>,
    session_id: Uuid,
) -> Result<KernelSession, String> {
    finish_kernel(&state, session_id, true).await
}

#[tauri::command]
pub async fn stop_kernel(
    state: State<'_, AppState>,
    session_id: Uuid,
) -> Result<KernelSession, String> {
    finish_kernel(&state, session_id, false).await
}

async fn finish_kernel(
    state: &State<'_, AppState>,
    session_id: Uuid,
    interrupted: bool,
) -> Result<KernelSession, String> {
    let active = state
        .active_kernels
        .lock()
        .await
        .remove(&session_id)
        .ok_or_else(|| "kernel session is not active".to_string())?;
    let mut kernel = active.inner.lock().await;
    if interrupted {
        kernel.metadata.interrupt();
    } else {
        if let Some(process) = kernel.process.as_mut() {
            let _ = process
                .send(&KernelRequest::Shutdown {
                    session_id,
                    request_id: Uuid::new_v4(),
                })
                .await;
        }
        kernel.metadata.stop();
    }
    if let Some(process) = kernel.process.take() {
        let _ = process.shutdown().await;
    }
    if let Some(ssh) = kernel.ssh.take() {
        let _ = ssh.disconnect().await;
    }
    state
        .repository
        .put_json("kernel_session", &session_id.to_string(), &kernel.metadata)
        .map_err(|error| error.to_string())?;
    Ok(kernel.metadata.clone())
}

async fn exchange_cell(
    app: &AppHandle,
    process: &mut SshJsonlProcess,
    project_id: Uuid,
    session_id: Uuid,
    request_id: Uuid,
    code: String,
    capture_paths: Vec<String>,
) -> Result<Vec<KernelEvent>, String> {
    process
        .send(&KernelRequest::Execute {
            session_id,
            request_id,
            code,
            capture_paths,
        })
        .await
        .map_err(|error| error.to_string())?;
    let mut decoder = KernelEventDecoder::new(project_id, session_id, request_id);
    let mut events = Vec::new();
    loop {
        let event: KernelEvent = process
            .receive()
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "kernel process disconnected".to_string())?;
        let event = decoder.accept(event).map_err(|error| error.to_string())?;
        let terminal = matches!(
            event.event,
            KernelEventKind::Completed | KernelEventKind::Failed { .. }
        );
        app.emit("kernel-event", &event)
            .map_err(|error| error.to_string())?;
        events.push(event);
        if terminal {
            return Ok(events);
        }
    }
}

async fn launch_remote_kernel(
    state: &State<'_, AppState>,
    project: &Project,
    metadata: &KernelSession,
) -> Result<(SshSession, SshJsonlProcess, String), String> {
    let profile_id = project
        .connection_id
        .ok_or_else(|| "project has no remote connection".to_string())?;
    let configured_root = project
        .remote_root
        .as_deref()
        .ok_or_else(|| "project has no remote root".to_string())?;
    let profile = find_profile(&state.repository, profile_id)?;
    require_trusted_host(&profile)?;
    let ssh = connect_profile(state, &profile).await?;
    let root = ssh
        .execute_checked(&format!(
            "root=$(realpath -- {}) && test -d \"$root\" && printf '%s' \"$root\"",
            shell_quote(configured_root)
        ))
        .await
        .map_err(|error| error.to_string())?
        .stdout
        .trim()
        .to_owned();
    if root.is_empty() {
        return Err("remote project root did not resolve".into());
    }
    let requested_kernel_dir = format!("{}/.omicsops/kernels", root.trim_end_matches('/'));
    let kernel_dir = ssh
        .execute_checked(&format!(
            "root={} && dir=$(realpath -m -- {}) && case \"$dir\" in \"$root\"/*) mkdir -p -- \"$dir\" && printf '%s' \"$dir\";; *) exit 73;; esac",
            shell_quote(&root),
            shell_quote(&requested_kernel_dir)
        ))
        .await
        .map_err(|error| error.to_string())?
        .stdout
        .trim()
        .to_owned();
    if kernel_dir.is_empty() {
        return Err("remote kernel directory did not resolve inside the project".into());
    }
    let extension = if metadata.language == KernelLanguage::Python {
        "py"
    } else {
        "R"
    };
    let driver_path = format!("{kernel_dir}/{}.driver.{extension}", metadata.id);
    ssh.upload_text(&driver_path, kernel_driver(metadata.language))
        .await
        .map_err(|error| error.to_string())?;
    let executable = if metadata.language == KernelLanguage::Python {
        "python3 -u"
    } else {
        "Rscript --vanilla"
    };
    let command = format!(
        "{executable} {} {} {} {}",
        shell_quote(&driver_path),
        shell_quote(&root),
        shell_quote(&project.id.to_string()),
        shell_quote(&metadata.id.to_string())
    );
    let process = ssh
        .open_jsonl_process(&command)
        .await
        .map_err(|error| error.to_string())?;
    Ok((ssh, process, root))
}

async fn verify_remote_artifact(
    ssh: &SshSession,
    project_id: Uuid,
    root: &str,
    relative_path: &str,
    claimed_size: u64,
    claimed_sha256: &str,
) -> Result<Artifact, String> {
    validate_capture_paths(&[relative_path.to_owned()]).map_err(|error| error.to_string())?;
    let candidate = format!("{}/{}", root.trim_end_matches('/'), relative_path);
    let output = ssh.execute_checked(&format!("root={} && file=$(realpath -- {}) && case \"$file\" in \"$root\"/*) test -f \"$file\" && printf '%s\\n' \"$file\" && stat -c '%s' -- \"$file\" && sha256sum -- \"$file\";; *) exit 73;; esac", shell_quote(root), shell_quote(&candidate))).await.map_err(|error| error.to_string())?;
    let mut lines = output.stdout.lines();
    let remote_path = lines
        .next()
        .ok_or_else(|| "kernel artifact path was empty".to_string())?;
    let size_bytes: u64 = lines
        .next()
        .ok_or_else(|| "kernel artifact size was empty".to_string())?
        .parse()
        .map_err(|_| "invalid kernel artifact size".to_string())?;
    let sha256 = lines
        .next()
        .and_then(|line| line.split_whitespace().next())
        .ok_or_else(|| "kernel artifact checksum was empty".to_string())?;
    if size_bytes != claimed_size || !sha256.eq_ignore_ascii_case(claimed_sha256) {
        return Err("kernel artifact metadata failed independent verification".into());
    }
    Ok(Artifact {
        id: Uuid::new_v4(),
        project_id,
        run_id: None,
        relative_path: relative_path.into(),
        remote_path: Some(remote_path.into()),
        media_type: media_type(relative_path).into(),
        size_bytes,
        sha256: sha256.into(),
        verified: true,
        created_at: Utc::now(),
    })
}

async fn project_queue(state: &State<'_, AppState>, project_id: Uuid) -> Arc<Semaphore> {
    state
        .project_kernel_queues
        .lock()
        .await
        .entry(project_id)
        .or_insert_with(|| Arc::new(Semaphore::new(1)))
        .clone()
}

fn project_for_kernel(state: &State<'_, AppState>, project_id: Uuid) -> Result<Project, String> {
    state
        .repository
        .get_project(project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project was not found".into())
}

fn media_type(path: &str) -> &'static str {
    match path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
    {
        Some(extension) if matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "svg") => {
            "image/*"
        }
        Some(extension) if matches!(extension.as_str(), "csv" | "tsv") => "text/tabular",
        Some(extension) if extension == "json" => "application/json",
        Some(extension) if extension == "html" => "text/html",
        _ => "application/octet-stream",
    }
}
