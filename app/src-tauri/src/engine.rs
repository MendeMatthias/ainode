//! Start, supervise and stop upstream's btx-modeld (the model network helper).
//!
//! Arguments and environment are exactly ENGINE-CONTRACT.md "Starting the
//! engine". Since upstream v0.34.16 the helper mints its ML-DSA-44 identity
//! in-process (btxchain/btx#228), so no OpenSSL tool is bundled and no
//! BTX_OPENSSL is passed. What this file adds is the lifecycle around them:
//!   - the socket dir is ours and 0700 before the helper sees it;
//!   - a socket left behind is probed: nothing answering -> deleted; a helper
//!     answering -> an orphan of a previous run, stopped before ours starts
//!     (two helpers on one socket would split the UI from the store);
//!   - exit code 2 is the helper failing closed (its TLS identity, or a
//!     socket folder it does not accept) and its first stderr line is what
//!     the user is shown;
//!   - stop is RPC `stop`, SIGTERM at 3 s, SIGKILL at 8 s.

use std::collections::VecDeque;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde_json::json;
#[cfg(unix)]
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
#[cfg(unix)]
use tokio::process::Command;
use tokio::sync::{watch, Mutex};
use tokio::time::{sleep, timeout_at, Instant};

use crate::rpc::Rpc;
use crate::state::Paths;

const READY_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const STOP_RPC_GRACE: Duration = Duration::from_secs(3);
const STOP_TERM_GRACE: Duration = Duration::from_secs(8);
/// Exit code the helper uses for "model subsystem fail-closed": observed on
/// v0.34.15 with no OpenSSL tool (`model subsystem fail-closed: openssl
/// mldsa44 certificate generation failed`), and on v0.34.16 with a socket
/// folder it could not make private (`unix dir mode`).
const EXIT_FAIL_CLOSED: i32 = 2;

/// What `signal` sends. Windows has no signals: there both end a process
/// (TerminateProcess); the engine inside WSL is asked over RPC first.
#[cfg(unix)]
pub(crate) use libc::{SIGKILL, SIGTERM};
#[cfg(windows)]
pub(crate) const SIGTERM: i32 = 15;
#[cfg(windows)]
pub(crate) const SIGKILL: i32 = 9;

#[derive(Debug, Clone, PartialEq)]
pub enum EngineState {
    Stopped,
    Starting,
    Running,
    Error(String),
}

impl EngineState {
    pub fn name(&self) -> &'static str {
        match self {
            EngineState::Stopped => "stopped",
            EngineState::Starting => "starting",
            EngineState::Running => "running",
            EngineState::Error(_) => "error",
        }
    }
}

pub struct StartArgs {
    /// Whole GiB for `-modelstorage=<n>GiB` (FIXED quota).
    pub engine_gib: u64,
    /// Validated `host:port`s, one `-modelpeer=` each.
    pub peers: Vec<String>,
}

/// The exact argv after the program name. Kept separate so a test pins it.
pub fn helper_args(paths: &Paths, a: &StartArgs) -> Vec<String> {
    let mut v = vec![
        format!("-modeldir={}", paths.modeldir().display()),
        format!("-modelstorage={}GiB", a.engine_gib),
        // Rule 3: download only. No -modelbind, the app never listens.
        "-modelseed=manual".to_string(),
        // Rule 2: never fetch a model the user did not ask for.
        "-modelfollowpeers=0".to_string(),
        format!("-modelrpcsocket={}", paths.socket.display()),
    ];
    v.extend(a.peers.iter().map(|p| format!("-modelpeer={p}")));
    v
}

#[derive(Debug, Clone)]
struct ExitInfo {
    message: String,
}

struct Proc {
    pid: u32,
    exit: watch::Receiver<Option<ExitInfo>>,
    stopping: Arc<AtomicBool>,
}

pub struct Engine {
    paths: Paths,
    pub rpc: Rpc,
    life: Mutex<Option<Proc>>,
    state: Arc<StdMutex<EngineState>>,
    /// Set by `stop` while a `start` is under way (it holds `life` for the
    /// whole start, up to the import's ceiling on Windows): the start gives
    /// up at its next check instead of making Stop or Quit wait it out.
    cancel: AtomicBool,
    /// Set for good when the app quits or installs an update (`close`): no
    /// start after that, whoever asks (the self-heal, a WSL recovery).
    closed: AtomicBool,
    /// Helpers started, counted before Running is visible. The backend
    /// counts a run only after it has handed the new helper its index peers;
    /// until its count catches up with this one the run is not reported, and
    /// no start-up search goes out (backend/network.rs).
    spawns: std::sync::atomic::AtomicU64,
}

impl Engine {
    pub fn new(paths: Paths) -> Self {
        let rpc = Rpc::new(paths.socket.clone());
        Engine {
            paths,
            rpc,
            life: Mutex::new(None),
            state: Arc::new(StdMutex::new(EngineState::Stopped)),
            cancel: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            spawns: std::sync::atomic::AtomicU64::new(0),
        }
    }

    #[cfg(windows)]
    fn cancelled(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::SeqCst) {
            Err("the engine was stopped while starting".into())
        } else {
            Ok(())
        }
    }

    pub fn state(&self) -> EngineState {
        self.state.lock().unwrap().clone()
    }

    pub fn is_running(&self) -> bool {
        self.state() == EngineState::Running
    }

    /// How many helpers this app has started (see `spawns`).
    pub fn spawns(&self) -> u64 {
        self.spawns.load(Ordering::SeqCst)
    }

    fn set(&self, s: EngineState) {
        *self.state.lock().unwrap() = s;
    }

    /// Tests only: pretend the engine runs, so calls go to a fake socket.
    /// Going from not running to running counts as a helper started.
    /// Used by the unix-only tests (they need a fake engine on a socket).
    #[cfg(all(test, unix))]
    pub(crate) fn mark_running_for_test(&self, on: bool) {
        if on && !self.is_running() {
            self.spawns.fetch_add(1, Ordering::SeqCst);
        }
        self.set(if on { EngineState::Running } else { EngineState::Stopped });
    }

    /// Tests only: pretend a helper was started and is alive, so `start` sees
    /// a running one. `pid` is a harmless stand-in process the test owns.
    #[cfg(all(test, unix))]
    pub(crate) async fn pretend_started_for_test(&self, pid: u32) {
        let (tx, rx) = watch::channel(None);
        std::mem::forget(tx);
        *self.life.lock().await = Some(Proc { pid, exit: rx, stopping: Arc::new(AtomicBool::new(false)) });
        self.spawns.fetch_add(1, Ordering::SeqCst);
        self.set(EngineState::Running);
    }

    /// A failed start's last word (Windows: after AInode's own recovery
    /// steps), replacing the first failure's. Never over a running engine.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn failed(&self, e: &str) {
        let mut st = self.state.lock().unwrap();
        if matches!(*st, EngineState::Error(_)) {
            *st = EngineState::Error(e.to_string());
        }
    }

    /// The app is quitting or installing an update: this engine does not
    /// start again (until `reopen`). A start under way ends at its next
    /// check (`cancel`).
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// An update that could not be installed: AInode stays open, as it was,
    /// and its engine may start again.
    pub fn reopen(&self) {
        self.closed.store(false, Ordering::SeqCst);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Idempotent: a running helper is left alone. Ok(true) when this call
    /// started a helper, Ok(false) when one was running already: a new
    /// engine run (the start-up search, the index peers) begins only on true.
    pub async fn start(&self, args: StartArgs) -> Result<bool, String> {
        let mut life = self.life.lock().await;
        if let Some(p) = life.as_ref() {
            if p.exit.borrow().is_none() {
                return Ok(false);
            }
        }
        *life = None;
        self.cancel.store(false, Ordering::SeqCst);
        // After the reset: a close() at any moment is seen here or by the
        // start's own checks of `cancel`.
        if self.is_closed() {
            return Err("AInode is closing".into());
        }
        self.set(EngineState::Starting);
        match self.spawn_and_wait(&args).await {
            Ok(p) => {
                *life = Some(p);
                // Counted first: nobody sees Running for a helper not yet counted.
                self.spawns.fetch_add(1, Ordering::SeqCst);
                self.set(EngineState::Running);
                Ok(true)
            }
            Err(e) => {
                self.set(EngineState::Error(e.clone()));
                Err(e)
            }
        }
    }

    /// Windows: upstream's Linux helper inside AInode's own WSL distribution,
    /// started and relayed by `ainode-wsl` through one `wsl.exe`
    /// (docs/windows.md). The same argv and environment as on Linux; the RPC
    /// goes over that process's pipe instead of a unix socket.
    #[cfg(windows)]
    async fn spawn_and_wait(&self, args: &StartArgs) -> Result<Proc, String> {
        let p = &self.paths;
        // An update writes over the old engine folder and removes nothing.
        for name in remove_old_engine_files(&p.engine_dir) {
            eprintln!("[engine] removed {name}, left in the engine folder by an older AInode");
        }
        for f in [p.modeld(), p.engine_dir.join("ainode-wsl")] {
            if !f.is_file() {
                return Err(format!("engine file missing: {} (a dev build needs scripts/stage-engine-windows.sh)", f.display()));
            }
        }
        crate::wsl::ensure_distro(p).await?;
        self.cancelled()?;
        // A leftover helper from a crashed run goes first (it would save
        // catalog.json once more on its way out), then the peers are pinned.
        crate::wsl::clear_leftovers(p).await?;
        self.cancelled()?;
        std::fs::create_dir_all(p.modeldir()).map_err(|e| format!("create {}: {e}", p.modeldir().display()))?;
        pin_helper_peers(&p.modeldir().join("catalog.json"), &args.peers)?;
        let mut child = crate::wsl::engine_command(p, args)?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("could not start the engine in WSL: {e}"))?;
        // The job that ends llama-server with the app ends this wsl.exe too.
        // The Linux side then gets SIGHUP, and ainode-wsl and the helper are
        // gone within half a second (measured 2026-10-14); what they leave in
        // /tmp/ainode the next ainode-wsl clears.
        crate::chat::kill_with_app(&child);
        let pid = child.id().unwrap_or(0);
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err("the engine's pipe did not open".into());
        };
        let pipe = Arc::new(crate::rpc::Pipe::new(stdin, stdout));
        self.rpc.set_pipe(Some(pipe.clone()));
        let stderr_lines = Arc::new(StdMutex::new(VecDeque::<String>::new()));
        let reader = child.stderr.take().map(|err| tokio::spawn(pump(err, "btx-modeld!", Some(stderr_lines.clone()))));

        let (exit_tx, exit_rx) = watch::channel::<Option<ExitInfo>>(None);
        let stopping = Arc::new(AtomicBool::new(false));
        {
            let stopping = stopping.clone();
            let state = self.state.clone();
            let rpc = self.rpc.clone();
            let own = pipe.clone();
            tokio::spawn(async move {
                let status = child.wait().await;
                // Only this engine's pipe: a stop that gave up waiting for
                // wsl.exe may have let a new start install a newer one.
                rpc.clear_pipe_if(&own);
                if let Some(r) = reader {
                    let _ = tokio::time::timeout(Duration::from_secs(1), r).await;
                }
                // ainode-wsl copies the helper's stdout with a prefix and
                // writes its own notices as "ainode-wsl: note: ...": the
                // first line that is neither is the helper's error line.
                let first = stderr_lines
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|l| !l.trim().is_empty() && !l.starts_with("[stdout] ") && !l.starts_with("ainode-wsl: note: ") && !crate::wsl::is_wsl_note(l))
                    .cloned();
                let info = exit_info(status.ok(), first);
                eprintln!("[engine] btx-modeld exited: {}", info.message);
                if !stopping.load(Ordering::SeqCst) {
                    let mut st = state.lock().unwrap();
                    if *st == EngineState::Running {
                        *st = EngineState::Error(info.message.clone());
                    }
                }
                let _ = exit_tx.send(Some(info));
            });
        }
        let proc_ = Proc { pid, exit: exit_rx, stopping };
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Some(info) = proc_.exit.borrow().clone() {
                // wsl.exe itself failing (virtualization off since a firmware
                // reset, WSL removed) ends up here: plain words when its code
                // is one a user can act on.
                return Err(crate::wsl::explain(&info.message).map(str::to_string).unwrap_or(info.message));
            }
            if self.rpc.call_with("hello", json!([]), PROBE_TIMEOUT).await.is_ok() {
                return Ok(proc_);
            }
            if self.cancel.load(Ordering::SeqCst) {
                proc_.stopping.store(true, Ordering::SeqCst);
                self.rpc.clear_pipe_if(&pipe);
                terminate(pid, proc_.exit.clone()).await;
                return Err("the engine was stopped while starting".into());
            }
            if Instant::now() >= deadline {
                proc_.stopping.store(true, Ordering::SeqCst);
                self.rpc.clear_pipe_if(&pipe);
                terminate(pid, proc_.exit.clone()).await;
                return Err(format!("the engine did not become ready in {} s", READY_TIMEOUT.as_secs()));
            }
            sleep(Duration::from_millis(150)).await;
        }
    }

    #[cfg(unix)]
    async fn spawn_and_wait(&self, args: &StartArgs) -> Result<Proc, String> {
        let p = &self.paths;
        let mut need = vec![p.modeld()];
        // Linux: the OpenSSL 3.5 the helper links (helper_env).
        if cfg!(target_os = "linux") {
            need.push(p.engine_dir.join("lib").join("libssl.so.3"));
        }
        for f in need {
            if !f.is_file() {
                return Err(format!(
                    "engine file missing: {} (a dev build needs scripts/stage-engine.sh)",
                    f.display()
                ));
            }
        }
        for name in remove_old_engine_files(&p.engine_dir) {
            eprintln!("[engine] removed {name}, left in the engine folder by an older AInode");
        }
        prepare_private_dir(&p.socket_dir())?;
        self.clear_socket().await?;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(p.modeldir())
            .map_err(|e| format!("create {}: {e}", p.modeldir().display()))?;
        pin_helper_peers(&p.modeldir().join("catalog.json"), &args.peers)?;

        let mut cmd = Command::new(p.modeld());
        cmd.args(helper_args(p, args))
            // A clean, explicit environment: nothing from the user's shell
            // (DYLD_*, OPENSSL_*, BTX_OPENSSL, proxies...) reaches the helper.
            // It needs no OpenSSL tool: v0.34.16 mints its ML-DSA-44 identity
            // in-process (measured 2026-11-23, docs/modelnet/1-engine-move.md).
            .env_clear()
            .envs(child_env())
            .envs(helper_env(&p.engine_dir))
            .current_dir(&p.data_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Last resort only: shutdown() stops it gracefully first.
            .kill_on_drop(true);
        with_parent_death(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("could not start {}: {e}", p.modeld().display()))?;
        let pid = child.id().unwrap_or(0);
        // So a later run can find this helper even when it no longer answers
        // on the socket (busy, wedged): see clear_socket.
        write_pidfile(&p.modeld_pidfile(), pid);

        let stderr_lines = Arc::new(StdMutex::new(VecDeque::<String>::new()));
        let mut readers = Vec::new();
        if let Some(out) = child.stdout.take() {
            readers.push(tokio::spawn(pump(out, "btx-modeld", None)));
        }
        if let Some(err) = child.stderr.take() {
            readers.push(tokio::spawn(pump(err, "btx-modeld!", Some(stderr_lines.clone()))));
        }

        let (exit_tx, exit_rx) = watch::channel::<Option<ExitInfo>>(None);
        let stopping = Arc::new(AtomicBool::new(false));
        {
            let stopping = stopping.clone();
            let state = self.state.clone();
            let pidfile = p.modeld_pidfile();
            tokio::spawn(async move {
                let status = child.wait().await;
                remove_pidfile_if(&pidfile, pid);
                // Let the readers reach EOF so the first stderr line is in.
                for r in readers {
                    let _ = tokio::time::timeout(Duration::from_secs(1), r).await;
                }
                let first = stderr_lines.lock().unwrap().iter().find(|l| !l.trim().is_empty()).cloned();
                let info = exit_info(status.ok(), first);
                eprintln!("[engine] btx-modeld exited: {}", info.message);
                if !stopping.load(Ordering::SeqCst) {
                    let mut st = state.lock().unwrap();
                    // Only a helper that was up (or coming up) becomes an
                    // error; a failed start reports through start()'s Err.
                    if *st == EngineState::Running {
                        *st = EngineState::Error(info.message.clone());
                    }
                }
                let _ = exit_tx.send(Some(info));
            });
        }
        let proc_ = Proc { pid, exit: exit_rx, stopping };

        // Ready = the socket exists AND hello answers. A socket file alone
        // proves nothing (the helper replaces a stale one; observed).
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Some(info) = proc_.exit.borrow().clone() {
                return Err(info.message);
            }
            if p.socket.exists() && self.rpc.call_with("hello", json!([]), PROBE_TIMEOUT).await.is_ok() {
                return Ok(proc_);
            }
            if self.cancel.load(Ordering::SeqCst) {
                proc_.stopping.store(true, Ordering::SeqCst);
                terminate(pid, proc_.exit.clone()).await;
                return Err("the engine was stopped while starting".into());
            }
            if Instant::now() >= deadline {
                proc_.stopping.store(true, Ordering::SeqCst);
                terminate(pid, proc_.exit.clone()).await;
                return Err(format!("the engine did not become ready in {} s", READY_TIMEOUT.as_secs()));
            }
            sleep(Duration::from_millis(150)).await;
        }
    }

    /// RPC stop, SIGTERM after 3 s, SIGKILL after 8 s. Idempotent.
    pub async fn stop(&self) -> Result<(), String> {
        // Before the lock: a start in progress sees it and ends.
        self.cancel.store(true, Ordering::SeqCst);
        let mut life = self.life.lock().await;
        if let Some(p) = life.take() {
            p.stopping.store(true, Ordering::SeqCst);
            if p.exit.borrow().is_none() {
                let t0 = Instant::now();
                let _ = self.rpc.call_with("stop", json!([]), STOP_RPC_GRACE).await;
                if !wait_exit(p.exit.clone(), t0 + STOP_RPC_GRACE).await {
                    // Windows: closing the pipe makes ainode-wsl stop the
                    // helper itself; ending wsl.exe hangs it up (SIGHUP).
                    #[cfg(windows)]
                    self.rpc.set_pipe(None);
                    #[cfg(windows)]
                    let _ = wait_exit(p.exit.clone(), t0 + STOP_RPC_GRACE + Duration::from_secs(2)).await;
                    signal(p.pid, SIGTERM);
                    if !wait_exit(p.exit.clone(), t0 + STOP_TERM_GRACE).await {
                        signal(p.pid, SIGKILL);
                        wait_exit(p.exit.clone(), Instant::now() + Duration::from_secs(2)).await;
                    }
                }
            }
        }
        // A clean stop removes the socket (observed); a kill may leave it.
        if is_socket(&self.paths.socket) && !self.answers().await {
            let _ = std::fs::remove_file(&self.paths.socket);
        }
        self.set(EngineState::Stopped);
        Ok(())
    }

    async fn answers(&self) -> bool {
        self.rpc.call_with("hello", json!([]), PROBE_TIMEOUT).await.is_ok()
    }

    /// At app launch. Under the lifecycle lock, and only while this instance
    /// has started nothing: the UI may call start_engine the moment it loads,
    /// and the helper THAT starts must not be mistaken for an orphan.
    pub async fn cleanup_at_launch(&self) -> Result<(), String> {
        let life = self.life.lock().await;
        if life.is_some() {
            return Ok(());
        }
        self.clear_socket().await
    }

    /// Before every spawn (lifecycle lock held). Never leaves an orphan helper
    /// and never deletes a socket something is still serving.
    ///
    /// Two kinds of leftover from an earlier run that crashed or was killed:
    ///   - a helper that still answers on the socket: asked to stop over RPC,
    ///     then signalled;
    ///   - a helper that is alive but does NOT answer (busy on a long call,
    ///     wedged): the pid file finds it, and it is only signalled when that
    ///     pid's executable is our own btx-modeld (proc_pidpath), so a reused
    ///     pid is never hit. Without this, "no answer" would read as "dead",
    ///     the socket would be unlinked under a live helper, and a second
    ///     helper would start on the same store.
    ///
    /// Windows: nothing to clear on this side. The socket lives inside WSL,
    /// ainode-wsl clears it, and a helper whose app died was hung up with it.
    #[cfg(windows)]
    async fn clear_socket(&self) -> Result<(), String> {
        Ok(())
    }

    #[cfg(unix)]
    async fn clear_socket(&self) -> Result<(), String> {
        let sock = &self.paths.socket;
        let pidfile = self.paths.modeld_pidfile();
        let leftover = read_pidfile(&pidfile).filter(|&pid| is_executable(pid, &self.paths.modeld()));
        if is_socket(sock) && self.answers().await {
            eprintln!("[engine] a helper from an earlier run still answers on {}; stopping it", sock.display());
            self.stop_orphan(leftover).await;
        } else if let Some(pid) = leftover {
            eprintln!("[engine] btx-modeld {pid} from an earlier run is alive but not answering; stopping it");
            kill_escalating(pid, STOP_TERM_GRACE - STOP_RPC_GRACE).await;
        }
        let _ = std::fs::remove_file(&pidfile);
        match std::fs::symlink_metadata(sock) {
            Err(_) => Ok(()),
            Ok(m) if m.file_type().is_socket() => {
                if self.answers().await {
                    return Err(format!("another engine is still serving {}", sock.display()));
                }
                std::fs::remove_file(sock).map_err(|e| format!("remove stale socket {}: {e}", sock.display()))
            }
            // Only ever unlink a socket. Anything else at that path is not
            // ours to delete, and the helper could not bind there anyway.
            Ok(_) => Err(format!("{} exists and is not a socket; not touching it", sock.display())),
        }
    }

    #[cfg(unix)]
    async fn stop_orphan(&self, pid_hint: Option<u32>) {
        // The helper reports its own pid; it is serving OUR private socket, so
        // any btx-modeld (an older app version's path included) is accepted.
        let pid = self
            .rpc
            .call_with("getmodelnetworkinfo", json!([]), PROBE_TIMEOUT)
            .await
            .ok()
            .and_then(|v| v.get("helper_pid").and_then(Value::as_u64))
            .and_then(|p| u32::try_from(p).ok())
            .filter(|&p| executable_path(p).is_some_and(|x| x.file_name().is_some_and(|n| n == "btx-modeld")))
            .or(pid_hint);
        let t0 = Instant::now();
        let _ = self.rpc.call_with("stop", json!([]), STOP_RPC_GRACE).await;
        let gone = |pid: Option<u32>| pid.map_or(!is_socket(&self.paths.socket), |p| !alive(p));
        while !gone(pid) && t0.elapsed() < STOP_RPC_GRACE {
            sleep(Duration::from_millis(100)).await;
        }
        if let Some(p) = pid.filter(|&p| alive(p)) {
            kill_escalating(p, STOP_TERM_GRACE - STOP_RPC_GRACE).await;
        }
    }
}

/// btx-modeld remembers every peer it was ever given, `-modelpeer` or
/// `addmodelnode`, in `<modeldir>/catalog.json` ("peers"), and has no RPC to
/// forget one (catalog.cpp:1172-1182, 500-507). In 0.34.15, two live hosts for
/// the same model make a retrieve fail with 403 "grant identity" (experiments
/// E22, upstream/e22-issue.md). So before each start, with the helper stopped,
/// its remembered list is set to exactly the hosts this start passes. Only the
/// "peers" array changes; a missing or unreadable file is left alone.
pub fn pin_helper_peers(catalog: &Path, peers: &[String]) -> Result<(), String> {
    let Ok(raw) = std::fs::read(catalog) else { return Ok(()) };
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&raw) else { return Ok(()) };
    let Some(obj) = v.as_object_mut() else { return Ok(()) };
    let want = serde_json::Value::from(peers.to_vec());
    if obj.get("peers") == Some(&want) {
        return Ok(());
    }
    obj.insert("peers".into(), want);
    crate::state::write_json_atomic(catalog, &v)?;
    #[cfg(unix)]
    std::fs::set_permissions(catalog, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod {}: {e}", catalog.display()))?;
    Ok(())
}

/// The socket's parent must be ours and 0700: upstream's next release refuses
/// anything else (contract "Paths"), and today it keeps other local users off
/// the control socket.
///
/// Windows: the folder is under the user's profile (`%TEMP%`), whose ACL
/// already keeps other users out; there are no modes to set.
#[cfg(windows)]
pub fn prepare_private_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    if !std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
        return Err(format!("{} is not a directory", dir.display()));
    }
    Ok(())
}

#[cfg(unix)]
pub fn prepare_private_dir(dir: &Path) -> Result<(), String> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("create {}: {e}", dir.display()))?;
    let md = std::fs::symlink_metadata(dir).map_err(|e| format!("stat {}: {e}", dir.display()))?;
    if !md.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    if md.uid() != me {
        return Err(format!("{} is owned by uid {}, not us ({me})", dir.display(), md.uid()));
    }
    if md.permissions().mode() & 0o777 != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("chmod 0700 {}: {e}", dir.display()))?;
    }
    Ok(())
}

/// What the user reads when the engine cannot be loaded because the system's
/// C++ library is older than the engine needs. Since v0.34.16 upstream's
/// Linux helper takes libstdc++ from the system and needs GCC 14's
/// (CXXABI_1.3.15). A `.deb` cannot be installed without it, but an AppImage
/// updates itself on any system, and then the helper does not load.
pub const OLD_CXX_RUNTIME: &str = "The engine cannot start on this system: its C++ library (libstdc++) is older than the engine needs. \
The engine needs the one that comes with GCC 14 or newer; Ubuntu 24.04, Debian 13 and Fedora 40 or newer have it. \
AInode 0.8.4 was the last version that brought its own.";

/// The dynamic loader's words for that: `.../btx-modeld: /lib64/libstdc++.so.6:
/// version `CXXABI_1.3.15' not found (required by .../btx-modeld)`, or no
/// libstdc++ at all.
fn old_cxx_runtime(first_stderr_line: &str) -> bool {
    let l = first_stderr_line;
    ((l.contains("GLIBCXX_") || l.contains("CXXABI_")) && l.contains("not found"))
        || ((l.contains("libstdc++.so") || l.contains("libgcc_s.so")) && l.contains("cannot open shared object file"))
}

/// What the helper in the engine folder is, as its MANIFEST.json says: the
/// BTX release, and whether AInode built it from upstream's source (the Mac)
/// or it is upstream's own file (Linux, Windows). The staging scripts write
/// this from the tag they pin, and Status shows it. The interface keeps no
/// version of its own: 0.9.0's first build still said "BTX v0.34.15,
/// unmodified" from a line written by hand.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Bundled {
    /// "v0.34.16"
    pub version: String,
    pub built_by_ainode: bool,
}

/// None when the manifest is missing, unreadable or does not say (a dev run
/// with an engine folder staged by an older script): Status then names no
/// version at all, which is true.
pub fn bundled(engine_dir: &Path) -> Option<Bundled> {
    let raw = std::fs::read(engine_dir.join("MANIFEST.json")).ok()?;
    let manifest = serde_json::from_slice::<serde_json::Value>(&raw).ok()?;
    let e = manifest.get("engine")?;
    let version = e.get("btx_version")?.as_str()?;
    // Shown to the user: a release tag and nothing else.
    let tag = version.strip_prefix('v')?;
    let parts: Vec<&str> = tag.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || p.len() > 4 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let built_by_ainode = match e.get("helper")?.as_str()? {
        "built-by-ainode" => true,
        "upstream-unmodified" => false,
        _ => return None,
    };
    Some(Bundled { version: version.to_string(), built_by_ainode })
}

/// What AInode 0.8.4 and older put in the engine folder and this version
/// does not: the OpenSSL tool with its wrapper and config, and the two
/// libraries upstream's v0.34.15 archive shipped and v0.34.16 takes from the
/// system instead.
pub const OLD_ENGINE_FILES: [&str; 5] = ["openssl", "openssl.sh", "openssl.cnf", "lib/libstdc++.so.6", "lib/libgcc_s.so.1"];

/// Remove those files from an engine folder that an update left them in.
/// A Windows update writes the new files over the old ones and removes
/// nothing (hooks.nsh), and the helper is started with `LD_LIBRARY_PATH` on
/// `lib/`: an old `lib/libstdc++.so.6` there would be loaded in place of the
/// system's, a file that is not in MANIFEST.json. Only these five names, and
/// only when the folder's MANIFEST.json can be read and does not name them.
/// Returns what it removed. A folder that cannot be written (a Mac bundle, a
/// `.deb` under /usr) is left as it is; neither can hold such leftovers.
pub fn remove_old_engine_files(engine_dir: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read(engine_dir.join("MANIFEST.json")) else { return Vec::new() };
    let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&raw) else { return Vec::new() };
    let Some(files) = manifest.get("files").and_then(serde_json::Value::as_array) else { return Vec::new() };
    let listed = |name: &str| files.iter().any(|f| f.get("path").and_then(serde_json::Value::as_str).is_some_and(|p| p.replace('\\', "/") == name));
    let mut removed = Vec::new();
    for name in OLD_ENGINE_FILES {
        if listed(name) {
            continue;
        }
        let path = name.split('/').fold(engine_dir.to_path_buf(), |acc, part| acc.join(part));
        let is_file = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() || m.file_type().is_symlink());
        if is_file && std::fs::remove_file(&path).is_ok() {
            removed.push(name.to_string());
        }
    }
    removed
}

fn exit_info(status: Option<std::process::ExitStatus>, first_stderr: Option<String>) -> ExitInfo {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    let line = first_stderr.map(|l| l.trim().to_string()).unwrap_or_default();
    let with = |s: String| if line.is_empty() { s } else { format!("{s}: {line}") };
    if old_cxx_runtime(&line) {
        return ExitInfo { message: format!("{OLD_CXX_RUNTIME} The system said: {line}") };
    }
    // Windows: wsl.exe passes ainode-wsl's code on, which is the helper's own
    // code, or 128 + the signal that ended it.
    #[cfg(windows)]
    let sig = |s: &std::process::ExitStatus| s.code().filter(|c| (129..160).contains(c)).map(|c| c - 128);
    #[cfg(unix)]
    let sig = |s: &std::process::ExitStatus| s.signal();
    match status {
        Some(s) => match (s.code().filter(|_| sig(&s).is_none()), sig(&s)) {
            (Some(EXIT_FAIL_CLOSED), _) => {
                ExitInfo { message: with("The engine refused to start (fail-closed, exit 2)".into()) }
            }
            (Some(c), _) => ExitInfo { message: with(format!("The engine exited with code {c}")) },
            (None, Some(sig)) => ExitInfo { message: with(format!("The engine was stopped by signal {sig}")) },
            _ => ExitInfo { message: with("The engine exited".into()) },
        },
        None => ExitInfo { message: with("The engine exited".into()) },
    }
}

/// Linux: the child gets SIGTERM if this process dies without stopping it
/// (a crash, SIGKILL): no helper or llama-server left holding the store or
/// gigabytes of RAM until the next launch. macOS has no such flag (the next
/// launch's cleanup finds them); Windows uses a job object (chat.rs
/// `kill_with_app`).
#[cfg(unix)]
pub(crate) fn with_parent_death(cmd: &mut Command) {
    #[cfg(target_os = "linux")]
    // SAFETY: prctl is async-signal-safe and runs in the child before exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = cmd;
}

/// Forward a child's output to our stderr line by line; optionally keep the
/// first lines for error messages. Keeps draining so the pipe never fills,
/// whatever bytes arrive: a line that is not UTF-8 (wsl.exe's own messages
/// come as UTF-16 on some builds, a model's metadata may be anything) is
/// shown lossily, never the end of the pump, which would close the pipe and
/// kill the child with SIGPIPE on its next write.
pub(crate) async fn pump<R: tokio::io::AsyncRead + Unpin>(
    r: R,
    tag: &'static str,
    keep: Option<Arc<StdMutex<VecDeque<String>>>>,
) {
    let mut rd = BufReader::new(r);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match rd.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(buf.strip_suffix(b"\n").unwrap_or(&buf)).trim_end_matches('\r').to_string();
        eprintln!("[{tag}] {line}");
        if let Some(k) = &keep {
            let mut k = k.lock().unwrap();
            // The FIRST stderr line is the contract's error text; keep the
            // first few and the most recent few.
            if k.len() < 8 {
                k.push_back(line);
            } else {
                if k.len() >= 64 {
                    k.remove(8);
                }
                k.push_back(line);
            }
        }
    }
}

async fn wait_exit(mut rx: watch::Receiver<Option<ExitInfo>>, deadline: Instant) -> bool {
    loop {
        if rx.borrow().is_some() {
            return true;
        }
        match timeout_at(deadline, rx.changed()).await {
            Err(_) => return false,
            Ok(Err(_)) => return true, // supervisor gone: the process was reaped
            Ok(Ok(())) => {}
        }
    }
}

async fn terminate(pid: u32, rx: watch::Receiver<Option<ExitInfo>>) {
    signal(pid, SIGTERM);
    if !wait_exit(rx.clone(), Instant::now() + Duration::from_secs(5)).await {
        signal(pid, SIGKILL);
        wait_exit(rx, Instant::now() + Duration::from_secs(2)).await;
    }
}

#[cfg(unix)]
pub(crate) fn signal(pid: u32, sig: libc::c_int) {
    if pid > 1 {
        // SAFETY: plain kill(2) on a pid we spawned or verified by name.
        unsafe {
            libc::kill(pid as libc::pid_t, sig);
        }
    }
}

#[cfg(unix)]
pub(crate) fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks existence/permission.
    pid > 1 && unsafe { libc::kill(pid as libc::pid_t, 0) } == 0
}

/// Windows: no signals; both end the process (callers ask nicely first).
#[cfg(windows)]
pub(crate) fn signal(pid: u32, _sig: i32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    if pid <= 4 {
        return;
    }
    // SAFETY: a handle we open, use once and close.
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            TerminateProcess(h, 1);
            CloseHandle(h);
        }
    }
}

#[cfg(windows)]
pub(crate) fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    if pid <= 4 {
        return false;
    }
    // SAFETY: a handle we open, query once and close.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}

/// Windows: QueryFullProcessImageNameW.
#[cfg(windows)]
pub(crate) fn executable_path(pid: u32) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    if pid <= 4 {
        return None;
    }
    let mut buf = vec![0u16; 32768];
    let mut len = buf.len() as u32;
    // SAFETY: buf holds `len` u16s; the handle is closed before returning.
    let ok = unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(h);
        ok
    };
    ok.then(|| PathBuf::from(std::ffi::OsString::from_wide(&buf[..len as usize])))
}

/// The executable path of a running pid (proc_pidpath), so a pid read from a
/// file or an RPC reply is only ever signalled when it is the program we
/// expect, never a stranger that inherited a recycled pid.
#[cfg(target_os = "macos")]
pub(crate) fn executable_path(pid: u32) -> Option<PathBuf> {
    if pid <= 1 {
        return None;
    }
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: proc_pidpath writes at most `buffersize` bytes into buf.
    let n = unsafe { libc::proc_pidpath(pid as libc::c_int, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(PathBuf::from(std::ffi::OsString::from_vec(buf)))
}

/// Linux: the `/proc/<pid>/exe` link. A binary deleted or unmounted since it
/// started (an AppImage that has exited) reads `<path> (deleted)`; the path
/// is what counts.
#[cfg(target_os = "linux")]
pub(crate) fn executable_path(pid: u32) -> Option<PathBuf> {
    if pid <= 1 {
        return None;
    }
    let p = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?.into_os_string().into_vec();
    let p = p.strip_suffix(b" (deleted)").map(<[u8]>::to_vec).unwrap_or(p);
    Some(PathBuf::from(std::ffi::OsString::from_vec(p)))
}

/// `pid` is alive and running exactly `exe` (both sides canonicalised).
pub(crate) fn is_executable(pid: u32, exe: &Path) -> bool {
    let (Some(actual), Ok(want)) = (executable_path(pid), std::fs::canonicalize(exe)) else {
        return false;
    };
    let actual = std::fs::canonicalize(&actual).unwrap_or(actual);
    actual == want || same_inside_appimage(&actual, &want)
}

/// An AppImage mounts itself at a new `/tmp/.mount_<name><random>` on every
/// launch. A helper left by a crashed earlier run is our binary at the same
/// place INSIDE its mount, but under another mount dir: compared below the
/// mount, it is ours. Only on Linux, and only when both paths are in one.
fn same_inside_appimage(a: &Path, b: &Path) -> bool {
    fn below_mount(p: &Path) -> Option<&Path> {
        let mut it = p.components();
        while let Some(c) = it.next() {
            if c.as_os_str().to_string_lossy().starts_with(".mount_") {
                return Some(it.as_path()).filter(|rest| !rest.as_os_str().is_empty());
            }
        }
        None
    }
    cfg!(target_os = "linux") && matches!((below_mount(a), below_mount(b)), (Some(x), Some(y)) if x == y)
}

/// SIGTERM, wait up to `grace`, then SIGKILL (which also ends a stopped or
/// wedged process).
pub(crate) async fn kill_escalating(pid: u32, grace: Duration) {
    signal(pid, SIGTERM);
    let t0 = Instant::now();
    while alive(pid) && t0.elapsed() < grace {
        sleep(Duration::from_millis(100)).await;
    }
    if alive(pid) {
        signal(pid, SIGKILL);
        let t1 = Instant::now();
        while alive(pid) && t1.elapsed() < Duration::from_secs(2) {
            sleep(Duration::from_millis(50)).await;
        }
    }
}

pub(crate) fn read_pidfile(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok().filter(|&p| p > 1)
}

pub(crate) fn write_pidfile(path: &Path, pid: u32) {
    use std::io::Write;
    let _ = std::fs::remove_file(path);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    o.mode(0o600);
    if let Ok(mut f) = o.open(path) {
        let _ = writeln!(f, "{pid}");
    }
}

/// Remove a pid file only if it still names `pid` (a newer process may have
/// replaced it).
pub(crate) fn remove_pidfile_if(path: &Path, pid: u32) {
    if read_pidfile(path) == Some(pid) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
pub(crate) fn is_socket(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_socket())
}

/// Windows: the socket is inside WSL, never on this side.
#[cfg(windows)]
pub(crate) fn is_socket(_p: &Path) -> bool {
    false
}

/// What only the helper adds to child_env. Linux: upstream's btx-modeld
/// needs the OpenSSL 3.5, SQLite, libgomp, zlib and zstd its release ships in
/// `lib/` (its RUNPATH `$ORIGIN/../lib` is for upstream's own layout, not
/// ours); its own `bin/btx-modeld` wrapper sets LD_LIBRARY_PATH the same
/// way. macOS: nothing, it links only the OS.
pub fn helper_env(engine_dir: &Path) -> Vec<(String, std::ffi::OsString)> {
    if cfg!(target_os = "linux") {
        vec![("LD_LIBRARY_PATH".to_string(), engine_dir.join("lib").into_os_string())]
    } else {
        Vec::new()
    }
}

/// Windows: the same idea, with what any Windows process needs to start
/// (SystemRoot, or Winsock fails; the profile and temp folders). PATH is the
/// system folders only. Nothing passes into WSL: WSLENV is not set.
#[cfg(windows)]
pub(crate) fn child_env() -> Vec<(String, std::ffi::OsString)> {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let mut path = std::ffi::OsString::from(&root);
    path.push("\\System32;");
    path.push(&root);
    let mut v = vec![("PATH".to_string(), path)];
    for k in ["SystemRoot", "windir", "SystemDrive", "TEMP", "TMP", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "ProgramData", "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE"] {
        if let Some(val) = std::env::var_os(k) {
            v.push((k.to_string(), val));
        }
    }
    v
}

/// The whole environment a child process gets: an allowlist, not the user's
/// shell. PATH is fixed to the system directories.
#[cfg(unix)]
pub(crate) fn child_env() -> Vec<(String, std::ffi::OsString)> {
    let mut v = vec![("PATH".to_string(), std::ffi::OsString::from("/usr/bin:/bin:/usr/sbin:/sbin"))];
    for k in ["HOME", "TMPDIR"] {
        if let Some(val) = std::env::var_os(k) {
            v.push((k.to_string(), val));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::path::PathBuf;

    #[test]
    fn pin_helper_peers_rewrites_only_the_peer_list() {
        let tmp = tempfile::tempdir().unwrap();
        let c = tmp.path().join("catalog.json");
        // Missing file: nothing to do, nothing created.
        pin_helper_peers(&c, &["a:1".into()]).unwrap();
        assert!(!c.exists());
        let orig = serde_json::json!({
            "models": [{"model_id": "m", "bytes": 2244022910u64, "files": [{"sha384": "ab"}]}],
            "peers": ["127.0.0.1:29447", "localhost:29447", "203.0.113.7:29447"],
        });
        std::fs::write(&c, serde_json::to_vec(&orig).unwrap()).unwrap();
        pin_helper_peers(&c, &["203.0.113.7:29447".into()]).unwrap();
        let got: serde_json::Value = serde_json::from_slice(&std::fs::read(&c).unwrap()).unwrap();
        assert_eq!(got["peers"], serde_json::json!(["203.0.113.7:29447"]));
        assert_eq!(got["models"], orig["models"], "models untouched");
        #[cfg(unix)]
        assert_eq!(std::fs::metadata(&c).unwrap().permissions().mode() & 0o777, 0o600);
        // No host at all: the helper must forget them all.
        pin_helper_peers(&c, &[]).unwrap();
        let got: serde_json::Value = serde_json::from_slice(&std::fs::read(&c).unwrap()).unwrap();
        assert_eq!(got["peers"], serde_json::json!([]));
        // A file it cannot read as JSON is left exactly as it was.
        std::fs::write(&c, b"not json").unwrap();
        pin_helper_peers(&c, &["a:1".into()]).unwrap();
        assert_eq!(std::fs::read(&c).unwrap(), b"not json");
    }

    #[cfg(unix)]
    #[test]
    fn argv_is_the_contract() {
        let paths = Paths {
            data_dir: PathBuf::from("/D"),
            engine_dir: PathBuf::from("/E"),
            socket: PathBuf::from("/T/ainode/modeld.sock"),
        };
        let a = StartArgs { engine_gib: 11, peers: vec!["h.example:29447".into(), "[::1]:29447".into()] };
        assert_eq!(
            helper_args(&paths, &a),
            vec![
                "-modeldir=/D/modelnet",
                "-modelstorage=11GiB",
                "-modelseed=manual",
                "-modelfollowpeers=0",
                "-modelrpcsocket=/T/ainode/modeld.sock",
                "-modelpeer=h.example:29447",
                "-modelpeer=[::1]:29447",
            ]
        );
        assert!(!helper_args(&paths, &a).iter().any(|x| x.starts_with("-modelbind")));
    }

    #[cfg(unix)]
    #[test]
    fn exit_two_is_fail_closed_with_first_stderr_line() {
        use std::os::unix::process::ExitStatusExt;
        let i = exit_info(
            Some(std::process::ExitStatus::from_raw(2 << 8)),
            Some("model subsystem fail-closed: openssl mldsa44 certificate generation failed".into()),
        );
        assert!(i.message.contains("fail-closed, exit 2"), "{}", i.message);
        assert!(i.message.ends_with("openssl mldsa44 certificate generation failed"));
        let k = exit_info(Some(std::process::ExitStatus::from_raw(9)), None);
        assert!(k.message.contains("signal 9"), "{}", k.message);
    }

    #[cfg(unix)]
    #[test]
    fn an_old_cxx_runtime_is_said_in_plain_words() {
        use std::os::unix::process::ExitStatusExt;
        // What ld.so prints (exit 1 or 127) when libstdc++ is GCC 13's.
        let line = "/tmp/.mount_AInodeXyz/usr/lib/AInode/resources/engine/btx-modeld: /lib64/libstdc++.so.6: version `CXXABI_1.3.15' not found (required by /tmp/.mount_AInodeXyz/usr/lib/AInode/resources/engine/btx-modeld)";
        let i = exit_info(Some(std::process::ExitStatus::from_raw(1 << 8)), Some(line.into()));
        assert!(i.message.starts_with(OLD_CXX_RUNTIME), "{}", i.message);
        assert!(i.message.contains("GCC 14") && i.message.ends_with(line), "{}", i.message);
        for l in [
            "btx-modeld: /usr/lib/x86_64-linux-gnu/libstdc++.so.6: version `GLIBCXX_3.4.32' not found (required by btx-modeld)",
            "btx-modeld: error while loading shared libraries: libstdc++.so.6: cannot open shared object file: No such file or directory",
        ] {
            assert!(exit_info(Some(std::process::ExitStatus::from_raw(127 << 8)), Some(l.into())).message.starts_with(OLD_CXX_RUNTIME), "{l}");
        }
        // Anything else keeps the plain exit message.
        for l in ["model subsystem fail-closed: unix dir mode", "btx-modeld: error while loading shared libraries: libssl.so.3: cannot open shared object file", ""] {
            let m = exit_info(Some(std::process::ExitStatus::from_raw(2 << 8)), Some(l.into())).message;
            assert!(!m.contains("C++ library"), "{m}");
        }
    }

    /// The whole start path with a stand-in for a helper that cannot be
    /// loaded: it fails as the dynamic loader does where libstdc++ is GCC
    /// 13's. The user gets the plain sentence, in the error and in the state.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_helper_that_cannot_load_on_an_old_system_gives_the_plain_message() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let engine = tmp.path().join("engine");
        std::fs::create_dir_all(engine.join("lib")).unwrap();
        std::fs::write(engine.join("lib").join("libssl.so.3"), b"").unwrap();
        let script = "#!/bin/sh\necho \"$0: /lib64/libstdc++.so.6: version \\`CXXABI_1.3.15' not found (required by $0)\" >&2\nexit 1\n";
        std::fs::write(engine.join("btx-modeld"), script).unwrap();
        std::fs::set_permissions(engine.join("btx-modeld"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let paths = Paths { data_dir: tmp.path().join("data"), engine_dir: engine, socket: tmp.path().join("s").join("m.sock") };
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        let e = Engine::new(paths);
        let err = e.start(StartArgs { engine_gib: 1, peers: vec![] }).await.unwrap_err();
        assert!(err.starts_with(OLD_CXX_RUNTIME) && err.contains("CXXABI_1.3.15"), "{err}");
        assert!(matches!(e.state(), EngineState::Error(m) if m.starts_with(OLD_CXX_RUNTIME)), "{:?}", e.state());
    }

    #[test]
    fn what_the_helper_is_comes_from_the_engine_folders_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let e = tmp.path();
        assert_eq!(bundled(e), None, "no manifest");
        let put = |v: serde_json::Value| std::fs::write(e.join("MANIFEST.json"), v.to_string()).unwrap();
        put(serde_json::json!({"files": []}));
        assert_eq!(bundled(e), None, "a manifest from before the field");
        put(serde_json::json!({"engine": {"btx_version": "v0.34.16", "helper": "built-by-ainode"}, "files": []}));
        assert_eq!(bundled(e), Some(Bundled { version: "v0.34.16".into(), built_by_ainode: true }));
        put(serde_json::json!({"engine": {"btx_version": "v0.34.16", "helper": "upstream-unmodified"}, "files": []}));
        assert_eq!(bundled(e), Some(Bundled { version: "v0.34.16".into(), built_by_ainode: false }));
        // Anything that is not a plain release tag, or an origin this code
        // does not know, is not shown.
        for (v, h) in [("0.34.16", "built-by-ainode"), ("v0.34", "built-by-ainode"), ("v0.34.16<b>", "built-by-ainode"), ("v0.34.16", "patched"), ("", "")] {
            put(serde_json::json!({"engine": {"btx_version": v, "helper": h}}));
            assert_eq!(bundled(e), None, "{v} {h}");
        }
    }

    /// The pin and the text shown cannot disagree: each staging script writes
    /// `engine` into its manifest from the tag it pins, the Mac's as built by
    /// AInode and the Linux one as upstream's file, and Windows passes the
    /// Linux one on. All of them pin the same tag.
    #[test]
    fn the_staging_scripts_write_what_the_helper_is_from_their_pin() {
        let read = |name: &str| std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("scripts").join(name)).unwrap();
        let pin = |s: &str| s.lines().find_map(|l| l.strip_prefix("BTX_TAG=")).map(str::to_string).expect("BTX_TAG=");
        let (mac, linux, windows) = (read("stage-engine.sh"), read("stage-engine-linux.sh"), read("stage-engine-windows.sh"));
        assert_eq!(pin(&mac), pin(&linux), "the Mac and Linux staging pin the same BTX release");
        assert!(pin(&mac).starts_with('v') && pin(&mac).len() >= 6, "{}", pin(&mac));
        for (s, origin) in [(&mac, "built-by-ainode"), (&linux, "upstream-unmodified")] {
            assert!(s.contains(r#"--arg btx "$BTX_TAG""#), "the manifest takes the version from the pin");
            assert!(s.contains(&format!(r#"engine:{{btx_version:$btx, helper:"{origin}"}}"#)), "{origin}");
        }
        assert!(windows.contains(r#"jq -c '.engine // empty' "$WORK/linux/MANIFEST.json""#) && windows.contains("engine:$engine"));
        // The Linux archive and its folder are named from the same tag.
        assert!(linux.contains("BTX_ARCHIVE=btx-${BTX_TAG#v}-x86_64-linux-gnu.tar.gz") && linux.contains("BTX_ROOT=btx-${BTX_TAG#v}"));
        // An engine folder staged here says the pinned tag (skipped when
        // nothing is staged, as in a fresh checkout).
        let staged = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("engine");
        if staged.join("MANIFEST.json").is_file() && bundled(&staged).is_some() {
            assert_eq!(bundled(&staged), Some(Bundled { version: pin(&mac), built_by_ainode: cfg!(target_os = "macos") }));
        }
    }

    #[test]
    fn files_an_older_version_left_in_the_engine_folder_are_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let e = tmp.path();
        std::fs::create_dir_all(e.join("lib")).unwrap();
        let all = ["btx-modeld", "openssl", "openssl.sh", "openssl.cnf", "LICENSE-openssl.txt", "lib/libssl.so.3", "lib/libstdc++.so.6", "lib/libgcc_s.so.1"];
        let fill = || {
            for f in all {
                std::fs::write(e.join(f), b"x").unwrap();
            }
        };
        let there = || all.iter().filter(|f| e.join(f).exists()).copied().collect::<Vec<_>>();
        // No manifest, or an unreadable one: nothing is touched.
        fill();
        assert!(remove_old_engine_files(e).is_empty());
        std::fs::write(e.join("MANIFEST.json"), b"not json").unwrap();
        assert!(remove_old_engine_files(e).is_empty());
        assert_eq!(there(), all);
        // The new version's manifest: the five leftovers go, nothing else.
        let new = serde_json::json!({"files": [{"path": "btx-modeld"}, {"path": "LICENSE-openssl.txt"}, {"path": "lib/libssl.so.3"}]});
        std::fs::write(e.join("MANIFEST.json"), new.to_string()).unwrap();
        let mut gone = remove_old_engine_files(e);
        gone.sort();
        assert_eq!(gone, ["lib/libgcc_s.so.1", "lib/libstdc++.so.6", "openssl", "openssl.cnf", "openssl.sh"]);
        assert_eq!(there(), ["btx-modeld", "LICENSE-openssl.txt", "lib/libssl.so.3"]);
        assert!(remove_old_engine_files(e).is_empty(), "nothing left to remove");
        // A manifest that names them (0.8.4's own, or a later version that
        // ships a libstdc++ again): they stay.
        fill();
        let old = serde_json::json!({"files": all.iter().map(|p| serde_json::json!({"path": p})).collect::<Vec<_>>()});
        std::fs::write(e.join("MANIFEST.json"), old.to_string()).unwrap();
        assert!(remove_old_engine_files(e).is_empty());
        assert_eq!(there(), all);
    }

    #[cfg(unix)]
    #[test]
    fn pid_checks_use_the_real_executable_path() {
        let me = std::process::id();
        let exe = std::env::current_exe().unwrap();
        assert!(is_executable(me, &exe));
        assert!(!is_executable(me, Path::new("/bin/sh")));
        assert!(!is_executable(0, &exe));
        let tmp = tempfile::tempdir().unwrap();
        let pf = tmp.path().join("x.pid");
        write_pidfile(&pf, 4242);
        assert_eq!(read_pidfile(&pf), Some(4242));
        remove_pidfile_if(&pf, 1);
        assert!(pf.exists(), "a pid file naming another pid stays");
        remove_pidfile_if(&pf, 4242);
        assert!(!pf.exists());
        // Only a real socket counts as one.
        std::fs::write(tmp.path().join("plain"), b"x").unwrap();
        assert!(!is_socket(&tmp.path().join("plain")));
        let _l = std::os::unix::net::UnixListener::bind(tmp.path().join("s.sock")).unwrap();
        assert!(is_socket(&tmp.path().join("s.sock")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_non_socket_at_the_socket_path_is_never_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let sockdir = std::env::temp_dir().join(format!("ainb-ns-{}", std::process::id()));
        std::fs::create_dir_all(&sockdir).unwrap();
        let paths = Paths { data_dir: tmp.path().into(), engine_dir: tmp.path().into(), socket: sockdir.join("m.sock") };
        std::fs::write(&paths.socket, b"precious").unwrap();
        let e = Engine::new(paths.clone());
        assert!(e.cleanup_at_launch().await.unwrap_err().contains("not a socket"));
        assert_eq!(std::fs::read(&paths.socket).unwrap(), b"precious");
        std::fs::remove_dir_all(&sockdir).unwrap();
    }

    /// Quit or an update install closed it: whoever asks after that (the
    /// self-heal, a WSL recovery), it never starts, and nothing is spawned.
    #[tokio::test]
    async fn a_closed_engine_never_starts_again() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths { data_dir: tmp.path().join("d"), engine_dir: tmp.path().join("e"), socket: tmp.path().join("s").join("m.sock") };
        let e = Engine::new(paths);
        e.close();
        let err = e.start(StartArgs { engine_gib: 1, peers: vec![] }).await.unwrap_err();
        assert!(err.contains("closing"), "{err}");
        assert_eq!(e.state(), EngineState::Stopped, "not even Starting");
        assert!(e.is_closed());
        // A failed update install reopens it: a start is tried again (and
        // fails here only for the missing engine files).
        e.reopen();
        let err = e.start(StartArgs { engine_gib: 1, peers: vec![] }).await.unwrap_err();
        assert!(!err.contains("closing"), "{err}");
    }

    #[test]
    fn an_appimage_mount_dir_does_not_hide_our_own_helper() {
        let a = Path::new("/tmp/.mount_AInodeQx1/usr/lib/AInode/resources/engine/btx-modeld");
        let b = Path::new("/tmp/.mount_AInodeZz9/usr/lib/AInode/resources/engine/btx-modeld");
        let other = Path::new("/tmp/.mount_AInodeZz9/usr/lib/AInode/resources/engine/llama/llama-server");
        assert_eq!(same_inside_appimage(a, b), cfg!(target_os = "linux"));
        assert!(!same_inside_appimage(a, other));
        assert!(!same_inside_appimage(Path::new("/usr/lib/AInode/btx-modeld"), Path::new("/opt/AInode/btx-modeld")));
        assert!(!same_inside_appimage(Path::new("/tmp/.mount_A"), Path::new("/tmp/.mount_B")));
    }

    #[test]
    fn the_helper_gets_its_libraries_on_linux_only() {
        let env = helper_env(Path::new("/E"));
        if cfg!(target_os = "linux") {
            assert_eq!(env, vec![("LD_LIBRARY_PATH".to_string(), std::ffi::OsString::from("/E/lib"))]);
        } else {
            assert!(env.is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn private_dir_is_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().join("ainode");
        std::fs::create_dir(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        prepare_private_dir(&d).unwrap();
        assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700);
        let f = tmp.path().join("file");
        std::fs::write(&f, b"x").unwrap();
        assert!(prepare_private_dir(&f).is_err());
    }

    /// The real helper needs nothing but itself: an engine folder that holds
    /// only btx-modeld (Linux: and its lib/), no OpenSSL tool, no wrapper, no
    /// BTX_OPENSSL. It must start, answer, and stop. On v0.34.15 this start
    /// exited 2 ("openssl mldsa44 certificate generation failed").
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "needs AINODE_TEST_ENGINE_DIR (a staged engine dir)"]
    async fn real_helper_starts_with_no_openssl_tool() {
        let engine = PathBuf::from(std::env::var_os("AINODE_TEST_ENGINE_DIR").expect("set AINODE_TEST_ENGINE_DIR"));
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("engine");
        std::fs::create_dir(&bare).unwrap();
        std::os::unix::fs::symlink(engine.join("btx-modeld"), bare.join("btx-modeld")).unwrap();
        if engine.join("lib").is_dir() {
            std::os::unix::fs::symlink(engine.join("lib"), bare.join("lib")).unwrap();
        }
        let sockdir = std::env::temp_dir().join(format!("ainb-no-{}", std::process::id()));
        let paths = Paths { data_dir: tmp.path().join("data"), engine_dir: bare, socket: sockdir.join("m.sock") };
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        let e = Engine::new(paths.clone());
        e.start(StartArgs { engine_gib: 1, peers: vec![] }).await.expect("the helper starts with no OpenSSL tool");
        assert_eq!(e.state(), EngineState::Running);
        assert!(paths.modeldir().join("tls").join("cert.pem").is_file(), "it minted its identity itself");
        e.stop().await.unwrap();
        let _ = std::fs::remove_dir_all(sockdir);
    }
}
