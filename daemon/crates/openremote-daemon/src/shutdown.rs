//! How the daemon ends, and what ends with it.
//!
//! The desktop app runs the daemon as a sidecar whose stdin is a pipe
//! from the app: end of file there means the app is gone, and the daemon
//! leaves too. A machine's service leaves on SIGTERM. Either way it
//! sweeps its chats first, so harness children don't outlive it, and then
//! exits.
//!
//! The way out must not depend on anything the departed app took with it.
//! The daemon's stderr is one of those things: the app holds its reading
//! end, so once the app is gone every log line fails, and `eprintln!`
//! turns that failure into a panic. A goodbye printed that way panics the
//! task that prints it before the sweep ever runs, and the daemon lives on
//! after its window closed, harness children and all. So the way out logs
//! with [`note`], which never panics, and runs the sweep where a panic
//! inside it can't stop the exit.
//!
//! Some exits skip the sweep: a crash, the app restarting its daemon (it
//! kills the old one), Task Manager. On Windows a job object covers those:
//! every process the daemon starts belongs to the job, and the job ends
//! them all the moment the daemon itself is gone. The daemon also keeps
//! its own pipes to the app out of its children, so no child can hold the
//! app's reading ends open after the daemon is gone. And the sidecar
//! watches the app's process as well as its stdin: a second witness of the
//! app's death that no stray pipe end can fool.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::app::App;

/// The bound on the whole exit sweep. Each chat's drain has a bound of its
/// own; this caps them together, so a daemon with many chats still leaves
/// promptly. Whatever it cuts short ends with the job on Windows, and the
/// store's reconcile heals the record at the next start.
const SWEEP_BOUND: Duration = Duration::from_secs(15);

/// How often a sidecar on Unix checks that the app that started it is
/// still its parent.
#[cfg(unix)]
const PARENT_POLL: Duration = Duration::from_secs(2);

/// One line on stderr that can't panic: when nobody reads the other end
/// any more, the line is dropped.
pub fn note(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// Leave the process: the browser's waiters are told, every live chat is
/// swept (bounded), and the daemon exits 0. The sweep runs as a task of
/// its own, so a panic inside it ends that task, not the exit. Two
/// witnesses can arrive at once (stdin and the parent watch): the first
/// leaves, the second waits for it.
pub async fn leave(app: Arc<App>, why: &str, data_dir: PathBuf) {
    static LEAVING: AtomicBool = AtomicBool::new(false);
    if LEAVING.swap(true, Ordering::SeqCst) {
        std::future::pending::<()>().await;
    }
    note(&format!("openremote-daemon: {why}, exiting"));
    let sweep = tokio::spawn(async move {
        app.browser.disconnect();
        app.supervisor.shutdown_all().await;
        // Plugin programs go with the daemon.
        app.plugin_runtime.shutdown().await;
    });
    let _ = tokio::time::timeout(SWEEP_BOUND, sweep).await;
    // The published port is no longer true, and the next start publishes
    // its own. Left behind it would point a chat's read-a-chat server at a
    // socket nobody holds.
    crate::mcp_chat::remove_port_file(&data_dir);
    std::process::exit(0);
}

/// Tie every process this daemon starts to the daemon's own life. On
/// Windows the daemon joins a job object that ends all of its members when
/// its last handle closes - the daemon's own, held until the process is
/// gone, however it goes. Elsewhere the exit sweep is what ends them.
pub fn hold_children() {
    #[cfg(windows)]
    if let Err(err) = windows::join_job() {
        note(&format!(
            "openremote-daemon: no job object for its processes ({err}); only a clean exit ends them"
        ));
    }
}

/// Keep this daemon's stdin, stdout and stderr (its pipes to the app) out
/// of the processes it starts. Windows hands a child every inheritable
/// handle its parent holds, not just the three it is given; a harness child
/// holding the daemon's stdout would keep the app from ever seeing the
/// daemon go. A child still gets the stdio it is given: those are
/// duplicated for it at spawn.
pub fn keep_pipes_private() {
    #[cfg(windows)]
    windows::keep_std_handles();
}

/// Start a program that should outlive the daemon: a browser opened for the
/// user belongs to the user. On Windows it leaves the daemon's job; where
/// an outer job forbids that, it starts inside, as any other child.
pub fn spawn_outside(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        if let Ok(child) = command.creation_flags(CREATE_BREAKAWAY_FROM_JOB).spawn() {
            return Ok(child);
        }
        command.creation_flags(0);
    }
    command.spawn()
}

/// The sidecar's second witness: leave when the process that started it
/// (the app) is gone, whatever its pipes say. Nothing is watched when the
/// parent can't be told apart from a stranger (gone already, or not one
/// this process may open): stdin still decides then.
pub fn watch_parent(app: Arc<App>, data_dir: PathBuf) {
    #[cfg(windows)]
    if let Some(parent) = windows::open_parent() {
        let runtime = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            if parent.wait() {
                runtime.spawn(leave(app, "the app that started it is gone", data_dir));
            }
        });
    }
    #[cfg(unix)]
    {
        let first = std::os::unix::process::parent_id();
        // Started by init itself: there is no app to outlive.
        if first == 1 {
            return;
        }
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(PARENT_POLL).await;
                if std::os::unix::process::parent_id() != first {
                    leave(app, "the app that started it is gone", data_dir).await;
                    return;
                }
            }
        });
    }
}

/// The Win32 calls behind the job, the handles and the parent watch.
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    use windows_sys::Win32::Foundation::{
        CloseHandle, FILETIME, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
        SetHandleInformation, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, INFINITE, OpenProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    /// Put this process in a new job that ends its members when its last
    /// handle closes. Children join the job as they start.
    pub fn join_job() -> std::io::Result<()> {
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // Breakaway stays allowed for what must outlive the daemon: a
        // browser it opens for the user (spawn_outside).
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        // SAFETY: the job handle is checked before use and closed on the
        // failure paths; the limits outlive the call that reads them.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            );
            if set == 0 || AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
                let err = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(err);
            }
            // The handle is never closed: the job's last handle goes with
            // this process, which is the moment its members should end.
        }
        Ok(())
    }

    /// Clear the inherit flag on this process's stdin, stdout and stderr.
    pub fn keep_std_handles() {
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: GetStdHandle lends the process's own handle; only its
            // inherit flag changes, which no reader or writer notices.
            unsafe {
                let handle = GetStdHandle(which);
                if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
    }

    /// The process that started this one, open to wait on.
    pub struct Parent(OwnedHandle);

    impl Parent {
        /// Blocks until the process is gone; false if the wait itself failed.
        pub fn wait(&self) -> bool {
            // SAFETY: the handle is open for as long as self lives.
            unsafe { WaitForSingleObject(self.0.as_raw_handle(), INFINITE) == WAIT_OBJECT_0 }
        }
    }

    /// The parent, or None when it can't be told apart from a stranger: not
    /// found, not openable, or started after this process (its id was
    /// reused, so the real parent is gone already and stdin will say so).
    pub fn open_parent() -> Option<Parent> {
        let pid = parent_id()?;
        // SAFETY: the handle is checked, then owned (and closed) by Parent.
        let parent = unsafe {
            let handle = OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            );
            if handle.is_null() {
                return None;
            }
            Parent(OwnedHandle::from_raw_handle(handle))
        };
        // SAFETY: the pseudo handle for this process needs no closing.
        let ours = created(unsafe { GetCurrentProcess() })?;
        let theirs = created(parent.0.as_raw_handle())?;
        (theirs <= ours).then_some(parent)
    }

    /// When a process started, in FILETIME ticks.
    fn created(process: HANDLE) -> Option<u64> {
        let mut times = [FILETIME::default(); 4];
        let [start, end, kernel, user] = &mut times;
        // SAFETY: four FILETIMEs owned here, one live process handle.
        let ok = unsafe { GetProcessTimes(process, start, end, kernel, user) };
        let start = times[0];
        (ok != 0).then(|| (u64::from(start.dwHighDateTime) << 32) | u64::from(start.dwLowDateTime))
    }

    /// This process's parent's id, from a snapshot of the process table.
    fn parent_id() -> Option<u32> {
        // SAFETY: the snapshot is closed on the one path that opened it;
        // the entry carries the size the API checks.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            let me = GetCurrentProcessId();
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut found = None;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                if entry.th32ProcessID == me {
                    found = Some(entry.th32ParentProcessID);
                    break;
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            found
        }
    }
}
