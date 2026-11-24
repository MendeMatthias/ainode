//! Local chat: one llama-server on 127.0.0.1, serving the runnable gguf.
//!
//! Why not upstream's `generatemodel`: it is one-shot, reloads the model per
//! call, and with current llama.cpp returns llama-cli's banner as the text
//! (contract E18). llama-server keeps the model loaded and speaks the OpenAI
//! chat API the UI calls directly on the returned port (the CSP allows
//! connect-src http://127.0.0.1:*).
//!
//! The staged llama-server finds its dylibs through its own LC_RPATH
//! `@loader_path` (stage-engine.sh proves it from another cwd).
//!
//! Lockdown (every flag checked against the staged b11559 `--help`):
//!   - a random per-session API key, handed over in a 0600 file in the 0700
//!     socket dir (`--api-key-file`) and deleted once the server is up. Not
//!     argv (`ps` shows it) and not the environment (`ps -E` shows that too,
//!     found by review). Every endpoint but /health then needs
//!     `Authorization: Bearer <key>`. Anything else on the machine can reach
//!     127.0.0.1, so loopback alone is no boundary.
//!   - `--no-webui` (no browser UI on the port), `--no-slots` (no slot
//!     monitoring endpoint). /props writes and /metrics are off by default.
//!   - CORS only for the Tauri origin, and `Authorization` listed explicitly:
//!     a `*` in Access-Control-Allow-Headers does not cover Authorization
//!     (Fetch spec), so without it the webview's preflight would fail.
//!   - `env_clear()` plus an allowlist (HOME, TMPDIR, a fixed system PATH).

use std::collections::VecDeque;
#[cfg(unix)]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout, Instant};

use crate::engine::{
    alive, child_env, is_executable, kill_escalating, pump, read_pidfile, remove_pidfile_if, signal, write_pidfile, SIGKILL,
    SIGTERM,
};

const READY_TIMEOUT: Duration = Duration::from_secs(60);
const CONTEXT: &str = "8192";

/// The webview's origin: tauri://localhost on macOS and Linux,
/// http://tauri.localhost on Windows; a debug build serves the UI from the
/// Vite dev server (tauri.conf.json devUrl).
/// The ONE origin allowed to read llama-server's replies. It must be a single
/// origin: llama-server copies `--cors-origins` into Access-Control-Allow-Origin
/// verbatim, and a comma-separated list there is invalid, so browsers (WKWebView
/// included) reject every response. A list made chat fail in the real UI while
/// the live test, an HTTP client that ignores CORS, passed. Release: the Tauri
/// webview's origin. Debug: the Vite dev server (`tauri dev`, the dev bridge).
fn cors_origin() -> &'static str {
    if cfg!(debug_assertions) {
        "http://localhost:1450"
    } else if cfg!(windows) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
}

struct ChatProc {
    uri: String,
    /// Started with the graphics card in use (`gpu_args`).
    gpu: bool,
    port: u16,
    key: String,
    child: Child,
    stderr: Arc<StdMutex<VecDeque<String>>>,
}

/// What is loaded or loading, readable without the load lock (which is held
/// for up to a minute while a model loads).
#[derive(Clone)]
struct Current {
    uri: String,
    pid: Option<u32>,
}

pub struct Chat {
    llama_server: PathBuf,
    workdir: PathBuf,
    pidfile: PathBuf,
    inner: Mutex<Option<ChatProc>>,
    current: StdMutex<Option<Current>>,
    /// Bumped by stop(): a start that began before it gives up.
    generation: AtomicU64,
}

impl Chat {
    pub fn new(llama_server: PathBuf, workdir: PathBuf, pidfile: PathBuf) -> Self {
        Chat {
            llama_server,
            workdir,
            pidfile,
            inner: Mutex::new(None),
            current: StdMutex::new(None),
            generation: AtomicU64::new(0),
        }
    }

    /// One line per chat event in `<socket dir>/ainode.log` (0600 in the 0700
    /// dir, restarted past 1 MB). A Finder-launched app's stderr goes nowhere,
    /// so this is what shows why a chat did or did not start.
    pub fn note(&self, msg: &str) {
        use std::io::Write;
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;
        let p = self.pidfile.with_file_name("ainode.log");
        if std::fs::metadata(&p).is_ok_and(|m| m.len() > 1_000_000) {
            let _ = std::fs::remove_file(&p);
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let mut o = std::fs::OpenOptions::new();
        o.create(true).append(true);
        #[cfg(unix)]
        o.mode(0o600);
        if let Ok(mut f) = o.open(&p) {
            let _ = writeln!(f, "{now} chat {msg}");
        }
    }

    /// Is `uri` loaded in llama-server, or being loaded right now?
    pub fn busy_with(&self, uri: &str) -> bool {
        match self.current.lock().unwrap().as_ref() {
            Some(c) if c.uri == uri => c.pid.is_none_or(alive),
            _ => false,
        }
    }

    /// Start (or reuse) the one chat server for `uri`; returns (port, key).
    /// A server for another model is stopped first: one at a time, a 3B
    /// model is ~4 GB of RAM.
    /// `node_here`: a BTX node answers on this computer (neighbour.rs); the
    /// chat then uses fewer threads (`thread_args`). Decided when a server
    /// starts: a node found later does not interrupt a running chat.
    pub async fn start(&self, uri: &str, gguf: &Path, gpu: bool, node_here: bool) -> Result<(u16, String), String> {
        let mut g = self.inner.lock().await;
        if let Some(p) = g.as_mut() {
            // The same model with the same graphics-card choice is reused; a
            // changed choice starts the server again.
            if p.uri == uri && p.gpu == gpu && matches!(p.child.try_wait(), Ok(None)) {
                self.note(&format!("reuse pid {} port {}", p.child.id().unwrap_or(0), p.port));
                return Ok((p.port, p.key.clone()));
            }
        }
        if let Some(p) = g.take() {
            stop_proc(p).await;
        }
        *self.current.lock().unwrap() = Some(Current { uri: uri.to_string(), pid: None });
        let gen = self.generation.load(Ordering::SeqCst);
        let t0 = Instant::now();
        let threads = thread_args(node_here, std::thread::available_parallelism().map_or(1, |n| n.get()));
        self.note(&format!(
            "start {uri} ({}{})",
            if gpu { "graphics card when there is one" } else { "processor only" },
            if threads.is_empty() { String::new() } else { format!(", {} threads beside a BTX node", threads[1]) }
        ));
        let r = self.spawn_and_wait(uri, gguf, gen, gpu, &threads).await;
        match &r {
            Ok(p) => self.note(&format!("ready pid {} port {} after {} ms", p.child.id().unwrap_or(0), p.port, t0.elapsed().as_millis())),
            Err(e) => self.note(&format!("failed after {} ms: {e}", t0.elapsed().as_millis())),
        }
        match r {
            Ok(p) => {
                let out = (p.port, p.key.clone());
                *g = Some(p);
                Ok(out)
            }
            Err(e) => {
                *self.current.lock().unwrap() = None;
                Err(e)
            }
        }
    }

    async fn spawn_and_wait(&self, uri: &str, gguf: &Path, gen: u64, gpu: bool, threads: &[String]) -> Result<ChatProc, String> {
        self.kill_orphan().await;
        if !self.llama_server.is_file() {
            return Err(format!(
                "chat runtime missing: {} (a dev build needs scripts/stage-engine.sh)",
                self.llama_server.display()
            ));
        }
        let port = free_port()?;
        let key = random_key()?;
        let keyfile = self.pidfile.with_file_name("llama-server.key");
        write_keyfile(&keyfile, &key)?;
        // llama-server reads it while parsing its arguments, long before
        // /health answers; every return from here removes it.
        let _keyfile = RemoveOnDrop(keyfile.clone());
        let mut cmd = Command::new(&self.llama_server);
        cmd.arg("-m")
            .arg(gguf)
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "-c", CONTEXT, "--jinja"])
            .args(["--no-webui", "--no-slots"])
            .args(["--cors-origins", cors_origin(), "--cors-headers", "Authorization,Content-Type"])
            .arg("--no-cors-credentials")
            .args(gpu_args(gpu))
            .args(threads)
            .arg("--api-key-file")
            .arg(&keyfile)
            .env_clear()
            .envs(child_env())
            .current_dir(&self.workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Windows: no console window, and a job object so the server ends
        // with the app even when the app is killed (it holds the model in RAM).
        // Linux: the same through the parent-death signal.
        #[cfg(windows)]
        cmd.creation_flags(creation_flags());
        #[cfg(unix)]
        crate::engine::with_parent_death(&mut cmd);
        #[cfg(target_os = "linux")]
        make_way(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("could not start llama-server: {e}"))?;
        #[cfg(windows)]
        kill_with_app(&child);
        let pid = child.id().unwrap_or(0);
        write_pidfile(&self.pidfile, pid);
        if let Some(c) = self.current.lock().unwrap().as_mut() {
            c.pid = Some(pid);
        }

        let stderr = Arc::new(StdMutex::new(VecDeque::new()));
        if let Some(out) = child.stdout.take() {
            tokio::spawn(pump(out, "llama-server", None));
        }
        if let Some(err) = child.stderr.take() {
            tokio::spawn(pump(err, "llama-server", Some(stderr.clone())));
        }
        let mut proc_ = ChatProc { uri: uri.to_string(), gpu, port, key, child, stderr };

        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if self.generation.load(Ordering::SeqCst) != gen {
                stop_proc(proc_).await;
                remove_pidfile_if(&self.pidfile, pid);
                return Err("chat stopped".into());
            }
            if let Ok(Some(status)) = proc_.child.try_wait() {
                remove_pidfile_if(&self.pidfile, pid);
                return Err(format!("llama-server exited ({status}): {}", last_line(&proc_.stderr)));
            }
            if health_ok(port).await {
                return Ok(proc_);
            }
            if Instant::now() >= deadline {
                stop_proc(proc_).await;
                remove_pidfile_if(&self.pidfile, pid);
                return Err(format!("the model did not load in {} s", READY_TIMEOUT.as_secs()));
            }
            sleep(Duration::from_millis(250)).await;
        }
    }

    /// Stop the chat server. Never waits for a model that is still loading:
    /// that start holds the lock for up to a minute, so instead its child is
    /// SIGKILLed by pid (a half-loaded model has nothing to save) and the
    /// start's own loop reaps it and returns. Quit is never held up by a load.
    pub async fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let cur = self.current.lock().unwrap().take();
        match self.inner.try_lock() {
            Ok(mut g) => {
                if let Some(p) = g.take() {
                    let pid = p.child.id();
                    stop_proc(p).await;
                    if let Some(pid) = pid {
                        remove_pidfile_if(&self.pidfile, pid);
                    }
                }
            }
            Err(_) => {
                if let Some(pid) = cur.and_then(|c| c.pid) {
                    signal(pid, SIGKILL);
                }
            }
        }
    }

    /// A llama-server left by a crashed earlier run keeps its model in RAM.
    /// The pid file names it; it is only signalled if that pid is running OUR
    /// llama-server binary, and escalates to SIGKILL.
    pub async fn kill_orphan(&self) {
        let Some(pid) = read_pidfile(&self.pidfile) else { return };
        let _ = std::fs::remove_file(&self.pidfile);
        let ours = self.current.lock().unwrap().as_ref().and_then(|c| c.pid) == Some(pid);
        if !ours && is_executable(pid, &self.llama_server) {
            eprintln!("[chat] stopping llama-server {pid} left by an earlier run");
            kill_escalating(pid, Duration::from_secs(3)).await;
        }
    }
}

/// 32 random bytes from the kernel, hex: the per-session API key.
/// Created fresh (never through a planted file or symlink) and 0600, inside
/// the private socket dir.
fn write_keyfile(path: &Path, key: &str) -> Result<(), String> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        crate::engine::prepare_private_dir(dir)?;
    }
    let _ = std::fs::remove_file(path);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    o.mode(0o600);
    o.open(path)
        .and_then(|mut f| f.write_all(format!("{key}\n").as_bytes()))
        .map_err(|e| format!("could not write the chat key file: {e}"))
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
pub(crate) fn random_key() -> Result<String, String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| format!("no randomness for the chat key: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Windows: put llama-server in a job object that kills it when the app's
/// last handle to the job closes, i.e. when the app exits for any reason.
#[cfg(windows)]
pub(crate) fn kill_with_app(child: &tokio::process::Child) {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    static JOB: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let job = *JOB.get_or_init(|| {
        // SAFETY: a job we create once and keep for the app's life (never closed).
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return 0;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            job as usize
        }
    });
    if let (true, Some(h)) = (job != 0, child.raw_handle()) {
        // SAFETY: both handles are valid; assigning only adds the limit above.
        unsafe { AssignProcessToJobObject(job as _, h as _) };
    }
}

#[cfg(unix)]
pub(crate) fn random_key() -> Result<String, String> {
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| format!("no randomness for the chat key: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

async fn stop_proc(mut p: ChatProc) {
    if let Some(pid) = p.child.id() {
        signal(pid, SIGTERM);
    }
    if timeout(Duration::from_secs(3), p.child.wait()).await.is_err() {
        let _ = p.child.kill().await;
    }
}

fn last_line(buf: &Arc<StdMutex<VecDeque<String>>>) -> String {
    buf.lock().unwrap().iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default()
}

/// What llama-server gets for the graphics-card choice: nothing (its default:
/// offload to Metal or Vulkan when there is a device), or `--device none
/// -ngl 0`, the processor only.
pub(crate) fn gpu_args(gpu: bool) -> &'static [&'static str] {
    if gpu {
        &[]
    } else {
        &["--device", "none", "-ngl", "0"]
    }
}

/// The chat makes way for everything else on the computer, a BTX node above
/// all. Below normal priority costs the chat nothing when the computer is
/// otherwise idle and helps whenever the processor is short. Windows: the
/// priority class at creation. Linux: nice 10, set in the child before exec, so
/// llama.cpp's threads inherit it; inside WSL that one kernel also runs the
/// node's distribution. The Mac is left as it was: its chat runs on Metal, and
/// no node shares it today.
///
/// Priority alone does not keep a node's blocks quick, though (`thread_args`).
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

#[cfg(windows)]
fn creation_flags() -> u32 {
    crate::wsl::NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
}

#[cfg(target_os = "linux")]
const CHAT_NICE: libc::c_int = 10;

#[cfg(target_os = "linux")]
fn make_way(cmd: &mut Command) {
    // SAFETY: setpriority is async-signal-safe and runs in the child before
    // exec. A failure leaves the normal priority, which is what it was before.
    unsafe {
        cmd.pre_exec(|| {
            libc::setpriority(libc::PRIO_PROCESS, 0, CHAT_NICE);
            Ok(())
        });
    }
}

/// With a BTX node on this computer the chat uses at most 4 threads (half the
/// processor's, if it has fewer than 8). Measured on the Windows PC that runs a
/// block signer in WSL, 2026-10-16, a scratch llama-server beside it, the 3B:
/// with llama.cpp's own choice (14 to 15 of 24 threads busy) the signer's
/// blocks took a median 19 s from header to tip during a reply, against 13 to
/// 14 s between replies, at normal AND at below-normal priority alike (so it
/// is the memory and clocks the threads share, not scheduling; 19 blocks in
/// two runs). With 4 threads, on a quiet chain (ten 3-minute rounds of
/// replies, 3 idle minutes between them): a median 14 s during replies (15
/// blocks, 13 to 22) against 12 s between them (13 blocks, 11 to 15), so about
/// 2 s a block instead of 5. The reply writes at 10.2 instead of 11.2 tokens/s
/// and reads the question at 77 instead of 140 tokens/s, which is why nothing
/// changes without a node.
pub(crate) fn thread_args(node_here: bool, cpus: usize) -> Vec<String> {
    if !node_here {
        return Vec::new();
    }
    vec!["-t".into(), (cpus / 2).clamp(1, 4).to_string()]
}

/// An unused loopback port. The kernel picks from the ephemeral range, which
/// on macOS (49152+) is clear of the fixed ports the lab uses.
fn free_port() -> Result<u16, String> {
    // Fixed ports other local tooling uses (the lab's experiments); the
    // ephemeral range never hands these out by default, this makes it certain.
    const AVOID: [std::ops::RangeInclusive<u16>; 2] = [29501..=29503, 39501..=39503];
    for _ in 0..16 {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("no free port: {e}"))?;
        let p = l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())?;
        if !AVOID.iter().any(|r| r.contains(&p)) {
            return Ok(p);
        }
    }
    Err("no free loopback port".into())
}

/// GET /health == 200 {"status":"ok"}. While the model loads llama-server
/// answers 503.
pub async fn health_ok(port: u16) -> bool {
    match http(port, "GET", "/health", None, None, Duration::from_secs(2)).await {
        Ok((200, body)) => body.contains("\"ok\""),
        _ => false,
    }
}

/// Minimal HTTP/1.1 client for llama-server on loopback (Connection: close),
/// so the backend needs no HTTP crate for two requests.
pub async fn http(
    port: u16,
    method: &str,
    path: &str,
    json_body: Option<&str>,
    bearer: Option<&str>,
    t: Duration,
) -> Result<(u16, String), String> {
    let fut = async {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.map_err(|e| e.to_string())?;
        let body = json_body.unwrap_or("");
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
        if let Some(k) = bearer {
            req.push_str(&format!("Authorization: Bearer {k}\r\n"));
        }
        if json_body.is_some() {
            req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len()));
        }
        req.push_str("\r\n");
        req.push_str(body);
        s.write_all(req.as_bytes()).await.map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).await.map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&raw).to_string();
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse::<u16>().ok())
            .ok_or_else(|| format!("bad HTTP reply: {text:.80}"))?;
        let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default();
        Ok((status, body))
    };
    timeout(t, fut).await.map_err(|_| "timed out".to_string())?
}

#[cfg(test)]
mod tests {
    #[test]
    fn leaving_the_graphics_card_free_means_no_device_and_no_layers_on_it() {
        assert!(super::gpu_args(true).is_empty(), "llama-server's own default: offload when it finds a device");
        assert_eq!(super::gpu_args(false), ["--device", "none", "-ngl", "0"]);
    }

    #[test]
    fn beside_a_node_the_chat_uses_at_most_four_threads() {
        assert!(super::thread_args(false, 24).is_empty(), "no node: llama.cpp's own choice");
        assert_eq!(super::thread_args(true, 24), ["-t", "4"]);
        assert_eq!(super::thread_args(true, 8), ["-t", "4"]);
        assert_eq!(super::thread_args(true, 6), ["-t", "3"]);
        assert_eq!(super::thread_args(true, 1), ["-t", "1"]);
    }

    #[test]
    fn cors_origin_is_a_single_origin() {
        // llama-server echoes it verbatim into Access-Control-Allow-Origin.
        let o = super::cors_origin();
        assert!(!o.contains(',') && !o.contains(' ') && !o.is_empty(), "{o}");
    }

    use super::*;

    #[cfg(unix)]
    #[test]
    fn keyfile_is_private_replaces_a_planted_link_and_goes_away() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("ainode");
        let p = dir.join("llama-server.key");
        std::fs::create_dir(&dir).unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        std::os::unix::fs::symlink(&elsewhere, &p).unwrap();
        write_keyfile(&p, "abc").unwrap();
        assert!(!elsewhere.exists(), "followed a planted symlink");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "abc\n");
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        drop(RemoveOnDrop(p.clone()));
        assert!(!p.exists());
    }

    #[tokio::test]
    async fn health_parses_ok_and_loading() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            for reply in [
                "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\n\r\n{\"error\":{\"code\":503,\"message\":\"Loading model\"}}",
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"status\":\"ok\"}",
            ] {
                let (mut c, _) = l.accept().await.unwrap();
                let mut buf = [0u8; 1024];
                let _ = c.read(&mut buf).await;
                c.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        assert!(!health_ok(port).await);
        assert!(health_ok(port).await);
    }

    #[test]
    fn free_port_is_bindable_and_clear_of_the_lab_ports() {
        let mut seen = std::collections::HashSet::new();
        let mut taken = 0;
        for _ in 0..20 {
            let p = free_port().unwrap();
            assert!(p >= 1024);
            assert!(!(29501..=29503).contains(&p) && !(39501..=39503).contains(&p));
            // Free means we can take it right now. On a busy shared network
            // stack (WSL's, the PC's mirrored loopback) another process can
            // take one in between, rarely: seen once in 2026-10-18's runs.
            if std::net::TcpListener::bind(("127.0.0.1", p)).is_err() {
                taken += 1;
            }
            seen.insert(p);
        }
        assert!(taken <= 2, "{taken} of 20 free ports could not be bound");
        assert!(seen.len() > 1, "the kernel hands out different ports");
    }

    /// The child really runs at nice 10: `nice` with no argument prints its own.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn the_chat_runs_at_nice_10_on_linux() {
        let mut cmd = Command::new("nice");
        make_way(&mut cmd);
        let out = cmd.output().await.unwrap();
        let own: i32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
        // Relative to the test's own niceness, which is 0 unless it was started niced.
        let base = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
        assert_eq!(own, (base + CHAT_NICE).min(19));
    }

    /// The child really is below normal: the same creation flags as the chat.
    #[cfg(windows)]
    #[tokio::test]
    async fn the_chat_is_below_normal_on_windows() {
        use windows_sys::Win32::System::Threading::{GetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS as WANT};
        assert_eq!(BELOW_NORMAL_PRIORITY_CLASS, WANT);
        let mut child = Command::new("ping")
            .args(["-n", "3", "127.0.0.1"])
            .creation_flags(creation_flags())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let h = child.raw_handle().unwrap();
        // SAFETY: the child's handle stays valid until it is waited for below.
        let class = unsafe { GetPriorityClass(h as _) };
        let _ = child.kill().await;
        assert_eq!(class, WANT);
    }

    #[test]
    fn keys_are_random_hex() {
        let (a, b) = (random_key().unwrap(), random_key().unwrap());
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
