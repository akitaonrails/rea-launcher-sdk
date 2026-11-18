//! Windows: a kill-on-close job joined before the first instruction, the
//! leader's descendants by parent and creation time, its windows.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus};

use rea_launcher::procs;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_MORE_DATA, FILETIME, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, RECT,
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
    EnumThreadWindows, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    SW_HIDE, SetForegroundWindow, ShowWindow, WM_CLOSE,
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
    /// `finish` ran: nothing is left to end on drop.
    finished: bool,
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
            finished: false,
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

    /// `WM_CLOSE` to the leader's windows, hidden ones too, and to the
    /// visible windows of the session's other processes (a loader whose
    /// game window is its child's).
    pub fn terminate(&self) {
        let snap = snapshot();
        for w in windows_of(&snap, &self.processes(&snap)) {
            if w.pid == self.id() || w.visible {
                w.close();
            }
        }
    }

    /// The session's processes with their creation times: the leader, its
    /// descendants, and whatever else the job holds.
    fn processes(&self, snap: &Snapshot) -> Vec<(u32, u64)> {
        let mut v: Vec<(u32, u64)> = self.created.map(|at| (self.id(), at)).into_iter().collect();
        v.extend(self.descendants(snap));
        for pid in self.job.as_ref().map(Job::processes).unwrap_or_default() {
            if !v.iter().any(|&(p, _)| p == pid)
                && let Some(at) = created(pid)
            {
                v.push((pid, at));
            }
        }
        v
    }

    /// The leader's descendants with their creation times, from one
    /// snapshot (empty when the leader's creation time is unknown).
    fn descendants(&self, snap: &Snapshot) -> Vec<(u32, u64)> {
        let at = |pid| {
            if pid == self.id() {
                self.created
            } else {
                created(pid)
            }
        };
        procs::descendants_at(&snap.processes, self.id(), at)
    }

    /// The session's processes besides the leader: its descendants and
    /// whatever else the job holds.
    fn others(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.descendants(&snapshot()).iter().map(|d| d.0).collect();
        if let Some(job) = &self.job {
            v.extend(job.processes().into_iter().filter(|p| *p != self.id()));
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    pub fn kill(&self) {
        for (pid, born) in self.descendants(&snapshot()) {
            end(pid, born);
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
        windows_of(&snap, &self.processes(&snap))
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
            .filter(|&pid| created(pid).is_some_and(|born| born >= since && end(pid, born)))
            .count()
    }

    pub fn finish(mut self) -> io::Result<Finished> {
        let status = self.child.wait()?;
        let left = self.others();
        self.kill();
        self.finished = true;
        Ok(Finished {
            status,
            leftovers: left.len(),
        })
    }
}

/// A group dropped without `finish` (an early return, a panic) takes its
/// session along, the job or not.
impl Drop for Group {
    fn drop(&mut self) {
        if !self.finished {
            self.kill();
            let _ = self.child.wait();
        }
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

    /// The processes in the job, however many.
    fn processes(&self) -> Vec<u32> {
        id_list(|buf| {
            // SAFETY: the buffer is word-aligned and sized as the call is told.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.0,
                    JobObjectBasicProcessIdList,
                    buf.as_mut_ptr().cast(),
                    size_of_val(buf) as u32,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        })
    }
}

/// Words before `ProcessIdList` in `JOBOBJECT_BASIC_PROCESS_ID_LIST`.
const HEADER: usize =
    std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();

/// A job's process ids through `query` (`QueryInformationJobObject`'s id
/// list into a buffer of words), with a bigger buffer while the job holds
/// more than fit; empty on any other error.
fn id_list(mut query: impl FnMut(&mut [usize]) -> io::Result<()>) -> Vec<u32> {
    let mut room = 256;
    for _ in 0..8 {
        let mut buf = vec![0usize; HEADER + room];
        let result = query(&mut buf);
        // SAFETY: the header's two u32s at the start of the word buffer.
        let (assigned, listed) = unsafe {
            let head = buf.as_ptr().cast::<u32>();
            (head.read() as usize, head.add(1).read() as usize)
        };
        match result {
            Ok(()) => {
                return buf[HEADER..HEADER + listed.min(room)]
                    .iter()
                    .map(|&id| id as u32)
                    .collect();
            }
            Err(e) if e.raw_os_error() == Some(ERROR_MORE_DATA as i32) => {
                // processes may join meanwhile: some room to spare
                room = room.max(assigned) + 64;
            }
            Err(_) => break,
        }
    }
    Vec::new()
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

/// Ends process `pid` if it is the one created at `born` (checked on the
/// handle that ends it, so a reused id is never hit); whether it did.
fn end(pid: u32, born: u64) -> bool {
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
        let ok = times(h) == Some(born) && TerminateProcess(h, 1) != 0;
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
    /// When `pid` was created: a reused id is never taken for it.
    born: u64,
    pub visible: bool,
    /// Big enough to be a game's window (not a tooltip or an IME box).
    pub big: bool,
}

// SAFETY: a window handle is a plain identifier, usable from any thread.
unsafe impl Send for Window {}

/// Each call first checks that the window still belongs to `pid`, the
/// process created at that same time: a window handle (and a process id)
/// is reused once its window (process) is gone, and a stale one must never
/// close, hide or raise another program's window.
impl Window {
    /// Asks it to close, as its close button does.
    pub fn close(&self) {
        if self.owned() {
            // SAFETY: a plain call on a window handle.
            unsafe { PostMessageW(self.hwnd, WM_CLOSE, 0, 0) };
        }
    }

    pub fn hide(&self) {
        if self.owned() {
            // SAFETY: as in `close`.
            unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
    }

    /// Brings it to the front, with the keyboard's focus.
    pub fn focus(&self) {
        if self.owned() {
            // SAFETY: as in `close`.
            unsafe { SetForegroundWindow(self.hwnd) };
        }
    }

    fn owned(&self) -> bool {
        let mut pid = 0u32;
        // SAFETY: plain out-parameter; a gone window returns 0.
        let owner = unsafe { GetWindowThreadProcessId(self.hwnd, &mut pid) };
        owner != 0 && pid == self.pid && created(pid) == Some(self.born)
    }
}

/// The top-level windows of the processes `pids` (with their creation
/// times), hidden ones too, found through their threads (never by title).
fn windows_of(snap: &Snapshot, pids: &[(u32, u64)]) -> Vec<Window> {
    unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> BOOL {
        // SAFETY: `out` is the Vec passed below, alive for the call.
        let out = unsafe { &mut *(out as *mut Vec<HWND>) };
        out.push(hwnd);
        1
    }
    let mut out = Vec::new();
    for &(tid, owner) in &snap.threads {
        let Some(&(pid, born)) = pids.iter().find(|p| p.0 == owner) else {
            continue;
        };
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
                born,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, MSG, PM_REMOVE, PeekMessageW,
        RegisterClassW, WNDCLASSW,
    };

    /// A process that waits for its input (held open by the returned child).
    fn waiter() -> Child {
        Command::new("cmd")
            .args(["/c", "pause"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn running(child: &mut Child) -> bool {
        child.try_wait().unwrap().is_none()
    }

    /// A job holding `n` processes, answered as Windows does: the header
    /// always, the ids when they fit, `ERROR_MORE_DATA` when they don't.
    fn job_of(n: usize) -> impl FnMut(&mut [usize]) -> io::Result<()> {
        move |buf| {
            let room = buf.len() - HEADER;
            let listed = n.min(room);
            // SAFETY: the header's two u32s at the buffer's start
            let head = buf.as_mut_ptr().cast::<u32>();
            unsafe { (head.write(n as u32), head.add(1).write(listed as u32)) };
            for (i, id) in buf[HEADER..HEADER + listed].iter_mut().enumerate() {
                *id = 1000 + i;
            }
            if listed < n {
                Err(io::Error::from_raw_os_error(ERROR_MORE_DATA as i32))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn every_process_of_a_big_job_is_listed() {
        for n in [0, 3, 256, 300, 5000] {
            let ids = id_list(job_of(n));
            assert_eq!(ids.len(), n);
            assert_eq!(
                ids.last().copied(),
                n.checked_sub(1).map(|i| 1000 + i as u32)
            );
        }
        // another error: nothing listed
        assert!(id_list(|_| Err(io::Error::from_raw_os_error(5))).is_empty());
    }

    #[test]
    fn a_process_is_ended_only_at_its_exact_creation_time() {
        let mut c = waiter();
        let born = created(c.id()).unwrap();
        // a process created later took a descendant's id: never ended
        assert!(!end(c.id(), born - 1));
        assert!(!end(c.id(), born + 1));
        assert!(running(&mut c));
        assert!(end(c.id(), born));
        c.wait().unwrap();
    }

    #[test]
    fn a_dropped_group_without_a_job_ends_its_game() {
        let mut probe = waiter();
        let pid = probe.id();
        let mut child = waiter();
        // held here: the game must not end because its input closed
        let _input = child.stdin.take();
        let g = Group {
            created: created(child.id()),
            child,
            job: None,
            warning: None,
            finished: false,
        };
        let leader = g.id();
        drop(g);
        let started = std::time::Instant::now();
        while created(leader).is_some() && started.elapsed().as_secs() < 10 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            created(leader).is_none(),
            "the game outlived its dropped group"
        );
        assert!(running(&mut probe) && probe.id() == pid);
        probe.kill().unwrap();
        probe.wait().unwrap();
    }

    unsafe extern "system" fn plain(h: HWND, msg: u32, w: usize, l: isize) -> isize {
        unsafe { DefWindowProcW(h, msg, w, l) }
    }

    /// Whether a `WM_CLOSE` waits in this thread's queue (taken out).
    fn closed() -> bool {
        // SAFETY: zeroed plain data the call fills in.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            PeekMessageW(
                &mut msg,
                std::ptr::null_mut(),
                WM_CLOSE,
                WM_CLOSE,
                PM_REMOVE,
            ) != 0
        }
    }

    #[test]
    fn a_window_is_closed_only_while_its_process_owns_it() {
        let class: Vec<u16> = "rea-group-unit\0".encode_utf16().collect();
        // SAFETY: a hidden window of this thread, destroyed below.
        let hwnd = unsafe {
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(plain);
            wc.lpszClassName = class.as_ptr();
            RegisterClassW(&wc);
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                10,
                10,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        let me = unsafe { GetCurrentProcessId() };
        let born = created(me).unwrap();
        let window = |pid, born| Window {
            hwnd,
            pid,
            born,
            visible: false,
            big: false,
        };
        // the handle now names another process's window: left alone
        window(me + 4, born).close();
        assert!(!closed(), "closed a window of another process");
        // the same id, but an older process's
        window(me, born - 1).close();
        assert!(!closed(), "closed a window of a reused process id");
        window(me, born).close();
        assert!(closed());
        unsafe { DestroyWindow(hwnd) };
        window(me, born).close();
        assert!(!closed(), "a gone window");
    }
}
