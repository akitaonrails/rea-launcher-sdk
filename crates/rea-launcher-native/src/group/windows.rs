//! Windows: a kill-on-close job joined before the first instruction, the
//! leader's descendants by parent and creation time, its windows.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus};

use rea_launcher::procs;
use windows_sys::Win32::Foundation::{
    CloseHandle, FILETIME, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, RECT,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationAll,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, GetCurrentProcessId, GetProcessTimes, OpenProcess,
    OpenThread, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, ResumeThread, SetProcessAffinityMask, THREAD_SUSPEND_RESUME,
    TerminateProcess,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, GetWindowRect, IsWindowVisible, PostMessageW, SW_HIDE, SetForegroundWindow,
    ShowWindow, WM_CLOSE,
};
use windows_sys::core::BOOL;

use super::{Finished, Pipes, Tag};

pub struct Group {
    child: Child,
    /// The kill-on-close job, when the leader could join it.
    job: Option<Job>,
    warning: Option<String>,
    /// When the leader was created (what its descendants are checked against).
    created: Option<u64>,
}

impl Group {
    pub fn spawn(cmd: &mut Command, tag: &Tag) -> io::Result<Group> {
        cmd.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let mut child = cmd.spawn()?;
        let handle = child.as_raw_handle();
        let mut warning = None;
        if !tag.cpus.is_empty() && !pin(handle, &tag.cpus) {
            warning = Some(format!("the game couldn't be kept on CPUs {:?}", tag.cpus));
        }
        let job = match Job::create().and_then(|j| j.assign(handle).map(|()| j)) {
            Ok(job) => Some(job),
            Err(e) => {
                let ungrouped = format!(
                    "the game's processes couldn't be grouped ({e}): \
                     they may outlive the launcher"
                );
                warning = Some(match warning {
                    Some(w) => format!("{w}; {ungrouped}"),
                    None => ungrouped,
                });
                None
            }
        };
        if !resume(child.id()) {
            let e = io::Error::other("the game started suspended and couldn't be resumed");
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        Ok(Group {
            created: times(handle),
            child,
            job,
            warning,
        })
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn take_pipes(&mut self) -> Pipes {
        Pipes {
            stdin: self.child.stdin.take(),
            stdout: self.child.stdout.take(),
            stderr: self.child.stderr.take(),
        }
    }

    pub fn exited(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub fn wait_exit(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }

    pub fn terminate(&self) {
        for w in windows_of(&snapshot(), &[self.id()]) {
            w.close();
        }
    }

    /// The leader's descendants, from one snapshot (empty when its
    /// creation time is unknown).
    fn descendants(&self, snap: &Snapshot) -> Vec<u32> {
        let at = |pid| {
            if pid == self.id() {
                self.created
            } else {
                created(pid)
            }
        };
        procs::descendants(&snap.processes, self.id(), at)
    }

    /// The session's processes besides the leader: its descendants and
    /// whatever else the job holds.
    fn others(&self) -> Vec<u32> {
        let mut v = self.descendants(&snapshot());
        if let Some(job) = &self.job {
            v.extend(job.processes().into_iter().filter(|p| *p != self.id()));
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    pub fn kill(&self) {
        if let Some(since) = self.created {
            for pid in self.descendants(&snapshot()) {
                end(pid, since);
            }
        }
        if let Some(job) = &self.job {
            // SAFETY: the job handle is open while self lives.
            unsafe { TerminateJobObject(job.0, 1) };
        }
        // SAFETY: the child's handle is open while `child` lives.
        unsafe { TerminateProcess(self.child.as_raw_handle(), 1) };
    }

    pub fn any_left(&self) -> bool {
        !self.others().is_empty() || self.child_running()
    }

    fn child_running(&self) -> bool {
        // SAFETY: as in `kill`; a zero timeout only polls.
        unsafe {
            windows_sys::Win32::System::Threading::WaitForSingleObject(
                self.child.as_raw_handle(),
                0,
            ) != 0
        }
    }

    pub fn windows(&self) -> Vec<Window> {
        let snap = snapshot();
        let mut pids = vec![self.id()];
        pids.extend(self.descendants(&snap));
        windows_of(&snap, &pids)
    }

    pub fn kill_under(&self, dir: &Path) -> usize {
        let Some(since) = self.created else { return 0 };
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let dir = dir.to_string_lossy();
        // SAFETY: no arguments.
        let me = unsafe { GetCurrentProcessId() };
        snapshot()
            .processes
            .iter()
            .map(|&(pid, _)| pid)
            .filter(|&pid| pid != 0 && pid != me)
            .filter(|&pid| image(pid).is_some_and(|exe| procs::under_dir(&exe, &dir)))
            .filter(|&pid| end(pid, since))
            .count()
    }

    pub fn finish(mut self) -> io::Result<Finished> {
        let status = self.child.wait()?;
        let left = self.others();
        self.kill();
        Ok(Finished {
            status,
            leftovers: left.len(),
        })
    }
}

/// A kill-on-close job object, closed (killing what it holds) on drop.
struct Job(HANDLE);

// SAFETY: a job handle may be used from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    fn create() -> io::Result<Job> {
        // SAFETY: plain Win32 calls; the struct is zeroed plain data that
        // outlives the call that reads it.
        unsafe {
            let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if h.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Job(h);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            // the launcher closed: its game goes with it
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }

    fn assign(&self, process: HANDLE) -> io::Result<()> {
        // SAFETY: both handles are open.
        if unsafe { AssignProcessToJobObject(self.0, process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// The processes in the job (up to 256).
    fn processes(&self) -> Vec<u32> {
        #[repr(C)]
        struct List {
            head: JOBOBJECT_BASIC_PROCESS_ID_LIST,
            more: [usize; 255],
        }
        // SAFETY: zeroed plain data, sized as the call is told.
        let mut list: List = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                self.0,
                JobObjectBasicProcessIdList,
                (&raw mut list).cast(),
                size_of::<List>() as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Vec::new();
        }
        let n = (list.head.NumberOfProcessIdsInList as usize).min(256);
        // SAFETY: the ids sit one after another from ProcessIdList, inside `list`.
        let ids = unsafe { std::slice::from_raw_parts(list.head.ProcessIdList.as_ptr(), n) };
        ids.iter().map(|&id| id as u32).collect()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: owned and open.
        unsafe { CloseHandle(self.0) };
    }
}

/// Resumes every thread of the suspended process `pid` (it has one).
fn resume(pid: u32) -> bool {
    let mut resumed = false;
    for (tid, _) in snapshot().threads.iter().filter(|(_, owner)| *owner == pid) {
        // SAFETY: a handle we open, use and close here.
        unsafe {
            let h = OpenThread(THREAD_SUSPEND_RESUME, 0, *tid);
            if !h.is_null() {
                resumed |= ResumeThread(h) != u32::MAX;
                CloseHandle(h);
            }
        }
    }
    resumed
}

/// Keeps a process (and what it starts after this) on `cpus`.
fn pin(process: HANDLE, cpus: &[u32]) -> bool {
    let mask = cpus
        .iter()
        .filter(|&&c| c < usize::BITS)
        .fold(0usize, |m, &c| m | (1 << c));
    // SAFETY: the handle is open.
    mask != 0 && unsafe { SetProcessAffinityMask(process, mask) } != 0
}

/// Ends process `pid` if it was created at or after `since` (checked on
/// the handle that ends it, so a reused id is never hit); whether it did.
fn end(pid: u32, since: u64) -> bool {
    // SAFETY: a handle we open, use and close here.
    unsafe {
        let h = OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        );
        if h.is_null() {
            return false;
        }
        let ok = times(h).is_some_and(|c| c >= since) && TerminateProcess(h, 1) != 0;
        CloseHandle(h);
        ok
    }
}

/// The system's processes (with their parents) and threads (with their
/// processes), from one snapshot.
struct Snapshot {
    processes: Vec<(u32, u32)>,
    threads: Vec<(u32, u32)>,
}

fn snapshot() -> Snapshot {
    let mut s = Snapshot {
        processes: Vec::new(),
        threads: Vec::new(),
    };
    // SAFETY: the entries are zeroed plain data with their sizes set; the
    // snapshot handle is closed below.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS | TH32CS_SNAPTHREAD, 0);
        if snap == INVALID_HANDLE_VALUE {
            return s;
        }
        let mut p: PROCESSENTRY32W = std::mem::zeroed();
        p.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut p) != 0;
        while more {
            s.processes.push((p.th32ProcessID, p.th32ParentProcessID));
            more = Process32NextW(snap, &mut p) != 0;
        }
        let mut t: THREADENTRY32 = std::mem::zeroed();
        t.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut more = Thread32First(snap, &mut t) != 0;
        while more {
            s.threads.push((t.th32ThreadID, t.th32OwnerProcessID));
            more = Thread32Next(snap, &mut t) != 0;
        }
        CloseHandle(snap);
    }
    s
}

/// When a process was created (100 ns since 1601), if it can be read.
fn created(pid: u32) -> Option<u64> {
    // SAFETY: a handle we open, use and close here.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let at = times(h);
        CloseHandle(h);
        at
    }
}

/// When the process behind the open handle `h` was created.
fn times(h: HANDLE) -> Option<u64> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut c, mut e, mut k, mut u) = (zero, zero, zero, zero);
    // SAFETY: plain out-parameters; the caller keeps `h` open.
    let ok = unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) } != 0;
    ok.then_some(((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64)
}

/// A process's program, as a full Win32 path.
fn image(pid: u32) -> Option<String> {
    // SAFETY: a handle we open, use and close here; the buffer outlives the call.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// A top-level window of one of the session's processes.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    hwnd: HWND,
    pub pid: u32,
    pub visible: bool,
    /// Big enough to be a game's window (not a tooltip or an IME box).
    pub big: bool,
}

// SAFETY: a window handle is a plain identifier, usable from any thread.
unsafe impl Send for Window {}

impl Window {
    /// Asks it to close, as its close button does.
    pub fn close(&self) {
        // SAFETY: posting to a window that may be gone is harmless.
        unsafe { PostMessageW(self.hwnd, WM_CLOSE, 0, 0) };
    }

    pub fn hide(&self) {
        // SAFETY: as in `close`.
        unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }

    /// Brings it to the front, with the keyboard's focus.
    pub fn focus(&self) {
        // SAFETY: as in `close`.
        unsafe { SetForegroundWindow(self.hwnd) };
    }
}

/// The top-level windows of the processes `pids`, hidden ones too, found
/// through their threads (never by title).
fn windows_of(snap: &Snapshot, pids: &[u32]) -> Vec<Window> {
    unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the Vec passed below, alive for the call.
        let out = unsafe { &mut *(out as *mut Vec<HWND>) };
        out.push(hwnd);
        1
    }
    let mut out = Vec::new();
    for &(tid, pid) in snap.threads.iter().filter(|(_, pid)| pids.contains(pid)) {
        let mut hwnds: Vec<HWND> = Vec::new();
        // SAFETY: the callback only pushes into `hwnds`, alive for the call.
        unsafe { EnumThreadWindows(tid, Some(each), &mut hwnds as *mut Vec<HWND> as LPARAM) };
        for hwnd in hwnds {
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: plain out-parameter; a gone window leaves it zero.
            unsafe { GetWindowRect(hwnd, &mut r) };
            out.push(Window {
                hwnd,
                pid,
                // SAFETY: as above.
                visible: unsafe { IsWindowVisible(hwnd) } != 0,
                big: r.right - r.left >= 200 && r.bottom - r.top >= 150,
            });
        }
    }
    out
}

/// The CPUs as Windows describes them (`GetLogicalProcessorInformationEx`,
/// `RelationAll`: the raw records, for the launcher to read).
pub fn logical_processor_information() -> Option<Vec<u8>> {
    let mut len = 0u32;
    // SAFETY: a null buffer asks for the size.
    unsafe { GetLogicalProcessorInformationEx(RelationAll, std::ptr::null_mut(), &mut len) };
    if len == 0 {
        return None;
    }
    // u64 elements: the records hold 8-byte masks
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: the buffer holds `len` bytes.
    let ok =
        unsafe { GetLogicalProcessorInformationEx(RelationAll, buf.as_mut_ptr().cast(), &mut len) };
    if ok == 0 {
        return None;
    }
    // SAFETY: the call wrote `len` bytes into the buffer.
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), len as usize) };
    Some(bytes.to_vec())
}
