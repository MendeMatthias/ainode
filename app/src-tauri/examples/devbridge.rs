//! Dev-only: serve the same `Backend` the Tauri commands call over plain HTTP
//! on 127.0.0.1, so the real UI (npm run dev, `?bridge`) can drive the real
//! engine in a browser, where it can be clicked, read and screenshotted. Never
//! binds anything but loopback; answers only the dev server's origin.
//!
//! It is a Cargo example, not a `src/bin` target, because the Tauri bundler
//! copies every bin target into AInode.app/Contents/MacOS: as a bin it shipped
//! inside the app. `scripts/check-bundle.sh` fails a build that has it.
//!
//!   AINODE_DATA_DIR=/scratch/data AINODE_ENGINE_DIR=$PWD/resources/engine \
//!   AINODE_SOCKET=/scratch/s/modeld.sock \
//!   AINODE_PEERS=127.0.0.1:29447 cargo run --example devbridge
//!
//! AINODE_SOCKET and AINODE_DATA_DIR are required, as full paths, and the
//! bridge refuses the installed app's own socket FOLDER and data folder,
//! also through links, and also a data folder the app was moved to
//! (`state::dev_run_refusal`). Without AINODE_SOCKET the
//! socket would be the default one, where an AInode that is open on this
//! computer has its engine: `startup_cleanup` would take that engine for a
//! leftover of a crashed run and stop it (engine.rs `clear_socket`). Found on
//! 2026-11-23, before it happened (docs/modelnet/1-engine-move.md, 7.3). The
//! socket path must be short (104 bytes on a Mac); give it a folder of its
//! own. The bundled host stays first: AINODE_PEERS only adds hosts.
//!
//! POST /invoke/<command> with the same JSON argument object the UI passes to
//! Tauri's invoke() (camelCase keys), e.g. {"totalGb":25} or {"uri":"btx://…"}.

use std::sync::Arc;

use ainode_lib::backend::{self, Backend};
use ainode_lib::state::{self, Paths};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const ADDR: &str = "127.0.0.1:1455";
const ORIGIN: &str = "http://localhost:1450";

/// Why this run must not start, if it would reach the installed app's engine
/// or data. Checked before anything is touched. The rules, and their tests,
/// are `state::dev_run_refusal`.
fn refusal(paths: &Paths) -> Option<String> {
    for k in ["AINODE_SOCKET", "AINODE_DATA_DIR"] {
        if state::dev_env(k).is_none() {
            return Some(format!("{k} is not set"));
        }
    }
    state::dev_run_refusal(paths, &state::LiveInstall::here())
}

#[tokio::main]
async fn main() {
    let paths = Paths::resolve(None).expect("set AINODE_DATA_DIR, AINODE_ENGINE_DIR and AINODE_SOCKET");
    if let Some(why) = refusal(&paths) {
        eprintln!(
            "devbridge: refusing to start: {why}.\n\
             An AInode that is open on this computer has its engine on the default socket, and this\n\
             bridge would stop it. Give the bridge its own: AINODE_SOCKET=<a short scratch folder>/modeld.sock\n\
             and AINODE_DATA_DIR=<a scratch folder>."
        );
        std::process::exit(2);
    }
    let b = Arc::new(Backend::new(paths, env!("CARGO_PKG_VERSION").to_string()));
    b.startup_cleanup().await;
    let listener = TcpListener::bind(ADDR).await.expect("bind 127.0.0.1:1455");
    eprintln!("devbridge on http://{ADDR}, origin {ORIGIN}");
    let shutdown = b.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        shutdown.shutdown().await;
        std::process::exit(0);
    });
    loop {
        let Ok((sock, _)) = listener.accept().await else { continue };
        let b = b.clone();
        tokio::spawn(async move {
            let _ = serve(sock, b).await;
        });
    }
}

async fn serve(mut sock: TcpStream, b: Arc<Backend>) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        let n = sock.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 1 << 20 {
            return Ok(());
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let request = head.lines().next().unwrap_or_default().to_string();
    let header = |name: &str| {
        head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    let len = header("content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    // CORS alone does not stop another page from SENDING a "simple" POST (it
    // only hides the reply), so refuse anything not from the dev server with a
    // JSON body: a JSON content type forces a preflight a foreign origin fails.
    let allowed = header("origin").as_deref() == Some(ORIGIN)
        && header("content-type").is_some_and(|v| v.starts_with("application/json"));
    while buf.len() < head_end + len {
        let n = sock.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = &buf[head_end..(head_end + len).min(buf.len())];

    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let (status, out) = if method == "OPTIONS" {
        (204, String::new())
    } else if !allowed {
        (403, json!({ "error": "dev bridge: wrong origin or content type" }).to_string())
    } else if let (Some(cmd), "POST") = (path.strip_prefix("/invoke/"), method) {
        let args: Value = serde_json::from_slice(body).unwrap_or(json!({}));
        match dispatch(&b, cmd, &args).await {
            Ok(v) => (200, v.to_string()),
            Err(e) => (400, json!({ "error": e }).to_string()),
        }
    } else {
        (404, json!({ "error": "POST /invoke/<command>" }).to_string())
    };
    let reason = match status { 200 => "OK", 204 => "No Content", 400 => "Bad Request", 403 => "Forbidden", _ => "Not Found" };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Access-Control-Allow-Origin: {ORIGIN}\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
    sock.write_all(resp.as_bytes()).await?;
    sock.shutdown().await
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| format!("missing argument {key}"))
}

fn json_of<T: serde::Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

async fn dispatch(b: &Arc<Backend>, cmd: &str, a: &Value) -> Result<Value, String> {
    let to = |v: Result<(), String>| v.map(|_| Value::Null);
    Ok(match cmd {
        "get_state" => serde_json::to_value(b.get_state().await).map_err(|e| e.to_string())?,
        "accept_consent" => {
            let gb = a.get("totalGb").and_then(Value::as_u64).ok_or("missing argument totalGb")?;
            let dir = a.get("dataDir").and_then(Value::as_str).map(str::to_string);
            json_of(b.accept_consent(gb as u32, dir).await?)?
        }
        "places" => json_of(b.places().await)?,
        // The bridge serves a browser page: nothing to relaunch.
        "relaunch" => Value::Null,
        "start_engine" => to(b.start_engine().await)?,
        "stop_engine" => to(b.stop_engine().await)?,
        "catalog" => {
            // As the Tauri command does: a model that just finished is kept once.
            let rows = b.catalog().await;
            b.settle_downloads(&rows);
            serde_json::to_value(rows).map_err(|e| e.to_string())?
        }
        "download" => to(b.download(arg(a, "uri")?).await)?,
        "cancel" => to(b.cancel(arg(a, "uri")?).await)?,
        "remove" => to(b.remove(arg(a, "uri")?).await)?,
        "reveal" => to(b.reveal(arg(a, "uri")?).await)?,
        "chat_start" => serde_json::to_value(b.chat_start(arg(a, "uri")?).await?).map_err(|e| e.to_string())?,
        "chat_stop" => {
            b.chat_stop().await;
            Value::Null
        }
        "set_theme" => to(b.set_theme(arg(a, "t")?))?,
        "set_pref" => {
            let on = a.get("on").and_then(Value::as_bool).ok_or("missing argument on")?;
            to(b.set_pref(arg(a, "name")?, on))?
        }
        "set_face" => to(b.set_face(arg(a, "face")?))?,
        "set_model_prefs" => {
            let prefs = serde_json::from_value(a.get("prefs").cloned().ok_or("missing argument prefs")?).map_err(|e| e.to_string())?;
            to(b.set_model_prefs(arg(a, "id")?, prefs))?
        }
        "knowledge_list" => json_of(b.knowledge_list())?,
        "knowledge_add" => {
            let passages = serde_json::from_value(a.get("passages").cloned().ok_or("missing argument passages")?).map_err(|e| e.to_string())?;
            json_of(b.knowledge_add(arg(a, "name")?, arg(a, "kind")?, passages).await?)?
        }
        "knowledge_remove" => to(b.knowledge_remove(arg(a, "id")?))?,
        "knowledge_passages" => json_of(b.knowledge_passages())?,
        "network_bounties" => b.network_bounties().await?,
        "network_campaigns" => b.network_campaigns().await?,
        "network_search" => b.network_search(arg(a, "text")?).await?,
        "network_start_search" => b.network_start_search().await?,
        "index_peers" => json_of(b.index_peers())?,
        "index_peer_add" => json_of(b.index_peer_add(arg(a, "hostport")?).await?)?,
        "index_peer_remove" => json_of(b.index_peer_remove(arg(a, "hostport")?).await?)?,
        "link_open" => json_of(b.link_open(arg(a, "uri")?).await?)?,
        "link_get" => to(b.link_get(arg(a, "uri")?, arg(a, "title").unwrap_or("")).await)?,
        "links" => json_of(b.links().await)?,
        "link_reveal" => to(b.link_reveal(arg(a, "uri")?).await)?,
        "link_remove" => to(b.link_remove(arg(a, "uri")?).await)?,
        "chats_list" => json_of(b.chats_list(arg(a, "modelId").unwrap_or("")))?,
        "chat_load" => json_of(b.chat_load(arg(a, "id")?)?)?,
        "chat_save" => {
            let chat = serde_json::from_value(a.get("chat").cloned().ok_or("missing argument chat")?).map_err(|e| e.to_string())?;
            to(b.chat_save(chat).await)?
        }
        "chat_delete" => to(b.chat_delete(arg(a, "id")?))?,
        "add_peer" => to(b.add_peer(arg(a, "hostport")?).await)?,
        "open_url" => to(backend::open_url(arg(a, "url")?).await)?,
        other => return Err(format!("unknown command {other}")),
    })
}
