//! The Tauri commands: ENGINE-CONTRACT.md "Commands the Rust side exposes",
//! names and argument names exactly as app/src/api.ts invokes them (Tauri
//! maps the JS camelCase `totalGb` to `total_gb`). Each is a thin wrapper over
//! `Backend`, which holds the logic and is what the integration test drives.

use std::sync::Arc;

use tauri::State;

use crate::backend::{self, AppStateOut, Backend, ChatPort};
use crate::catalog::CatalogModel;

type B<'a> = State<'a, Arc<Backend>>;

#[tauri::command]
pub async fn get_state(b: B<'_>) -> Result<AppStateOut, String> {
    Ok(b.get_state().await)
}

/// `invoke("accept_consent", { totalGb, dataDir })`, decimal GB; `dataDir`
/// only at the first start, when another drive or folder was chosen.
#[tauri::command]
pub async fn accept_consent(b: B<'_>, total_gb: u32, data_dir: Option<String>) -> Result<backend::ConsentOutcome, String> {
    b.accept_consent(total_gb, data_dir).await
}

/// The drives or volumes AInode's data can live on, with the sizes each allows.
#[tauri::command]
pub async fn places(b: B<'_>) -> Result<Vec<backend::PlaceOut>, String> {
    Ok(b.places().await)
}

/// After a first start that chose another folder: the app reads its paths
/// again. (The handle is injected, like the state.)
#[tauri::command]
pub async fn relaunch(app: tauri::AppHandle) -> Result<(), String> {
    crate::restart::now(&app)
}

#[tauri::command]
pub async fn start_engine(b: B<'_>) -> Result<(), String> {
    b.start_engine().await
}

/// Windows: restart WSL on the user's click, `others` being the other
/// distributions the button named (backend/wslheal.rs).
#[tauri::command]
pub async fn restart_wsl(b: B<'_>, others: Vec<String>) -> Result<(), String> {
    #[cfg(windows)]
    return b.restart_wsl(others).await;
    #[cfg(not(windows))]
    {
        let _ = (b, others);
        Err("only on Windows".into())
    }
}

#[tauri::command]
pub async fn stop_engine(b: B<'_>) -> Result<(), String> {
    b.stop_engine().await
}

#[tauri::command]
pub async fn catalog(b: B<'_>) -> Result<Vec<CatalogModel>, String> {
    let rows = b.catalog().await;
    // A model that just finished is kept once in the background (plan 3.4).
    b.inner().settle_downloads(&rows);
    Ok(rows)
}

#[tauri::command]
pub async fn download(b: B<'_>, uri: String) -> Result<(), String> {
    b.download(&uri).await
}

#[tauri::command]
pub async fn cancel(b: B<'_>, uri: String) -> Result<(), String> {
    b.cancel(&uri).await
}

#[tauri::command]
pub async fn remove(b: B<'_>, uri: String) -> Result<(), String> {
    b.remove(&uri).await
}

#[tauri::command]
pub async fn reveal(b: B<'_>, uri: String) -> Result<(), String> {
    b.reveal(&uri).await
}

/// Returns `{ port, key }`; the UI sends `Authorization: Bearer <key>`.
#[tauri::command]
pub async fn chat_start(b: B<'_>, uri: String) -> Result<ChatPort, String> {
    b.chat_start(&uri).await
}

#[tauri::command]
pub async fn chat_stop(b: B<'_>) -> Result<(), String> {
    b.chat_stop().await;
    Ok(())
}

#[tauri::command]
pub async fn set_theme(b: B<'_>, t: String) -> Result<(), String> {
    b.set_theme(&t)
}

#[tauri::command]
pub async fn set_pref(b: B<'_>, name: String, on: bool) -> Result<(), String> {
    b.set_pref(&name, on)
}

#[tauri::command]
pub async fn set_face(b: B<'_>, face: String) -> Result<(), String> {
    b.set_face(&face)
}

/// `invoke("set_model_prefs", { id, prefs })`: the settings button on a card.
#[tauri::command]
pub async fn set_model_prefs(b: B<'_>, id: String, prefs: crate::state::ModelPrefs) -> Result<(), String> {
    b.set_model_prefs(&id, prefs)
}

/// The user's own files: list, add (passages read by the webview), remove,
/// and every passage for the chat's search worker.
#[tauri::command]
pub async fn knowledge_list(b: B<'_>) -> Result<Vec<crate::own::OwnFile>, String> {
    Ok(b.knowledge_list())
}

#[tauri::command]
pub async fn knowledge_add(b: B<'_>, name: String, kind: String, passages: Vec<crate::own::Passage>) -> Result<crate::own::OwnFile, String> {
    b.knowledge_add(&name, &kind, passages).await
}

#[tauri::command]
pub async fn knowledge_remove(b: B<'_>, id: String) -> Result<(), String> {
    b.knowledge_remove(&id)
}

#[tauri::command]
pub async fn knowledge_passages(b: B<'_>) -> Result<Vec<crate::own::Found>, String> {
    Ok(b.knowledge_passages())
}

/// The Network view (backend/network.rs): read-only bounties and campaigns,
/// and the human path: open a btx:// link, fetch it free, search.
#[tauri::command]
pub async fn network_bounties(b: B<'_>) -> Result<serde_json::Value, String> {
    b.network_bounties().await
}

#[tauri::command]
pub async fn network_campaigns(b: B<'_>) -> Result<serde_json::Value, String> {
    b.network_campaigns().await
}

#[tauri::command]
pub async fn network_search(b: B<'_>, text: String) -> Result<serde_json::Value, String> {
    b.network_search(&text).await
}

/// The start-up search goes out only while the window is shown: checked
/// here against the window itself, not only in the page (a login launch
/// minimizes the window before the page knows it is minimized).
#[tauri::command]
pub async fn network_start_search(b: B<'_>, window: tauri::WebviewWindow) -> Result<serde_json::Value, String> {
    let shown = window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true);
    if !shown {
        // Nothing sent: the page may ask again when the window is shown.
        return Err(format!("{}the window is not shown: the start-up search waits until it is", crate::backend::NOT_SENT));
    }
    b.network_start_search().await
}

#[tauri::command]
pub async fn index_peers(b: B<'_>) -> Result<Vec<backend::IndexPeer>, String> {
    Ok(b.index_peers())
}

#[tauri::command]
pub async fn index_peer_add(b: B<'_>, hostport: String) -> Result<Vec<backend::IndexPeer>, String> {
    b.index_peer_add(&hostport).await
}

#[tauri::command]
pub async fn index_peer_remove(b: B<'_>, hostport: String) -> Result<Vec<backend::IndexPeer>, String> {
    b.index_peer_remove(&hostport).await
}

#[tauri::command]
pub async fn link_open(b: B<'_>, uri: String) -> Result<backend::LinkCard, String> {
    b.link_open(&uri).await
}

#[tauri::command]
pub async fn link_get(b: B<'_>, uri: String, title: String) -> Result<(), String> {
    b.link_get(&uri, &title).await
}

#[tauri::command]
pub async fn links(b: B<'_>) -> Result<Vec<backend::LinkModel>, String> {
    Ok(b.links().await)
}

#[tauri::command]
pub async fn link_reveal(b: B<'_>, uri: String) -> Result<(), String> {
    b.link_reveal(&uri).await
}

#[tauri::command]
pub async fn link_remove(b: B<'_>, uri: String) -> Result<(), String> {
    b.link_remove(&uri).await
}

/// Saved conversations (chats.rs), only when Keep chats is on.
#[tauri::command]
pub async fn chats_list(b: B<'_>, model_id: String) -> Result<Vec<crate::chats::ChatMeta>, String> {
    Ok(b.chats_list(&model_id))
}

#[tauri::command]
pub async fn chat_load(b: B<'_>, id: String) -> Result<crate::chats::Chat, String> {
    b.chat_load(&id)
}

#[tauri::command]
pub async fn chat_save(b: B<'_>, chat: crate::chats::Chat) -> Result<(), String> {
    b.chat_save(chat).await
}

#[tauri::command]
pub async fn chat_delete(b: B<'_>, id: String) -> Result<(), String> {
    b.chat_delete(&id)
}

#[tauri::command]
pub async fn add_peer(b: B<'_>, hostport: String) -> Result<(), String> {
    b.add_peer(&hostport).await
}

#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    backend::open_url(&url).await
}

/// Look for a newer AInode (update.rs). Never downloads.
#[tauri::command]
pub async fn update_check(app: tauri::AppHandle, b: B<'_>) -> Result<crate::update::UpdateOut, String> {
    crate::update::check(&app, &b.paths).await
}

/// Download the update found by the last check, stop the engine, install and
/// restart.
#[tauri::command]
pub async fn update_install(app: tauri::AppHandle, b: B<'_>) -> Result<(), String> {
    crate::update::install(&app, &b).await
}

/// Send feedback: the OS version, the machine, its chip and memory, for the
/// issue the browser opens. Nothing is sent from here.
#[tauri::command]
pub async fn system_info() -> Result<crate::os::SystemInfo, String> {
    Ok(crate::os::system_info().await)
}

/// A BTX node on this computer, for Status (neighbour.rs). Loopback only,
/// read-only, and only after consent: before it AInode asks nothing.
#[tauri::command]
pub async fn neighbour(b: B<'_>) -> Result<crate::neighbour::Neighbour, String> {
    if crate::state::load_consent(&b.paths).is_none() {
        return Ok(crate::neighbour::nothing());
    }
    Ok(crate::neighbour::look().await)
}
