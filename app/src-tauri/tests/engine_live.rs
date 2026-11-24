//! End to end against the REAL binaries: upstream btx-modeld fetching the
//! 350M catalog model from a live host, then llama-server answering a chat
//! request from the runnable copy. Ignored by default (needs a staged engine
//! dir and a reachable host). Run:
//!
//!   AINODE_TEST_ENGINE_DIR=$PWD/resources/engine \
//!   AINODE_TEST_PEER=127.0.0.1:29447 \
//!   AINODE_TEST_DATA_DIR=/some/scratch/dir \
//!   cargo test --test engine_live -- --ignored --nocapture
//!
//! Never beside an AInode's own folders: the test takes its socket folder
//! from the temp dir (`ainb-it-<pid>`, its own) and its data from
//! AINODE_TEST_DATA_DIR, and stops before it touches anything if either is
//! an installed AInode's (`state::dev_run_refusal`). Run it in debug, as
//! above: a `--release` test build would write the start-at-login entry
//! (login.rs). The engine dir is one staged for the v0.34.16 engine: no
//! OpenSSL tool in it, and the helper is started without BTX_OPENSSL.
//!
//! Windows: the same flow, with the helper inside AInode's own WSL
//! distribution (docs/windows.md). The test imports `AInode` into its data
//! dir itself, so no distribution of that name may exist when it starts
//! (`wsl --unregister AInode`), and it unregisters it at the end. The engine
//! dir is what scripts/stage-engine-windows.sh staged; on a PC whose GPU must
//! stay idle, take `llama/ggml-vulkan.dll` out of a copy so the chat runs on
//! the CPU. In PowerShell:
//!
//!   $env:AINODE_TEST_ENGINE_DIR = "$PWD\resources\engine"
//!   $env:AINODE_TEST_PEER = "20.196.8.98:29447"
//!   $env:AINODE_TEST_DATA_DIR = "$env:TEMP\ainode-it"
//!   cargo test --test engine_live -- --ignored --nocapture
//!
//! Drives `Backend`, the same code the Tauri commands call. The 3B model is
//! never downloaded: its getmodel is only used to make the helper enforce its
//! quota (it refuses before fetching a single piece, the store is 1 GiB).
//! The flow is one; what differs per platform (where the helper runs, how it
//! is found, killed or frozen, where a crashed app's leftovers are cleared)
//! is `sys` at the bottom.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ainode_lib::backend::Backend;
use ainode_lib::catalog;
use ainode_lib::chat;
use ainode_lib::engine::EngineState;
use ainode_lib::state::{self, Paths};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const SMALL: &str = "granite-4.0-350m-q4km";
const BIG: &str = "granite-4.2-3b-q4km";

fn sha256_file(p: &Path) -> String {
    let mut f = std::fs::File::open(p).unwrap();
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

async fn listed(b: &Backend, uri: &str) -> Option<Value> {
    let v = b.engine.rpc.call("listmodels", json!([])).await.unwrap();
    v["models"].as_array().unwrap().iter().find(|m| m["uri"] == uri).cloned()
}

#[derive(Debug, PartialEq)]
struct Row {
    local: &'static str,
    progress: f64,
    runnable: bool,
}

/// The catalog row the UI shows. The 350M is not offered, so the catalog
/// lists it only while it holds disk (catalog::shown): not listed reads as
/// nothing local.
async fn row(b: &Backend, uri: &str) -> Row {
    match b.catalog().await.into_iter().find(|r| r.uri == uri) {
        Some(r) => Row { local: r.local, progress: r.progress, runnable: r.runnable },
        None => Row { local: "none", progress: 0.0, runnable: false },
    }
}

fn stored_once(paths: &Paths) -> Value {
    std::fs::read(paths.stored_once_file())
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_else(|| json!({"models": {}}))
}

/// Download through the backend and watch the catalog until ready.
async fn download_until_ready(b: &Backend, uri: &str) -> Vec<(u128, &'static str, f64, u64)> {
    let t = Instant::now();
    b.download(uri).await.unwrap();
    b.download(uri).await.unwrap(); // idempotent
    let mut seen: Vec<(u128, &'static str, f64, u64)> = Vec::new();
    loop {
        let r = row(b, uri).await;
        let st = b.get_state().await;
        let e = (t.elapsed().as_millis(), r.local, r.progress, st.network.peers_connected);
        if seen.last().map(|l| (l.1, l.2)) != Some((e.1, e.2)) {
            println!("  t={}ms local={} progress={:.4} peers_connected={}", e.0, e.1, e.2, e.3);
            seen.push(e);
        }
        if r.local == "ready" {
            return seen;
        }
        assert_eq!(r.local, "downloading", "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(180), "download did not finish");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// getmodel the 3B straight over RPC (the backend refuses it: too big) and
/// wait for the helper to give up on it.
async fn provoke_quota(b: &Backend, big_uri: &str) -> String {
    let r = b.engine.rpc.call("getmodel", json!([big_uri, "FREE_ONLY"])).await.unwrap();
    let id = r["job_id"].as_str().expect("job").to_string();
    let t = Instant::now();
    loop {
        let jobs = b.engine.rpc.call("getmodeljob", json!([id])).await.unwrap();
        let j = &jobs["jobs"][0];
        if j["status"] != "running" && j["status"] != "queued" {
            return format!("{} {}", j["status"], j["error"]);
        }
        if t.elapsed() > Duration::from_secs(20) {
            b.engine.rpc.call("cancelmodeljob", json!([id])).await.unwrap();
            return format!("still {} after 20 s ({}), cancelled", j["status"], j["last_err"]);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs AINODE_TEST_ENGINE_DIR and a reachable model host"]
async fn download_pin_chat_remove_stop() {
    // The whole flow is one large future; on the heap, not the test thread's
    // 2 MiB stack.
    tokio::spawn(flow()).await.unwrap();
}

async fn flow() {
    let engine_dir = PathBuf::from(std::env::var("AINODE_TEST_ENGINE_DIR").expect("set AINODE_TEST_ENGINE_DIR"));
    let peer = std::env::var("AINODE_TEST_PEER").unwrap_or_else(|_| "127.0.0.1:29447".into());
    let base = std::env::var_os("AINODE_TEST_DATA_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let data_dir = base.join(format!("it-{}", std::process::id()));
    let sock_dir = std::env::temp_dir().join(format!("ainb-it-{}", std::process::id()));
    let paths = Paths { data_dir: data_dir.clone(), engine_dir, socket: sock_dir.join("modeld.sock") };
    paths.check().unwrap();
    // Before anything is touched: the backend's cleanup works on the socket's
    // folder and stops a helper that answers there.
    if let Some(why) = state::dev_run_refusal(&paths, &state::LiveInstall::here()) {
        panic!("this test must not run on an installed AInode's folders: {why}");
    }
    // Windows: the test's own `AInode` distribution, gone again at the end.
    let distro = sys::Distro::claim().await;
    let models = catalog::bundled().models;
    let small = models.iter().find(|m| m.id == SMALL).unwrap().clone();
    let big = models.iter().find(|m| m.id == BIG).unwrap().clone();
    let uri = small.uri.clone();

    let b = std::sync::Arc::new(Backend::new(paths.clone(), "0.0.0-test".into()));

    // ── consent gate ────────────────────────────────────────────────────────
    let err = b.start_engine().await.unwrap_err();
    println!("start before consent refused: {err}");
    assert!(!data_dir.exists() && !sock_dir.exists(), "nothing on disk before consent");
    b.accept_consent(4, None).await.unwrap(); // 4 GB -> a 1 GiB engine store
    b.add_peer(&peer).await.unwrap();

    // ── start ───────────────────────────────────────────────────────────────
    let t = Instant::now();
    b.start_engine().await.unwrap();
    let st = b.get_state().await;
    println!(
        "engine {} in {:?}: openssl={:?} pq1_ready={:?} peers_configured={}",
        st.engine.state,
        t.elapsed(),
        st.engine.openssl,
        st.engine.pq1_ready,
        st.network.peers_configured
    );
    assert_eq!(st.engine.state, "running");
    assert_eq!(st.engine.pq1_ready, Some(true));
    // The OpenSSL inside the helper (its post-quantum TLS), not a tool.
    assert!(st.engine.openssl.as_deref().unwrap_or("").starts_with("OpenSSL 3."));
    assert!(!paths.engine_dir.join("openssl").exists() && !paths.engine_dir.join("openssl.sh").exists(), "no OpenSSL tool in the engine folder");
    assert_eq!(st.network.peers_configured, 1);
    let pid1 = sys::helper_pid(&b, &paths).await;
    assert!(sys::alive(pid1).await, "the helper is running ({})", sys::WHERE);
    sys::assert_isolated(&paths).await;
    let r = row(&b, &uri).await;
    assert_eq!((r.local, r.progress, r.runnable), ("none", 0.0, false));

    // ── the store-size check refuses the 3B before asking the helper ────────
    let err = b.download(&big.uri).await.unwrap_err();
    println!("3B refused by the size check: {err}");
    assert!(err.contains("needs 2.2 GB"), "{err}");

    // ── download (twice = one job), then it gets pinned ─────────────────────
    let seen = download_until_ready(&b, &uri).await;
    assert_eq!(seen.first().unwrap().1, "downloading");
    assert!(seen.first().unwrap().2 < 1.0, "first observation must be < 1");
    assert!(seen.windows(2).all(|w| w[1].2 >= w[0].2), "progress never goes back");
    let jobs = b.engine.rpc.call("getmodeljob", json!([])).await.unwrap();
    println!("getmodeljob after two download() calls: {} job(s)", jobs["jobs"].as_array().unwrap().len());
    assert_eq!(jobs["jobs"].as_array().unwrap().len(), 1, "download must be idempotent per uri");
    let t = Instant::now();
    while listed(&b, &uri).await.unwrap()["pinned"] != true {
        assert!(t.elapsed() < Duration::from_secs(5), "never pinned");
        let _ = b.catalog().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    println!("pinned by the backend after {:?}", t.elapsed());

    // ── quota enforcement: pinned survives; the engine's delete makes room ──
    assert!(!row(&b, &uri).await.runnable, "no runnable copy before exportmodelpath");
    let out = provoke_quota(&b, &big.uri).await;
    let still = listed(&b, &uri).await;
    println!("3B getmodel against a 1 GiB store with the 350M PINNED: job {out}; 350M listed={}", still.is_some());
    assert!(still.is_some_and(|m| m["complete"] == true), "a pinned model must survive quota enforcement");
    b.engine.rpc.call("unpinmodel", json!([uri])).await.unwrap();
    let out = provoke_quota(&b, &big.uri).await;
    let kept = listed(&b, &uri).await;
    println!("3B getmodel with the 350M UNPINNED: job {out}; 350M listed={}", kept.is_some());
    // v0.34.15 evicted the unpinned 350M here, for a model that could never
    // fit. v0.34.16 refuses the 3B ("disk quota") and keeps what it holds
    // (measured 2026-11-23). Whether it evicts for a model that WOULD fit is
    // not covered: the host offers no second model small enough.
    assert!(out.contains("disk quota"), "{out}");
    assert!(kept.is_some_and(|m| m["complete"] == true), "v0.34.16 gives up nothing for a model that cannot fit");
    // The engine's own delete, new in v0.34.16 (btxchain/btx#232), makes
    // the room the next step needs.
    let r = b.engine.rpc.call("removemodel", json!([uri])).await.unwrap();
    println!("removemodel: {r}");
    assert!(listed(&b, &uri).await.is_none(), "removemodel makes the engine forget the model");
    // (The catalog snapshot is up to 1 s old: wait it out.)
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(row(&b, &uri).await, Row { local: "none", progress: 0.0, runnable: false });

    // ── fetch it again, then the runnable copy: byte-identical, kept once ───
    download_until_ready(&b, &uri).await;
    let store = data_dir.join("modelnet").join("store");
    let store_before = ainode_lib::disk::tree_bytes(&store);
    // 0.8.0 (plan 3.4): the poll that sees the download done keeps it once in
    // the background, before any chat: the Tauri `catalog` command does this.
    let t = Instant::now();
    let rows = b.catalog().await;
    assert!(b.settle_downloads(&rows), "a keep-once task starts after the download");
    assert!(!b.settle_downloads(&rows), "one at a time");
    // A chat pressed at that very moment waits for the keep-once (which
    // restarts the engine) instead of reaching a helper that is stopping.
    let early = tokio::time::timeout(Duration::from_secs(180), b.chat_start(&uri)).await.expect("chat_start hung");
    let early = early.expect("a chat started during the keep-once works");
    println!("a chat pressed during the keep-once started after {:?} (port {})", t.elapsed(), early.port);
    tokio::time::timeout(Duration::from_secs(180), b.settled()).await.expect("never kept once after the download");
    assert!(stored_once(&paths)["models"].get(&uri).is_some(), "kept once");
    assert_eq!(b.engine.state(), EngineState::Running, "running again after the keep-once restart");
    println!("kept once in the background {:?} after the download, before any chat", t.elapsed());
    let gguf = b.runnable_path(&uri).await.unwrap();
    let sha = sha256_file(&gguf);
    println!("runnable gguf sha256 {sha}");
    assert_eq!(sha, "771c588a49607f274a2bba3185733607ebe6f74b996ab90e2d6bee0d98bcec52");
    assert_eq!(sha, small.gguf_sha256().unwrap());
    // 0.7.1 (store_once.rs): the verified copy is now the model, its pieces
    // are deleted and the engine restarted so its accounting matches.
    let store_after = ainode_lib::disk::tree_bytes(&store);
    println!("kept once: store {store_before} -> {store_after} bytes, stored-once.json {}", stored_once(&paths));
    assert!(stored_once(&paths)["models"].get(&uri).is_some(), "recorded as kept once");
    assert!(store_before >= small.size_bytes && store_after < small.size_bytes / 10, "{store_before} -> {store_after}");
    assert_eq!(b.engine.state(), EngineState::Running, "restarted after the pieces went");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(row(&b, &uri).await, Row { local: "ready", progress: 1.0, runnable: true });
    let st = b.get_state().await;
    println!("usage after kept once: {:?}", st.usage);
    assert_eq!(st.usage.runnable_bytes, small.size_bytes);
    assert!(st.usage.engine_used_bytes < small.size_bytes / 10, "{}", st.usage.engine_used_bytes);

    // ── chat ────────────────────────────────────────────────────────────────
    let t = Instant::now();
    let c = b.chat_start(&uri).await.unwrap();
    println!("llama-server ready on 127.0.0.1:{} in {:?}", c.port, t.elapsed());
    assert_eq!(b.chat_start(&uri).await.unwrap(), c, "same model reuses the server and key");
    let err = b.remove(&uri).await.unwrap_err();
    println!("remove while chatting refused: {err}");
    assert!(err.contains("chat"));
    let body = json!({
        "messages": [{"role": "user", "content": "Reply with one word: hello."}],
        "max_tokens": 16,
        "temperature": 0
    })
    .to_string();
    let path = "/v1/chat/completions";
    let (code, _) = chat::http(c.port, "POST", path, Some(&body), None, Duration::from_secs(30)).await.unwrap();
    println!("chat without the key: HTTP {code}");
    assert_eq!(code, 401);
    let (code, _) = chat::http(c.port, "POST", path, Some(&body), Some("wrong"), Duration::from_secs(30)).await.unwrap();
    assert_eq!(code, 401, "a wrong key is refused too");
    let (code, _) = chat::http(c.port, "GET", "/slots", None, Some(&c.key), Duration::from_secs(5)).await.unwrap();
    println!("GET /slots with the key: HTTP {code} (disabled)");
    assert_ne!(code, 200);
    let (code, reply) = chat::http(c.port, "POST", path, Some(&body), Some(&c.key), Duration::from_secs(120)).await.unwrap();
    assert_eq!(code, 200, "{reply}");
    let v: Value = serde_json::from_str(&reply).unwrap();
    let text = v["choices"][0]["message"]["content"].as_str().unwrap_or("");
    println!("chat with the key: {text:?}");
    assert!(!text.trim().is_empty());
    b.chat_stop().await;
    assert!(!chat::health_ok(c.port).await, "llama-server must be gone after chat_stop");

    // ── remove frees the space at once; Get fetches it again ────────────────
    let checkout = gguf.parent().unwrap().to_path_buf();
    let held_before = sys::held_bytes(&paths);
    b.remove(&uri).await.unwrap();
    let held_after = sys::held_bytes(&paths);
    println!("Remove: AInode's folder {held_before} -> {held_after} bytes");
    assert!(!checkout.exists(), "checkout dir deleted");
    assert!(held_before.saturating_sub(held_after) >= small.size_bytes, "the copy's bytes are free again");
    assert!(stored_once(&paths)["models"].get(&uri).is_none(), "no longer kept once");
    assert!(listed(&b, &uri).await.is_none(), "the engine forgot it (its catalog entry is gone)");
    assert_eq!(row(&b, &uri).await, Row { local: "none", progress: 0.0, runnable: false });
    // The engine forgot it, so the backend dropped its "removed" mark too
    // (backend.rs remove): the refusal comes from the engine now.
    let err = b.chat_start(&uri).await.expect_err("no chat on a removed model");
    println!("chat on the removed model refused: {err}");
    let seen = download_until_ready(&b, &uri).await;
    assert_eq!(seen.first().unwrap().1, "downloading", "a real fetch, not an answer from stale pieces");
    let t = Instant::now();
    while listed(&b, &uri).await.unwrap()["pinned"] != true {
        assert!(t.elapsed() < Duration::from_secs(5), "re-download never pinned");
        let _ = b.catalog().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // ── the helper dies under us: surfaced as an error ──────────────────────
    // (Kept once restarted it: ask for the helper running now.)
    let pid1 = sys::helper_pid(&b, &paths).await;
    sys::kill(pid1).await;
    let t = Instant::now();
    while !matches!(b.engine.state(), EngineState::Error(_)) && t.elapsed() < Duration::from_secs(10) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!("after the helper was killed ({}): {:?}", sys::KILL, b.engine.state());
    assert!(matches!(b.engine.state(), EngineState::Error(_)));
    b.stop_engine().await.unwrap();

    // ── leftovers of a crashed app (a second launch can no longer happen:
    //    single-instance focuses the first) ─────────────────────────────────
    let b2 = Backend::new(paths.clone(), "0.0.0-test".into());
    #[cfg(unix)]
    {
        // The app clears them at launch (startup_cleanup).
        // (a) a leftover helper that still answers: asked to stop.
        let mut lo = sys::leftover_helper(&paths, &peer).await;
        sys::wait_answering(&mut lo, &paths).await;
        b2.startup_cleanup().await;
        println!("answering leftover {}: alive after cleanup = {}", lo.pid, sys::alive(lo.pid).await);
        assert!(!sys::alive(lo.pid).await && !paths.socket.exists());
        // (b) a leftover that is alive but does NOT answer (frozen): found by
        //     the pid file + executable path, not mistaken for dead, killed.
        let mut lo = sys::leftover_helper(&paths, &peer).await;
        sys::wait_answering(&mut lo, &paths).await;
        sys::freeze(lo.pid).await;
        let t = Instant::now();
        b2.startup_cleanup().await;
        println!("frozen leftover {}: alive after cleanup = {} ({:?})", lo.pid, sys::alive(lo.pid).await, t.elapsed());
        assert!(!sys::alive(lo.pid).await, "a frozen leftover must be killed, not left beside a new helper");
        assert!(!paths.socket.exists(), "its stale socket is unlinked");
    }
    #[cfg(windows)]
    {
        // The app clears nothing at launch (the socket is inside WSL):
        // ainode-wsl clears leftovers before it starts the helper, through
        // the pid file and socket in /tmp/ainode. So each case is "the next
        // start succeeds, and the leftover is gone".
        // (a) a leftover helper that still answers: asked to stop.
        let mut lo = sys::leftover_helper(&paths, &peer).await;
        sys::wait_answering(&mut lo, &paths).await;
        b2.startup_cleanup().await;
        let t = Instant::now();
        b2.start_engine().await.unwrap();
        let gone = sys::wait_dead(lo.pid, Duration::from_secs(5)).await;
        let n = sys::helpers_running().await;
        println!("answering leftover {}: gone after the next start = {gone} ({:?}); helpers inside WSL: {n}", lo.pid, t.elapsed());
        assert!(gone);
        assert_eq!(n, 1, "one helper on the store");
        let notice = lo.exit_notice().await;
        println!("the leftover's host announced: {notice}");
        assert!(notice.contains("\"ainode_exit\""), "{notice}");
        b2.stop_engine().await.unwrap();
        // (b) a leftover that is alive but does NOT answer (frozen): found by
        //     the pid file + executable path, not mistaken for dead, killed.
        let mut lo = sys::leftover_helper(&paths, &peer).await;
        sys::wait_answering(&mut lo, &paths).await;
        sys::freeze(lo.pid).await;
        let t = Instant::now();
        b2.start_engine().await.unwrap();
        let gone = sys::wait_dead(lo.pid, Duration::from_secs(5)).await;
        let n = sys::helpers_running().await;
        println!("frozen leftover {}: gone after the next start = {gone} ({:?}); helpers inside WSL: {n}", lo.pid, t.elapsed());
        assert!(gone, "a frozen leftover must be killed, not left beside a new helper");
        assert_eq!(n, 1, "one helper on the store");
        assert!(t.elapsed() < Duration::from_secs(20), "found through its pid file, not by timing out the start");
        let notice = lo.exit_notice().await;
        println!("the leftover's host announced: {notice}");
        assert!(notice.contains("\"signal\":9"), "{notice}");
    }

    // ── a fresh start finds the store intact ────────────────────────────────
    b2.start_engine().await.unwrap();
    assert_eq!(row(&b2, &uri).await.local, "ready");
    let t = Instant::now();
    b2.stop_engine().await.unwrap();
    println!("stopped in {:?}", t.elapsed());
    assert_eq!(b2.get_state().await.engine.state, "stopped");
    assert!(!paths.socket.exists() && !paths.modeld_pidfile().exists());
    sys::assert_nothing_runs().await;
    b.shutdown().await;
    distro.release().await;

    let _ = std::fs::remove_dir_all(&sock_dir);
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// macOS and Linux: the helper is a child on this machine, named by its pid
/// file, signalled directly.
#[cfg(unix)]
mod sys {
    use super::*;
    use ainode_lib::engine::{helper_args, helper_env, StartArgs};

    pub const WHERE: &str = "the pid file names it";
    pub const KILL: &str = "SIGKILL";

    /// Nothing to claim: the helper needs no distribution here.
    pub struct Distro;

    impl Distro {
        pub async fn claim() -> Distro {
            Distro
        }
        pub async fn release(self) {}
    }

    pub async fn helper_pid(_b: &Backend, paths: &Paths) -> u32 {
        std::fs::read_to_string(paths.modeld_pidfile()).unwrap().trim().parse::<u32>().unwrap()
    }

    pub async fn alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    pub async fn kill(pid: u32) {
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    }

    pub async fn freeze(pid: u32) {
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGSTOP) };
    }

    pub fn held_bytes(paths: &Paths) -> u64 {
        ainode_lib::disk::tree_bytes(&paths.data_dir)
    }

    pub struct Leftover {
        pub pid: u32,
    }

    /// A helper this test process does not supervise, like one left behind by
    /// a crashed app. Reaped by a thread so a dead one does not linger as a
    /// zombie.
    pub async fn leftover_helper(paths: &Paths, peer: &str) -> Leftover {
        let mut c = std::process::Command::new(paths.modeld())
            .args(helper_args(paths, &StartArgs { engine_gib: 1, peers: vec![peer.to_string()] }))
            .env_clear()
            .envs(helper_env(&paths.engine_dir))
            .env("PATH", "/usr/bin:/bin")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = c.id();
        std::thread::spawn(move || {
            let _ = c.wait();
        });
        std::fs::write(paths.modeld_pidfile(), format!("{pid}\n")).unwrap();
        Leftover { pid }
    }

    pub async fn wait_answering(_lo: &mut Leftover, paths: &Paths) {
        let t = Instant::now();
        while ainode_lib::rpc::raw_call(&paths.socket, "hello", json!([])).await.is_err() {
            assert!(t.elapsed() < Duration::from_secs(15), "leftover helper never answered");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn assert_nothing_runs() {}

    /// The helper runs as this user, with this user's files: nothing to check.
    pub async fn assert_isolated(_paths: &Paths) {}
}

/// Windows: the helper runs inside AInode's own WSL distribution, started
/// and relayed by ainode-wsl through one `wsl.exe` (docs/windows.md). It is
/// reached with `wsl.exe` commands inside that distribution.
#[cfg(windows)]
mod sys {
    use super::*;
    use ainode_lib::engine::StartArgs;
    use ainode_lib::wsl::{self, SOCKET};
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
    use tokio::process::{Child, ChildStdin, ChildStdout, Command};

    pub const WHERE: &str = "inside AInode's WSL distribution, the pid the engine reports";
    pub const KILL: &str = "kill -9 inside WSL";
    const NO_WINDOW: u32 = 0x0800_0000;

    fn wsl_exe() -> PathBuf {
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        root.join("System32").join("wsl.exe")
    }

    fn wsl() -> Command {
        let mut c = Command::new(wsl_exe());
        // UTF-8 output instead of UTF-16, so the lists below read as text.
        c.env("WSL_UTF8", "1").creation_flags(NO_WINDOW).stdin(Stdio::null());
        c
    }

    /// `sh -c <script>` inside the distribution as root: (success, stdout).
    async fn in_distro(script: &str) -> (bool, String) {
        let out = wsl()
            .args(["-d", wsl::distro(), "-u", "root", "--cd", "/", "--exec", "sh", "-c", script])
            .output()
            .await
            .expect("wsl.exe");
        (out.status.success(), String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// wsl.exe's text: UTF-8 with WSL_UTF8, UTF-16LE from a build that
    /// ignores it (then every second byte of these ASCII names is NUL).
    fn text(b: &[u8]) -> String {
        if b.contains(&0) {
            let (pairs, _) = b.as_chunks::<2>();
            let u: Vec<u16> = pairs.iter().map(|&c| u16::from_le_bytes(c)).collect();
            String::from_utf16_lossy(&u)
        } else {
            String::from_utf8_lossy(b).into_owned()
        }
    }

    async fn distributions(running_only: bool) -> Vec<String> {
        let mut c = wsl();
        c.arg("--list");
        if running_only {
            c.arg("--running");
        }
        let out = c.arg("--quiet").output().await.expect("wsl.exe");
        assert!(out.status.success(), "wsl --list failed: {}", text(&out.stderr));
        text(&out.stdout)
            .lines()
            .map(|l| l.trim().trim_start_matches('\u{feff}').to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    fn unregister() {
        let _ = std::process::Command::new(wsl_exe())
            .args(["--unregister", wsl::distro()])
            .creation_flags(NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    /// The test's own `AInode` distribution: none may exist before it (the
    /// app's own, or a crashed run's), the engine imports it into the test's
    /// data dir on the first start, and it is unregistered at the end, also
    /// when the test panics (Drop), before the data dir it lives in goes.
    pub struct Distro {
        armed: bool,
    }

    impl Distro {
        pub async fn claim() -> Distro {
            let have = distributions(false).await;
            assert!(
                !have.iter().any(|d| d == wsl::distro()),
                "a WSL distribution named {0} exists (the app's own, or a crashed run's): wsl --unregister {0} first, then run again", wsl::distro()
            );
            Distro { armed: true }
        }

        /// What the app does at quit, then the test's own cleanup.
        pub async fn release(mut self) {
            wsl::terminate().await;
            let running = distributions(true).await;
            println!("after terminate(): distributions running = {running:?}");
            assert!(!running.iter().any(|d| d == wsl::distro()), "AInode's distribution still runs after terminate()");
            unregister();
            self.armed = false;
            assert!(!distributions(false).await.iter().any(|d| d == wsl::distro()), "unregistered");
        }
    }

    impl Drop for Distro {
        fn drop(&mut self) {
            if self.armed {
                unregister();
            }
        }
    }

    /// Inside the distribution only the engine folder (read-only) and the
    /// models folder (never executable) are there: no drive mounted whole
    /// (security audit M1).
    pub async fn assert_isolated(paths: &Paths) {
        let (ok, mounts) = in_distro("awk '$1 ~ /^[A-Za-z]:/ {print $2, $4}' /proc/mounts").await;
        assert!(ok, "read /proc/mounts");
        println!("Windows folders inside the distribution:
{mounts}");
        let lines: Vec<&str> = mounts.lines().collect();
        assert_eq!(lines.len(), 2, "exactly the engine and the models: {mounts}");
        let esc = |p: &Path| wsl::linux_path(p).unwrap().replace(' ', r"\040");
        let engine = lines.iter().find(|l| l.starts_with(&format!("{} ", esc(&paths.engine_dir)))).expect("the engine folder");
        let models = lines.iter().find(|l| l.starts_with(&format!("{} ", esc(&paths.modeldir())))).expect("the models folder");
        assert!(engine.contains(" ro,"), "{engine}");
        assert!(models.contains(" rw,") && models.contains("noexec"), "{models}");
        let inside = wsl::linux_path(&paths.engine_dir).unwrap();
        let (_, probe) = in_distro(&format!("test -e /mnt/c/Windows && echo windows-visible; touch '{inside}/x' 2>/dev/null && echo engine-writable; :")).await;
        assert_eq!(probe, "", "no C drive, no writing the engine");
        // A clean boot: WSL says nothing about fstab on a new session.
        let out = wsl().args(["-d", wsl::distro(), "-u", "root", "--cd", "/", "--exec", "true"]).output().await.expect("wsl.exe");
        assert_eq!(text(&out.stderr).trim(), "", "a new session's stderr");
    }

    pub async fn helper_pid(b: &Backend, _paths: &Paths) -> u32 {
        let v = b.engine.rpc.call("getmodelnetworkinfo", json!([])).await.unwrap();
        v["helper_pid"].as_u64().expect("helper_pid in getmodelnetworkinfo") as u32
    }

    /// Answered by the distribution itself, so a wsl.exe that failed to run
    /// the check is a test failure, not "dead".
    pub async fn alive(pid: u32) -> bool {
        match in_distro(&format!("kill -0 {pid} 2>/dev/null && echo alive || echo dead")).await {
            (true, s) if s == "alive" => true,
            (true, s) if s == "dead" => false,
            other => panic!("wsl.exe did not run the liveness check: {other:?}"),
        }
    }

    pub async fn wait_dead(pid: u32, t: Duration) -> bool {
        let t0 = Instant::now();
        while alive(pid).await {
            if t0.elapsed() > t {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        true
    }

    pub async fn kill(pid: u32) {
        assert!(in_distro(&format!("kill -9 {pid}")).await.0, "kill -9 {pid} inside WSL");
    }

    pub async fn freeze(pid: u32) {
        assert!(in_distro(&format!("kill -STOP {pid}")).await.0, "kill -STOP {pid} inside WSL");
    }

    /// AInode's folder without the distribution's disk image, which lives in
    /// it too (`wsl/`) and grows on its own.
    pub fn held_bytes(paths: &Paths) -> u64 {
        ainode_lib::disk::tree_bytes(&paths.data_dir).saturating_sub(ainode_lib::disk::tree_bytes(&paths.data_dir.join("wsl")))
    }

    /// btx-modeld processes inside the distribution.
    pub async fn helpers_running() -> usize {
        in_distro("n=0; for p in /proc/[0-9]*; do case \"$(readlink $p/exe 2>/dev/null)\" in */btx-modeld) n=$((n+1));; esac; done; echo $n")
            .await
            .1
            .parse()
            .unwrap_or(usize::MAX)
    }

    /// A helper this test's backend does not supervise: its own `wsl.exe` and
    /// ainode-wsl, exactly as the app starts them, with the pipe held open
    /// (an app that hangs instead of quitting).
    pub struct Leftover {
        pub pid: u32,
        _child: Child,
        stdin: ChildStdin,
        lines: Lines<BufReader<ChildStdout>>,
    }

    impl Leftover {
        async fn ask(&mut self, method: &str) -> Value {
            let line = format!("{}\n", json!({"jsonrpc": "1.0", "id": 1, "method": method, "params": []}));
            self.stdin.write_all(line.as_bytes()).await.unwrap();
            self.stdin.flush().await.unwrap();
            let reply = tokio::time::timeout(Duration::from_secs(15), self.lines.next_line())
                .await
                .expect("the leftover's host did not answer")
                .unwrap()
                .expect("the leftover's host closed its pipe");
            assert!(!reply.starts_with("{\"ainode_exit\""), "the leftover's helper ended before it was asked: {reply}");
            serde_json::from_str(&reply).unwrap()
        }

        /// The `{"ainode_exit":...}` line its host writes when its helper has
        /// ended.
        pub async fn exit_notice(&mut self) -> String {
            tokio::time::timeout(Duration::from_secs(15), self.lines.next_line())
                .await
                .expect("no exit notice from the leftover's host")
                .unwrap()
                .unwrap_or_default()
        }
    }

    pub async fn leftover_helper(paths: &Paths, peer: &str) -> Leftover {
        let mut cmd = wsl::engine_command(paths, &StartArgs { engine_gib: 1, peers: vec![peer.to_string()] }).unwrap();
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        Leftover { pid: 0, _child: child, stdin, lines }
    }

    /// Through the leftover's own pipe: until hello answers, then the pid
    /// the helper reports for itself.
    pub async fn wait_answering(lo: &mut Leftover, _paths: &Paths) {
        let t = Instant::now();
        loop {
            let r = lo.ask("hello").await;
            if r["error"]["code"] != "RELAY_IO" {
                break;
            }
            assert!(t.elapsed() < Duration::from_secs(15), "leftover helper never answered: {r}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let info = lo.ask("getmodelnetworkinfo").await;
        lo.pid = info["result"]["helper_pid"].as_u64().expect("helper_pid") as u32;
    }

    /// After a clean stop: no socket and no helper inside the distribution.
    pub async fn assert_nothing_runs() {
        let sock = in_distro(&format!("test -e {SOCKET}")).await.0;
        let n = helpers_running().await;
        println!("inside the distribution after stop: socket exists = {sock}, helpers running = {n}");
        assert!(!sock, "the helper's socket inside WSL is gone after a clean stop");
        assert_eq!(n, 0, "no helper left inside WSL");
    }
}
