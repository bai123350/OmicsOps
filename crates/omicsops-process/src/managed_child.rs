//! Windows job ownership. The initial thread stays suspended until assignment succeeds.
use std::{collections::BTreeMap, ffi::OsString, io, path::PathBuf, process::ExitStatus};
pub type ProcessInput = Box<dyn tokio::io::AsyncWrite + Unpin + Send>;
pub type ProcessOutput = Box<dyn tokio::io::AsyncRead + Unpin + Send>;
pub struct BackgroundLaunchSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: BTreeMap<OsString, OsString>,
}
pub struct ManagedBackgroundChild {
    #[cfg(windows)]
    inner: windows::Child,
}
impl ManagedBackgroundChild {
    pub fn spawn(spec: BackgroundLaunchSpec) -> io::Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: windows::Child::spawn(spec)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = spec;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "managed subscription children require Windows",
            ))
        }
    }
    pub fn take_stdin(&mut self) -> Option<ProcessInput> {
        #[cfg(windows)]
        {
            self.inner.stdin.take()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
    pub fn take_stdout(&mut self) -> Option<ProcessOutput> {
        #[cfg(windows)]
        {
            self.inner.stdout.take()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
    pub fn take_stderr(&mut self) -> Option<ProcessOutput> {
        #[cfg(windows)]
        {
            self.inner.stderr.take()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        #[cfg(windows)]
        {
            self.inner.wait().await
        }
        #[cfg(not(windows))]
        {
            Err(io::ErrorKind::Unsupported.into())
        }
    }
    pub async fn terminate_and_wait(&mut self) -> io::Result<ExitStatus> {
        #[cfg(windows)]
        {
            self.inner.job.take();
            self.inner.wait().await
        }
        #[cfg(not(windows))]
        {
            Err(io::ErrorKind::Unsupported.into())
        }
    }
}
#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        fs::File,
        mem::size_of,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
            process::ExitStatusExt,
        },
        sync::Arc,
    };
    use windows_sys::Win32::{
        Foundation::*,
        Security::SECURITY_ATTRIBUTES,
        System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
    };
    pub(super) struct Child {
        pub job: Option<OwnedHandle>,
        process: Arc<OwnedHandle>,
        pub stdin: Option<ProcessInput>,
        pub stdout: Option<ProcessOutput>,
        pub stderr: Option<ProcessOutput>,
        exit: Option<ExitStatus>,
    }
    fn wide(value: &std::ffi::OsStr) -> io::Result<Vec<u16>> {
        let mut bytes: Vec<_> = value.encode_wide().collect();
        if bytes.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "NUL in process argument",
            ));
        }
        bytes.push(0);
        Ok(bytes)
    }
    fn quote(value: &std::ffi::OsStr) -> io::Result<Vec<u16>> {
        let value = wide(value)?;
        let mut result = vec![b'"' as u16];
        let mut slashes = 0;
        for c in &value[..value.len() - 1] {
            if *c == b'\\' as u16 {
                slashes += 1;
                continue;
            }
            if *c == b'"' as u16 {
                result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
            } else {
                result.extend(std::iter::repeat_n(b'\\' as u16, slashes));
            }
            slashes = 0;
            result.push(*c);
        }
        result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        result.push(b'"' as u16);
        Ok(result)
    }
    fn pipe(parent_reads: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
        let mut read = std::ptr::null_mut();
        let mut write = std::ptr::null_mut();
        let attrs = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        if unsafe { CreatePipe(&mut read, &mut write, &attrs, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let read = unsafe { OwnedHandle::from_raw_handle(read) };
        let write = unsafe { OwnedHandle::from_raw_handle(write) };
        let (parent, child) = if parent_reads {
            (read, write)
        } else {
            (write, read)
        };
        if unsafe { SetHandleInformation(parent.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((parent, child))
    }
    struct Attributes {
        storage: Vec<usize>,
    }
    impl Attributes {
        fn new(handles: &[HANDLE]) -> io::Result<Self> {
            let mut bytes = 0;
            unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut bytes) };
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let list = storage.as_mut_ptr().cast();
            if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut bytes) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let attributes = Self { storage };
            if unsafe {
                UpdateProcThreadAttribute(
                    list,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    handles.as_ptr().cast(),
                    std::mem::size_of_val(handles),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(attributes)
        }
        fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
            self.storage.as_mut_ptr().cast()
        }
    }
    impl Drop for Attributes {
        fn drop(&mut self) {
            unsafe { DeleteProcThreadAttributeList(self.pointer()) };
        }
    }
    impl Child {
        pub fn spawn(spec: BackgroundLaunchSpec) -> io::Result<Self> {
            if !spec.program.is_absolute() || !spec.cwd.is_absolute() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "process paths must be absolute",
                ));
            }
            let application = wide(spec.program.as_os_str())?;
            let cwd = wide(spec.cwd.as_os_str())?;
            let mut command = quote(spec.program.as_os_str())?;
            for arg in &spec.args {
                command.push(b' ' as u16);
                command.extend(quote(arg)?);
            }
            command.push(0);
            if command.len() > 32767 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "process command too long",
                ));
            }
            let mut environment = vec![];
            let mut entries: Vec<_> = spec.env.into_iter().collect();
            entries.sort_by_key(|(key, _)| key.to_string_lossy().to_uppercase());
            for (key, value) in entries {
                let key = wide(&key)?;
                if key.len() == 1 || key.contains(&(b'=' as u16)) {
                    return Err(io::ErrorKind::InvalidInput.into());
                }
                environment.extend(&key[..key.len() - 1]);
                environment.push(b'=' as u16);
                environment.extend(wide(&value)?);
            }
            environment.push(0);
            if environment.len() == 1 {
                environment.push(0);
            }
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = unsafe { OwnedHandle::from_raw_handle(job) };
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if unsafe {
                SetInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let (stdin, child_stdin) = pipe(false)?;
            let (stdout, child_stdout) = pipe(true)?;
            let (stderr, child_stderr) = pipe(true)?;
            let handles = [
                child_stdin.as_raw_handle(),
                child_stdout.as_raw_handle(),
                child_stderr.as_raw_handle(),
            ];
            let mut attributes = Attributes::new(&handles)?;
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = handles[0];
            startup.StartupInfo.hStdOutput = handles[1];
            startup.StartupInfo.hStdError = handles[2];
            startup.lpAttributeList = attributes.pointer();
            let mut info = PROCESS_INFORMATION::default();
            if unsafe {
                CreateProcessW(
                    application.as_ptr(),
                    command.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                    CREATE_SUSPENDED
                        | CREATE_NO_WINDOW
                        | CREATE_UNICODE_ENVIRONMENT
                        | EXTENDED_STARTUPINFO_PRESENT,
                    environment.as_ptr().cast(),
                    cwd.as_ptr(),
                    &startup.StartupInfo,
                    &mut info,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
            let thread = unsafe { OwnedHandle::from_raw_handle(info.hThread) };
            if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) }
                == 0
            {
                let error = io::Error::last_os_error();
                unsafe {
                    TerminateProcess(process.as_raw_handle(), 1);
                    WaitForSingleObject(process.as_raw_handle(), INFINITE);
                }
                return Err(error);
            }
            if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                let error = io::Error::last_os_error();
                drop(job);
                unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) };
                return Err(error);
            }
            drop((child_stdin, child_stdout, child_stderr));
            Ok(Self {
                job: Some(job),
                process: Arc::new(process),
                stdin: Some(Box::new(tokio::fs::File::from_std(File::from(stdin)))),
                stdout: Some(Box::new(tokio::fs::File::from_std(File::from(stdout)))),
                stderr: Some(Box::new(tokio::fs::File::from_std(File::from(stderr)))),
                exit: None,
            })
        }
        pub async fn wait(&mut self) -> io::Result<ExitStatus> {
            if let Some(exit) = self.exit {
                return Ok(exit);
            }
            let process = self.process.clone();
            // The owned kernel handle, rather than a PID, survives cancellation without reuse races.
            let exit = tokio::task::spawn_blocking(move || {
                if unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) }
                    != WAIT_OBJECT_0
                {
                    return Err(io::Error::last_os_error());
                }
                let mut code = 0;
                if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(ExitStatus::from_raw(code))
            })
            .await
            .map_err(|_| io::Error::other("process wait failed"))??;
            self.exit = Some(exit);
            Ok(exit)
        }
    }
    impl Drop for Child {
        fn drop(&mut self) {
            self.job.take();
            if self.exit.is_none() {
                let process = self.process.clone();
                std::thread::spawn(move || {
                    unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) };
                    drop(process);
                });
            }
        }
    }
    #[test]
    fn windows_argv_preserves_empty_quotes_and_trailing_slash() {
        assert_eq!(quote(std::ffi::OsStr::new("")).unwrap(), [34, 34]);
        assert_eq!(
            String::from_utf16(&quote(std::ffi::OsStr::new("x\\")).unwrap()).unwrap(),
            "\"x\\\\\""
        );
        assert_eq!(
            String::from_utf16(&quote(std::ffi::OsStr::new("a\"b")).unwrap()).unwrap(),
            "\"a\\\"b\""
        );
    }
}
