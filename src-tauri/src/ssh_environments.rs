use std::{collections::BTreeMap, time::Duration};

use chrono::Utc;
use omicsops_core::ssh_environment::{SshEnvironment, safe_environment_basename};
use omicsops_dto::{
    ProbeSshProjectEnvironmentsRequestV4, SshEnvironmentOptionV4, SshEnvironmentReportV4,
    SshInterpreterObservationV4, SshInterpreterStatusV4,
};
use omicsops_protocol::KernelLanguageV4;
use tauri::State;
use tokio::time::Instant;

use crate::{
    commands::{AppState, connect_profile, find_profile, require_trusted_host},
    environment_probe_v4::{
        InspectionErrorV4, RemoteProbeRunner, SshProbeRunner, inspect_remote_with,
    },
};

const MAX_CANDIDATES: usize = 8;
const MAX_DISCOVERY_BYTES: usize = 4096;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const REPORT_TIMEOUT: Duration = Duration::from_secs(20);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(25);
const PROBE_ERROR: &str =
    "Remote environment inspection could not be verified. Refresh to try again.";

#[tauri::command]
pub async fn probe_ssh_project_environments(
    state: State<'_, AppState>,
    request: ProbeSshProjectEnvironmentsRequestV4,
) -> Result<SshEnvironmentReportV4, String> {
    with_command_deadline(probe_for_state(&state, request)).await
}

async fn with_command_deadline(
    operation: impl std::future::Future<Output = Result<SshEnvironmentReportV4, String>>,
) -> Result<SshEnvironmentReportV4, String> {
    tokio::time::timeout(COMMAND_TIMEOUT, operation)
        .await
        .map_err(|_| PROBE_ERROR.to_owned())?
}

async fn probe_for_state(
    state: &AppState,
    request: ProbeSshProjectEnvironmentsRequestV4,
) -> Result<SshEnvironmentReportV4, String> {
    SshEnvironment::parse(&request.environment)
        .map_err(|_| "Select a valid project environment before inspecting it.".to_owned())?;
    let project = state
        .repository
        .get_project(request.project_id)
        .await
        .map_err(|_| PROBE_ERROR.to_owned())?
        .ok_or_else(|| "The selected project is unavailable.".to_owned())?;
    let connection_id = project.connection_id.ok_or_else(|| {
        "Bind an SSH connection to this project before inspecting environments.".to_owned()
    })?;
    let root = project
        .remote_root
        .as_deref()
        .filter(|root| valid_root(root))
        .ok_or_else(|| "The project has no valid remote directory.".to_owned())?;
    let profile = find_profile(&state.repository, connection_id)
        .await
        .map_err(|_| PROBE_ERROR.to_owned())?;
    require_trusted_host(&profile).map_err(|_| {
        "Confirm this SSH host in Settings before inspecting environments.".to_owned()
    })?;
    let session = tokio::time::timeout(Duration::from_secs(5), connect_profile(state, &profile))
        .await
        .map_err(|_| PROBE_ERROR.to_owned())?
        .map_err(|_| PROBE_ERROR.to_owned())?;
    let mut report = inspect_project_with(&SshProbeRunner(&session), root, &request.environment)
        .await
        .map_err(|_| PROBE_ERROR.to_owned())?;
    let current = state
        .repository
        .get_project(request.project_id)
        .await
        .map_err(|_| PROBE_ERROR.to_owned())?
        .ok_or_else(|| PROBE_ERROR.to_owned())?;
    if current.connection_id != project.connection_id || current.remote_root != project.remote_root
    {
        return Err("The project's SSH binding changed. Refresh its environments.".into());
    }
    let current_profile = find_profile(&state.repository, connection_id)
        .await
        .map_err(|_| PROBE_ERROR.to_owned())?;
    if current_profile != profile {
        return Err("The SSH connection changed. Refresh the project's environments.".into());
    }
    report.project_id = request.project_id;
    report.connection_id = connection_id;
    Ok(report)
}

fn valid_root(root: &str) -> bool {
    root.len() <= 4096
        && root.starts_with('/')
        && !root.trim_end_matches('/').is_empty()
        && !root.chars().any(char::is_control)
        && !root.contains('\\')
        && !root.split('/').any(|part| part == "." || part == "..")
}

fn discovery_command(root: &str) -> String {
    let script = r#"root=__OMICSOPS_PROJECT_ROOT__;
root_real=$(realpath -e -- "$root") || exit 70;
test -d "$root_real" || exit 70;
printf 'READY\n';
printf 'ROOT\t%s\n' "$root_real";
for kind in envs managed; do
  if test "$kind" = envs; then suffix=.envs; else suffix=.omicsops/environments; fi;
  base="$root/$suffix";
  test -d "$base" && ! test -L "$base" || continue;
  base_real=$(realpath -e -- "$base") || continue;
  test "$base_real" = "$root_real/$suffix" || continue;
  find "$base" -mindepth 1 -maxdepth 1 -type d -print | head -n 9 |
  while IFS= read -r prefix; do
    seen=$(( ${seen:-0} + 1 ));
    if test "$seen" -gt 8; then printf 'TRUNCATED\n'; break; fi;
    name=${prefix##*/};
    case "$name" in ''|.|..|*[!A-Za-z0-9._-]*) continue;; esac;
    test ${#name} -le 64 && ! test -L "$prefix" || continue;
    actual=$(realpath -e -- "$prefix") || continue;
    test "$actual" = "$base_real/$name" || continue;
    printf '%s\t%s\n' "$kind" "$name";
  done;
done;
printf 'DONE\n'"#
        .replacen(
            "__OMICSOPS_PROJECT_ROOT__",
            &omicsops_core::project::shell_quote(root),
            1,
        );
    format!(
        "command -v timeout >/dev/null 2>&1 && command -v realpath >/dev/null 2>&1 && command -v find >/dev/null 2>&1 && command -v head >/dev/null 2>&1 || exit 70; timeout --kill-after=1s 3s sh -c {} 2>/dev/null | head -c {MAX_DISCOVERY_BYTES}",
        omicsops_core::project::shell_quote(&script)
    )
}

fn parse_discovery(
    output: &str,
) -> Result<(String, Vec<(String, String)>, bool), InspectionErrorV4> {
    if output.len() >= MAX_DISCOVERY_BYTES {
        return Err(InspectionErrorV4::Protocol);
    }
    let mut lines = output.lines();
    if lines.next() != Some("READY") {
        return Err(InspectionErrorV4::Protocol);
    }
    let root = lines
        .next()
        .and_then(|line| line.strip_prefix("ROOT\t"))
        .filter(|root| valid_root(root))
        .ok_or(InspectionErrorV4::Protocol)?
        .to_owned();
    let mut options = BTreeMap::new();
    let mut finished = false;
    let mut truncated = false;
    for line in lines {
        if finished {
            return Err(InspectionErrorV4::Protocol);
        }
        if line == "DONE" {
            finished = true;
            continue;
        }
        if line == "TRUNCATED" {
            truncated = true;
            continue;
        }
        let (source, name) = line.split_once('\t').ok_or(InspectionErrorV4::Protocol)?;
        if !safe_environment_basename(name, 64) {
            return Err(InspectionErrorV4::Protocol);
        }
        let (token, label) = match source {
            "envs" => (format!("envs:{name}"), format!(".envs/{name}")),
            "managed" if name != "system" => {
                (name.to_owned(), format!(".omicsops/environments/{name}"))
            }
            "managed" => continue,
            _ => return Err(InspectionErrorV4::Protocol),
        };
        options.insert(token, label);
        if options.len() > MAX_CANDIDATES {
            truncated = true;
        }
    }
    if !finished {
        return Err(InspectionErrorV4::Protocol);
    }
    let mut options = options.into_iter().collect::<Vec<_>>();
    options.sort_by(|left, right| {
        (!left.0.starts_with("envs:"), &left.0).cmp(&(!right.0.starts_with("envs:"), &right.0))
    });
    Ok((
        root,
        options.into_iter().take(MAX_CANDIDATES).collect(),
        truncated,
    ))
}

fn failed_observations(status: SshInterpreterStatusV4) -> Vec<SshInterpreterObservationV4> {
    [KernelLanguageV4::Python, KernelLanguageV4::R]
        .into_iter()
        .map(|language| SshInterpreterObservationV4::new(language, status))
        .collect()
}

async fn inspect_project_with(
    runner: &impl RemoteProbeRunner,
    root: &str,
    requested: &str,
) -> Result<SshEnvironmentReportV4, InspectionErrorV4> {
    if !valid_root(root) || SshEnvironment::parse(requested).is_err() {
        return Err(InspectionErrorV4::Protocol);
    }
    let deadline = Instant::now() + REPORT_TIMEOUT;
    let output = tokio::time::timeout(DISCOVERY_TIMEOUT, runner.run(&discovery_command(root)))
        .await
        .map_err(|_| InspectionErrorV4::Timeout)??;
    if output.status != 0 {
        return Err(InspectionErrorV4::Transport);
    }
    let (canonical_root, mut candidates, mut truncated) = parse_discovery(&output.stdout)?;
    if requested != "system" && !candidates.iter().any(|(token, _)| token == requested) {
        candidates.insert(0, (requested.to_owned(), environment_label(requested)));
        if candidates.len() > MAX_CANDIDATES {
            candidates.pop();
            truncated = true;
        }
    }
    candidates.insert(0, ("system".into(), "system".into()));
    let mut options = Vec::new();
    for (environment, label) in candidates {
        if Instant::now() >= deadline {
            truncated = true;
            break;
        }
        let observation = tokio::time::timeout_at(
            deadline,
            inspect_remote_with(runner, &canonical_root, &environment),
        )
        .await;
        let interpreters = match observation {
            Ok(Ok(report)) => report.interpreters,
            Ok(Err(error)) => failed_observations(if error == InspectionErrorV4::Timeout {
                SshInterpreterStatusV4::Timeout
            } else {
                SshInterpreterStatusV4::Error
            }),
            Err(_) => {
                truncated = true;
                failed_observations(SshInterpreterStatusV4::Timeout)
            }
        };
        options.push(SshEnvironmentOptionV4 {
            environment,
            label,
            interpreters,
        });
    }
    let suggestion = if truncated {
        None
    } else {
        suggested_option(&options)
    };
    let suggested_environment = suggestion.as_ref().map(|option| option.environment.clone());
    if let Some(suggestion) = suggestion {
        if !options
            .iter()
            .any(|option| option.environment == suggestion.environment)
        {
            options.push(suggestion);
        }
    }
    Ok(SshEnvironmentReportV4 {
        project_id: uuid::Uuid::nil(),
        connection_id: uuid::Uuid::nil(),
        remote_root: root.into(),
        requested_environment: requested.into(),
        observed_at: Utc::now().to_rfc3339(),
        environments: options,
        suggested_environment,
        truncated,
    })
}

fn environment_label(token: &str) -> String {
    match SshEnvironment::parse(token) {
        Ok(SshEnvironment::Project { python, r }) if python != r => format!(
            "Python: {}; R: {}",
            python
                .map(|name| format!(".envs/{name}"))
                .unwrap_or_else(|| "—".into()),
            r.map(|name| format!(".envs/{name}"))
                .unwrap_or_else(|| "—".into())
        ),
        Ok(SshEnvironment::Project {
            python: Some(name), ..
        }) => format!(".envs/{name}"),
        Ok(SshEnvironment::Managed(name)) => format!(".omicsops/environments/{name}"),
        _ => "system".into(),
    }
}

fn suggested_option(options: &[SshEnvironmentOptionV4]) -> Option<SshEnvironmentOptionV4> {
    // A failed observation might conceal another suitable interpreter. Do not
    // infer absence or uniqueness from a timeout, error, or omitted language.
    if options
        .iter()
        .filter(|option| option.environment.starts_with("envs:"))
        .any(|option| {
            [KernelLanguageV4::Python, KernelLanguageV4::R]
                .into_iter()
                .any(|language| {
                    !option.interpreters.iter().any(|item| {
                        item.language == language
                            && matches!(
                                item.status,
                                SshInterpreterStatusV4::Available
                                    | SshInterpreterStatusV4::NotFound
                            )
                    })
                })
        })
    {
        return None;
    }
    let choose = |language| -> Result<Option<(&str, SshInterpreterObservationV4)>, ()> {
        let mut matches = Vec::new();
        for option in options {
            let Some(name) = option
                .environment
                .strip_prefix("envs:")
                .filter(|name| !name.contains(':'))
            else {
                continue;
            };
            let Some(observation) = option.interpreters.iter().find(|item| {
                item.language == language && item.status == SshInterpreterStatusV4::Available
            }) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            let preferred = match language {
                KernelLanguageV4::Python => lower.starts_with("python") || lower.starts_with("py"),
                KernelLanguageV4::R => {
                    lower == "r"
                        || lower.starts_with("r-")
                        || lower
                            .strip_prefix('r')
                            .is_some_and(|suffix| suffix.starts_with(|c: char| c.is_ascii_digit()))
                }
            };
            matches.push((preferred, name, observation.clone()));
        }
        let Some(preferred) = matches.iter().map(|(preferred, _, _)| *preferred).max() else {
            return Ok(None);
        };
        matches.retain(|(value, _, _)| *value == preferred);
        if matches.len() != 1 {
            return Err(());
        }
        let (_, name, observation) = matches.remove(0);
        Ok(Some((name, observation)))
    };
    let python = choose(KernelLanguageV4::Python).ok()?;
    let r = choose(KernelLanguageV4::R).ok()?;
    if python.is_none() && r.is_none() {
        return None;
    }
    let token = format!(
        "envs:{}:{}",
        python.as_ref().map(|(name, _)| *name).unwrap_or(""),
        r.as_ref().map(|(name, _)| *name).unwrap_or("")
    );
    Some(SshEnvironmentOptionV4 {
        label: environment_label(&token),
        environment: token,
        interpreters: vec![
            python
                .map(|(_, observation)| observation)
                .unwrap_or_else(|| {
                    SshInterpreterObservationV4::new(
                        KernelLanguageV4::Python,
                        SshInterpreterStatusV4::NotFound,
                    )
                }),
            r.map(|(_, observation)| observation).unwrap_or_else(|| {
                SshInterpreterObservationV4::new(
                    KernelLanguageV4::R,
                    SshInterpreterStatusV4::NotFound,
                )
            }),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment_probe_v4::ProbeCommandOutput;
    use async_trait::async_trait;
    use std::{collections::VecDeque, sync::Mutex};

    struct Runner {
        output: Mutex<VecDeque<String>>,
        commands: Mutex<Vec<String>>,
        delay: Duration,
    }
    #[async_trait]
    impl RemoteProbeRunner for Runner {
        async fn run(&self, command: &str) -> Result<ProbeCommandOutput, InspectionErrorV4> {
            let index = {
                let mut commands = self.commands.lock().unwrap();
                commands.push(command.into());
                commands.len()
            };
            if index > 1 {
                tokio::time::sleep(self.delay).await;
            }
            Ok(ProbeCommandOutput {
                status: 0,
                stdout: self
                    .output
                    .lock()
                    .unwrap()
                    .pop_front()
                    .ok_or(InspectionErrorV4::Transport)?,
            })
        }
    }
    fn runner(output: &[&str]) -> Runner {
        Runner {
            output: Mutex::new(output.iter().map(|value| value.to_string()).collect()),
            commands: Mutex::new(Vec::new()),
            delay: Duration::ZERO,
        }
    }

    #[tokio::test]
    async fn ssh_project_probe_binds_separate_actual_python_and_r_without_marking_system_available()
    {
        let runner = runner(&[
            "READY\nROOT\t/project\nenvs\tpython311\nenvs\tr443\nDONE\n",
            "NOT_FOUND\n",
            "NOT_FOUND\n",
            "PATH\t/project/.envs/python311/bin/python\n3.11.18\nEXIT\t0\n",
            "NOT_FOUND\n",
            "PATH\t/project/.envs/r443/bin/python\n3.12.2\nEXIT\t0\n",
            "PATH\t/project/.envs/r443/bin/Rscript\nRscript (R) version 4.4.3 (2025-02-28)\nEXIT\t0\n",
        ]);
        let report = inspect_project_with(&runner, "/project", "system")
            .await
            .unwrap();
        assert_eq!(
            report.suggested_environment.as_deref(),
            Some("envs:python311:r443")
        );
        assert_eq!(report.environments[0].environment, "system");
        assert!(
            report.environments[0]
                .interpreters
                .iter()
                .all(|observation| observation.status == SshInterpreterStatusV4::NotFound)
        );
        let combined = report.environments.last().unwrap();
        assert_eq!(combined.interpreters[0].version.as_deref(), Some("3.11.18"));
        assert_eq!(combined.interpreters[1].version.as_deref(), Some("4.4.3"));
        assert_eq!(runner.commands.lock().unwrap().len(), 7);
    }

    #[test]
    fn ssh_project_discovery_frames_reject_untrusted_names_and_bound_candidates() {
        for value in [
            "",
            "READY\nROOT\t/project\nenvs\t../outside\nDONE\n",
            "READY\nROOT\t/project\nenvs\tbad;id\nDONE\n",
            "READY\nROOT\t/project\nunknown\tsafe\nDONE\n",
            "READY\nROOT\t/project\nenvs\t.\nDONE\n",
            "READY\nROOT\t/project\nenvs\tsafe\n",
            "READY\nROOT\t/project\nDONE\nTRAILING\n",
        ] {
            assert!(parse_discovery(value).is_err());
        }
        let text = format!(
            "READY\nROOT\t/project\n{}DONE\n",
            (0..9)
                .map(|index| format!("envs\tenv{index}\n"))
                .collect::<String>()
        );
        let (_, candidates, truncated) = parse_discovery(&text).unwrap();
        assert_eq!(candidates.len(), MAX_CANDIDATES);
        assert!(truncated);
        assert!(parse_discovery(&"x".repeat(MAX_DISCOVERY_BYTES)).is_err());
    }

    #[test]
    fn ssh_project_discovery_is_bounded_first_level_and_rejects_symlink_roots() {
        let command = discovery_command("/project with 'quotes'");
        for expected in [
            "-mindepth 1 -maxdepth 1",
            "head -n 9",
            "head -c 4096",
            "--kill-after=1s 3s",
            "test -L",
            "realpath -e",
        ] {
            assert!(command.contains(expected), "{expected}");
        }
        for forbidden in ["mkdir", "micromamba create", "source ", "eval ", "rm -"] {
            assert!(!command.contains(forbidden));
        }
        for root in [
            "/",
            "relative",
            "/project/../other",
            "/project\nprivate",
            "/project\\other",
        ] {
            assert!(!valid_root(root));
        }
    }

    #[test]
    fn ssh_project_discovery_preserves_the_wire_frame_marker_when_quoting_root() {
        let command = discovery_command("/project with 'quotes'");
        assert_eq!(command.matches("ROOT\\t%s\\n").count(), 1);
        assert!(command.contains("READY\\n"));
        assert!(command.contains("DONE\\n"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ssh_project_discovery_shell_fixture_preserves_real_frames_and_quoted_paths() {
        let temporary = tempfile::tempdir().unwrap();
        let project = temporary.path().join("study 'with spaces'");
        for name in ["python311", "r443"] {
            std::fs::create_dir_all(project.join(".envs").join(name)).unwrap();
        }
        let output = std::process::Command::new("sh")
            .args(["-c", &discovery_command(project.to_str().unwrap())])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        let (canonical, options, truncated) = parse_discovery(&stdout).unwrap();
        assert_eq!(canonical, project.canonicalize().unwrap().to_str().unwrap());
        assert_eq!(
            options
                .into_iter()
                .map(|(token, _)| token)
                .collect::<Vec<_>>(),
            ["envs:python311", "envs:r443"]
        );
        assert!(!truncated);
    }

    #[tokio::test(start_paused = true)]
    async fn ssh_project_probe_deadline_preserves_partial_report_without_automatic_selection() {
        let mut runner =
            runner(&["READY\nROOT\t/project\nenvs\ta\nenvs\tb\nenvs\tc\nenvs\td\nenvs\te\nDONE\n"]);
        runner.delay = Duration::from_secs(300);
        let start = Instant::now();
        let report = inspect_project_with(&runner, "/project", "system")
            .await
            .unwrap();
        assert!(report.truncated);
        assert!(report.suggested_environment.is_none());
        assert!(start.elapsed() <= REPORT_TIMEOUT);
        assert!(
            report
                .environments
                .iter()
                .flat_map(|option| &option.interpreters)
                .all(|item| item.status == SshInterpreterStatusV4::Timeout)
        );
        assert!(runner.commands.lock().unwrap().len() <= 5);
    }

    #[tokio::test]
    async fn ssh_project_probe_fails_closed_before_transport_for_unsafe_scope() {
        let runner = runner(&[]);
        for (root, environment) in [
            ("/project", "envs:../outside"),
            ("/project/../other", "system"),
        ] {
            assert!(
                inspect_project_with(&runner, root, environment)
                    .await
                    .is_err()
            );
        }
        assert!(runner.commands.lock().unwrap().is_empty());
    }

    #[test]
    fn ssh_project_suggestion_does_not_choose_between_equally_suitable_candidates() {
        let options = ["python311", "python312"].map(|name| SshEnvironmentOptionV4 {
            environment: format!("envs:{name}"),
            label: format!(".envs/{name}"),
            interpreters: vec![
                SshInterpreterObservationV4 {
                    language: KernelLanguageV4::Python,
                    status: SshInterpreterStatusV4::Available,
                    executable: Some(format!("/project/.envs/{name}/bin/python")),
                    version: Some("3.11.18".into()),
                },
                SshInterpreterObservationV4::new(
                    KernelLanguageV4::R,
                    SshInterpreterStatusV4::NotFound,
                ),
            ],
        });
        assert!(suggested_option(&options).is_none());
    }

    #[test]
    fn ssh_project_suggestion_does_not_turn_probe_errors_into_absence() {
        for status in [
            SshInterpreterStatusV4::Error,
            SshInterpreterStatusV4::Timeout,
        ] {
            let options = [SshEnvironmentOptionV4 {
                environment: "envs:python311".into(),
                label: ".envs/python311".into(),
                interpreters: vec![
                    SshInterpreterObservationV4 {
                        language: KernelLanguageV4::Python,
                        status: SshInterpreterStatusV4::Available,
                        executable: Some("/project/.envs/python311/bin/python".into()),
                        version: Some("3.11.18".into()),
                    },
                    SshInterpreterObservationV4::new(KernelLanguageV4::R, status),
                ],
            }];
            assert!(suggested_option(&options).is_none(), "{status:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn ssh_project_command_deadline_also_bounds_binding_and_final_scope_reads() {
        let start = Instant::now();
        let result = with_command_deadline(std::future::pending()).await;
        assert_eq!(result.unwrap_err(), PROBE_ERROR);
        assert_eq!(start.elapsed(), COMMAND_TIMEOUT);
    }

    #[tokio::test]
    async fn ssh_project_probe_uses_canonical_root_but_retains_the_requested_scope() {
        let runner = runner(&[
            "READY\nROOT\t/canonical\nenvs\tpython311\nDONE\n",
            "NOT_FOUND\n",
            "NOT_FOUND\n",
            "PATH\t/canonical/.envs/python311/bin/python\n3.11.18\nEXIT\t0\n",
            "NOT_FOUND\n",
        ]);
        let report = inspect_project_with(&runner, "/project-alias", "system")
            .await
            .unwrap();
        assert_eq!(report.remote_root, "/project-alias");
        let commands = runner.commands.lock().unwrap();
        assert!(commands[3].contains("/canonical/.envs/python311"));
        assert!(!commands[3].contains("/project-alias"));
    }
}
