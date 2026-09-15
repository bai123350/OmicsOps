use std::{io, path::Path, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use omicsops_dto::{
    GeneralNativePreferences, GeneralSystemStatus, GeneralUpdateStatus,
    SystemInterpreterDiagnostic, SystemInterpreterDiagnostics, SystemInterpreterStatus,
};
use omicsops_process::background_command;
use omicsops_store::Store;
use tauri::{AppHandle, Manager, State};

use crate::commands::AppState;

const GENERAL_PREFERENCES_KIND: &str = "general_native_preferences_v1";
const GENERAL_PREFERENCES_ID: &str = "current";
const INTERPRETER_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeRunOutcome {
    Found,
    Missing,
    Failed,
    TimedOut,
}

#[async_trait]
pub(crate) trait InterpreterProbeRunner: Send + Sync {
    async fn run(&self, program: &str) -> ProbeRunOutcome;
}

struct SystemInterpreterProbeRunner;

#[async_trait]
impl InterpreterProbeRunner for SystemInterpreterProbeRunner {
    async fn run(&self, program: &str) -> ProbeRunOutcome {
        let output = tokio::time::timeout(
            INTERPRETER_PROBE_TIMEOUT,
            background_command(program)
                .arg("--version")
                .kill_on_drop(true)
                .output(),
        )
        .await;
        match output {
            Err(_) => ProbeRunOutcome::TimedOut,
            Ok(Err(error)) if error.kind() == io::ErrorKind::NotFound => ProbeRunOutcome::Missing,
            Ok(Err(_)) => ProbeRunOutcome::Failed,
            Ok(Ok(output)) if output.status.success() => ProbeRunOutcome::Found,
            Ok(Ok(_)) => ProbeRunOutcome::Failed,
        }
    }
}

fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn path_contains_link_or_reparse(
    path: &Path,
    mut inspect: impl FnMut(&Path) -> Result<bool, String>,
) -> Result<bool, String> {
    let mut ancestors = path.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    for ancestor in ancestors {
        if inspect(ancestor)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_project_directory_start(value: &str) -> Result<String, String> {
    let value = value.trim();
    let normalized = value.replace('/', "\\");
    let lower = normalized.to_ascii_lowercase();
    if value.is_empty()
        || lower.starts_with(r"\\?\")
        || lower.starts_with(r"\\.\")
        || lower.starts_with(r"\??\")
    {
        return Err("project directory start must be an absolute local directory".into());
    }
    let colon_offset = usize::from(normalized.as_bytes().get(1) == Some(&b':')) * 2;
    if normalized[colon_offset..].contains(':') {
        return Err("project directory start cannot contain an alternate data stream".into());
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err("project directory start must be an absolute local directory".into());
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "project directory start must be an existing directory".to_owned())?;
    if !metadata.is_dir() {
        return Err("project directory start must be an existing directory".into());
    }
    if path_contains_link_or_reparse(path, |ancestor| {
        std::fs::symlink_metadata(ancestor)
            .map(|metadata| is_link_or_reparse(&metadata))
            .map_err(|_| "project directory start must be an existing directory".to_owned())
    })? {
        return Err("project directory start cannot be a link or reparse point".into());
    }
    Ok(value.to_owned())
}

pub(crate) fn project_directory_start_available(value: &str) -> bool {
    validate_project_directory_start(value).is_ok()
}

pub(crate) async fn load_preferences(
    repository: &Store,
) -> Result<GeneralNativePreferences, String> {
    repository
        .get_json(GENERAL_PREFERENCES_KIND, GENERAL_PREFERENCES_ID)
        .await
        .map(|preferences| preferences.unwrap_or_default())
        .map_err(|error| error.to_string())
}

pub(crate) async fn save_preferences(
    repository: &Store,
    mut preferences: GeneralNativePreferences,
) -> Result<GeneralNativePreferences, String> {
    preferences.project_directory_start = preferences
        .project_directory_start
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(validate_project_directory_start)
        .transpose()?;
    repository
        .put_json(
            GENERAL_PREFERENCES_KIND,
            GENERAL_PREFERENCES_ID,
            &preferences,
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(preferences)
}

pub(crate) fn build_system_status(
    app_version: &str,
    app_data_directory: &str,
    update_source_configured: bool,
) -> GeneralSystemStatus {
    GeneralSystemStatus {
        app_version: app_version.to_owned(),
        app_data_directory: app_data_directory.to_owned(),
        update_status: if update_source_configured {
            GeneralUpdateStatus::Configured
        } else {
            GeneralUpdateStatus::Unconfigured
        },
        update_source_configured,
    }
}

fn diagnostic(program: &str, outcome: ProbeRunOutcome) -> SystemInterpreterDiagnostic {
    let (status, detail) = match outcome {
        ProbeRunOutcome::Found => (SystemInterpreterStatus::Found, None),
        ProbeRunOutcome::Missing => (
            SystemInterpreterStatus::Missing,
            Some("not found on system PATH".into()),
        ),
        ProbeRunOutcome::Failed => (
            SystemInterpreterStatus::Error,
            Some("probe process failed".into()),
        ),
        ProbeRunOutcome::TimedOut => (
            SystemInterpreterStatus::Error,
            Some("probe timed out".into()),
        ),
    };
    SystemInterpreterDiagnostic {
        program: program.into(),
        status,
        detail,
    }
}

pub(crate) async fn probe_system_interpreters_with(
    runner: &impl InterpreterProbeRunner,
    checked_at: DateTime<Utc>,
) -> SystemInterpreterDiagnostics {
    let python = diagnostic("python", runner.run("python").await);
    let r = diagnostic("Rscript", runner.run("Rscript").await);
    SystemInterpreterDiagnostics {
        python,
        r,
        checked_at: checked_at.to_rfc3339(),
    }
}

pub(crate) async fn program_available(program: &str) -> bool {
    matches!(
        SystemInterpreterProbeRunner.run(program).await,
        ProbeRunOutcome::Found
    )
}

#[tauri::command]
pub async fn settings_general_preferences(
    state: State<'_, AppState>,
) -> Result<GeneralNativePreferences, String> {
    load_preferences(&state.repository).await
}

#[tauri::command]
pub async fn settings_save_general_preferences(
    state: State<'_, AppState>,
    preferences: GeneralNativePreferences,
) -> Result<GeneralNativePreferences, String> {
    save_preferences(&state.repository, preferences).await
}

#[tauri::command]
pub fn settings_project_directory_start_available(path: String) -> bool {
    project_directory_start_available(&path)
}

#[tauri::command]
pub fn settings_general_system_status(app: AppHandle) -> Result<GeneralSystemStatus, String> {
    let app_data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let update_source_configured = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|value| value.get("endpoints"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|endpoints| !endpoints.is_empty());
    Ok(build_system_status(
        &app.package_info().version.to_string(),
        &app_data_directory.to_string_lossy(),
        update_source_configured,
    ))
}

#[tauri::command]
pub async fn settings_probe_system_interpreters() -> SystemInterpreterDiagnostics {
    probe_system_interpreters_with(&SystemInterpreterProbeRunner, Utc::now()).await
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::Utc;
    use omicsops_dto::{GeneralNativePreferences, GeneralUpdateStatus, SystemInterpreterStatus};
    use omicsops_store::Store;

    use super::*;

    #[tokio::test]
    async fn directory_preference_round_trips_and_clears_without_creating_paths() {
        let store = Store::open_in_memory().await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let saved = save_preferences(
            &store,
            GeneralNativePreferences {
                project_directory_start: Some(root.path().to_string_lossy().into_owned()),
            },
        )
        .await
        .unwrap();
        assert_eq!(load_preferences(&store).await.unwrap(), saved);

        let missing = root.path().join("must-not-be-created");
        let error = save_preferences(
            &store,
            GeneralNativePreferences {
                project_directory_start: Some(missing.to_string_lossy().into_owned()),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            "project directory start must be an existing directory"
        );
        assert!(!missing.exists());
        assert_eq!(load_preferences(&store).await.unwrap(), saved);

        let cleared = save_preferences(&store, GeneralNativePreferences::default())
            .await
            .unwrap();
        assert_eq!(cleared, GeneralNativePreferences::default());
        assert_eq!(load_preferences(&store).await.unwrap(), cleared);
    }

    #[test]
    fn system_status_reports_configuration_instead_of_claiming_an_update() {
        let status = build_system_status("1.2.3", r"C:\OmicsOps\Data", false);
        assert_eq!(status.app_version, "1.2.3");
        assert_eq!(status.app_data_directory, r"C:\OmicsOps\Data");
        assert_eq!(status.update_status, GeneralUpdateStatus::Unconfigured);
        assert!(!status.update_source_configured);
    }

    struct FakeRunner {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl InterpreterProbeRunner for FakeRunner {
        async fn run(&self, program: &str) -> ProbeRunOutcome {
            self.calls.lock().unwrap().push(program.into());
            match program {
                "python" => ProbeRunOutcome::Found,
                "Rscript" => ProbeRunOutcome::TimedOut,
                _ => panic!("unexpected probe {program}"),
            }
        }
    }

    #[tokio::test]
    async fn interpreter_probe_is_fixed_and_distinguishes_timeout_from_missing() {
        let runner = FakeRunner {
            calls: Mutex::new(vec![]),
        };
        let diagnostics = probe_system_interpreters_with(&runner, Utc::now()).await;
        assert_eq!(
            *runner.calls.lock().unwrap(),
            vec!["python".to_string(), "Rscript".to_string()]
        );
        assert_eq!(diagnostics.python.status, SystemInterpreterStatus::Found);
        assert_eq!(diagnostics.r.status, SystemInterpreterStatus::Error);
        assert_eq!(diagnostics.r.detail.as_deref(), Some("probe timed out"));
    }

    struct MissingAndFailureRunner;

    #[async_trait]
    impl InterpreterProbeRunner for MissingAndFailureRunner {
        async fn run(&self, program: &str) -> ProbeRunOutcome {
            match program {
                "python" => ProbeRunOutcome::Missing,
                "Rscript" => ProbeRunOutcome::Failed,
                _ => panic!("unexpected probe {program}"),
            }
        }
    }

    #[tokio::test]
    async fn interpreter_probe_keeps_missing_separate_from_execution_errors() {
        let diagnostics =
            probe_system_interpreters_with(&MissingAndFailureRunner, Utc::now()).await;
        assert_eq!(diagnostics.python.status, SystemInterpreterStatus::Missing);
        assert_eq!(
            diagnostics.python.detail.as_deref(),
            Some("not found on system PATH")
        );
        assert_eq!(diagnostics.r.status, SystemInterpreterStatus::Error);
        assert_eq!(
            diagnostics.r.detail.as_deref(),
            Some("probe process failed")
        );
    }

    #[tokio::test]
    async fn directory_preference_rejects_relative_device_and_ads_paths() {
        let store = Store::open_in_memory().await.unwrap();
        for (path, expected) in [
            (
                "relative/path",
                "project directory start must be an absolute local directory",
            ),
            (
                r"\\?\C:\Science",
                "project directory start must be an absolute local directory",
            ),
            (
                r"C:\Science:private",
                "project directory start cannot contain an alternate data stream",
            ),
        ] {
            let error = save_preferences(
                &store,
                GeneralNativePreferences {
                    project_directory_start: Some(path.into()),
                },
            )
            .await
            .unwrap_err();
            assert_eq!(error, expected);
        }
        assert_eq!(
            load_preferences(&store).await.unwrap(),
            GeneralNativePreferences::default()
        );
    }

    #[test]
    fn directory_validation_checks_intermediate_components_for_reparse_points() {
        let root = tempfile::tempdir().unwrap();
        let junction = root.path().join("junction");
        let child = junction.join("child");
        let mut inspected = vec![];
        let found = path_contains_link_or_reparse(&child, |candidate| {
            inspected.push(candidate.to_path_buf());
            Ok(candidate == junction)
        })
        .unwrap();
        assert!(found);
        assert!(inspected.contains(&root.path().to_path_buf()));
        assert!(inspected.contains(&junction));
        assert!(!inspected.contains(&child));
    }

    #[test]
    fn directory_availability_detects_a_saved_folder_removed_after_save() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().to_string_lossy().into_owned();
        assert!(project_directory_start_available(&path));
        drop(root);
        assert!(!project_directory_start_available(&path));
    }
}
