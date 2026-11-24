//! Client for btx-modeld's unix JSON-RPC socket.
//!
//! Framing is upstream's (contrib/modelnet/btx-model, mirrored by lab/rpc.py):
//! one JSON object per connection, newline terminated, write side shut down,
//! read until the first newline, close.
//!
//! The helper serves the unix socket with TWO workers (`unix_workers=2` in its
//! ready line). A long call (getmodel resolving peers, exportmodelpath copying
//! gigabytes) occupies one; two of them wedge the socket for everyone. So every
//! call in the app goes through one gate and at most one request is ever on
//! the wire:
//!   - the permit belongs to the task doing the I/O and is released only when
//!     that connection has ended, even if the caller timed out and left;
//!   - user actions wait for the gate (`call_with`, the wait counts against
//!     their timeout); the UI's polls never wait (`try_call`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
#[cfg(unix)]
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::sync::{oneshot, Mutex, OwnedMutexGuard};
use tokio::time::{timeout_at, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// getmodel returns its job id at once (async, observed), but resolving the
/// uri against peers can take a while on a slow network.
pub const GETMODEL_TIMEOUT: Duration = Duration::from_secs(60);
/// exportmodelpath rebuilds the runnable files from pieces: a full second copy
/// of the model (contract E19). 2.2 GB took seconds on the lab SSD; a slower
/// disk gets a generous ceiling. Windows writes it through WSL's `/mnt/c` at
/// ~85 MB/s (docs/windows.md): the 12 GB gpt-oss takes minutes there.
pub const EXPORT_TIMEOUT: Duration = Duration::from_secs(if cfg!(windows) { 600 } else { 180 });

/// A reply line larger than this is not a reply. listmodels with a large store
/// is the biggest real one and is a few hundred KB.
#[cfg(unix)]
const MAX_REPLY: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum RpcError {
    /// Could not connect, write or read: the helper is not there.
    Io(String),
    /// The gate was held (another call in flight) past our deadline.
    Busy(String),
    Timeout(String),
    /// The helper answered with `{"error":{"code":..,"message":..}}`.
    Remote { method: String, code: String, message: String },
    BadReply(String),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::Io(e) => write!(f, "engine not reachable: {e}"),
            RpcError::Busy(m) => write!(f, "engine busy, {m} not sent"),
            RpcError::Timeout(m) => write!(f, "engine did not answer {m} in time"),
            RpcError::Remote { method, code, message } => write!(f, "{method}: {code}: {message}"),
            RpcError::BadReply(e) => write!(f, "unreadable engine reply: {e}"),
        }
    }
}

impl RpcError {
    pub fn code(&self) -> Option<&str> {
        match self {
            RpcError::Remote { code, .. } => Some(code),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct Rpc {
    sock: PathBuf,
    gate: Arc<Mutex<()>>,
    /// Windows: the engine host's pipe (`ainode-wsl` inside WSL), set by
    /// engine.rs while it runs. The helper's socket is not reachable from
    /// Windows; the same requests go over this pipe (docs/windows.md).
    #[cfg(windows)]
    pipe: Arc<std::sync::Mutex<Option<Arc<Pipe>>>>,
}

impl Rpc {
    pub fn new(sock: PathBuf) -> Self {
        Rpc {
            sock,
            gate: Arc::new(Mutex::new(())),
            #[cfg(windows)]
            pipe: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Windows: the pipe requests go over, or none (the engine is not running).
    /// Dropping the last handle closes ainode-wsl's stdin, which stops the helper.
    #[cfg(windows)]
    pub fn set_pipe(&self, p: Option<Arc<Pipe>>) {
        *self.pipe.lock().unwrap() = p;
    }

    /// Windows: the watcher of one engine clears only its own pipe, never a
    /// newer engine's.
    #[cfg(windows)]
    pub fn clear_pipe_if(&self, p: &Arc<Pipe>) {
        let mut g = self.pipe.lock().unwrap();
        if g.as_ref().is_some_and(|cur| Arc::ptr_eq(cur, p)) {
            *g = None;
        }
    }

    pub fn socket(&self) -> &Path {
        &self.sock
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let t = match method {
            "getmodel" => GETMODEL_TIMEOUT,
            "exportmodelpath" => EXPORT_TIMEOUT,
            _ => DEFAULT_TIMEOUT,
        };
        self.call_with(method, params, t).await
    }

    /// Tests only: hold the gate, as a long download or export does.
    /// Used by the unix-only tests in backend/network.rs.
    #[cfg(all(test, unix))]
    pub(crate) async fn hold_gate_for_test(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.gate.clone().lock_owned().await
    }

    /// Wait for the gate (up to `t`), then call. For user actions.
    pub async fn call_with(&self, method: &str, params: Value, t: Duration) -> Result<Value, RpcError> {
        let deadline = Instant::now() + t;
        let permit = timeout_at(deadline, self.gate.clone().lock_owned())
            .await
            .map_err(|_| RpcError::Busy(method.to_string()))?;
        self.run(permit, method, params, deadline).await
    }

    /// Call only if the gate is free RIGHT NOW, else `Busy` at once. For the
    /// UI's polls: a poll must never queue behind a 60 s getmodel or a
    /// multi-gigabyte export; the caller serves its previous snapshot instead.
    pub async fn try_call(&self, method: &str, params: Value, t: Duration) -> Result<Value, RpcError> {
        let permit = self
            .gate
            .clone()
            .try_lock_owned()
            .map_err(|_| RpcError::Busy(method.to_string()))?;
        self.run(permit, method, params, Instant::now() + t).await
    }

    /// The request runs in its own task that OWNS the permit, and gives it
    /// back only when that connection has really ended. A caller that times
    /// out gets `Timeout` but the gate stays shut until the helper has
    /// finished with the request, so a timed-out exportmodelpath can never
    /// run alongside a second one on the helper's two unix workers.
    async fn run(
        &self,
        permit: OwnedMutexGuard<()>,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, RpcError> {
        let (tx, rx) = oneshot::channel();
        #[cfg(unix)]
        let sock = self.sock.clone();
        #[cfg(windows)]
        let pipe = self.pipe.lock().unwrap().clone();
        let m = method.to_string();
        tokio::spawn(async move {
            #[cfg(unix)]
            let r = raw_call(&sock, &m, params).await;
            #[cfg(windows)]
            let r = match pipe {
                Some(p) => p.call(&m, params).await,
                None => Err(RpcError::Io("the engine is not running".into())),
            };
            drop(permit);
            let _ = tx.send(r);
        });
        match timeout_at(deadline, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(RpcError::Io(format!("{method}: request task ended"))),
            Err(_) => Err(RpcError::Timeout(method.to_string())),
        }
    }
}

/// Windows: one request line to `ainode-wsl`, one reply line back. The gate
/// keeps a single request in flight, and the task holding it reads its own
/// reply even after its caller timed out, so replies never shift onto the
/// next request (the same rule as one connection per request). Each request
/// carries its own id, and a line that is not the reply to it (anything
/// wsl.exe itself might print on stdout) is logged and skipped, so one stray
/// line cannot shift every later reply by one.
#[cfg(windows)]
pub struct Pipe {
    io: Mutex<(tokio::process::ChildStdin, tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>)>,
    next_id: std::sync::atomic::AtomicU64,
}

#[cfg(windows)]
impl Pipe {
    pub fn new(stdin: tokio::process::ChildStdin, stdout: tokio::process::ChildStdout) -> Self {
        use tokio::io::AsyncBufReadExt;
        Pipe { io: Mutex::new((stdin, tokio::io::BufReader::new(stdout).lines())), next_id: std::sync::atomic::AtomicU64::new(1) }
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut io = self.io.lock().await;
        let mut line = serde_json::to_vec(&json!({
            "jsonrpc": "1.0", "id": id, "method": method, "params": params,
        }))
        .map_err(|e| RpcError::BadReply(e.to_string()))?;
        line.push(b'\n');
        io.0.write_all(&line).await.map_err(|e| RpcError::Io(e.to_string()))?;
        io.0.flush().await.map_err(|e| RpcError::Io(e.to_string()))?;
        loop {
            let reply = io.1.next_line().await.map_err(|e| RpcError::Io(e.to_string()))?;
            let Some(reply) = reply else { return Err(RpcError::Io("the engine host closed".into())) };
            match pipe_reply(method, &reply, id) {
                Some(r) => return r,
                None => eprintln!("[engine] not the reply to {method} #{id}, skipped: {}", reply.chars().take(200).collect::<String>()),
            }
        }
    }
}

/// A line from `ainode-wsl` read as the reply to request `id`: the helper's
/// own reply (its `id` echoed), ainode-wsl's "helper not reachable" error
/// (RELAY_IO with `id` null, read as Io like a refused connection), or the
/// notice that the helper exited. `None` for anything else: not JSON, or a
/// reply to another id.
#[cfg(any(windows, test))]
pub(crate) fn pipe_reply(method: &str, reply: &str, id: u64) -> Option<Result<Value, RpcError>> {
    if reply.starts_with("{\"ainode_exit\"") {
        return Some(Err(RpcError::Io("the engine exited".into())));
    }
    let v: Value = serde_json::from_str(reply.trim()).ok()?;
    match v.get("id") {
        None | Some(Value::Null) => {}
        Some(Value::Number(n)) if n.as_u64() == Some(id) => {}
        Some(_) => return None,
    }
    Some(match parse_reply(method, reply.as_bytes()) {
        Err(RpcError::Remote { code, message, .. }) if code == "RELAY_IO" => Err(RpcError::Io(message)),
        r => r,
    })
}

/// One request on one connection. Not gated: only `Rpc` and tests call it.
#[cfg(unix)]
pub async fn raw_call(sock: &Path, method: &str, params: Value) -> Result<Value, RpcError> {
    let mut s = UnixStream::connect(sock)
        .await
        .map_err(|e| RpcError::Io(e.to_string()))?;
    let mut line = serde_json::to_vec(&json!({
        "jsonrpc": "1.0", "id": 1, "method": method, "params": params,
    }))
    .map_err(|e| RpcError::BadReply(e.to_string()))?;
    line.push(b'\n');
    s.write_all(&line).await.map_err(|e| RpcError::Io(e.to_string()))?;
    // Half-close: the helper reads to EOF before it answers (lab/rpc.py does
    // the same). Ignored on error like the reference client.
    let _ = s.shutdown().await;

    // Read into a heap Vec, not a stack array: a buffer held across an await
    // lives inside the future, and a 64 KiB one made every command future
    // ~66 KB (enough to overflow a 2 MiB test thread in a debug build).
    let mut buf: Vec<u8> = Vec::with_capacity(16 * 1024);
    loop {
        let before = buf.len();
        buf.reserve(16 * 1024);
        let n = s.read_buf(&mut buf).await.map_err(|e| RpcError::Io(e.to_string()))?;
        if n == 0 || buf[before..].contains(&b'\n') {
            break;
        }
        if buf.len() > MAX_REPLY {
            return Err(RpcError::BadReply(format!("{method}: reply over {MAX_REPLY} bytes")));
        }
    }
    parse_reply(method, &buf)
}

pub fn parse_reply(method: &str, buf: &[u8]) -> Result<Value, RpcError> {
    let end = buf.iter().position(|&b| b == b'\n').unwrap_or(buf.len());
    let body = &buf[..end];
    if body.iter().all(|b| b.is_ascii_whitespace()) {
        return Err(RpcError::BadReply(format!("{method}: empty reply")));
    }
    let reply: Value = serde_json::from_slice(body).map_err(|e| RpcError::BadReply(format!("{method}: {e}")))?;
    match reply.get("error") {
        None | Some(Value::Null) => {}
        Some(Value::Object(o)) if o.is_empty() => {}
        Some(Value::Array(a)) if a.is_empty() => {}
        Some(e) => {
            let code = e.get("code").map(value_text).unwrap_or_default();
            let message = e
                .get("message")
                .map(value_text)
                .unwrap_or_else(|| e.to_string());
            return Err(RpcError::Remote { method: method.to_string(), code, message });
        }
    }
    Ok(reply.get("result").cloned().unwrap_or(Value::Null))
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_result_and_string_error_codes() {
        let ok = parse_reply("x", b"{\"result\":{\"a\":1},\"error\":null,\"id\":1}\n").unwrap();
        assert_eq!(ok["a"], 1);
        // Observed shape: the code is a STRING, not JSON-RPC's integer.
        let e = parse_reply(
            "help",
            b"{\"result\":null,\"error\":{\"code\":\"METHOD_NOT_FOUND\",\"message\":\"unknown model RPC\",\"schema_version\":2},\"id\":1}\n",
        )
        .unwrap_err();
        assert_eq!(e.code(), Some("METHOD_NOT_FOUND"));
        assert!(parse_reply("x", b"").is_err());
        assert!(parse_reply("x", b"{\"result\":1,\"error\":{}}").is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn one_request_per_connection_and_gate_serialises() {
        // A fake helper: answers each connection after reading to EOF, and
        // counts how many connections are open at once.
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("s.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let open = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let peak = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (o2, p2) = (open.clone(), peak.clone());
        tokio::spawn(async move {
            loop {
                let (mut c, _) = listener.accept().await.unwrap();
                let (o3, p3) = (o2.clone(), p2.clone());
                tokio::spawn(async move {
                    let now = o3.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    p3.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                    let mut req = Vec::new();
                    c.read_to_end(&mut req).await.unwrap();
                    let v: Value = serde_json::from_slice(&req[..req.len() - 1]).unwrap();
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    let reply = json!({"result": {"echo": v["method"]}, "error": null, "id": 1});
                    c.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
                    o3.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                });
            }
        });
        let rpc = Rpc::new(sock);
        let mut tasks = Vec::new();
        for i in 0..6 {
            let r = rpc.clone();
            tasks.push(tokio::spawn(async move { r.call(&format!("m{i}"), json!([])).await }));
        }
        for (i, t) in tasks.into_iter().enumerate() {
            assert_eq!(t.await.unwrap().unwrap()["echo"], format!("m{i}"));
        }
        assert_eq!(peak.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_timed_out_call_keeps_the_gate_until_its_connection_ends() {
        // A helper that takes 300 ms to answer one request.
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("slow.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        tokio::spawn(async move {
            loop {
                let (mut c, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut req = Vec::new();
                    c.read_to_end(&mut req).await.unwrap();
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    let _ = c.write_all(b"{\"result\":1,\"error\":null}\n").await;
                });
            }
        });
        let rpc = Rpc::new(sock);
        let e = rpc.call_with("exportmodelpath", json!([]), Duration::from_millis(50)).await.unwrap_err();
        assert_eq!(e, RpcError::Timeout("exportmodelpath".into()));
        // The first request is still on the wire: the gate is shut.
        let e = rpc.try_call("listmodels", json!([]), Duration::from_secs(1)).await.unwrap_err();
        assert_eq!(e, RpcError::Busy("listmodels".into()));
        // Once it has finished, calls go through again.
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(rpc.try_call("listmodels", json!([]), Duration::from_secs(1)).await.unwrap(), 1);
    }

    #[test]
    fn pipe_replies_read_like_socket_replies() {
        // The helper's reply, unchanged.
        assert_eq!(pipe_reply("x", r#"{"result":{"a":1},"error":null,"id":7}"#, 7).unwrap().unwrap()["a"], 1);
        // ainode-wsl could not reach the helper: the same as a refused connection.
        let e = pipe_reply("hello", r#"{"result":null,"error":{"code":"RELAY_IO","message":"No such file"},"id":null}"#, 7).unwrap().unwrap_err();
        assert_eq!(e, RpcError::Io("No such file".into()));
        // The helper's own errors stay its own.
        let e = pipe_reply("getmodel", r#"{"result":null,"error":{"code":"NOT_FOUND","message":"m"},"id":7}"#, 7).unwrap().unwrap_err();
        assert_eq!(e.code(), Some("NOT_FOUND"));
        // The helper exited.
        assert!(matches!(pipe_reply("x", r#"{"ainode_exit":{"code":2,"signal":null,"stderr":"x"}}"#, 7), Some(Err(RpcError::Io(_)))));
        // Not a reply to this request: a stray line, or another id.
        assert!(pipe_reply("x", "The system cannot find the path specified.", 7).is_none());
        assert!(pipe_reply("x", r#"{"result":1,"error":null,"id":6}"#, 7).is_none());
        assert!(pipe_reply("x", "", 7).is_none());
    }

    #[tokio::test]
    async fn busy_gate_gives_up_at_the_deadline() {
        let rpc = Rpc::new(PathBuf::from("/nonexistent.sock"));
        let held = rpc.gate.clone().lock_owned().await;
        let e = rpc.call_with("listmodels", json!([]), Duration::from_millis(50)).await.unwrap_err();
        assert_eq!(e, RpcError::Busy("listmodels".into()));
        let t = Instant::now();
        let e = rpc.try_call("listmodels", json!([]), Duration::from_secs(5)).await.unwrap_err();
        assert_eq!(e, RpcError::Busy("listmodels".into()));
        assert!(t.elapsed() < Duration::from_millis(20), "try_call must not wait");
        drop(held);
        let e = rpc.call_with("listmodels", json!([]), Duration::from_millis(50)).await.unwrap_err();
        assert!(matches!(e, RpcError::Io(_)), "{e:?}");
    }
}
