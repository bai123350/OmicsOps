use std::path::{Component, Path};

use omicsops_agent::KernelLanguage;

use crate::{AdapterError, AdapterResult};

pub fn validate_kernel_code(code: &str) -> AdapterResult<()> {
    if code.trim().is_empty() {
        return Err(AdapterError::InvalidInput("kernel code is empty".into()));
    }
    if code.len() > 1024 * 1024 {
        return Err(AdapterError::InvalidInput(
            "kernel code exceeds 1 MiB".into(),
        ));
    }
    Ok(())
}

pub fn validate_capture_paths(paths: &[String]) -> AdapterResult<()> {
    if paths.len() > 128 {
        return Err(AdapterError::InvalidInput(
            "too many kernel artifact capture paths".into(),
        ));
    }
    for value in paths {
        let normalized = value.replace('\\', "/");
        if normalized.is_empty() || normalized.starts_with('/') || normalized.contains(':') {
            return Err(AdapterError::InvalidInput(format!(
                "unsafe kernel capture path: {value}"
            )));
        }
        let mut depth = 0usize;
        for component in Path::new(&normalized).components() {
            match component {
                Component::Normal(part) => {
                    if depth == 0 && part == ".omicsops" {
                        return Err(AdapterError::InvalidInput(
                            "kernel cannot capture control files".into(),
                        ));
                    }
                    depth += 1;
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    if depth == 0 {
                        return Err(AdapterError::InvalidInput(format!(
                            "kernel capture path escapes project: {value}"
                        )));
                    }
                    depth -= 1;
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(AdapterError::InvalidInput(format!(
                        "absolute kernel capture path: {value}"
                    )));
                }
            }
        }
        if depth == 0 {
            return Err(AdapterError::InvalidInput(format!(
                "empty kernel capture path: {value}"
            )));
        }
    }
    Ok(())
}

pub fn kernel_driver(language: KernelLanguage) -> &'static str {
    match language {
        KernelLanguage::Python => PYTHON_DRIVER,
        KernelLanguage::R => R_DRIVER,
    }
}

const PYTHON_DRIVER: &str = concat!(
    r#"import contextlib, datetime, hashlib, io, json, pathlib, sys, traceback
sys.stdin.reconfigure(encoding="utf-8")
sys.stdout.reconfigure(encoding="utf-8")
project = pathlib.Path(sys.argv[1]).resolve()
project_id = sys.argv[2]
session_id = sys.argv[3]
scope = {"__name__": "__omicsops_kernel__"}
"#,
    include_str!("../../../scripts/kernel_inspection_helpers.py"),
    r#"def emit(request_id, sequence, kind, payload=None):
    event = {"project_id": project_id, "session_id": session_id, "request_id": request_id, "sequence": sequence, "occurred_at": datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z"), "event": {"kind": kind}}
    if payload is not None: event["event"]["payload"] = payload
    print(json.dumps(event, ensure_ascii=False), flush=True)
for raw in sys.stdin:
    request_id = "protocol"
    sequence = 1
    try:
        request = json.loads(raw)
        request_id = request.get("request_id", "protocol")
        if request["action"] == "shutdown":
            emit(request_id, sequence, "stopped")
            break
        if request["action"] == "inspect":
            try:
                emit(request_id, sequence, "inspection", inspect_objects(scope))
            except Exception:
                emit(request_id, sequence, "failed", {"message": "inspection unavailable"})
            continue
        emit(request_id, sequence, "started"); sequence += 1
        stdout, stderr = io.StringIO(), io.StringIO()
        try:
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                exec(compile(request["code"], "<omicsops-cell>", "exec"), scope, scope)
            if stdout.getvalue(): emit(request_id, sequence, "stdout", stdout.getvalue()); sequence += 1
            if stderr.getvalue(): emit(request_id, sequence, "stderr", stderr.getvalue()); sequence += 1
            for relative in request.get("capture_paths", []):
                path = (project / relative).resolve()
                if project not in path.parents or ".omicsops" in path.relative_to(project).parts or not path.is_file():
                    raise ValueError("artifact path is not a regular project file: " + relative)
                digest = hashlib.sha256()
                with path.open("rb") as stream:
                    for chunk in iter(lambda: stream.read(1024 * 1024), b""): digest.update(chunk)
                emit(request_id, sequence, "artifact", {"relative_path": path.relative_to(project).as_posix(), "size_bytes": path.stat().st_size, "sha256": digest.hexdigest()}); sequence += 1
            emit(request_id, sequence, "completed")
        except Exception as error:
            if stdout.getvalue(): emit(request_id, sequence, "stdout", stdout.getvalue()); sequence += 1
            emit(request_id, sequence, "stderr", traceback.format_exc()); sequence += 1
            emit(request_id, sequence, "failed", {"message": str(error)})
    except Exception as error:
        event = {"project_id": project_id, "session_id": session_id, "request_id": request_id, "sequence": sequence, "occurred_at": datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z"), "event": {"kind": "failed", "payload": {"message": str(error)}}}
        print(json.dumps(event), flush=True)
"#,
);

const R_DRIVER: &str = concat!(
    r#"suppressPackageStartupMessages(library(jsonlite))
args <- commandArgs(trailingOnly=TRUE); project <- normalizePath(args[[1]], mustWork=TRUE); project_id <- args[[2]]; session_id <- args[[3]]; scope <- new.env(parent=globalenv())
"#,
    include_str!("../../../scripts/kernel_inspection_helpers.R"),
    r#"emit <- function(request_id, sequence, kind, payload=NULL) { event <- list(project_id=project_id, session_id=session_id, request_id=request_id, sequence=sequence, occurred_at=format(Sys.time(), tz="UTC", format="%Y-%m-%dT%H:%M:%OS3Z"), event=list(kind=kind)); if (!is.null(payload)) event$event$payload <- payload; cat(toJSON(event, auto_unbox=TRUE, null="null"), "\n", sep=""); flush.console() }
input <- file("stdin", "r")
repeat { raw <- readLines(input, n=1, warn=FALSE); if (!length(raw)) break; request <- fromJSON(raw, simplifyVector=FALSE); request_id <- request$request_id; sequence <- 1L; if (request$action == "shutdown") { emit(request_id, sequence, "stopped"); break }; if (identical(request$action,"inspect")) { tryCatch(emit(request_id,sequence,"inspection",inspect_bindings(scope)),error=function(e) emit(request_id,sequence,"failed",list(message="inspection unavailable"))); next }; emit(request_id, sequence, "started"); sequence <- sequence + 1L; output <- character(); error <- NULL; tryCatch({ output <- capture.output(eval(parse(text=request$code), envir=scope)); if (length(output)) { emit(request_id, sequence, "stdout", paste0(output, collapse="\n")); sequence <- sequence + 1L }; if (length(request$capture_paths)) for (relative in request$capture_paths) { path <- normalizePath(file.path(project, relative), mustWork=TRUE); if (!startsWith(path, paste0(project, .Platform$file.sep)) || !file.exists(path) || dir.exists(path)) stop("artifact escaped project root"); size <- file.info(path)$size; sha <- strsplit(system2("sha256sum", c("--", shQuote(path)), stdout=TRUE), "\\s+")[[1]][1]; emit(request_id, sequence, "artifact", list(relative_path=relative, size_bytes=size, sha256=sha)); sequence <- sequence + 1L }; emit(request_id, sequence, "completed") }, error=function(e) { emit(request_id, sequence, "failed", list(message=conditionMessage(e))) }) }
"#,
);

#[cfg(test)]
mod kernel_inspection_tests {
    #[test]
    fn inspect_compiled_workers_embed_canonical_policy_once() {
        for (worker, policy, definition) in [
            (
                PYTHON_DRIVER,
                include_str!("../../../scripts/kernel_inspection_helpers.py"),
                concat!("def inspect_", "objects("),
            ),
            (
                R_DRIVER,
                include_str!("../../../scripts/kernel_inspection_helpers.R"),
                concat!("inspect_bindings", " <- function"),
            ),
        ] {
            assert_eq!(worker.matches(policy).count(), 1);
            assert_eq!(worker.matches(definition).count(), 1);
        }
    }

    use super::*;
    #[test]
    fn kernel_workers_dispatch_fixed_inspection_before_execution() {
        assert!(PYTHON_DRIVER.contains("inspect_objects(scope)"));
        assert!(R_DRIVER.contains("inspect_bindings(scope)"));
        assert!(
            PYTHON_DRIVER.find("inspect_objects(scope)").unwrap()
                < PYTHON_DRIVER.find("exec(compile").unwrap()
        );
        assert!(
            R_DRIVER.find("inspect_bindings(scope)").unwrap()
                < R_DRIVER.find("eval(parse").unwrap()
        );
    }
    #[tokio::test]
    #[ignore = "requires explicitly opted-in disposable local R with jsonlite; never connects SSH"]
    async fn live_r_inspection_bindings_have_no_side_effects() {
        use std::process::Stdio;
        use tokio::io::AsyncWriteExt;
        assert_eq!(
            std::env::var("OMICSOPS_LIVE_R_INSPECTION_DISPOSABLE").as_deref(),
            Ok("1"),
            "explicit disposable R gate required"
        );
        let directory = tempfile::tempdir().unwrap();
        let driver = directory.path().join("inspection.R");
        std::fs::write(&driver, R_DRIVER).unwrap();
        let project = uuid::Uuid::new_v4();
        let session = uuid::Uuid::new_v4();
        let mut child = omicsops_process::background_command("Rscript");
        child
            .arg("--vanilla")
            .arg(&driver)
            .arg(directory.path())
            .arg(project.to_string())
            .arg(session.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = child
            .spawn()
            .expect("disposable Rscript must already exist");
        let mut input = child.stdin.take().unwrap();
        let setup = r#"sentinel_count <- 0L
makeActiveBinding('active', function(value) { sentinel_count <<- sentinel_count + 1L; stop('active forced') }, scope)
delayedAssign('promise_call', { sentinel_count <<- sentinel_count + 1L; stop('promise forced') }, assign.env=scope)
delayedAssign('promise_scalar', 123456789L, assign.env=scope)
plain <- matrix(1:6, nrow=2)
s3 <- structure(1:3, class='hostile')
length.hostile <- function(x) { sentinel_count <<- sentinel_count + 1L; stop('S3 invoked') }
setClass('InspectionOpaque', slots=c(x='numeric'))
s4 <- new('InspectionOpaque', x=1)
"#;
        for request in [
            omicsops_agent::KernelRequest::Execute {
                session_id: session,
                request_id: uuid::Uuid::new_v4(),
                code: setup.into(),
                capture_paths: vec![],
            },
            omicsops_agent::KernelRequest::Inspect {
                session_id: session,
                request_id: uuid::Uuid::new_v4(),
            },
            omicsops_agent::KernelRequest::Execute {
                session_id: session,
                request_id: uuid::Uuid::new_v4(),
                code: "stopifnot(sentinel_count == 0L); cat('SIDE_EFFECT_COUNT_ZERO')".into(),
                capture_paths: vec![],
            },
            omicsops_agent::KernelRequest::Shutdown {
                session_id: session,
                request_id: uuid::Uuid::new_v4(),
            },
        ] {
            let mut line = serde_json::to_vec(&request).unwrap();
            line.push(b'\n');
            input.write_all(&line).await.unwrap();
        }
        drop(input);
        let output = child.wait_with_output().await.unwrap();
        assert!(output.status.success());
        let events: Vec<omicsops_agent::KernelEvent> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(
            !events
                .iter()
                .any(|event| matches!(event.event, omicsops_agent::KernelEventKind::Failed { .. }))
        );
        let objects = events
            .iter()
            .find_map(|event| match &event.event {
                omicsops_agent::KernelEventKind::Inspection { objects, .. } => Some(objects),
                _ => None,
            })
            .expect("inspection terminal required");
        assert!(!objects.iter().any(|object| object.name == "active"));
        for name in ["promise_call", "s3", "s4"] {
            let object = objects.iter().find(|object| object.name == name).unwrap();
            assert_eq!(object.type_label, "opaque");
            assert_eq!(object.element_count, None);
            assert_eq!(object.dimensions, None);
        }
        let plain = objects
            .iter()
            .find(|object| object.name == "plain")
            .unwrap();
        assert_eq!(plain.dimensions, Some(vec![2, 3]));
        assert!(events.iter().any(|event|matches!(&event.event,omicsops_agent::KernelEventKind::Stdout(text) if text.contains("SIDE_EFFECT_COUNT_ZERO"))));
    }
}
