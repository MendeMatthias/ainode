//! AInode starting itself again: after an update is installed, and after a
//! first start that chose another data folder.
//!
//! Mac and Windows use Tauri's own restart. On Linux, Tauri starts the new
//! process as a plain child, in the session of the process that is about to
//! exit. A launcher that ends its session when the process it started exits
//! takes that child down with it: `wslg.exe -- ainode` under WSL does, and
//! AInode installed its update and then never came back. So on Linux the new
//! process gets a session of its own.
//!
//! The order is Tauri's: the page asks, the event loop exits, the engine is
//! stopped and the single-instance name is given up, and only then, in
//! `RunEvent::Exit`, does [`on_exit`] start the new process. Started earlier,
//! it would find the name still taken and exit as a second copy.

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by [`now`]; read by [`on_exit`].
#[cfg(target_os = "linux")]
static WANTED: AtomicBool = AtomicBool::new(false);

/// Restart AInode. Never returns. Call it from a command, not from the main
/// thread: on Linux it waits there for the event loop to end the process.
pub fn now(app: &tauri::AppHandle) -> ! {
    #[cfg(target_os = "linux")]
    {
        WANTED.store(true, Ordering::SeqCst);
        app.exit(0);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
    #[cfg(not(target_os = "linux"))]
    app.restart()
}

/// From `RunEvent::Exit`, after the engine is stopped: start the new AInode
/// if [`now`] asked for one. Does nothing on a plain quit.
#[cfg(target_os = "linux")]
pub fn on_exit(app: &tauri::AppHandle) {
    use tauri::Manager;

    if !WANTED.swap(false, Ordering::SeqCst) {
        return;
    }
    let env = app.env();
    // The AppImage's own path when run from one, as Tauri's restart does.
    let path = match tauri::process::current_binary(&env) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[restart] the path of AInode could not be read: {e}");
            return;
        }
    };
    let mut cmd = std::process::Command::new(path);
    cmd.args(env.args_os.iter().skip(1));
    own_session(&mut cmd);
    if let Err(e) = cmd.spawn() {
        eprintln!("[restart] AInode could not be started again: {e}");
    }
}

/// Give the process `cmd` starts a session of its own, so it outlives the
/// session of the one that starts it. Its input is closed; its output and its
/// errors are closed too where they go to a terminal or a pipe, because the
/// reader of those ends with the old session, and a later write would fail.
/// A file or the journal is kept.
#[cfg(any(target_os = "linux", all(unix, test)))]
fn own_session(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    cmd.stdin(Stdio::null());
    if ends_with_the_session(libc::STDOUT_FILENO) {
        cmd.stdout(Stdio::null());
    }
    if ends_with_the_session(libc::STDERR_FILENO) {
        cmd.stderr(Stdio::null());
    }
    // SAFETY: setsid is async-signal-safe, and nothing else runs between the
    // fork and the exec. It cannot fail here: the child is not the leader of
    // a process group. If it did, the child starts as it did before.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
}

/// Before an update is installed: our output and our errors go to /dev/null
/// where nobody reads them any more. The installer inherits them, and a
/// `dpkg -i` that writes into a pipe whose reader is gone is killed
/// (SIGPIPE), possibly halfway, which leaves the package half installed.
/// An AInode started under WSL with `wslg.exe -- setsid ainode` has such a
/// pipe once `wslg.exe` has exited; so has one whose terminal was closed. A
/// reader that is still there, a file or the journal is kept.
#[cfg(target_os = "linux")]
pub fn quiet_dead_output() {
    quiet_if_dead(libc::STDOUT_FILENO);
    quiet_if_dead(libc::STDERR_FILENO);
}

/// Point `fd` at /dev/null if nobody reads it any more: a pipe without a
/// reader (POLLERR), a terminal that hung up (POLLHUP), or a descriptor
/// that is closed (POLLNVAL).
#[cfg(any(target_os = "linux", all(unix, test)))]
fn quiet_if_dead(fd: libc::c_int) {
    let mut p = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
    // SAFETY: poll reads and writes only `p`, and returns at once (timeout 0).
    let n = unsafe { libc::poll(&mut p, 1, 0) };
    if n != 1 || p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) == 0 {
        return;
    }
    // SAFETY: the path is a NUL-terminated literal. dup2 replaces `fd`
    // atomically, and the copy it makes is inherited by a child (it does not
    // carry close-on-exec), so the temporary descriptor may have it.
    unsafe {
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
        if null < 0 {
            return;
        }
        if null == fd {
            // `fd` was closed and the open landed on its number: it is the
            // descriptor we want. Keep it, and let a child inherit it.
            libc::fcntl(fd, libc::F_SETFD, 0);
            return;
        }
        libc::dup2(null, fd);
        libc::close(null);
    }
}

/// Is this descriptor a terminal or a pipe?
#[cfg(any(target_os = "linux", all(unix, test)))]
fn ends_with_the_session(fd: libc::c_int) -> bool {
    // SAFETY: fstat writes only into `st`, and both calls take any descriptor.
    unsafe {
        if libc::isatty(fd) == 1 {
            return true;
        }
        let mut st: libc::stat = std::mem::zeroed();
        libc::fstat(fd, &mut st) == 0 && (st.st_mode & libc::S_IFMT) == libc::S_IFIFO
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;

    fn session_of(pid: u32) -> i32 {
        // SAFETY: getsid only reads.
        unsafe { libc::getsid(pid as libc::pid_t) }
    }

    #[test]
    fn the_new_process_leads_a_session_of_its_own() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        own_session(&mut cmd);
        let mut child = cmd.spawn().expect("sleep starts");
        let (theirs, ours) = (session_of(child.id()), session_of(0));
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(theirs, child.id() as i32, "it leads its own session");
        assert_ne!(theirs, ours, "which is not ours");
    }

    /// The control: without it, a child shares our session, which is what
    /// went wrong.
    #[test]
    fn a_plain_child_shares_our_session() {
        let mut child = Command::new("sleep").arg("30").spawn().expect("sleep starts");
        let (theirs, ours) = (session_of(child.id()), session_of(0));
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(theirs, ours);
    }

    #[test]
    fn a_pipe_ends_with_the_session_and_a_file_does_not() {
        use std::os::fd::AsRawFd;
        let mut fds = [0; 2];
        // SAFETY: pipe fills the two descriptors, closed below.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        assert!(ends_with_the_session(fds[1]));
        // SAFETY: both were just opened here.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        let file = tempfile::tempfile().expect("a temp file");
        assert!(!ends_with_the_session(file.as_raw_fd()));
        let null = std::fs::File::open("/dev/null").expect("/dev/null");
        assert!(!ends_with_the_session(null.as_raw_fd()));
    }

    /// Is `fd` the null device? By device number, which is the same check on
    /// Linux and on a Mac.
    fn is_dev_null(fd: libc::c_int) -> bool {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let null = std::fs::metadata("/dev/null").expect("/dev/null");
        // SAFETY: fstat writes only into `st`.
        let st = unsafe {
            let mut st: libc::stat = std::mem::zeroed();
            if libc::fstat(fd, &mut st) != 0 {
                return false;
            }
            st
        };
        null.file_type().is_char_device() && (st.st_mode & libc::S_IFMT) == libc::S_IFCHR && st.st_rdev as u64 == null.rdev()
    }

    /// A pipe whose reading end is already closed: only the writer is left.
    fn pipe_nobody_reads() -> libc::c_int {
        let mut fds = [0; 2];
        // SAFETY: pipe fills the two descriptors; the reader is closed at
        // once, the caller closes the writer.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        unsafe { libc::close(fds[0]) };
        fds[1]
    }

    #[test]
    fn a_pipe_nobody_reads_is_pointed_at_dev_null() {
        let w = pipe_nobody_reads();
        assert!(!is_dev_null(w));
        quiet_if_dead(w);
        assert!(is_dev_null(w), "the writer now goes to /dev/null");
        // SAFETY: opened in this test.
        unsafe { libc::close(w) };
    }

    #[test]
    fn a_pipe_still_read_a_file_and_dev_null_are_kept() {
        use std::os::fd::AsRawFd;
        let mut fds = [0; 2];
        // SAFETY: pipe fills the two descriptors, closed below.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        quiet_if_dead(fds[1]);
        assert!(!is_dev_null(fds[1]), "a pipe with its reader is kept");
        // SAFETY: both were just opened here.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        let file = tempfile::tempfile().expect("a temp file");
        quiet_if_dead(file.as_raw_fd());
        assert!(!is_dev_null(file.as_raw_fd()), "a file is kept");
        let null = std::fs::OpenOptions::new().write(true).open("/dev/null").expect("/dev/null");
        quiet_if_dead(null.as_raw_fd());
        assert!(is_dev_null(null.as_raw_fd()), "/dev/null stays what it is");
    }

    /// The failure itself: a child that writes into a pipe nobody reads is
    /// killed by SIGPIPE; after `quiet_if_dead` the same child finishes.
    #[test]
    fn a_child_writing_into_the_dead_pipe_survives_after_the_fix() {
        use std::os::fd::FromRawFd;
        use std::os::unix::process::ExitStatusExt;
        let run = |fd: libc::c_int| {
            // SAFETY: a duplicate, owned by the Stdio from here on.
            let out = unsafe { std::process::Stdio::from_raw_fd(libc::dup(fd)) };
            Command::new("sh").args(["-c", "sleep 0.2; echo one; echo two"]).stdout(out).status().expect("sh runs")
        };
        let w = pipe_nobody_reads();
        let before = run(w);
        assert_eq!(before.signal(), Some(libc::SIGPIPE), "without the fix the child dies of SIGPIPE");
        quiet_if_dead(w);
        let after = run(w);
        assert!(after.success(), "with it the child finishes: {after:?}");
        // SAFETY: opened in this test.
        unsafe { libc::close(w) };
    }
}
