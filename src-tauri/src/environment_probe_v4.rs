use std::{io, path::PathBuf, process::Stdio, time::Duration};

use async_trait::async_trait;
use omicsops_adapters::ssh::SshSession;
use omicsops_core::{
    project::shell_quote,
    ssh_environment::{SshEnvironment, SshEnvironmentLanguage},
};
pub(crate) use omicsops_dto::{
    SshInterpreterObservationV4 as InterpreterObservationV4,
    SshInterpreterStatusV4 as ProbeStatusV4,
};
use omicsops_process::background_command;
use omicsops_protocol::KernelLanguageV4;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const INSPECTION_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_OUTPUT_BYTES: usize = 4096;

#[cfg(test)]
mod project_prefix_regression_tests {
    use super::*;

    #[test]
    fn envs_probe_uses_the_frozen_language_prefix() {
        let python =
            remote_probe_command(KernelLanguageV4::Python, "/project", "envs:python311:r443");
        let r = remote_probe_command(KernelLanguageV4::R, "/project", "envs:python311:r443");
        assert!(python.contains("/project/.envs/python311"));
        assert!(r.contains("/project/.envs/r443"));
        assert!(!python.contains(".omicsops/environments/envs:"));
        assert!(!r.contains("micromamba run"));
    }

    #[test]
    fn envs_probe_missing_language_never_falls_back_to_system_or_another_prefix() {
        let missing = remote_probe_command(KernelLanguageV4::R, "/project", "envs:python311:");
        assert_eq!(missing, "printf 'NOT_FOUND\\n'");
        assert!(!missing.contains("command -v Rscript"));
        for token in ["envs:..", "envs:outside/name", "envs:py:r;id"] {
            assert_eq!(
                remote_probe_command(KernelLanguageV4::Python, "/project", token),
                "printf 'ERROR\\n'"
            );
        }
    }

    #[test]
    fn envs_probe_allows_interpreter_file_symlinks_but_keeps_directory_scope() {
        let command = remote_probe_command(KernelLanguageV4::Python, "/project", "envs:python311");
        assert!(command.contains("test ! -L '/project/.envs/python311/bin'"));
        assert!(command.contains("prefix_real=$(realpath -e"));
        assert!(command.contains("test -f \"$p\""));
        assert!(command.contains("test -x \"$p\""));
        assert!(!command.contains("p_real="));
        assert!(!command.contains("test -L \"$p\""));
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct EnvironmentInspectionV4 {
    pub(super) interpreters: Vec<InterpreterObservationV4>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InspectionErrorV4 {
    Unsupported,
    Transport,
    Timeout,
    Protocol,
}

pub(crate) struct ProbeCommandOutput {
    pub(crate) status: u32,
    pub(crate) stdout: String,
}

#[async_trait]
pub(crate) trait RemoteProbeRunner: Sync {
    async fn run(&self, command: &str) -> Result<ProbeCommandOutput, InspectionErrorV4>;
}

pub(crate) struct SshProbeRunner<'a>(pub(crate) &'a SshSession);

#[async_trait]
impl RemoteProbeRunner for SshProbeRunner<'_> {
    async fn run(&self, command: &str) -> Result<ProbeCommandOutput, InspectionErrorV4> {
        let output = self
            .0
            .execute(command)
            .await
            .map_err(|_| InspectionErrorV4::Transport)?;
        Ok(ProbeCommandOutput {
            status: output.status,
            stdout: output.stdout,
        })
    }
}

pub(super) async fn inspect_remote(
    session: &SshSession,
    root: &str,
    environment: &str,
) -> Result<EnvironmentInspectionV4, InspectionErrorV4> {
    inspect_remote_with(&SshProbeRunner(session), root, environment).await
}

pub(crate) async fn inspect_remote_language(
    session: &SshSession,
    root: &str,
    environment: &str,
    language: KernelLanguageV4,
) -> Result<InterpreterObservationV4, InspectionErrorV4> {
    inspect_remote_language_with(&SshProbeRunner(session), root, environment, language).await
}

async fn inspect_remote_language_with(
    runner: &impl RemoteProbeRunner,
    root: &str,
    environment: &str,
    language: KernelLanguageV4,
) -> Result<InterpreterObservationV4, InspectionErrorV4> {
    let command = remote_probe_command(language, root, environment);
    let output = tokio::time::timeout(PROBE_TIMEOUT, runner.run(&command))
        .await
        .map_err(|_| InspectionErrorV4::Timeout)??;
    if output.status != 0 {
        return Err(InspectionErrorV4::Transport);
    }
    parse_remote_probe(language, &output.stdout)
}

pub(crate) async fn inspect_remote_with(
    runner: &impl RemoteProbeRunner,
    root: &str,
    environment: &str,
) -> Result<EnvironmentInspectionV4, InspectionErrorV4> {
    let mut interpreters = Vec::new();
    for language in [KernelLanguageV4::Python, KernelLanguageV4::R] {
        interpreters.push(inspect_remote_language_with(runner, root, environment, language).await?);
    }
    Ok(EnvironmentInspectionV4 { interpreters })
}

fn version_arguments(language: KernelLanguageV4) -> Vec<&'static str> {
    match language {
        KernelLanguageV4::Python => vec![
            "-I",
            "-S",
            "-c",
            "import sys; print('.'.join(map(str, sys.version_info[:3])))",
        ],
        KernelLanguageV4::R => vec!["--vanilla", "--version"],
    }
}

fn interpreter_program(language: KernelLanguageV4) -> &'static str {
    match language {
        KernelLanguageV4::Python => "python",
        KernelLanguageV4::R => "Rscript",
    }
}

fn remote_probe_command(language: KernelLanguageV4, root: &str, environment: &str) -> String {
    let program = interpreter_program(language);
    let parsed = match SshEnvironment::parse(environment) {
        Ok(value) => value,
        Err(_) => return "printf 'ERROR\\n'".into(),
    };
    let prefix = match parsed.prefix(
        root,
        match language {
            KernelLanguageV4::Python => SshEnvironmentLanguage::Python,
            KernelLanguageV4::R => SshEnvironmentLanguage::R,
        },
    ) {
        Ok(value) => value,
        Err(_) => return "printf 'NOT_FOUND\\n'".into(),
    };
    let lookup = if let Some(prefix) = prefix {
        let guard = if parsed.is_project() {
            format!(
                "root_real=$(realpath -e -- {root}) || {{ printf 'ERROR\\n'; exit 0; }}; base_real=$(realpath -e -- {base}) || {{ printf 'ERROR\\n'; exit 0; }}; test ! -L {base} && test \"$base_real\" = \"$root_real/.envs\" && test ! -L {prefix} && test ! -L {bin} || {{ printf 'ERROR\\n'; exit 0; }}; prefix_real=$(realpath -e -- {prefix}) || {{ printf 'ERROR\\n'; exit 0; }}; test \"$prefix_real\" = \"$base_real/{}\" || {{ printf 'ERROR\\n'; exit 0; }}; export PATH={bin}:\"$PATH\"; ",
                prefix.rsplit('/').next().unwrap_or(""),
                root = shell_quote(root),
                base = shell_quote(&format!("{}/.envs", root.trim_end_matches('/'))),
                prefix = shell_quote(&prefix),
                bin = shell_quote(&format!("{prefix}/bin")),
            )
        } else {
            String::new()
        };
        format!(
            "p={}; if ! test -e \"$p\"; then printf 'NOT_FOUND\\n'; exit 0; fi; {guard}",
            shell_quote(&format!("{prefix}/bin/{program}"))
        )
    } else {
        format!("p=$(command -v {program}) || {{ printf 'NOT_FOUND\\n'; exit 0; }}")
    };
    let arguments = match language {
        KernelLanguageV4::Python => {
            format!("-I -S -c {}", shell_quote(version_arguments(language)[3]))
        }
        KernelLanguageV4::R => "--vanilla --version".into(),
    };
    // The cap is applied on the remote side, before SshSession buffers output.
    // A full/truncated frame is rejected; timeout still bounds the producer
    // after head stops reading. No files, profiles, packages or kernels are used.
    format!(
        "if ! command -v head >/dev/null 2>&1 || ! command -v timeout >/dev/null 2>&1; then printf 'ERROR\\n'; exit 0; fi; {{ {lookup}; case \"$p\" in /*) ;; *) printf 'ERROR\\n'; exit 0;; esac; if ! test -f \"$p\" || ! test -x \"$p\"; then printf 'ERROR\\n'; exit 0; fi; printf 'PATH\\t%s\\n' \"$p\"; timeout --kill-after=1s 3s \"$p\" {arguments} 2>&1; rc=$?; printf '\\nEXIT\\t%s\\n' \"$rc\"; }} 2>/dev/null | head -c {MAX_OUTPUT_BYTES}"
    )
}

fn parse_remote_probe(
    language: KernelLanguageV4,
    output: &str,
) -> Result<InterpreterObservationV4, InspectionErrorV4> {
    if output.len() >= MAX_OUTPUT_BYTES {
        return Err(InspectionErrorV4::Protocol);
    }
    if output.trim_end() == "NOT_FOUND" {
        return Ok(InterpreterObservationV4::new(
            language,
            ProbeStatusV4::NotFound,
        ));
    }
    if output.trim_end() == "ERROR" {
        return Ok(InterpreterObservationV4::new(
            language,
            ProbeStatusV4::Error,
        ));
    }
    let lines = output
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let path = lines
        .first()
        .and_then(|line| line.strip_prefix("PATH\t"))
        .filter(|path| path.starts_with('/') && !path.chars().any(char::is_control))
        .ok_or(InspectionErrorV4::Protocol)?;
    let status = lines
        .last()
        .and_then(|line| line.strip_prefix("EXIT\t"))
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or(InspectionErrorV4::Protocol)?;
    let mut observation = InterpreterObservationV4::new(
        language,
        match status {
            124 | 137 => ProbeStatusV4::Timeout,
            _ => ProbeStatusV4::Error,
        },
    );
    observation.executable = Some(path.into());
    if status == 0 && lines.len() == 3 {
        if let Some(version) = parse_version(language, lines[1]) {
            observation.status = ProbeStatusV4::Available;
            observation.version = Some(version);
        }
    }
    Ok(observation)
}

fn parse_version(language: KernelLanguageV4, text: &str) -> Option<String> {
    let version = match language {
        KernelLanguageV4::Python => text.trim(),
        KernelLanguageV4::R => text
            .trim()
            .strip_prefix("Rscript (R) version ")
            .or_else(|| text.trim().strip_prefix("R scripting front-end version "))?
            .split_whitespace()
            .next()?,
    };
    let parts = version.split('.').collect::<Vec<_>>();
    (parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 4 && part.chars().all(|c| c.is_ascii_digit())
        }))
    .then(|| version.into())
}

fn find_local_program(program: &str) -> Result<Option<PathBuf>, io::Error> {
    let Some(path) = std::env::var_os("PATH") else {
        return Ok(None);
    };
    let mut unavailable = None;
    for directory in std::env::split_paths(&path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(if cfg!(windows) {
            format!("{program}.exe")
        } else {
            program.into()
        });
        match std::fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o111 == 0 {
                        continue;
                    }
                }
                return Ok(Some(candidate));
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) => {}
            Err(error) => unavailable = Some(error),
        }
    }
    match unavailable {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

async fn bounded_output(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, io::Error> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_OUTPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "probe output exceeded its limit",
        ));
    }
    Ok(bytes)
}

async fn probe_local(language: KernelLanguageV4) -> InterpreterObservationV4 {
    let mut observation = InterpreterObservationV4::new(language, ProbeStatusV4::Error);
    let program = interpreter_program(language);
    let path = match tokio::task::spawn_blocking(move || find_local_program(program)).await {
        Ok(Ok(Some(path))) => path,
        Ok(Ok(None)) => return InterpreterObservationV4::new(language, ProbeStatusV4::NotFound),
        _ => return observation,
    };
    observation.executable = Some(path.to_string_lossy().into());
    let mut command = background_command(&path);
    command
        .args(version_arguments(language))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let Ok(mut child) = command.spawn() else {
        return observation;
    };
    let Some(stdout) = child.stdout.take() else {
        return observation;
    };
    let Some(stderr) = child.stderr.take() else {
        return observation;
    };
    let Ok((stdout, stderr, status)) =
        tokio::try_join!(bounded_output(stdout), bounded_output(stderr), child.wait())
    else {
        return observation;
    };
    if !status.success() {
        return observation;
    }
    let stdout = String::from_utf8_lossy(&stdout);
    let stderr = String::from_utf8_lossy(&stderr);
    let version = if stderr.trim().is_empty() {
        parse_version(language, &stdout)
    } else if stdout.trim().is_empty() {
        parse_version(language, &stderr)
    } else {
        None
    };
    if let Some(version) = version {
        observation.status = ProbeStatusV4::Available;
        observation.version = Some(version);
    }
    observation
}

async fn bounded_local_probe(
    language: KernelLanguageV4,
    probe: impl std::future::Future<Output = InterpreterObservationV4>,
) -> InterpreterObservationV4 {
    tokio::time::timeout(PROBE_TIMEOUT, probe)
        .await
        .unwrap_or_else(|_| InterpreterObservationV4::new(language, ProbeStatusV4::Timeout))
}

pub(super) async fn inspect_local() -> EnvironmentInspectionV4 {
    let mut interpreters = Vec::new();
    for language in [KernelLanguageV4::Python, KernelLanguageV4::R] {
        interpreters.push(bounded_local_probe(language, probe_local(language)).await);
    }
    EnvironmentInspectionV4 { interpreters }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    struct Fixture {
        outputs: Mutex<Vec<String>>,
        calls: AtomicUsize,
        fail: bool,
        delay: Duration,
    }

    #[async_trait]
    impl RemoteProbeRunner for Fixture {
        async fn run(&self, _: &str) -> Result<ProbeCommandOutput, InspectionErrorV4> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            if self.fail {
                return Err(InspectionErrorV4::Transport);
            }
            Ok(ProbeCommandOutput {
                status: 0,
                stdout: self.outputs.lock().unwrap().remove(0),
            })
        }
    }

    fn fixture(outputs: &[&str]) -> Fixture {
        Fixture {
            outputs: Mutex::new(outputs.iter().map(|s| s.to_string()).collect()),
            calls: AtomicUsize::new(0),
            fail: false,
            delay: Duration::ZERO,
        }
    }

    #[tokio::test]
    async fn environment_inspection_records_positive_and_negative_observations_without_code() {
        let runner = fixture(&["PATH\t/usr/bin/python\n3.12.1\nEXIT\t0\n", "NOT_FOUND\n"]);
        let report = inspect_remote_with(&runner, "/project", "system")
            .await
            .unwrap();
        assert_eq!(runner.calls.load(Ordering::SeqCst), 2);
        assert_eq!(report.interpreters[0].status, ProbeStatusV4::Available);
        assert_eq!(report.interpreters[0].version.as_deref(), Some("3.12.1"));
        assert_eq!(report.interpreters[1].status, ProbeStatusV4::NotFound);
        assert!(report.interpreters[1].version.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn environment_inspection_remote_wait_is_bounded_without_claiming_termination() {
        let runner = Fixture {
            delay: Duration::from_secs(300),
            ..fixture(&[])
        };
        let started = tokio::time::Instant::now();
        assert_eq!(
            inspect_remote_with(&runner, "/project", "system")
                .await
                .unwrap_err(),
            InspectionErrorV4::Timeout
        );
        assert_eq!(started.elapsed(), PROBE_TIMEOUT);
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn environment_inspection_transport_failure_is_not_a_missing_interpreter() {
        let runner = Fixture {
            fail: true,
            ..fixture(&[])
        };
        assert_eq!(
            inspect_remote_with(&runner, "/project", "system")
                .await
                .unwrap_err(),
            InspectionErrorV4::Transport
        );
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn environment_inspection_distinguishes_timeout_error_and_valid_r_version() {
        for (text, status) in [
            (
                "PATH\t/env/bin/Rscript\nEXIT\t124\n",
                ProbeStatusV4::Timeout,
            ),
            ("ERROR\n", ProbeStatusV4::Error),
            (
                "PATH\t/env/bin/Rscript\nPRIVATE_ERROR\nEXIT\t1\n",
                ProbeStatusV4::Error,
            ),
            (
                "PATH\t/env/bin/Rscript\nRscript (R) version 4.4.0 (2024-04-24)\nEXIT\t0\n",
                ProbeStatusV4::Available,
            ),
        ] {
            let observation = parse_remote_probe(KernelLanguageV4::R, text).unwrap();
            assert_eq!(observation.status, status);
            assert!(
                !serde_json::to_string(&observation)
                    .unwrap()
                    .contains("PRIVATE_ERROR")
            );
        }
        for text in ["", "PATH\t/env/bin/Rscript\n", "NOT_FOUND\ntrailing\n"] {
            assert!(parse_remote_probe(KernelLanguageV4::R, text).is_err());
        }
        assert!(
            parse_remote_probe(KernelLanguageV4::R, &"x".repeat(MAX_OUTPUT_BYTES + 1)).is_err()
        );
        assert!(parse_remote_probe(KernelLanguageV4::R, &"x".repeat(MAX_OUTPUT_BYTES)).is_err());
    }

    #[test]
    fn environment_inspection_commands_have_fixed_deadlines_and_no_discovery_or_mutation() {
        for language in [KernelLanguageV4::Python, KernelLanguageV4::R] {
            let system = remote_probe_command(language, "/project", "system");
            let managed = remote_probe_command(language, "/project with spaces", "analysis-r");
            for command in [&system, &managed] {
                assert!(command.contains("--kill-after=1s 3s"));
                assert!(command.contains("head -c 4096"));
                assert!(!command.contains("mkdir"));
                assert!(!command.contains("glob"));
                assert!(!command.contains("require("));
                assert!(!command.contains("conda env"));
                assert!(!command.contains("micromamba run"));
            }
            assert!(
                managed.contains("/project with spaces/.omicsops/environments/analysis-r/bin/")
            );
            assert!(system.contains(match language {
                KernelLanguageV4::Python => "-I -S",
                KernelLanguageV4::R => "--vanilla --version",
            }));
        }
    }

    #[tokio::test]
    async fn environment_inspection_local_output_limit_is_checked_before_capture_completes() {
        let exact = vec![b'x'; MAX_OUTPUT_BYTES];
        assert_eq!(
            bounded_output(exact.as_slice()).await.unwrap().len(),
            MAX_OUTPUT_BYTES
        );
        let excessive = vec![b'x'; MAX_OUTPUT_BYTES + 1];
        assert_eq!(
            bounded_output(excessive.as_slice())
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test(start_paused = true)]
    async fn environment_inspection_local_timeout_drops_the_owned_probe() {
        struct Dropped<'a>(&'a AtomicUsize);
        impl Drop for Dropped<'_> {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let dropped = AtomicUsize::new(0);
        let started = tokio::time::Instant::now();
        let result = bounded_local_probe(KernelLanguageV4::Python, async {
            let _guard = Dropped(&dropped);
            std::future::pending::<InterpreterObservationV4>().await
        })
        .await;
        assert_eq!(result.status, ProbeStatusV4::Timeout);
        assert_eq!(started.elapsed(), PROBE_TIMEOUT);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert!(result.version.is_none());
    }
}
