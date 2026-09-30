use omicsops_process::managed_child::{BackgroundLaunchSpec, ManagedBackgroundChild};
#[cfg(windows)]
use std::{collections::BTreeMap, ffi::OsString};
#[cfg(windows)]
use tokio::io::{AsyncBufReadExt, BufReader};

#[test]
fn managed_child_helper() {
    #[cfg(windows)]
    {
        let Some(mode) = std::env::var_os("OMICSOPS_MANAGED_HELPER") else {
            return;
        };
        unsafe {
            assert!(windows_sys::Win32::System::Console::GetConsoleWindow().is_null());
        }
        if mode == "tree" || mode == "exit_parent" {
            use std::os::windows::process::CommandExt;
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "managed_child_helper", "--nocapture"])
                .env("OMICSOPS_MANAGED_HELPER", "leaf")
                .creation_flags(0x08000000)
                .spawn()
                .unwrap();
            println!("CHILD:{}", child.id());
            std::mem::forget(child);
            if mode == "exit_parent" {
                return;
            }
        }
        println!("PID:{}", std::process::id());
        loop {
            std::thread::park_timeout(std::time::Duration::from_secs(1));
        }
    }
}
#[cfg(windows)]
fn spec(mode: &str) -> BackgroundLaunchSpec {
    let mut env: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    env.insert("OMICSOPS_MANAGED_HELPER".into(), mode.into());
    BackgroundLaunchSpec {
        program: std::env::current_exe().unwrap(),
        args: ["--exact", "managed_child_helper", "--nocapture"]
            .map(OsString::from)
            .to_vec(),
        cwd: std::env::current_dir().unwrap(),
        env,
    }
}
#[cfg(windows)]
async fn descendant(
    child: &mut ManagedBackgroundChild,
) -> (
    u32,
    BufReader<omicsops_process::managed_child::ProcessOutput>,
) {
    let mut lines = BufReader::new(child.take_stdout().unwrap()).lines();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some(pid) = line.strip_prefix("CHILD:") {
                return (pid.parse().unwrap(), lines.into_inner());
            }
        }
        panic!("helper did not report descendant")
    })
    .await
    .unwrap()
}
#[cfg(windows)]
unsafe fn owned_process(pid: u32) -> std::os::windows::io::OwnedHandle {
    use std::os::windows::io::FromRawHandle;
    let handle = unsafe {
        windows_sys::Win32::System::Threading::OpenProcess(
            windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION
                | windows_sys::Win32::System::Threading::PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    assert!(!handle.is_null());
    unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) }
}
#[cfg(windows)]
fn exited(handle: &std::os::windows::io::OwnedHandle) {
    use std::os::windows::io::AsRawHandle;
    assert_eq!(
        unsafe {
            windows_sys::Win32::System::Threading::WaitForSingleObject(handle.as_raw_handle(), 5000)
        },
        windows_sys::Win32::Foundation::WAIT_OBJECT_0
    );
}
#[cfg(windows)]
#[tokio::test]
async fn managed_child_terminates_entire_tree_without_console() {
    let mut child = ManagedBackgroundChild::spawn(spec("tree")).unwrap();
    let (pid, _reader) = descendant(&mut child).await;
    let process = unsafe { owned_process(pid) };
    let first = child.terminate_and_wait().await.unwrap();
    let second = child.terminate_and_wait().await.unwrap();
    assert_eq!(first, second);
    exited(&process);
}
#[cfg(windows)]
#[tokio::test]
async fn managed_child_drop_kills_descendants_after_parent_exits() {
    let mut child = ManagedBackgroundChild::spawn(spec("exit_parent")).unwrap();
    let (pid, _reader) = descendant(&mut child).await;
    let process = unsafe { owned_process(pid) };
    assert!(child.wait().await.unwrap().success());
    drop(child);
    exited(&process);
}
#[cfg(windows)]
#[tokio::test]
async fn managed_child_drop_recovers_cancelled_wait() {
    let mut child = ManagedBackgroundChild::spawn(spec("tree")).unwrap();
    let (pid, _reader) = descendant(&mut child).await;
    let process = unsafe { owned_process(pid) };
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), child.wait())
            .await
            .is_err()
    );
    drop(child);
    exited(&process);
}
#[cfg(not(windows))]
#[test]
fn managed_child_reports_unsupported_on_non_windows() {
    let spec = BackgroundLaunchSpec {
        program: "unused".into(),
        args: vec![],
        cwd: ".".into(),
        env: Default::default(),
    };
    assert_eq!(
        ManagedBackgroundChild::spawn(spec).err().unwrap().kind(),
        std::io::ErrorKind::Unsupported
    );
}
