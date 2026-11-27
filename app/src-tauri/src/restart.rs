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
}
