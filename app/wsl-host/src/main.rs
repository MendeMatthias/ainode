//! ainode-wsl: the Windows app's engine host, inside AInode's own WSL
//! distribution (docs/windows.md, route 1).
//!
//!   ainode-wsl serve <socket> -- <btx-modeld argument>...
//!   ainode-wsl clear <socket>
//!
//! The Windows app starts it as `wsl.exe -d AInode -- <engine>/ainode-wsl serve
//! ...` and keeps that one process for the engine's whole life:
//!   - it starts upstream's `btx-modeld` from its own folder with exactly the
//!     environment the Linux app gives it (engine.rs `child_env` and
//!     `helper_env`): a fixed PATH, HOME, `LD_LIBRARY_PATH=<folder>/lib`.
//!     No OpenSSL tool: upstream v0.34.16 mints its identity in-process;
//!   - before that it clears what an earlier run left in the socket's folder,
//!     as engine.rs does on Linux: a helper still answering on the socket is
//!     asked to stop, then signalled; a helper that is alive but silent is
//!     found through the pid file written here, and ended only when that pid
//!     runs our own btx-modeld; a dead socket is unlinked, anything else at
//!     its path is never touched. Two helpers on one store would split the
//!     app from its models. The probe has a timeout; a relayed request never
//!     does (an export can take minutes: the app's gate keeps time);
//!   - it relays: each line on stdin is one JSON-RPC request, sent on its own
//!     connection to the helper's unix socket (upstream's framing: write,
//!     half-close, read to the first newline), and the reply line goes to
//!     stdout. The app sends one request at a time (rpc.rs, its gate);
//!   - the helper's stderr is copied to ours unchanged, its stdout with a
//!     "[stdout] " prefix and our own notices as "ainode-wsl: note: ...", so
//!     the app keeps the helper's first error line as on the Mac;
//!   - when the helper exits it writes one line `{"ainode_exit":{...}}` to
//!     stdout and exits with the helper's code;
//!   - when stdin closes (the app quit, or died and Windows closed the pipe) it
//!     stops the helper: RPC `stop`, SIGTERM at 3 s, SIGKILL at 8 s, the same
//!     ladder as engine.rs;
//!   - `clear` does the leftover clearing alone, so the app can run it before
//!     it pins the helper's peers into catalog.json (a leftover asked to stop
//!     saves that file once more on its way out).
//!
//! Measured (lab/evidence/2026-10-13/windows): a request through one such
//! process costs 0.2 ms; a `wsl.exe` per request cost 92 ms.

use std::io::{BufRead, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A reply line larger than this is not a reply (rpc.rs MAX_REPLY).
const MAX_REPLY: usize = 64 << 20;
const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;
/// The helper's pid, beside its socket (engine.rs `modeld_pidfile`).
const PIDFILE: &str = "modeld.pid";
/// A probe (hello) that gets no answer in this time is "not answering"
/// (engine.rs PROBE_TIMEOUT).
const PROBE: Duration = Duration::from_secs(2);
/// RPC stop, SIGTERM at 3 s, SIGKILL at 8 s (engine.rs).
const STOP_RPC_GRACE: Duration = Duration::from_secs(3);
const STOP_TERM_GRACE: Duration = Duration::from_secs(8);
const HELLO: &str = r#"{"jsonrpc":"1.0","id":1,"method":"hello","params":[]}"#;
const STOP: &str = r#"{"jsonrpc":"1.0","id":1,"method":"stop","params":[]}"#;
const INFO: &str = r#"{"jsonrpc":"1.0","id":1,"method":"getmodelnetworkinfo","params":[]}"#;

/// prctl(PR_SET_PDEATHSIG): the helper gets SIGTERM if this process dies
/// without stopping it (SIGHUP when wsl.exe goes, an abort), so nothing is
/// left running inside the distribution.
const PR_SET_PDEATHSIG: i32 = 1;

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn geteuid() -> u32;
    fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(String::as_str) {
        Some("serve") => serve(&args[2..]),
        Some("clear") => clear(&args[2..]),
        Some("--version") => {
            println!("ainode-wsl {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("usage: ainode-wsl serve <socket> -- <btx-modeld argument>... | clear <socket>");
            64
        }
    };
    std::process::exit(code);
}

/// `ainode-wsl clear <socket>`: what `serve` does before starting the helper,
/// on its own. 0 when the socket's folder is clear, 1 with the reason on
/// stderr when a helper still serves it.
fn clear(a: &[String]) -> i32 {
    let Some(sock) = a.first() else {
        eprintln!("usage: ainode-wsl clear <socket>");
        return 64;
    };
    let sock = PathBuf::from(sock);
    let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) else {
        eprintln!("ainode-wsl: cannot find its own folder");
        return 1;
    };
    match prepare_private_dir(sock.parent().unwrap_or(Path::new("/tmp")))
        .and_then(|_| clear_leftovers(&sock, &sock.with_file_name(PIDFILE), &dir.join("btx-modeld")))
    {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("ainode-wsl: {e}");
            1
        }
    }
}

fn serve(a: &[String]) -> i32 {
    let (Some(sock), Some("--")) = (a.first(), a.get(1).map(String::as_str)) else {
        eprintln!("usage: ainode-wsl serve <socket> -- <btx-modeld argument>...");
        return 64;
    };
    let sock = PathBuf::from(sock);
    let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) else {
        eprintln!("ainode-wsl: cannot find its own folder");
        return 1;
    };
    let modeld = dir.join("btx-modeld");
    let pidfile = sock.with_file_name(PIDFILE);
    if let Err(e) = prepare_private_dir(sock.parent().unwrap_or(Path::new("/tmp"))).and_then(|_| clear_leftovers(&sock, &pidfile, &modeld)) {
        eprintln!("ainode-wsl: {e}");
        return 1;
    }
    let mut cmd = Command::new(&modeld);
    cmd.args(&a[2..])
        .env_clear()
        .envs(helper_env(&dir))
        .current_dir(std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: prctl is async-signal-safe and runs in the child before exec.
    unsafe {
        cmd.pre_exec(|| {
            prctl(PR_SET_PDEATHSIG, SIGTERM as u64, 0, 0, 0);
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ainode-wsl: could not start {}: {e}", modeld.display());
            return 127;
        }
    };
    let pid = child.id() as i32;
    // So the next ainode-wsl can find this helper even when it no longer
    // answers on the socket (busy, wedged): see clear_leftovers.
    write_pidfile(&pidfile, pid);
    let first_err = Arc::new(Mutex::new(String::new()));
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || copy_lines(out, "[stdout] ", None));
    }
    if let Some(err) = child.stderr.take() {
        let first = first_err.clone();
        std::thread::spawn(move || copy_lines(err, "", Some(first)));
    }

    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    {
        // The helper's end is ours too: one notice line, then the same code.
        let stdout = stdout.clone();
        let pidfile = pidfile.clone();
        std::thread::spawn(move || {
            let st = child.wait();
            remove_pidfile_if(&pidfile, pid);
            std::thread::sleep(Duration::from_millis(200)); // let stderr drain
            let (code, signal) = match st {
                Ok(s) => (s.code(), s.signal()),
                Err(_) => (None, None),
            };
            let first = first_err.lock().unwrap().clone();
            let notice = format!(
                "{{\"ainode_exit\":{{\"code\":{},\"signal\":{},\"stderr\":{}}}}}\n",
                code.map_or("null".to_string(), |c| c.to_string()),
                signal.map_or("null".to_string(), |s| s.to_string()),
                json_string(&first)
            );
            if let Ok(mut o) = stdout.lock() {
                let _ = o.write_all(notice.as_bytes());
                let _ = o.flush();
            }
            std::process::exit(code.unwrap_or_else(|| 128 + signal.unwrap_or(0)));
        });
    }

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = relay(&sock, &line);
        let mut o = stdout.lock().unwrap();
        let _ = o.write_all(reply.as_bytes());
        let _ = o.write_all(b"\n");
        let _ = o.flush();
    }
    // stdin closed: the app is gone or done. Stop the helper; the waiting
    // thread above exits the process when it has.
    stop(&sock, pid);
    std::thread::sleep(Duration::from_secs(3));
    1
}

/// The helper's output to our stderr, line by line, until it closes. Bytes
/// that are not UTF-8 are shown lossily rather than ending the copy (which
/// would close the pipe and kill the helper with SIGPIPE on its next write).
/// A write that fails (the app is gone and stderr with it) is dropped, never
/// a panic: with `panic = "abort"` that would end this process mid-stop.
fn copy_lines<R: Read>(r: R, prefix: &str, first: Option<Arc<Mutex<String>>>) {
    let mut rd = std::io::BufReader::new(r);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match rd.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(buf.strip_suffix(b"\n").unwrap_or(&buf)).into_owned();
        if let Some(f) = &first {
            let mut f = f.lock().unwrap();
            if f.is_empty() && !line.trim().is_empty() {
                *f = line.clone();
            }
        }
        let _ = writeln!(std::io::stderr(), "{prefix}{line}");
    }
}

/// What the helper gets: the Linux app's allowlist (engine.rs `child_env`,
/// `helper_env`), nothing from the session that started us.
fn helper_env(dir: &Path) -> Vec<(String, std::ffi::OsString)> {
    let mut v = vec![
        ("PATH".to_string(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
        ("LD_LIBRARY_PATH".to_string(), dir.join("lib").into_os_string()),
    ];
    for k in ["HOME", "TMPDIR"] {
        if let Some(val) = std::env::var_os(k) {
            v.push((k.to_string(), val));
        }
    }
    v
}

/// One request on one connection, upstream's framing. A helper that is not
/// there (yet) is an error reply the app reads as "not reachable". No
/// timeout: the app's gate keeps time, and a reply that arrived after a
/// timeout here would shift onto the next request.
fn relay(sock: &Path, line: &str) -> String {
    match relay_with(sock, line, None) {
        Ok(r) => r,
        Err(e) => format!("{{\"result\":null,\"error\":{{\"code\":\"RELAY_IO\",\"message\":{}}},\"id\":null}}", json_string(&e.to_string())),
    }
}

fn relay_with(sock: &Path, line: &str, read_timeout: Option<Duration>) -> std::io::Result<String> {
    let mut s = UnixStream::connect(sock)?;
    s.set_read_timeout(read_timeout)?;
    s.write_all(line.as_bytes())?;
    s.write_all(b"\n")?;
    let _ = s.shutdown(std::net::Shutdown::Write);
    let mut buf = Vec::with_capacity(16 * 1024);
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let n = s.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if chunk[..n].contains(&b'\n') {
            break;
        }
        if buf.len() > MAX_REPLY {
            return Err(std::io::Error::other("reply too large"));
        }
    }
    let end = buf.iter().position(|&b| b == b'\n').unwrap_or(buf.len());
    if buf[..end].iter().all(u8::is_ascii_whitespace) {
        return Err(std::io::Error::other("empty reply"));
    }
    Ok(String::from_utf8_lossy(&buf[..end]).into_owned())
}

/// Something answers hello on the socket within the probe timeout. A helper
/// that is alive but stopped or wedged accepts the connection and never
/// answers: that reads as "not answering", like a dead one.
fn answers(sock: &Path) -> bool {
    relay_with(sock, HELLO, Some(PROBE)).is_ok()
}

/// The pid the helper on the socket reports for itself (getmodelnetworkinfo
/// `helper_pid`), read without a JSON parser: this program is the standard
/// library only.
fn helper_pid(sock: &Path) -> Option<i32> {
    let r = relay_with(sock, INFO, Some(PROBE)).ok()?;
    let key = "\"helper_pid\":";
    let at = r.find(key)? + key.len();
    let digits: String = r[at..].trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok().filter(|&p| p > 1)
}

/// RPC stop, SIGTERM at 3 s, SIGKILL at 8 s (engine.rs `stop`).
fn stop(sock: &Path, pid: i32) {
    let t0 = Instant::now();
    let _ = relay_with(sock, STOP, Some(STOP_RPC_GRACE));
    while alive(pid) && t0.elapsed() < STOP_RPC_GRACE {
        std::thread::sleep(Duration::from_millis(100));
    }
    if alive(pid) {
        unsafe { kill(pid, SIGTERM) };
    }
    while alive(pid) && t0.elapsed() < STOP_TERM_GRACE {
        std::thread::sleep(Duration::from_millis(100));
    }
    if alive(pid) {
        unsafe { kill(pid, SIGKILL) };
    }
}

/// SIGTERM, wait up to `grace`, then SIGKILL (which also ends a stopped or
/// wedged process). engine.rs `kill_escalating`.
fn kill_escalating(pid: i32, grace: Duration) {
    unsafe { kill(pid, SIGTERM) };
    let t0 = Instant::now();
    while alive(pid) && t0.elapsed() < grace {
        std::thread::sleep(Duration::from_millis(100));
    }
    if alive(pid) {
        unsafe { kill(pid, SIGKILL) };
        let t1 = Instant::now();
        while alive(pid) && t1.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn alive(pid: i32) -> bool {
    pid > 1 && unsafe { kill(pid, 0) } == 0
}

/// The `/proc/<pid>/exe` link (engine.rs `executable_path` on Linux).
fn executable_path(pid: i32) -> Option<PathBuf> {
    if pid <= 1 {
        return None;
    }
    let p = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let s = p.to_string_lossy();
    Some(PathBuf::from(s.strip_suffix(" (deleted)").unwrap_or(&s)))
}

/// `pid` is alive and running exactly `exe` (both sides canonicalised), so a
/// pid read from a file is only ever signalled when it is our helper, never
/// a stranger that inherited a recycled pid.
fn is_executable(pid: i32, exe: &Path) -> bool {
    let (Some(actual), Ok(want)) = (executable_path(pid), std::fs::canonicalize(exe)) else {
        return false;
    };
    std::fs::canonicalize(&actual).unwrap_or(actual) == want
}

fn read_pidfile(p: &Path) -> Option<i32> {
    std::fs::read_to_string(p).ok()?.trim().parse().ok().filter(|&p| p > 1)
}

fn write_pidfile(p: &Path, pid: i32) {
    let _ = std::fs::write(p, format!("{pid}\n"));
}

/// Remove the pid file only if it still names `pid` (a newer helper may have
/// replaced it).
fn remove_pidfile_if(p: &Path, pid: i32) {
    if read_pidfile(p) == Some(pid) {
        let _ = std::fs::remove_file(p);
    }
}

/// What an earlier run left behind, before our helper starts (engine.rs
/// `clear_socket` on Linux, the same two cases):
///   - a helper that still answers on the socket: asked to stop over RPC,
///     then signalled;
///   - a helper that is alive but does NOT answer (busy on a long call,
///     wedged, stopped): the pid file finds it, and it is only signalled when
///     that pid's executable is our own btx-modeld, so a reused pid is never
///     hit. Without this, "no answer" would read as "dead", the socket would
///     be unlinked under a live helper, and a second helper would start on the
///     same store.
fn clear_leftovers(sock: &Path, pidfile: &Path, modeld: &Path) -> Result<(), String> {
    let leftover = read_pidfile(pidfile).filter(|&p| is_executable(p, modeld));
    if is_socket(sock) && answers(sock) {
        // "note:": the app does not take these for the helper's error line.
        eprintln!("ainode-wsl: note: a helper from an earlier run still answers on {}; stopping it", sock.display());
        // The helper reports its own pid; it is serving OUR socket, so any
        // btx-modeld (an older version's path included) is accepted.
        let pid = helper_pid(sock)
            .filter(|&p| executable_path(p).is_some_and(|x| x.file_name().is_some_and(|n| n == "btx-modeld")))
            .or(leftover);
        let t0 = Instant::now();
        let _ = relay_with(sock, STOP, Some(STOP_RPC_GRACE));
        let gone = |pid: Option<i32>| pid.map_or_else(|| !is_socket(sock) || !answers(sock), |p| !alive(p));
        while !gone(pid) && t0.elapsed() < STOP_RPC_GRACE {
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Some(p) = pid.filter(|&p| alive(p)) {
            kill_escalating(p, STOP_TERM_GRACE - STOP_RPC_GRACE);
        }
    } else if let Some(pid) = leftover {
        eprintln!("ainode-wsl: note: btx-modeld {pid} from an earlier run is alive but not answering; stopping it");
        kill_escalating(pid, STOP_TERM_GRACE - STOP_RPC_GRACE);
    }
    let _ = std::fs::remove_file(pidfile);
    clear_socket(sock)
}

/// The socket's folder is ours and 0700 (engine.rs `prepare_private_dir`).
fn prepare_private_dir(dir: &Path) -> Result<(), String> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("create {}: {e}", dir.display()))?;
    let md = std::fs::symlink_metadata(dir).map_err(|e| format!("stat {}: {e}", dir.display()))?;
    if !md.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let me = unsafe { geteuid() };
    if md.uid() != me {
        return Err(format!("{} is owned by uid {}, not us ({me})", dir.display(), md.uid()));
    }
    if md.permissions().mode() & 0o777 != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| format!("chmod 0700 {}: {e}", dir.display()))?;
    }
    Ok(())
}

fn is_socket(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_socket())
}

/// A socket left behind with nothing answering is unlinked; one still being
/// served is an error. Anything that is not a socket is never deleted.
fn clear_socket(sock: &Path) -> Result<(), String> {
    let Ok(md) = std::fs::symlink_metadata(sock) else { return Ok(()) };
    if !md.file_type().is_socket() {
        return Err(format!("{} exists and is not a socket; not touching it", sock.display()));
    }
    if answers(sock) {
        return Err(format!("another engine is still serving {}", sock.display()));
    }
    match std::fs::remove_file(sock) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove stale socket {}: {e}", sock.display())),
    }
}

/// A JSON string literal (quotes, backslashes and control characters escaped).
fn json_string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn private_tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ainode-wsl-{tag}-{}", std::process::id()));
        prepare_private_dir(&dir).unwrap();
        dir
    }

    /// A listener that accepts and never answers: a stopped or wedged helper.
    fn silent_listener(sock: &Path) {
        let l = UnixListener::bind(sock).unwrap();
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            while let Ok((c, _)) = l.accept() {
                kept.push(c);
            }
        });
    }

    /// A process standing in for a leftover helper, reaped by a thread like a
    /// real one is by its own ainode-wsl; returns (pid, its exit status later).
    fn stand_in() -> (i32, std::thread::JoinHandle<std::process::ExitStatus>) {
        let mut c = Command::new("sleep").arg("60").stdin(Stdio::null()).spawn().unwrap();
        let pid = c.id() as i32;
        (pid, std::thread::spawn(move || c.wait().unwrap()))
    }

    #[test]
    fn non_utf8_output_does_not_end_the_copy() {
        let bytes: &[u8] = b"first\n\xff\xfebad bytes\nlast\n";
        let first = Arc::new(Mutex::new(String::new()));
        copy_lines(bytes, "", Some(first.clone()));
        assert_eq!(first.lock().unwrap().as_str(), "first");
    }

    #[test]
    fn clear_leaves_a_clean_folder_and_refuses_a_served_socket() {
        let dir = private_tmp("clear");
        let sock = dir.join("s.sock");
        // A dead socket and a stale pid file: cleared, exit 0.
        drop(UnixListener::bind(&sock).unwrap());
        write_pidfile(&sock.with_file_name(PIDFILE), 999_999);
        assert_eq!(clear(&[sock.to_string_lossy().into_owned()]), 0);
        assert!(!sock.exists() && !sock.with_file_name(PIDFILE).exists());
        // A socket something still answers on (a stand-in that always replies): refused, exit 1.
        let l = UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || loop {
            let Ok((mut c, _)) = l.accept() else { return };
            let mut req = Vec::new();
            let _ = c.read_to_end(&mut req);
            let _ = c.write_all(b"{\"result\":{},\"error\":null}\n");
        });
        assert_eq!(clear(&[sock.to_string_lossy().into_owned()]), 1);
        assert_eq!(clear(&[]), 64);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(json_string("a\"b\\c\nd\u{1}"), r#""a\"b\\c\nd\u0001""#);
    }

    #[test]
    fn a_missing_helper_is_a_relay_io_error() {
        let r = relay(Path::new("/nonexistent/ainode.sock"), "{}");
        assert!(r.contains("\"RELAY_IO\""), "{r}");
        assert!(!answers(Path::new("/nonexistent/ainode.sock")));
    }

    #[test]
    fn relays_one_request_per_connection() {
        let dir = private_tmp("relay");
        let sock = dir.join("s.sock");
        let l = UnixListener::bind(&sock).unwrap();
        let t = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut c, _) = l.accept().unwrap();
                let mut req = Vec::new();
                c.read_to_end(&mut req).unwrap();
                let v = String::from_utf8(req).unwrap();
                c.write_all(format!("{{\"result\":{},\"error\":null}}\nignored", v.trim()).as_bytes()).unwrap();
            }
        });
        assert_eq!(relay(&sock, "1"), r#"{"result":1,"error":null}"#);
        assert_eq!(relay(&sock, "2"), r#"{"result":2,"error":null}"#);
        t.join().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_non_socket_is_never_deleted() {
        let dir = std::env::temp_dir().join(format!("ainode-wsl-ns-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.sock");
        std::fs::write(&p, b"precious").unwrap();
        assert!(clear_socket(&p).unwrap_err().contains("not a socket"));
        assert!(clear_leftovers(&p, &p.with_file_name(PIDFILE), Path::new("/bin/true")).unwrap_err().contains("not a socket"));
        assert_eq!(std::fs::read(&p).unwrap(), b"precious");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_silent_helper_is_not_answering_after_the_probe_timeout() {
        let dir = private_tmp("silent");
        let sock = dir.join("s.sock");
        silent_listener(&sock);
        let t0 = Instant::now();
        assert!(!answers(&sock));
        let took = t0.elapsed();
        assert!(took >= PROBE && took < PROBE + Duration::from_secs(2), "{took:?}");
        assert!(helper_pid(&sock).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_frozen_leftover_is_found_through_its_pid_file_and_ended() {
        let dir = private_tmp("frozen");
        let sock = dir.join("s.sock");
        let pidfile = sock.with_file_name(PIDFILE);
        silent_listener(&sock);
        let (pid, reaper) = stand_in();
        write_pidfile(&pidfile, pid);
        assert_eq!(read_pidfile(&pidfile), Some(pid));
        // The pid file names a live process running "our" program (the
        // stand-in's own binary): it is ended, the pid file and the dead
        // socket go, and the whole thing takes the probe timeout, not the
        // start timeout.
        let exe = executable_path(pid).unwrap();
        assert!(is_executable(pid, &exe));
        assert!(!is_executable(pid, Path::new("/bin/true")));
        let t0 = Instant::now();
        clear_leftovers(&sock, &pidfile, &exe).unwrap();
        let took = t0.elapsed();
        let st = reaper.join().unwrap();
        assert_eq!(st.signal(), Some(SIGTERM), "{st:?}");
        assert!(took < PROBE + Duration::from_secs(3), "{took:?}");
        assert!(!pidfile.exists(), "pid file removed");
        assert!(!sock.exists(), "dead socket unlinked");
        assert!(!is_executable(pid, &exe), "a dead pid runs nothing");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_pid_file_naming_another_program_is_left_alone() {
        let dir = private_tmp("stranger");
        let sock = dir.join("s.sock");
        let pidfile = sock.with_file_name(PIDFILE);
        let (pid, reaper) = stand_in();
        write_pidfile(&pidfile, pid);
        // No socket, and the pid runs sleep, not btx-modeld: nothing is
        // signalled; the stale pid file is dropped.
        clear_leftovers(&sock, &pidfile, Path::new("/bin/true")).unwrap();
        assert!(alive(pid), "a stranger with a recycled pid is never hit");
        assert!(!pidfile.exists());
        unsafe { kill(pid, SIGKILL) };
        reaper.join().unwrap();
        // The pid file is only removed by the helper's end when it still
        // names that helper.
        write_pidfile(&pidfile, 4242);
        remove_pidfile_if(&pidfile, 1);
        assert!(pidfile.exists());
        remove_pidfile_if(&pidfile, 4242);
        assert!(!pidfile.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_answering_leftover_is_asked_to_stop() {
        let dir = private_tmp("answering");
        let sock = dir.join("s.sock");
        let pidfile = sock.with_file_name(PIDFILE);
        let (pid, reaper) = stand_in();
        write_pidfile(&pidfile, pid);
        // A fake helper: answers hello, reports the stand-in as its pid, and
        // on stop ends the stand-in and removes its socket (a clean stop).
        let l = UnixListener::bind(&sock).unwrap();
        let s2 = sock.clone();
        std::thread::spawn(move || loop {
            let (mut c, _) = l.accept().unwrap();
            let mut req = Vec::new();
            c.read_to_end(&mut req).unwrap();
            let req = String::from_utf8_lossy(&req);
            if req.contains("\"stop\"") {
                unsafe { kill(pid, SIGTERM) };
                let _ = std::fs::remove_file(&s2);
                let _ = c.write_all(b"{\"result\":null,\"error\":null}\n");
                return;
            }
            let reply = if req.contains("getmodelnetworkinfo") {
                format!("{{\"result\":{{\"helper_pid\":{pid},\"pq1_ready\":true}},\"error\":null}}\n")
            } else {
                "{\"result\":{},\"error\":null}\n".to_string()
            };
            c.write_all(reply.as_bytes()).unwrap();
        });
        assert!(answers(&sock));
        assert_eq!(helper_pid(&sock), Some(pid));
        let t0 = Instant::now();
        // The stand-in is not btx-modeld, so neither the reported pid nor the
        // pid file names a helper to signal: the RPC stop alone has to do it,
        // and here it does.
        clear_leftovers(&sock, &pidfile, Path::new("/bin/true")).unwrap();
        assert!(t0.elapsed() < STOP_RPC_GRACE + Duration::from_secs(1));
        assert_eq!(reaper.join().unwrap().signal(), Some(SIGTERM));
        assert!(!sock.exists() && !pidfile.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
