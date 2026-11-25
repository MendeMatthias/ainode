//! Everything the UI commands do, without Tauri, so the integration test can
//! drive the same code against a real helper.
//!
//! Polling: the UI calls `get_state` + `catalog` every 1.5 s while visible.
//! Both read one shared snapshot (getmodelnetworkinfo, listmodels,
//! getmodeljob) refreshed at most once a second, so the helper is never
//! polled faster than the contract allows however often the UI asks. A poll
//! never waits: it only talks to the helper when the RPC gate is free at that
//! instant (`Rpc::try_call`), only one refresh runs at a time, the snapshot
//! lock is never held across an RPC, and a refresh that found the gate busy
//! still stamps the snapshot, so polls back off for a second and the UI gets
//! the previous answer at once.
//!
//! Pinning: a completed catalog model is pinned (`pinmodel`). The helper's
//! quota enforcement evicts every unpinned model, oldest first, whenever a
//! download needs room (EnforceQuota, btx src/modelnet/catalog.cpp:375, read;
//! the live test shows it happening), so an unpinned model the user chose
//! could vanish when they download the next one.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::catalog::{self, Catalog, CatalogEntry, CatalogModel, Job, Listed, View};
use crate::chat::Chat;
use crate::disk::{self, DiskSpace};
use crate::engine::{Engine, EngineState, StartArgs};
use crate::chats;
use crate::own;
use crate::store_once;
use crate::rpc::{RpcError, DEFAULT_TIMEOUT};
use crate::state::{self, Consent, Paths, Settings};
use crate::validate;

mod network;
pub use network::{IndexPeer, LinkCard, LinkModel, NOT_SENT};
mod keeper;
pub use keeper::{KeeperOut, TICK as KEEP_TICK};
#[cfg(windows)]
mod wslheal;
#[cfg(windows)]
pub use wslheal::WslOut;
#[cfg(not(windows))]
pub type WslOut = ();

const SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(1);
const RUNNABLE_BYTES_TTL: Duration = Duration::from_secs(10);
/// Ceiling for one poll-path RPC once it has the gate (they answer in ms).
const POLL_RPC_BUDGET: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NetInfo {
    #[serde(default)]
    pub pq1_ready: bool,
    #[serde(default)]
    pub openssl: String,
    #[serde(default)]
    pub peers_connected: u64,
    #[serde(default)]
    pub used_bytes: u64,
    #[serde(default)]
    pub quota_bytes: u64,
    #[serde(default)]
    pub storage_reclaimable_bytes: u64,
}

#[derive(Debug, Clone, Default)]
struct Snapshot {
    /// When the last refresh finished, success or not (polls back off on it).
    at: Option<Instant>,
    /// When the data in it was READ: an older refresh finishing late never
    /// overwrites a newer one.
    data_at: Option<Instant>,
    info: Option<NetInfo>,
    listed: Vec<Listed>,
    jobs: Vec<Job>,
    /// `getmodelpeers` count, refreshed at start and after add_peer.
    peers: Option<usize>,
    /// Last store usage the helper reported, for get_state while stopped.
    used_bytes: Option<u64>,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// UI poll: never wait for the gate.
    Poll,
    /// A user action that needs current facts: wait for the gate.
    Wait,
}

// ── get_state shape (ENGINE-CONTRACT.md "Commands") ─────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AppStateOut {
    pub version: String,
    pub consent: Option<ConsentOut>,
    pub disk: DiskSpace,
    /// Where AInode's data lives (the default, or the folder chosen at the first start).
    pub data_dir: String,
    pub engine: EngineOut,
    pub network: NetworkOut,
    pub usage: UsageOut,
    pub space: SpaceOut,
    /// This Mac's memory, for the catalog's ram_gb.
    pub ram_bytes: u64,
    pub theme: String,
    pub autostart: bool,
    pub think: bool,
    /// The loading scene's face: random, swarm, glyphs or chip.
    pub face: String,
    /// Per-model chat settings, by catalog id (state::ModelPrefs).
    pub models: std::collections::BTreeMap<String, state::ModelPrefs>,
    /// Saved conversations are on (chats.rs).
    pub keep_chats: bool,
    /// Chat on the graphics card (state::Settings::gpu).
    pub gpu: bool,
    /// Be counted among the AInodes online (state::Settings::count_me).
    pub count_me: bool,
    /// Start with the computer (state::Settings::start_at_login, login.rs).
    pub start_at_login: bool,
    /// Why the start entry could not be written, if it could not.
    pub login_error: Option<String>,
    /// This install's random id in that count; null before consent.
    pub community_id: Option<String>,
    /// Windows: what AInode is doing about WSL (backend/wslheal.rs); null elsewhere.
    pub wsl: Option<WslOut>,
    /// The engine's self-heal and the check-in (backend/keeper.rs).
    pub keeper: KeeperOut,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsentOut {
    pub total_bytes: u64,
    pub engine_bytes: u64,
    pub accepted_at: u64,
}

/// `accept_consent`'s answer: `relaunch` when another folder was chosen, so
/// the app reads its paths again (the UI calls `relaunch`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConsentOutcome {
    pub relaunch: bool,
}

/// A drive or volume for the first-run choice: where AInode's data would go
/// on it, and the sizes that drive allows now (state::consent_limits).
#[derive(Debug, Clone, Serialize)]
pub struct PlaceOut {
    pub root: String,
    pub free_bytes: u64,
    pub total_bytes: u64,
    pub data_dir: String,
    pub min_gb: u32,
    pub max_gb: u32,
    /// The one the current data dir is on.
    pub current: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineOut {
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openssl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pq1_ready: Option<bool>,
    /// What the helper in the engine folder is (engine.rs `bundled`): shown
    /// in Status. Running or not: it is about the files, not the process.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundled: Option<crate::engine::Bundled>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkOut {
    pub peers_configured: u64,
    pub peers_connected: u64,
    /// The host the engine is given (`active_peer`), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_host: Option<String>,
    /// How many hosts AInode knows (hosts.json and the user's).
    pub hosts_known: u64,
    /// The last automatic switch: "from -> to", while it is recent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switched: Option<String>,
    /// Which engine run this is: the page asks for the start-up search once
    /// per value (network_start_search). 0 before the first start.
    pub engine_run: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageOut {
    pub engine_used_bytes: u64,
    pub runnable_bytes: u64,
    /// The user's own files: the text AInode read from them (own.rs).
    pub own_bytes: u64,
    pub total_bytes: u64,
}

impl UsageOut {
    pub fn held(&self) -> state::Held {
        state::Held { engine: self.engine_used_bytes, total: self.total_bytes }
    }
}

/// The sizes the user may choose right now (`state::consent_limits`), so the
/// UI never offers one the backend would refuse. max < min: the disk is full.
#[derive(Debug, Clone, Serialize)]
pub struct SpaceOut {
    pub min_gb: u32,
    pub max_gb: u32,
}

/// chat_start's reply. The UI sends `Authorization: Bearer <key>` to
/// http://127.0.0.1:<port>.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatPort {
    pub port: u16,
    pub key: String,
}

pub struct Backend {
    pub paths: Paths,
    version: String,
    catalog: Catalog,
    pub engine: Engine,
    chat: Chat,
    /// In memory always; on disk only once consent.json exists (rule 1).
    settings: StdMutex<Settings>,
    removed: StdMutex<HashSet<String>>,
    /// Kept once (store_once.rs): uri -> artifact id, pieces deleted.
    stored_once: StdMutex<std::collections::BTreeMap<String, String>>,
    /// uri -> job_id from our own getmodel, until that job is seen ending.
    pending: StdMutex<HashMap<String, String>>,
    /// uri -> the last job_id our own getmodel returned, kept after it ends,
    /// so a fetch that failed before the model was listed still shows why.
    started: StdMutex<HashMap<String, String>>,
    /// uri -> checkout dir exportmodelpath returned this session.
    exported: StdMutex<HashMap<String, PathBuf>>,
    /// Runnable ggufs whose sha256 matched the catalog: path -> mtime at the
    /// time. Hashed once per copy, not per chat.
    verified: StdMutex<HashMap<PathBuf, std::time::SystemTime>>,
    /// Never held across an await.
    snap: StdMutex<Snapshot>,
    /// One poll refresh at a time; the others serve the current snapshot.
    refreshing: AtomicBool,
    /// Bytes of runnable copies: a walk of a few files, redone after an
    /// export or remove and at most every RUNNABLE_BYTES_TTL otherwise, so a
    /// value read mid-export, or a copy deleted in Finder, does not stick.
    runnable_bytes: StdMutex<Option<(Instant, u64)>>,
    /// Store size from disk, for get_state before the engine first answers.
    store_bytes: StdMutex<Option<u64>>,
    /// Serialises download / cancel / remove so two clicks cannot both see
    /// "no job" and start two workers (getmodel twice = two workers, contract).
    jobs_lock: Mutex<()>,
    /// uris with a chat_start in flight (export + model load), so remove can
    /// refuse before llama-server has even been spawned.
    chat_inflight: StdMutex<HashSet<String>>,
    /// Failover (0.7.0): the host the app switched to after the one in use
    /// stopped answering, hosts that failed and when, and the last switch.
    host_override: StdMutex<Option<String>>,
    host_failed: StdMutex<HashMap<String, Instant>>,
    /// Why the start entry could not be written or removed (login.rs), for Status.
    login_error: StdMutex<Option<String>>,
    last_switch: StdMutex<Option<(Instant, String, String)>>,
    switching: AtomicBool,
    /// Kept once right after a download (plan 3.4): held by the background
    /// task for its whole run (copy, check, delete, engine restart), and by
    /// chat_start and reveal around their own runnable_path, so a chat never
    /// reaches the engine while that task restarts it. A model that could not
    /// be kept waits SETTLE_RETRY before the next try.
    settle_lock: std::sync::Arc<Mutex<()>>,
    settle_failed: StdMutex<HashMap<String, Instant>>,
    /// Windows: a stuck WSL, restarted where that is safe (wslheal.rs).
    #[cfg(windows)]
    wsl_heal: wslheal::WslHeal,
    /// The timers that must run whether or not the window does (keeper.rs).
    keeper: keeper::Keeper,
    /// Counts the engine's successful starts (the user's, the keeper's, a host
    /// switch). The start-up search runs once per value (backend/network.rs).
    engine_runs: std::sync::atomic::AtomicU64,
    /// The start-up search of the current engine run: its run, when it
    /// answered, and the answer or the error. At most one search per run.
    start_search: Mutex<Option<network::StartSearch>>,
    /// Held for the whole of a change to the search list: read, engine, write.
    index_lock: Mutex<()>,
}

/// After a failed keep-once (no room for the copy, a checksum that did not
/// match), wait this long before trying that model again.
const SETTLE_RETRY: Duration = Duration::from_secs(10 * 60);

/// The model to keep once now, if any: one the engine has complete, not kept
/// once yet, not removed, not tried and failed recently, and only while
/// nothing downloads (the store is never touched under a running fetch).
pub fn to_settle(
    rows: &[CatalogModel],
    once: &std::collections::BTreeMap<String, String>,
    removed: &HashSet<String>,
    failed: &HashMap<String, Instant>,
    now: Instant,
) -> Option<String> {
    if rows.iter().any(|m| m.local == "downloading") {
        return None;
    }
    rows.iter()
        .find(|m| {
            m.local == "ready"
                && !once.contains_key(&m.uri)
                && !removed.contains(&m.uri)
                && failed.get(&m.uri).is_none_or(|t| now.duration_since(*t) >= SETTLE_RETRY)
        })
        .map(|m| m.uri.clone())
}

/// Holds a uri in `chat_inflight` for the life of one chat_start.
struct InFlight<'a> {
    set: &'a StdMutex<HashSet<String>>,
    uri: String,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.set.lock().unwrap().remove(&self.uri);
    }
}

impl Backend {
    pub fn new(paths: Paths, version: String) -> Self {
        let settings = state::load_settings(&paths);
        let removed = state::load_removed(&paths).uris.into_iter().collect();
        let stored_once = state::load_stored_once(&paths).models;
        let chat = Chat::new(paths.llama_server(), paths.data_dir.clone(), paths.chat_pidfile());
        Backend {
            engine: Engine::new(paths.clone()),
            chat,
            paths,
            version,
            catalog: catalog::bundled(),
            settings: StdMutex::new(settings),
            removed: StdMutex::new(removed),
            stored_once: StdMutex::new(stored_once),
            pending: StdMutex::new(HashMap::new()),
            started: StdMutex::new(HashMap::new()),
            exported: StdMutex::new(HashMap::new()),
            verified: StdMutex::new(HashMap::new()),
            snap: StdMutex::new(Snapshot::default()),
            refreshing: AtomicBool::new(false),
            runnable_bytes: StdMutex::new(None),
            store_bytes: StdMutex::new(None),
            jobs_lock: Mutex::new(()),
            chat_inflight: StdMutex::new(HashSet::new()),
            host_override: StdMutex::new(None),
            host_failed: StdMutex::new(HashMap::new()),
            last_switch: StdMutex::new(None),
            switching: AtomicBool::new(false),
            settle_lock: std::sync::Arc::new(Mutex::new(())),
            settle_failed: StdMutex::new(HashMap::new()),
            login_error: StdMutex::new(None),
            #[cfg(windows)]
            wsl_heal: wslheal::WslHeal::default(),
            keeper: keeper::Keeper::default(),
            engine_runs: std::sync::atomic::AtomicU64::new(0),
            start_search: Mutex::new(None),
            index_lock: Mutex::new(()),
        }
    }

    // ── lifecycle ───────────────────────────────────────────────────────────

    /// At app start: delete a stale socket, stop an orphaned helper or chat
    /// server from a run that crashed. Starts nothing.
    pub async fn startup_cleanup(&self) {
        #[cfg(windows)]
        let _ = self.protect_data_dir().await;
        #[cfg(unix)]
        match state::make_private(&self.paths.data_dir) {
            Ok(0) => {}
            Ok(n) => self.chat.note(&format!("made {n} entries of {} private to this user", self.paths.data_dir.display())),
            Err(e) => self.chat.note(&format!("could not make the data folder private: {e}")),
        }
        self.chat.kill_orphan().await;
        if let Err(e) = self.engine.cleanup_at_launch().await {
            eprintln!("[startup] {e}");
        }
    }

    /// Windows: the data folder is the user's alone (acl.rs). A folder an
    /// earlier AInode made on another drive is repaired here, at the app's
    /// start, and checked again before every engine start (the UI may start
    /// the engine before the start-up task gets here). A drive without
    /// permissions (exFAT) cannot be helped; that is noted, never a reason
    /// not to start. A folder that stays open to another account is: the
    /// engine's disk image and the models would be theirs to change.
    #[cfg(windows)]
    async fn protect_data_dir(&self) -> Result<(), String> {
        let dir = self.paths.data_dir.clone();
        let r = tokio::task::spawn_blocking(move || crate::acl::ensure_private(&dir)).await.map_err(|e| e.to_string())?;
        match r {
            Ok(crate::acl::Outcome::Private) => Ok(()),
            Ok(crate::acl::Outcome::Repaired { fixed }) => {
                self.chat.note(&format!("made {} private to this user ({fixed} files or folders in it reset)", self.paths.data_dir.display()));
                Ok(())
            }
            Ok(crate::acl::Outcome::NoPermissions) => {
                self.chat.note(&format!("{} is on a drive without permissions: every account here can open it", self.paths.data_dir.display()));
                Ok(())
            }
            Err(e) => {
                self.chat.note(&format!("could not make the data folder private: {e}"));
                Err(format!(
                    "AInode's folder {} is open to another account on this computer and could not be made yours alone: {e}",
                    self.paths.data_dir.display()
                ))
            }
        }
    }

    /// RunEvent::Exit. Never leave an orphan helper (contract). The chat
    /// server is killed without waiting for a model that is still loading.
    pub async fn shutdown(&self) {
        // First: nothing starts the engine again from here on (the self-heal,
        // a WSL recovery under way), and AInode never restarts WSL now.
        self.engine.close();
        self.chat.stop().await;
        let _ = self.engine.stop().await;
    }

    // ── get_state ───────────────────────────────────────────────────────────

    pub async fn get_state(&self) -> AppStateOut {
        let consent = state::load_consent(&self.paths);
        let disk = disk::space_for(&self.paths.data_dir).unwrap_or_default();
        let data_dir = self.paths.data_dir.display().to_string();
        let snap = self.snapshot().await;
        let st = self.engine.state();
        // Windows: restarting WSL, or waiting for easyNode's restart of it,
        // is part of starting.
        #[cfg(windows)]
        let st = if self.wsl_heal.busy() && st != EngineState::Running { EngineState::Starting } else { st };
        let running = st == EngineState::Running;
        let info = snap.info.as_ref().filter(|_| running);

        let engine = EngineOut {
            state: st.name(),
            error: match &st {
                EngineState::Error(e) => Some(e.clone()),
                _ => None,
            },
            openssl: info.map(|i| i.openssl.clone()).filter(|s| !s.is_empty()),
            pq1_ready: info.map(|i| i.pq1_ready),
            bundled: crate::engine::bundled(&self.paths.engine_dir),
        };
        let active = self.active_peer();
        let network = NetworkOut {
            // The host in use (0 or 1), the same running or not: see active_peer.
            peers_configured: u64::from(active.is_some()),
            active_host: active,
            hosts_known: self.bootstrap_peers().len() as u64,
            switched: self
                .last_switch
                .lock()
                .unwrap()
                .as_ref()
                .filter(|(at, _, _)| at.elapsed() < SWITCH_NOTE_FOR)
                .map(|(_, from, to)| format!("{from} -> {to}")),
            peers_connected: info.map(|i| i.peers_connected).unwrap_or(0),
            // 0 until the new helper has its index peers (handed_over_run).
            engine_run: if running { self.handed_over_run().unwrap_or(0) } else { 0 },
        };
        let engine_used_bytes = match (info, snap.used_bytes) {
            (Some(i), _) => i.used_bytes,
            (None, Some(u)) => u,
            (None, None) => *self
                .store_bytes
                .lock()
                .unwrap()
                .get_or_insert_with(|| disk::tree_bytes(&self.paths.modeldir().join("store"))),
        };
        let runnable_bytes = self.runnable_bytes();
        // The user's files and kept chats: their own things, counted together.
        let own_bytes = own::total_bytes(&self.paths.data_dir).saturating_add(chats::total_bytes(&self.paths.data_dir));
        let settings = self.settings.lock().unwrap().clone();
        AppStateOut {
            version: self.version.clone(),
            consent: consent.map(|c| ConsentOut {
                total_bytes: c.total_bytes,
                engine_bytes: c.engine_bytes,
                accepted_at: c.accepted_at,
            }),
            disk,
            engine,
            network,
            data_dir,
            ram_bytes: disk::ram_bytes(),
            space: {
                let held = state::Held { engine: engine_used_bytes, total: engine_used_bytes.saturating_add(runnable_bytes).saturating_add(own_bytes) };
                let (min_gb, max_gb) = state::consent_limits(disk.free_bytes, held);
                SpaceOut { min_gb, max_gb }
            },
            usage: UsageOut {
                engine_used_bytes,
                runnable_bytes,
                own_bytes,
                total_bytes: engine_used_bytes.saturating_add(runnable_bytes).saturating_add(own_bytes),
            },
            theme: settings.theme,
            autostart: settings.autostart,
            think: settings.think,
            face: settings.face,
            models: settings.models,
            keep_chats: settings.keep_chats,
            gpu: settings.gpu,
            count_me: settings.count_me,
            start_at_login: settings.start_at_login,
            login_error: self.login_error.lock().unwrap().clone(),
            community_id: self.community_id(),
            #[cfg(windows)]
            wsl: Some(self.wsl_heal.out()),
            #[cfg(not(windows))]
            wsl: None,
            keeper: self.keeper.out(),
        }
    }

    fn runnable_bytes(&self) -> u64 {
        let mut c = self.runnable_bytes.lock().unwrap();
        match *c {
            Some((at, b)) if at.elapsed() < RUNNABLE_BYTES_TTL => b,
            _ => {
                let b = disk::tree_bytes(&self.paths.checkout_root());
                *c = Some((Instant::now(), b));
                b
            }
        }
    }

    fn forget_runnable_bytes(&self) {
        *self.runnable_bytes.lock().unwrap() = None;
    }

    /// This install's random id among the AInodes online, made once, after
    /// consent only (rule 1: nothing is written before it).
    fn community_id(&self) -> Option<String> {
        if let Some(id) = self.settings.lock().unwrap().community_id.clone() {
            return Some(id);
        }
        state::load_consent(&self.paths)?;
        let id: String = crate::chat::random_key().ok()?.chars().take(32).collect();
        self.settings.lock().unwrap().community_id = Some(id.clone());
        let _ = self.persist_settings();
        Some(id)
    }

    // ── consent / engine ────────────────────────────────────────────────────

    /// The first write the app ever makes. `total_gb` is decimal GB.
    /// The first choice of size, and every later change. A change is checked
    /// against what AInode already holds (`state::consent_limits`, the same
    /// numbers get_state shows). The engine takes its quota when it starts, so
    /// a running one restarts onto the new size; downloads in progress resume
    /// by themselves (E9).
    pub async fn accept_consent(&self, total_gb: u32, data_dir: Option<String>) -> Result<ConsentOutcome, String> {
        if let Some(dir) = data_dir.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            let chosen = validate::data_dir(dir, &self.paths.engine_dir)?;
            if chosen != self.paths.data_dir {
                return self.move_to(chosen, total_gb).await;
            }
        }
        let disk = disk::space_for(&self.paths.data_dir)?;
        let held = self.get_state().await.usage.held();
        let before = state::load_consent(&self.paths);
        let c = state::consent_for(total_gb, disk.free_bytes, held, state::now_unix(), &self.version)?;
        state::write_json_atomic(&self.paths.consent_file(), &c)?;
        // Now that writing is allowed, persist what the UI set before.
        self.persist_settings()?;
        self.persist_removed()?;
        self.apply_login();
        if self.engine.is_running() && before.map(|b| b.engine_bytes) != Some(c.engine_bytes) {
            self.stop_engine().await?;
            self.start_engine().await?;
        }
        Ok(ConsentOutcome { relaunch: false })
    }

    /// The first start with another drive or folder chosen for AInode's data
    /// (Mende, 2026-10-14: the user picks the drive with room). Only before
    /// the first consent: nothing of AInode's exists yet to move. The size is
    /// checked against that drive, the consent and the settings go into the
    /// chosen folder, the pointer into the default one, and the app relaunches
    /// to read its paths again.
    async fn move_to(&self, chosen: PathBuf, total_gb: u32) -> Result<ConsentOutcome, String> {
        if state::load_consent(&self.paths).is_some() {
            return Err("AInode's folder can be chosen at the first start only: it holds models now".into());
        }
        state::create_private_dir_all(&chosen)?;
        #[cfg(windows)]
        crate::acl::ensure_private(&chosen).map_err(|e| format!("That folder cannot be made yours alone, choose another: {e}"))?;
        let disk = disk::space_for(&chosen)?;
        let c = state::consent_for(total_gb, disk.free_bytes, state::Held::default(), state::now_unix(), &self.version)?;
        let there = self.paths.with_data_dir(chosen.clone());
        state::write_json_atomic(&there.consent_file(), &c)?;
        let s = self.settings.lock().unwrap().clone();
        state::write_json_atomic(&there.settings_file(), &s)?;
        let default_dir = state::DEFAULT_DATA_DIR.get().cloned().unwrap_or_else(|| self.paths.data_dir.clone());
        state::write_location(&default_dir, &chosen)?;
        Ok(ConsentOutcome { relaunch: true })
    }

    /// The drives or volumes AInode's data can live on, each with the folder
    /// it would use there and the sizes it allows now.
    pub async fn places(&self) -> Vec<PlaceOut> {
        let held = self.get_state().await.usage.held();
        let here = disk::root_of(&self.paths.data_dir);
        disk::places(&self.paths.data_dir)
            .into_iter()
            .map(|p| {
                let current = p.root == here;
                let data_dir = if current { self.paths.data_dir.clone() } else { PathBuf::from(&p.root).join("AInode") };
                let (min_gb, max_gb) = state::consent_limits(p.free_bytes, held);
                PlaceOut { root: p.root, free_bytes: p.free_bytes, total_bytes: p.total_bytes, data_dir: data_dir.display().to_string(), min_gb, max_gb, current }
            })
            .collect()
    }

    /// Start the engine. Windows: wait while easyNode restarts WSL first, and
    /// when WSL itself is stuck, restart it where that is safe (wslheal.rs).
    pub async fn start_engine(&self) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.wait_for_easynode().await;
            let r = match self.start_once().await {
                Err(e) if crate::wsl::is_stuck(&e) => self.on_wsl_stuck(e).await,
                r => r,
            };
            if let Err(e) = &r {
                // The last word is what the user reads, not the first failure.
                self.engine.failed(e);
                // The Restart button stands only while that is what failed: a
                // start that failed otherwise (WSL back, something else wrong)
                // must not leave a button that would stop the user's Linux.
                if e != crate::wsl::WSL_STUCK_ASK {
                    self.wsl_heal.clear();
                }
            }
            r
        }
        #[cfg(not(windows))]
        self.start_once().await
    }

    async fn start_once(&self) -> Result<(), String> {
        let consent: Consent = state::load_consent(&self.paths)
            .ok_or("Choose how much disk space AInode may use before starting the engine")?;
        #[cfg(windows)]
        self.protect_data_dir().await?;
        // One host at a time: see active_peer.
        let peers: Vec<String> = self.active_peer().into_iter().collect();
        let spawned = self.engine.start(StartArgs { engine_gib: consent.engine_gib(), peers }).await?;
        #[cfg(windows)]
        self.wsl_heal.clear();
        // A start on a running engine (a second click, the keeper and the user
        // at once) is not a new run: no new search, no second hand-over.
        if spawned {
            self.after_engine_start().await;
        }
        let n = self.peer_count().await;
        let mut s = self.snap.lock().unwrap();
        s.at = None;
        s.peers = n;
        Ok(())
    }

    pub async fn stop_engine(&self) -> Result<(), String> {
        self.engine.stop().await?;
        let mut s = self.snap.lock().unwrap();
        s.at = None;
        s.jobs.clear();
        Ok(())
    }

    /// The one host the engine is given. 0.34.15 cannot use two live hosts
    /// for the same model: the retrieve fails with 403 "grant identity" near
    /// the end (experiments E22; fixed by upstream/patches/0008). Until
    /// upstream ships that, the engine knows exactly one host: the one the
    /// user added last, else the first bundled one, else (debug builds) the
    /// first of AINODE_PEERS. Failover between hosts is upstream's job once
    /// the fix is in; then this can hand over every known host again.
    pub fn active_peer(&self) -> Option<String> {
        let known = self.bootstrap_peers();
        if let Some(o) = self.host_override.lock().unwrap().clone().filter(|o| known.contains(o)) {
            return Some(o);
        }
        self.chosen_peer()
    }

    /// The model host as chosen, without a failover's switch (which lasts
    /// until the next add_peer or app start): the user's last, else the
    /// first bundled one.
    pub fn chosen_peer(&self) -> Option<String> {
        let latest_user = self.settings.lock().unwrap().extra_peers.iter().rev().find_map(|p| validate::host_port(p).ok());
        latest_user.or_else(|| self.bootstrap_peers().into_iter().next())
    }

    /// Every host AInode knows: hosts.json, then the user's extra peers, then
    /// AINODE_PEERS (debug builds only, comma separated). Each validated;
    /// duplicates dropped. The engine is given only `active_peer`.
    pub fn bootstrap_peers(&self) -> Vec<String> {
        let mut raw = catalog::bundled_hosts();
        raw.extend(self.settings.lock().unwrap().extra_peers.iter().cloned());
        if let Some(env) = state::dev_env("AINODE_PEERS") {
            let env = env.to_string_lossy();
            raw.extend(env.split(',').map(str::to_string).filter(|s| !s.trim().is_empty()));
        }
        let mut out: Vec<String> = Vec::new();
        for r in raw {
            if let Ok(p) = validate::host_port(&r) {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// Failover (0.7.0). The engine is given one host at a time (E22), so
    /// when that host stops answering a download waits forever (E10). If a
    /// download has had no piece for FAILOVER_AFTER and a transport error,
    /// and another host is known, restart the engine onto the next host:
    /// downloads resume by themselves, verified pieces are kept (E9). At most
    /// one switch per SWITCH_EVERY, so two dead hosts cannot make it flap.
    async fn maybe_failover(&self, snap: &Snapshot) {
        let stuck = snap.jobs.iter().any(|j| {
            j.active() && j.stalled_for_ms >= FAILOVER_AFTER.as_millis() as u64 && j.last_err.as_deref().is_some_and(|e| !e.trim().is_empty())
        });
        if !stuck || !self.engine.is_running() {
            return;
        }
        if self.last_switch.lock().unwrap().as_ref().is_some_and(|(at, _, _)| at.elapsed() < SWITCH_EVERY) {
            return;
        }
        let Some(current) = self.active_peer() else { return };
        let next = {
            let mut failed = self.host_failed.lock().unwrap();
            failed.insert(current.clone(), Instant::now());
            next_host(&self.bootstrap_peers(), &current, &failed, Instant::now())
        };
        let Some(next) = next else { return };
        if self.switching.swap(true, Ordering::SeqCst) {
            return;
        }
        self.chat.note(&format!("host {current} not answering; switching to {next}"));
        *self.host_override.lock().unwrap() = Some(next.clone());
        *self.last_switch.lock().unwrap() = Some((Instant::now(), current, next));
        let _ = self.stop_engine().await;
        if let Err(e) = self.start_engine().await {
            eprintln!("[failover] restart failed: {e}");
        }
        self.switching.store(false, Ordering::SeqCst);
    }

    async fn peer_count(&self) -> Option<usize> {
        let v = self.engine.rpc.call("getmodelpeers", json!([])).await.ok()?;
        v.get("peers").and_then(Value::as_array).map(Vec::len)
    }

    // ── snapshot ────────────────────────────────────────────────────────────

    /// The poll path: never waits for the RPC gate or another refresh.
    async fn snapshot(&self) -> Snapshot {
        {
            let mut s = self.snap.lock().unwrap();
            if !self.engine.is_running() {
                s.info = None;
                s.jobs.clear();
                return s.clone();
            }
            if s.at.is_some_and(|at| at.elapsed() < SNAPSHOT_MAX_AGE) {
                return s.clone();
            }
        }
        if self.refreshing.swap(true, Ordering::SeqCst) {
            return self.snap.lock().unwrap().clone();
        }
        let (snap, _) = self.refresh(Mode::Poll).await;
        self.refreshing.store(false, Ordering::SeqCst);
        snap
    }

    /// For user actions: current facts, waiting for the gate. `Err` when the
    /// helper could not be read, because acting on a stale job list could
    /// start a second worker for the same model.
    async fn fresh(&self) -> Result<Snapshot, String> {
        let (snap, ok) = self.refresh(Mode::Wait).await;
        if ok {
            Ok(snap)
        } else {
            Err("The engine is busy or not answering; try again in a moment".into())
        }
    }

    async fn rpc(&self, mode: Mode, method: &str, params: Value) -> Result<Value, RpcError> {
        match mode {
            Mode::Poll => self.engine.rpc.try_call(method, params, POLL_RPC_BUDGET).await,
            Mode::Wait => self.engine.rpc.call_with(method, params, DEFAULT_TIMEOUT).await,
        }
    }

    async fn refresh(&self, mode: Mode) -> (Snapshot, bool) {
        let started = Instant::now();
        let mut ok = true;
        let mut info = None;
        let mut listed = None;
        let mut jobs = None;
        match self.rpc(mode, "getmodelnetworkinfo", json!([])).await {
            Ok(v) => info = serde_json::from_value::<NetInfo>(v).ok(),
            Err(e) => {
                ok = false;
                log_poll_error("getmodelnetworkinfo", &e);
            }
        }
        if ok {
            match self.rpc(mode, "listmodels", json!([])).await {
                Ok(v) => listed = Some(parse_list::<Listed>(&v, "models")),
                Err(e) => {
                    ok = false;
                    log_poll_error("listmodels", &e);
                }
            }
        }
        if ok {
            match self.rpc(mode, "getmodeljob", json!([])).await {
                Ok(v) => jobs = Some(parse_list::<Job>(&v, "jobs")),
                Err(e) => {
                    ok = false;
                    log_poll_error("getmodeljob", &e);
                }
            }
        }

        // Pin every completed catalog model the user has not removed.
        if let Some(l) = listed.as_mut() {
            let removed = self.removed.lock().unwrap().clone();
            for m in l.iter_mut() {
                let ours = self.catalog.models.iter().any(|e| e.uri == m.uri);
                if ours && m.complete && !m.pinned && !removed.contains(&m.uri) {
                    match self.rpc(mode, "pinmodel", json!([m.uri])).await {
                        Ok(_) => m.pinned = true,
                        Err(e) => log_poll_error("pinmodel", &e),
                    }
                }
            }
        }

        if let Some(j) = &jobs {
            // A pending job id is dropped once the helper shows it ended or
            // no longer lists it. Only on a SUCCESSFUL read: a missed poll
            // must not make download() think nothing is running.
            self.pending.lock().unwrap().retain(|_, id| j.iter().any(|x| x.job_id == *id && x.active()));
        }

        let mut s = self.snap.lock().unwrap();
        s.at = Some(Instant::now());
        if s.data_at.is_none_or(|d| started >= d) {
            if let Some(i) = info {
                s.used_bytes = Some(i.used_bytes);
                s.info = Some(i);
            }
            if let Some(l) = listed {
                s.listed = l;
            }
            if let Some(j) = jobs {
                s.jobs = j;
            }
            if ok {
                s.data_at = Some(started);
            }
        }
        (s.clone(), ok)
    }

    fn invalidate(&self) {
        self.snap.lock().unwrap().at = None;
    }

    // ── catalog ─────────────────────────────────────────────────────────────

    pub async fn catalog(&self) -> Vec<CatalogModel> {
        let snap = self.snapshot().await;
        self.maybe_failover(&snap).await;
        self.catalog_rows(&snap)
    }

    /// The catalog without its side effects (no failover): for the check-in,
    /// which must never switch hosts or restart the engine.
    pub(crate) async fn catalog_quiet(&self) -> Vec<CatalogModel> {
        let snap = self.snapshot().await;
        self.catalog_rows(&snap)
    }

    fn catalog_rows(&self, snap: &Snapshot) -> Vec<CatalogModel> {
        let mut listed = snap.listed.clone();
        if listed.is_empty() && !self.engine.is_running() {
            // App just opened, engine not started: nothing from the helper
            // yet. A runnable copy on disk is proof the model was complete.
            listed = self.listed_from_disk();
        }
        let pending = self.pending.lock().unwrap().clone();
        let started = self.started.lock().unwrap().clone();
        let removed = self.removed.lock().unwrap().clone();
        let runnable = |e: &CatalogEntry, artifact: Option<&str>| self.runnable_gguf(e, artifact).is_some();
        let rows = catalog::join(
            &self.catalog,
            &View { listed: &listed, jobs: &snap.jobs, pending: &pending, started: &started, removed: &removed, runnable: &runnable },
        );
        let once = self.stored_once.lock().unwrap().clone();
        let rows = rows
            .into_iter()
            .map(|mut m| {
                // Kept once: the engine lists it as incomplete (its pieces are
                // gone on purpose); the verified runnable copy is the model.
                if once.contains_key(&m.uri) && m.runnable && m.local != "downloading" && !removed.contains(&m.uri) {
                    m.local = "ready";
                    m.progress = 1.0;
                    m.error = None;
                }
                m
            })
            .collect();
        catalog::shown(&self.catalog, rows)
    }

    /// Plan 3.4: free the space right after a download, not at the first
    /// chat. Given the rows a catalog poll just returned, start one background
    /// task that makes the runnable copy, checks its sha256 and deletes the
    /// pieces (`runnable_path`, which ends in `store_once`). Never inside the
    /// poll's RPC path: the poll returns at once. The Tauri `catalog` command
    /// calls this after every poll; returns whether a task was started.
    pub fn settle_downloads(self: &std::sync::Arc<Self>, rows: &[CatalogModel]) -> bool {
        if !self.engine.is_running() {
            return false;
        }
        let uri = {
            let once = self.stored_once.lock().unwrap();
            let removed = self.removed.lock().unwrap();
            let failed = self.settle_failed.lock().unwrap();
            match to_settle(rows, &once, &removed, &failed, Instant::now()) {
                Some(u) => u,
                None => return false,
            }
        };
        // Busy (another keep-once, a chat starting, Show file): next poll.
        let Ok(guard) = self.settle_lock.clone().try_lock_owned() else { return false };
        let b = self.clone();
        tokio::spawn(async move {
            let _guard = guard;
            let r = b.runnable_path(&uri).await;
            let kept = b.stored_once.lock().unwrap().contains_key(&uri);
            match r {
                Err(e) => b.chat.note(&format!("keep once after download {uri}: {e}")),
                Ok(_) if kept => b.chat.note(&format!("kept once right after its download: {uri}")),
                Ok(_) => {}
            }
            if !kept {
                b.settle_failed.lock().unwrap().insert(uri, Instant::now());
            }
        });
        true
    }

    /// Wait for a keep-once in progress to finish (the engine is running
    /// again then, if it was restarted).
    pub async fn settled(&self) {
        drop(self.settle_lock.lock().await);
    }

    fn listed_from_disk(&self) -> Vec<Listed> {
        self.catalog
            .models
            .iter()
            .filter_map(|e| {
                let gguf = self.runnable_gguf(e, None)?;
                let artifact = gguf.parent()?.file_name()?.to_string_lossy().to_string();
                Some(Listed { uri: e.uri.clone(), artifact_id: artifact, bytes: e.size_bytes, complete: true, ..Default::default() })
            })
            .collect()
    }

    /// The runnable gguf for a catalog entry, if a checkout holds it at its
    /// catalog size: `<checkout>/<artifact_id>/<gguf>`, or any checkout dir
    /// when the artifact id is not known (the helper no longer lists the
    /// model, e.g. evicted while unpinned: the copy is still on disk and
    /// still counts, and the UI must be able to Remove it).
    pub fn runnable_gguf(&self, e: &CatalogEntry, artifact: Option<&str>) -> Option<PathBuf> {
        let size = e.gguf_size()?;
        let root = self.paths.checkout_root();
        let candidates: Vec<PathBuf> = match artifact.filter(|a| !a.is_empty()) {
            Some(a) => vec![root.join(a).join(&e.gguf)],
            None => std::fs::read_dir(&root)
                .ok()?
                .flatten()
                .map(|d| d.path().join(&e.gguf))
                .collect(),
        };
        candidates
            .into_iter()
            .find(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file() && m.len() == size))
    }

    fn entry(&self, uri: &str) -> Result<&CatalogEntry, String> {
        // Only catalog uris ever reach the helper: rule 2, and nothing passes
        // user-supplied ids to it while btxchain/btx#225 is open (README).
        self.catalog
            .models
            .iter()
            .find(|e| e.uri == uri)
            .ok_or_else(|| "That model is not in AInode's catalog".to_string())
    }

    fn require_running(&self) -> Result<(), String> {
        if self.engine.is_running() {
            Ok(())
        } else {
            Err("The engine is not running".into())
        }
    }

    fn is_removed(&self, uri: &str) -> bool {
        self.removed.lock().unwrap().contains(uri)
    }

    // ── download / cancel / remove ──────────────────────────────────────────

    /// getmodel FREE_ONLY, at most once per uri while a job for it is active,
    /// and only if the model can fit in the engine's store.
    pub async fn download(&self, uri: &str) -> Result<(), String> {
        let e = self.entry(uri)?.clone();
        self.require_running()?;
        if self.stored_once.lock().unwrap().contains_key(uri) {
            if self.runnable_gguf(&e, None).is_some() && !self.is_removed(uri) {
                return Ok(()); // already here, kept once
            }
            // The copy is gone (removed, or deleted in Finder) and so are the
            // pieces: the engine would answer "local" and fetch nothing.
            self.forget_stored_once(uri).await?;
            if self.removed.lock().unwrap().remove(uri) {
                self.persist_removed()?;
            }
        }
        let _g = self.jobs_lock.lock().await;
        // Fresh read first: it is what retires a pending id whose job ended.
        let snap = self.fresh().await?;
        if self.pending.lock().unwrap().contains_key(uri) {
            return Ok(());
        }
        let listed = snap.listed.iter().find(|l| l.uri == e.uri);
        let removed = self.is_removed(uri);
        let no_pending = HashMap::new();
        let no_removed = HashSet::new();
        let no = |_: &CatalogEntry, _: Option<&str>| false;
        let view =
            View { listed: &snap.listed, jobs: &snap.jobs, pending: &no_pending, started: &no_pending, removed: &no_removed, runnable: &no };
        if !catalog::active_jobs_for(uri, listed, &view).is_empty() {
            return Ok(());
        }
        let whole = listed.is_some_and(|l| l.complete);
        if whole && !removed {
            return Ok(());
        }
        if !whole {
            let quota = snap
                .info
                .as_ref()
                .map(|i| i.quota_bytes)
                .filter(|&q| q > 0)
                .or_else(|| state::load_consent(&self.paths).map(|c| c.engine_bytes))
                .unwrap_or(0);
            let used = snap.info.as_ref().map(|i| i.used_bytes).unwrap_or(0);
            check_room(&e, &snap.listed, &snap.jobs, quota, used)?;
        }
        let r = self
            .engine
            .rpc
            .call("getmodel", json!([uri, "FREE_ONLY"]))
            .await
            .map_err(|e| e.to_string())?;
        match (r.get("job_id").and_then(Value::as_str), r.get("status").and_then(Value::as_str)) {
            (Some(id), _) => {
                self.pending.lock().unwrap().insert(uri.to_string(), id.to_string());
                self.started.lock().unwrap().insert(uri.to_string(), id.to_string());
            }
            // Already whole in the store (e.g. after Remove, before eviction):
            // the helper answers at once with no job. Pin it now, the user
            // asked for it again.
            (None, Some("local")) => {
                self.engine.rpc.call("pinmodel", json!([uri])).await.map_err(|e| e.to_string())?;
            }
            // With no peers at all the helper starts no job and says
            // `not_local`, `peers: 0` (observed). hosts.json ships empty until
            // the first public host exists, so this is the normal first-run
            // answer today.
            (None, Some("not_local")) if r.get("peers").and_then(Value::as_u64) == Some(0) => {
                return Err("No model host is configured yet. Add a host:port to download from.".into());
            }
            (None, other) => {
                return Err(format!("The engine did not start a download ({})", other.unwrap_or("no status")));
            }
        }
        if self.removed.lock().unwrap().remove(uri) {
            self.persist_removed()?;
        }
        self.invalidate();
        Ok(())
    }

    /// cancelmodeljob [job_id] for every active job of this uri. Pieces stay
    /// (upstream calls it pause). Idempotent.
    pub async fn cancel(&self, uri: &str) -> Result<(), String> {
        self.entry(uri)?;
        self.require_running()?;
        let _g = self.jobs_lock.lock().await;
        self.cancel_locked(uri).await
    }

    async fn cancel_locked(&self, uri: &str) -> Result<(), String> {
        let snap = self.fresh().await?;
        let listed = snap.listed.iter().find(|l| l.uri == uri);
        let pending = self.pending.lock().unwrap().clone();
        let none = HashSet::new();
        let no = |_: &CatalogEntry, _: Option<&str>| false;
        let no_started = HashMap::new();
        let view = View { listed: &snap.listed, jobs: &snap.jobs, pending: &pending, started: &no_started, removed: &none, runnable: &no };
        let mut ids: Vec<String> = catalog::active_jobs_for(uri, listed, &view).iter().map(|j| j.job_id.clone()).collect();
        if let Some(p) = pending.get(uri) {
            if !ids.contains(p) {
                ids.push(p.clone());
            }
        }
        for id in ids {
            // An empty id would LIST jobs instead of cancelling (helper.cpp:7558).
            if id.is_empty() {
                continue;
            }
            self.engine
                .rpc
                .call("cancelmodeljob", json!([id]))
                .await
                .map_err(|e| e.to_string())?;
        }
        self.pending.lock().unwrap().remove(uri);
        self.invalidate();
        Ok(())
    }

    /// unhostmodel (unpins it), then delete this model's runnable copy. The
    /// store pieces are not deleted (no RPC can, E20); unpinned, they become
    /// evictable. Refused while the model is loaded or loading for chat.
    pub async fn remove(&self, uri: &str) -> Result<(), String> {
        let e = self.entry(uri)?.clone();
        self.require_running()?;
        let _g = self.jobs_lock.lock().await;
        // Mark first, then look for a chat; chat_start registers itself first,
        // then looks for the mark. Whichever runs second sees the other, so a
        // model can never be deleted under a llama-server that is loading it.
        let was_removed = !self.removed.lock().unwrap().insert(uri.to_string());
        let unmark = || {
            if !was_removed {
                self.removed.lock().unwrap().remove(uri);
            }
        };
        if self.chat.busy_with(uri) || self.chat_inflight.lock().unwrap().contains(uri) {
            unmark();
            return Err("This model is open in chat. Stop the chat first, then remove it.".into());
        }
        let steps = async {
            self.cancel_locked(uri).await?;
            let snap = self.fresh().await?;
            match self.engine.rpc.call("unhostmodel", json!([uri])).await {
                Ok(_) => Ok(snap),
                // Never stored, or already evicted: nothing to unhost.
                Err(err) if err.code() == Some("NOT_FOUND") => Ok(snap),
                Err(err) => Err(err.to_string()),
            }
        };
        let snap = match steps.await {
            Ok(s) => s,
            Err(err) => {
                unmark();
                return Err(err);
            }
        };
        let listed = snap.listed.iter().find(|l| l.uri == uri);
        self.persist_removed()?;

        // The checkout dir is `<checkout>/<artifact_id>`, which is exactly the
        // `path` exportmodelpath returns (observed). Derived rather than asked
        // for: exportmodelpath on a model with no checkout would first write
        // a full second copy (E19) just so we could delete it.
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(a) = listed.map(|l| l.artifact_id.as_str()).filter(|a| !a.is_empty()) {
            dirs.push(self.paths.checkout_root().join(a));
        }
        if let Some(p) = self.exported.lock().unwrap().remove(uri) {
            dirs.push(p);
        }
        if let Some(g) = self.runnable_gguf(&e, None) {
            if let Some(d) = g.parent() {
                dirs.push(d.to_path_buf());
            }
        }
        dirs.dedup();
        let res = dirs.iter().try_for_each(|d| self.delete_checkout_dir(d));
        self.forget_runnable_bytes();
        self.invalidate();
        res?;
        // Kept once: nothing else holds its bytes, so forget it in the engine
        // too. Remove then frees its space now, and a later Get fetches it.
        if self.stored_once.lock().unwrap().contains_key(uri) {
            drop(_g);
            self.forget_stored_once(uri).await?;
            if self.removed.lock().unwrap().remove(uri) {
                self.persist_removed()?;
            }
            self.invalidate();
        }
        Ok(())
    }

    /// The only directory deletion in the app. Refuses anything that is not
    /// strictly inside `$DATA/modelnet/checkout` (symlinks resolved).
    pub fn delete_checkout_dir(&self, dir: &Path) -> Result<(), String> {
        let root = self.paths.checkout_root();
        if !validate::strictly_inside(&root, dir) {
            return Err(format!("refusing to delete {}: not inside {}", dir.display(), root.display()));
        }
        match std::fs::symlink_metadata(dir) {
            Err(_) => Ok(()),
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(dir).map_err(|e| format!("delete {}: {e}", dir.display())),
            Ok(_) => Err(format!("refusing to delete {}: not a directory", dir.display())),
        }
    }

    // ── runnable path / reveal / chat ───────────────────────────────────────

    /// exportmodelpath, then the catalog's gguf inside the returned dir,
    /// checked against the catalog size and (once per copy) its sha256.
    /// Creates the runnable copy if absent, but only if it fits the size the
    /// user consented to: the copy counts against it like the store does.
    pub async fn runnable_path(&self, uri: &str) -> Result<PathBuf, String> {
        let e = self.entry(uri)?.clone();
        self.require_running()?;
        if self.is_removed(uri) {
            return Err("This model was removed. Download it again first.".into());
        }
        if self.runnable_gguf(&e, None).is_none() {
            let st = self.get_state().await;
            let total = st.consent.map(|c| c.total_bytes).ok_or("No storage size has been chosen yet")?;
            copy_fits(&e, total, st.usage.total_bytes)?;
        }
        let r = self.engine.rpc.call("exportmodelpath", json!([uri])).await;
        self.forget_runnable_bytes();
        let r = match r {
            Ok(v) => v,
            Err(err) => return Err(self.export_failed(uri, &e, err.to_string()).await),
        };
        let dir = r
            .get("path")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
            .map(state::from_engine)
            .ok_or("The engine returned no runnable path")?;
        // Only ever open, reveal or load a file inside our own checkout dir.
        if !validate::strictly_inside(&self.paths.checkout_root(), &dir) {
            return Err(format!("The engine returned a path outside AInode's folder: {}", dir.display()));
        }
        self.exported.lock().unwrap().insert(uri.to_string(), dir.clone());
        let gguf = dir.join(&e.gguf);
        let want = e.gguf_size().unwrap_or(0);
        let mtime = match std::fs::symlink_metadata(&gguf) {
            Ok(m) if m.is_file() && m.len() == want => m.modified().ok(),
            Ok(m) => return Err(format!("{} is {} bytes, the catalog says {want}", gguf.display(), m.len())),
            Err(err) => return Err(format!("{}: {err}", gguf.display())),
        };
        let known = mtime.is_some() && self.verified.lock().unwrap().get(&gguf) == mtime.as_ref();
        if !known {
            let want_sha = e.gguf_sha256().ok_or("The catalog has no sha256 for this model")?.to_string();
            let path = gguf.clone();
            let got = tokio::task::spawn_blocking(move || disk::sha256_file(&path))
                .await
                .map_err(|e| e.to_string())??;
            if got != want_sha {
                // exportmodelpath itself replaces a damaged checkout (observed
                // 2026-10-12: one flipped byte, a new inode with the right bytes,
                // 5.8 s), so this only catches a change between that call and
                // here, or a helper that stops doing it. Drop the copy so the
                // next try makes a new one.
                let _ = self.delete_checkout_dir(&dir);
                self.exported.lock().unwrap().remove(uri);
                self.forget_runnable_bytes();
                self.invalidate();
                return Err(format!(
                    "The runnable copy of {} did not match its published checksum and was deleted. Try again to make a fresh copy.",
                    e.name
                ));
            }
            if let Some(t) = mtime {
                self.verified.lock().unwrap().insert(gguf.clone(), t);
            }
        }
        // The copy is verified: the pieces are a second copy now. Never let
        // this cost the user the chat they asked for.
        if let Some(artifact) = dir.file_name().map(|a| a.to_string_lossy().to_string()) {
            if let Err(e) = self.store_once(uri, &artifact).await {
                self.chat.note(&format!("store once {uri}: {e}"));
            }
        }
        self.invalidate();
        Ok(gguf)
    }

    /// exportmodelpath failed. For a model kept once the engine has no pieces
    /// left to rebuild a damaged copy from (it answers "missing piece", seen
    /// on 0.8.4 and 0.9.1 on 2026-11-24), and Get does nothing while the
    /// damaged copy has the right size. So: if the copy no longer matches its
    /// published checksum, delete it and forget the model in the engine, and
    /// say in plain words that Get fetches it again. Any other failure is
    /// passed on as it was.
    async fn export_failed(&self, uri: &str, e: &CatalogEntry, err: String) -> String {
        if !self.stored_once.lock().unwrap().contains_key(uri) {
            return err;
        }
        let (Some(gguf), Some(want)) = (self.runnable_gguf(e, None), e.gguf_sha256().map(str::to_string)) else {
            return err;
        };
        let path = gguf.clone();
        let got = match tokio::task::spawn_blocking(move || disk::sha256_file(&path)).await {
            Ok(Ok(s)) => s,
            _ => return err,
        };
        if got == want {
            return err;
        }
        // Forgetting the model restarts the engine: never under a running
        // download (store_once keeps the same rule). Nothing is touched then.
        // Nor under a chat server that has this file open (remove() has
        // the same rule).
        if self.chat.busy_with(uri) {
            return format!(
                "The copy of {} on this computer is damaged. Stop the chat first, then open the model again: AInode removes the damaged copy then.",
                e.name
            );
        }
        let _g = self.jobs_lock.lock().await;
        // An engine that does not answer: nothing is known about its
        // downloads, so nothing is touched and its own error is shown.
        let Ok(snap) = self.fresh().await else { return err };
        let busy = snap.jobs.iter().any(|j| j.active()) || !self.pending.lock().unwrap().is_empty();
        if busy {
            return format!(
                "The copy of {} on this computer is damaged. AInode replaces it once the download that is running has finished: open the chat again then.",
                e.name
            );
        }
        if let Some(dir) = gguf.parent() {
            if let Err(why) = self.delete_checkout_dir(dir) {
                return format!("{err} (the copy of {} is damaged and could not be deleted: {why})", e.name);
            }
        }
        self.exported.lock().unwrap().remove(uri);
        self.forget_runnable_bytes();
        let forgot = self.forget_stored_once(uri).await;
        self.invalidate();
        self.chat.note(&format!("damaged copy of {uri} deleted ({err}); sha256 {got}"));
        if let Err(why) = forgot {
            self.chat.note(&format!("forget {uri} after a damaged copy: {why}"));
            return format!(
                "The copy of {} on this computer was damaged and is removed now, but AInode could not finish tidying up after it ({why}). If the engine is not running, press Start; then Get the model again.",
                e.name
            );
        }
        format!(
            "The copy of {} on this computer was damaged, and AInode keeps only one copy, so it could not be repaired. It is removed now: Get it again to chat.",
            e.name
        )
    }

    /// Delete a verified model's pieces (store_once.rs), only with no download
    /// running, then restart the engine so its accounting matches (it reports
    /// the old size until it restarts: observed).
    async fn store_once(&self, uri: &str, artifact: &str) -> Result<(), String> {
        if self.stored_once.lock().unwrap().contains_key(uri) {
            return Ok(());
        }
        let _g = self.jobs_lock.lock().await;
        // Removed while its copy was being checked (remove holds jobs_lock):
        // nothing to keep.
        if self.is_removed(uri) {
            return Ok(());
        }
        let snap = self.fresh().await?;
        if snap.jobs.iter().any(|j| j.active()) || !self.pending.lock().unwrap().is_empty() {
            return Ok(()); // next time: never touch the store under a running fetch
        }
        let store = self.paths.modeldir().join("store");
        let a = artifact.to_string();
        let freed = tokio::task::spawn_blocking(move || store_once::delete_pieces(&store, &a))
            .await
            .map_err(|e| e.to_string())??;
        self.stored_once.lock().unwrap().insert(uri.to_string(), artifact.to_string());
        self.persist_stored_once()?;
        self.chat.note(&format!("store once {uri}: {freed} bytes of pieces deleted"));
        if freed > 0 && self.engine.is_running() {
            self.stop_engine().await?;
            self.start_engine().await?;
        }
        *self.store_bytes.lock().unwrap() = None;
        Ok(())
    }

    /// Make the engine forget a model kept once, so Get fetches it again:
    /// stop it, drop the model from its catalog.json, delete the artifact's
    /// folder in its store, start it again if it was running.
    async fn forget_stored_once(&self, uri: &str) -> Result<(), String> {
        let Some(artifact) = self.stored_once.lock().unwrap().get(uri).cloned() else { return Ok(()) };
        let was_running = self.engine.is_running();
        if was_running {
            self.stop_engine().await?;
        }
        let res = store_once::forget_in_catalog(&self.paths.modeldir().join("catalog.json"), &artifact)
            .and_then(|_| store_once::delete_artifact(&self.paths.modeldir().join("store"), &artifact));
        if res.is_ok() {
            self.stored_once.lock().unwrap().remove(uri);
            self.persist_stored_once()?;
        }
        if was_running {
            self.start_engine().await?;
        }
        *self.store_bytes.lock().unwrap() = None;
        res
    }

    fn persist_stored_once(&self) -> Result<(), String> {
        if state::load_consent(&self.paths).is_none() {
            return Ok(());
        }
        let models = self.stored_once.lock().unwrap().clone();
        state::write_json_atomic(&self.paths.stored_once_file(), &state::StoredOnce { models })
    }

    pub async fn reveal(&self, uri: &str) -> Result<(), String> {
        let gguf = {
            let _settle = self.settle_lock.lock().await;
            self.runnable_path(uri).await?
        };
        crate::os::reveal(&gguf).await
    }

    pub async fn chat_start(&self, uri: &str) -> Result<ChatPort, String> {
        self.entry(uri)?;
        self.chat_inflight.lock().unwrap().insert(uri.to_string());
        let _inflight = InFlight { set: &self.chat_inflight, uri: uri.to_string() };
        let t0 = Instant::now();
        let settle = self.settle_lock.lock().await;
        let made = self.runnable_path(uri).await;
        drop(settle);
        let gguf = match made {
            Ok(g) => g,
            Err(e) => {
                self.chat.note(&format!("runnable copy failed after {} ms: {e}", t0.elapsed().as_millis()));
                return Err(e);
            }
        };
        self.chat.note(&format!("runnable copy ready after {} ms", t0.elapsed().as_millis()));
        let gpu = self.settings.lock().unwrap().gpu;
        // A BTX node on this computer (one loopback request, refused at once
        // when there is none): the chat then leaves it room (chat::thread_args).
        let node_here = crate::neighbour::node_here().await;
        let (port, key) = self.chat.start(uri, &gguf, gpu, node_here).await?;
        Ok(ChatPort { port, key })
    }

    pub async fn chat_stop(&self) {
        self.chat.stop().await;
    }

    // ── settings ────────────────────────────────────────────────────────────

    pub fn set_theme(&self, t: &str) -> Result<(), String> {
        if !state::valid_theme(t) {
            return Err(format!("unknown theme {t:?}"));
        }
        self.settings.lock().unwrap().theme = t.to_string();
        self.persist_settings()
    }

    /// The chat's face (state::valid_face). Random by default.
    pub fn set_face(&self, f: &str) -> Result<(), String> {
        if !state::valid_face(f) {
            return Err(format!("unknown face {f:?}"));
        }
        self.settings.lock().unwrap().face = f.to_string();
        self.persist_settings()
    }

    // ── the user's own files (own.rs) ──────────────────────────────────────

    pub fn knowledge_list(&self) -> Vec<own::OwnFile> {
        own::list(&self.paths.data_dir)
    }

    /// Store the passages read from one dropped file, if they fit the space
    /// the user allowed. Never before consent (rule 1).
    pub async fn knowledge_add(&self, name: &str, kind: &str, passages: Vec<own::Passage>) -> Result<own::OwnFile, String> {
        let st = self.get_state().await;
        let total = st.consent.map(|c| c.total_bytes).ok_or("Choose how much space AInode may use first")?;
        let room = total.saturating_sub(st.usage.total_bytes);
        own::add(&self.paths.data_dir, name, kind, passages, room, state::now_unix())
    }

    pub fn knowledge_remove(&self, id: &str) -> Result<(), String> {
        own::remove(&self.paths.data_dir, id)
    }

    pub fn knowledge_passages(&self) -> Vec<own::Found> {
        own::passages(&self.paths.data_dir)
    }

    // ── saved conversations (chats.rs) ─────────────────────────────────────

    pub fn chats_list(&self, model_id: &str) -> Vec<chats::ChatMeta> {
        chats::list(&self.paths.data_dir, Some(model_id).filter(|m| !m.is_empty()))
    }

    pub fn chat_load(&self, id: &str) -> Result<chats::Chat, String> {
        chats::load(&self.paths.data_dir, id)
    }

    /// Kept only when the user switched Keep chats on, and never before consent.
    pub async fn chat_save(&self, chat: chats::Chat) -> Result<(), String> {
        if !self.settings.lock().unwrap().keep_chats {
            return Ok(());
        }
        let st = self.get_state().await;
        let total = st.consent.map(|c| c.total_bytes).ok_or("Choose how much space AInode may use first")?;
        chats::save(&self.paths.data_dir, chat, total.saturating_sub(st.usage.total_bytes), state::now_unix())
    }

    pub fn chat_delete(&self, id: &str) -> Result<(), String> {
        chats::delete(&self.paths.data_dir, id)
    }

    /// One model's chat settings, from the settings button on its card.
    pub fn set_model_prefs(&self, id: &str, prefs: state::ModelPrefs) -> Result<(), String> {
        if !self.catalog.models.iter().any(|e| e.id == id) {
            return Err(format!("unknown model {id:?}"));
        }
        prefs.check()?;
        self.settings.lock().unwrap().models.insert(id.to_string(), prefs);
        self.persist_settings()
    }

    /// The UI's two remembered switches. Kept in settings.json, not the
    /// webview's localStorage: the webview is incognito so that nothing is
    /// written before consent (rule 1).
    pub fn set_pref(&self, name: &str, on: bool) -> Result<(), String> {
        {
            let mut s = self.settings.lock().unwrap();
            match name {
                "autostart" => s.autostart = on,
                "think" => s.think = on,
                "keep_chats" => s.keep_chats = on,
                "gpu" => s.gpu = on,
                "count_me" => s.count_me = on,
                "start_at_login" => s.start_at_login = on,
                other => return Err(format!("unknown preference {other:?}")),
            }
        }
        self.persist_settings()?;
        if name == "start_at_login" {
            self.apply_login();
        }
        Ok(())
    }

    /// Make the computer's start entry match the setting (login.rs), after
    /// consent only: before it nothing is written, and the switch is only
    /// remembered until consent writes it. A failure is shown in Status, not
    /// fatal: AInode works the same without the entry.
    pub fn apply_login(&self) {
        if state::load_consent(&self.paths).is_none() {
            return;
        }
        let on = self.settings.lock().unwrap().start_at_login;
        let r = crate::login::apply(on);
        *self.login_error.lock().unwrap() = r.err();
    }

    /// Validate, persist, and make it the host in use. A running engine is
    /// restarted onto it rather than given a second host (`active_peer`);
    /// downloads in progress resume by themselves after the restart (E9).
    pub async fn add_peer(&self, hostport: &str) -> Result<(), String> {
        let p = validate::host_port(hostport)?;
        {
            // The search list's lock: an index_peer_add of the same host at
            // the same moment either comes first (and the host leaves the list
            // here) or sees it as the model host (second review, finding 8).
            let _change = self.index_lock.lock().await;
            {
                let mut s = self.settings.lock().unwrap();
                s.extra_peers.retain(|x| x != &p);
                s.extra_peers.push(p.clone()); // last added = in use
                // The new model host leaves the search list: it is asked anyway.
                if let Some(list) = s.index_peers.as_mut() {
                    list.retain(|x| validate::host_port(x).ok().as_deref() != Some(p.as_str()));
                }
            }
            // The user's choice beats an automatic switch.
            *self.host_override.lock().unwrap() = None;
            self.persist_settings()?;
        }
        if self.engine.is_running() {
            self.stop_engine().await?;
            self.start_engine().await?;
        }
        Ok(())
    }

    fn persist_settings(&self) -> Result<(), String> {
        if state::load_consent(&self.paths).is_none() {
            return Ok(()); // rule 1: kept in memory until consent
        }
        let s = self.settings.lock().unwrap().clone();
        state::write_json_atomic(&self.paths.settings_file(), &s)
    }

    fn persist_removed(&self) -> Result<(), String> {
        if state::load_consent(&self.paths).is_none() {
            return Ok(());
        }
        let mut uris: Vec<String> = self.removed.lock().unwrap().iter().cloned().collect();
        uris.sort();
        state::write_json_atomic(&self.paths.removed_file(), &state::Removed { uris })
    }
}

/// A download with no piece for this long and a transport error means the
/// host is gone (E10: the engine retries a dead host for a long time).
const FAILOVER_AFTER: Duration = Duration::from_secs(90);
/// No second automatic switch sooner than this.
const SWITCH_EVERY: Duration = Duration::from_secs(120);
/// Status says a switch happened for this long.
const SWITCH_NOTE_FOR: Duration = Duration::from_secs(30 * 60);
/// A host that failed is skipped for this long, then tried again.
const FAILED_FOR: Duration = Duration::from_secs(10 * 60);

/// The host to switch to: the next known one after `current`, in order,
/// skipping hosts that failed within FAILED_FOR. None when there is no other.
fn next_host(known: &[String], current: &str, failed: &HashMap<String, Instant>, now: Instant) -> Option<String> {
    let start = known.iter().position(|h| h == current).map(|i| i + 1).unwrap_or(0);
    (0..known.len())
        .map(|k| &known[(start + k) % known.len()])
        .find(|h| h.as_str() != current && failed.get(*h).is_none_or(|at| now.duration_since(*at) >= FAILED_FOR))
        .cloned()
}

/// Will `e` fit in the engine's fixed store? Room is the quota minus what is
/// stored, plus what the helper WOULD evict for it: other unpinned models not
/// being downloaded (EnforceQuota skips pinned and active ones, catalog.cpp).
/// Counted from listmodels ranges, not `storage_reclaimable_bytes`, which
/// adds the FULL size of a half-downloaded model (catalog.cpp:850, read).
fn check_room(e: &CatalogEntry, listed: &[Listed], jobs: &[Job], quota: u64, used: u64) -> Result<(), String> {
    let mine = listed.iter().find(|l| l.uri == e.uri);
    let need = e.size_bytes.saturating_sub(mine.map(catalog::committed_bytes).unwrap_or(0));
    let evictable: u64 = listed
        .iter()
        .filter(|l| l.uri != e.uri && !l.pinned)
        .filter(|l| !jobs.iter().any(|j| j.active() && j.model_id == l.model_id))
        .map(catalog::committed_bytes)
        .sum();
    let room = quota.saturating_sub(used).saturating_add(evictable);
    if need > room {
        return Err(format!(
            "{} needs {} but AInode's model store has room for {} of its {}. \
             Choose a larger size, or remove a model first.",
            e.name,
            gb(need),
            gb(room.min(quota)),
            gb(quota)
        ));
    }
    Ok(())
}

/// A new runnable copy of `e` (its gguf, about the model's size again) must
/// fit inside the total the user consented to, next to everything already
/// used (store plus existing copies). The store alone is capped by the
/// engine's quota; the copies are capped here.
fn copy_fits(e: &CatalogEntry, consented: u64, used: u64) -> Result<(), String> {
    let need = e.gguf_size().unwrap_or(e.size_bytes);
    if used.saturating_add(need) > consented {
        return Err(format!(
            "Chatting with {} needs a runnable copy of {}, and AInode has {} left of the {} you allowed. \
             Remove a model, or choose a larger size.",
            e.name,
            gb(need),
            gb(consented.saturating_sub(used)),
            gb(consented)
        ));
    }
    Ok(())
}

/// Decimal GB, one decimal: the unit the consent screen uses.
fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / 1e9)
}

fn log_poll_error(method: &str, e: &RpcError) {
    // Busy is the expected answer while an export or getmodel runs.
    if !matches!(e, RpcError::Busy(_)) {
        eprintln!("[poll] {method}: {e}");
    }
}

fn parse_list<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> Vec<T> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| serde_json::from_value(x.clone()).ok()).collect())
        .unwrap_or_default()
}

/// The default browser, https only.
pub async fn open_url(url: &str) -> Result<(), String> {
    let u = validate::https_url(url)?;
    crate::os::open_url(&u).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend(tmp: &Path) -> Backend {
        let paths = Paths {
            data_dir: tmp.join("AInode"),
            engine_dir: tmp.join("engine"),
            socket: std::env::temp_dir().join(format!("ainb-u{}-{}/m.sock", std::process::id(), tmp.file_name().unwrap().to_string_lossy())),
        };
        Backend::new(paths, "0.1.0".into())
    }

    #[tokio::test]
    async fn a_finished_download_is_kept_once_when_nothing_else_downloads() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let mut rows = b.catalog().await;
        let (a, c) = (rows[0].uri.clone(), rows[1].uri.clone());
        let mut once = std::collections::BTreeMap::new();
        let mut removed = HashSet::new();
        let mut failed = HashMap::new();
        let now = Instant::now();
        assert_eq!(to_settle(&rows, &once, &removed, &failed, now), None, "nothing is ready");
        rows[0].local = "ready".into();
        assert_eq!(to_settle(&rows, &once, &removed, &failed, now).as_deref(), Some(a.as_str()));
        rows[1].local = "downloading".into();
        assert_eq!(to_settle(&rows, &once, &removed, &failed, now), None, "never while a fetch runs");
        rows[1].local = "ready".into();
        once.insert(a.clone(), "x".repeat(96));
        assert_eq!(to_settle(&rows, &once, &removed, &failed, now).as_deref(), Some(c.as_str()), "the next one");
        removed.insert(c.clone());
        assert_eq!(to_settle(&rows, &once, &removed, &failed, now), None, "a removed model is left alone");
        removed.clear();
        failed.insert(c.clone(), now);
        assert_eq!(to_settle(&rows, &once, &removed, &failed, now + Duration::from_secs(60)), None, "a failure waits");
        assert_eq!(to_settle(&rows, &once, &removed, &failed, now + SETTLE_RETRY).as_deref(), Some(c.as_str()));
        // And nothing starts while the engine is stopped.
        assert!(!std::sync::Arc::new(b).settle_downloads(&rows));
    }

    #[tokio::test]
    async fn the_app_offers_the_3b_qwen3_8b_and_gpt_oss_but_not_the_350m() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let ids: Vec<String> = b.catalog().await.into_iter().map(|m| m.id).collect();
        // gpt-oss-20b since 0.5.0: hosted and acceptance-tested on 2026-10-12.
        // Qwen3 8B since 0.7.0, for 16 GB Macs (lab/evidence/2026-10-13/candidates).
        assert_eq!(ids, ["granite-4.2-3b-q4km", "qwen3-8b-q4km", "gpt-oss-20b-mxfp4"]);
        // The 350M stays known to the engine side (and the host) on purpose.
        assert!(b.catalog.models.iter().any(|m| m.id == "granite-4.0-350m-q4km" && !m.offered));
    }

    #[tokio::test]
    async fn model_settings_are_checked_and_shown() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let p = |length: &str, temperature: f32| state::ModelPrefs { think: false, length: length.into(), temperature };
        assert!(b.set_model_prefs("no-such-model", p("short", 0.6)).is_err());
        assert!(b.set_model_prefs("granite-4.2-3b-q4km", p("huge", 0.6)).is_err());
        assert!(b.set_model_prefs("granite-4.2-3b-q4km", p("short", 2.0)).is_err());
        assert!(b.set_model_prefs("granite-4.2-3b-q4km", p("short", f32::NAN)).is_err());
        b.set_model_prefs("granite-4.2-3b-q4km", p("short", 0.2)).unwrap();
        assert!(!b.get_state().await.keep_chats, "chats are not kept unless the user asks");
        assert!(b.get_state().await.gpu, "the graphics card is used unless the user leaves it free");
        b.set_pref("gpu", false).unwrap();
        assert!(!b.get_state().await.gpu);
        let st = b.get_state().await;
        assert_eq!(st.models.get("granite-4.2-3b-q4km"), Some(&p("short", 0.2)));
        // Before consent nothing is written (rule 1), only kept in memory.
        assert!(!b.paths.settings_file().exists());
        // With consent, the choice reaches settings.json and survives a new backend.
        b.accept_consent(4, None).await.unwrap();
        assert!(!state::load_settings(&b.paths).gpu, "kept in settings.json");
        assert!(!Backend::new(b.paths.clone(), "0.1.0".into()).get_state().await.gpu);
        b.set_pref("gpu", true).unwrap();
        assert!(state::load_settings(&b.paths).gpu);
    }

    #[tokio::test]
    async fn starting_with_the_computer_is_on_by_default_and_a_switch() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let st = b.get_state().await;
        assert!(st.start_at_login, "on unless the user says no");
        assert_eq!(st.login_error, None);
        // Before consent the switch is remembered, nothing is written.
        b.set_pref("start_at_login", false).unwrap();
        assert!(!b.paths.data_dir.exists(), "rule 1: nothing written");
        b.accept_consent(4, None).await.unwrap();
        assert!(!state::load_settings(&b.paths).start_at_login, "the switch reached settings.json with consent");
        b.set_pref("start_at_login", true).unwrap();
        assert!(b.get_state().await.start_at_login);
        assert!(state::load_settings(&b.paths).start_at_login);
        // A settings.json from before 0.8.1 has no such field: on.
        std::fs::write(b.paths.settings_file(), "{\"theme\":\"dark\"}").unwrap();
        assert!(state::load_settings(&b.paths).start_at_login);
    }

    #[tokio::test]
    async fn the_community_id_is_made_once_after_consent_and_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let st = b.get_state().await;
        assert_eq!(st.community_id, None, "no id before consent");
        assert!(st.count_me, "counted unless the user says no");
        assert!(!b.paths.data_dir.exists(), "rule 1: nothing written");
        b.accept_consent(4, None).await.unwrap();
        let id = b.get_state().await.community_id.expect("an id after consent");
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{id}");
        assert_eq!(b.get_state().await.community_id.as_deref(), Some(id.as_str()), "the same id every time");
        assert_eq!(Backend::new(b.paths.clone(), "0.1.0".into()).get_state().await.community_id.as_deref(), Some(id.as_str()), "kept in settings.json");
        let other = backend(tempfile::tempdir().unwrap().path());
        other.accept_consent(4, None).await.unwrap();
        assert_ne!(other.get_state().await.community_id, Some(id), "each install its own");
        b.set_pref("count_me", false).unwrap();
        assert!(!b.get_state().await.count_me);
        assert!(!state::load_settings(&b.paths).count_me);
    }

    #[test]
    fn failover_picks_the_next_host_and_skips_recent_failures() {
        let k = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let now = Instant::now();
        let none = HashMap::new();
        let hosts = k(&["a:1", "b:1", "c:1"]);
        assert_eq!(next_host(&hosts, "a:1", &none, now).as_deref(), Some("b:1"));
        assert_eq!(next_host(&hosts, "c:1", &none, now).as_deref(), Some("a:1"), "wraps around");
        let mut failed = HashMap::new();
        failed.insert("b:1".to_string(), now);
        assert_eq!(next_host(&hosts, "a:1", &failed, now).as_deref(), Some("c:1"));
        failed.insert("c:1".to_string(), now);
        assert_eq!(next_host(&hosts, "a:1", &failed, now), None, "every other host failed just now");
        assert_eq!(next_host(&hosts, "a:1", &failed, now + FAILED_FOR).as_deref(), Some("b:1"), "tried again later");
        assert_eq!(next_host(&k(&["a:1"]), "a:1", &none, now), None, "one host: nowhere to go");
        assert_eq!(next_host(&k(&["a:1", "b:1"]), "z:1", &none, now).as_deref(), Some("a:1"));
    }

    #[tokio::test]
    async fn the_engine_gets_one_host_the_last_one_added() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let before = b.active_peer();
        assert_eq!(b.get_state().await.network.peers_configured, u64::from(before.is_some()));
        b.add_peer("203.0.113.7:29447").await.unwrap();
        b.add_peer("[::1]:29447").await.unwrap();
        assert_eq!(b.active_peer().as_deref(), Some("[::1]:29447"));
        // Adding a known host again makes it the one in use, without a duplicate.
        b.add_peer("203.0.113.7:29447").await.unwrap();
        assert_eq!(b.active_peer().as_deref(), Some("203.0.113.7:29447"));
        assert_eq!(b.settings.lock().unwrap().extra_peers, vec!["[::1]:29447".to_string(), "203.0.113.7:29447".to_string()]);
        assert_eq!(b.get_state().await.network.peers_configured, 1);
    }

    #[tokio::test]
    async fn nothing_is_written_before_consent() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let st = b.get_state().await;
        assert!(st.consent.is_none());
        assert_eq!(st.engine.state, "stopped");
        b.set_theme("dark").unwrap();
        b.add_peer("[::1]:29447").await.unwrap();
        b.set_pref("autostart", false).unwrap();
        b.set_pref("think", true).unwrap();
        assert!(b.set_pref("telemetry", true).is_err());
        assert!(b.set_theme("neon").is_err());
        b.set_face("chip").unwrap();
        assert!(b.set_face("ellipse").is_err());
        assert_eq!(b.get_state().await.face, "chip");
        assert!(b.add_peer("nope").await.is_err());
        let err = b.start_engine().await.unwrap_err();
        assert!(err.contains("disk space"), "{err}");
        b.startup_cleanup().await;
        let _ = b.catalog().await;
        assert!(!b.paths.data_dir.exists(), "$DATA must not exist before consent");
        assert!(!b.paths.socket_dir().exists(), "the socket dir is made at engine start, not before");
        let st = b.get_state().await;
        assert_eq!((st.theme.as_str(), st.autostart, st.think), ("dark", false, true));

        assert_eq!(b.accept_consent(4, None).await.unwrap(), ConsentOutcome { relaunch: false });
        let c: Consent = serde_json::from_slice(&std::fs::read(b.paths.consent_file()).unwrap()).unwrap();
        assert_eq!((c.total_bytes, c.engine_bytes), (4_000_000_000, 1 << 30));
        let s = state::load_settings(&b.paths);
        assert_eq!(s.theme, "dark");
        assert_eq!(s.extra_peers, vec!["[::1]:29447".to_string()]);
        assert!(!s.autostart && s.think, "prefs set before consent reach settings.json with it");
        assert!(b.bootstrap_peers().contains(&"[::1]:29447".to_string()));
        assert!(b.accept_consent(3, None).await.is_err());
    }

    #[tokio::test]
    async fn another_folder_at_the_first_start_moves_the_data_and_asks_for_a_relaunch() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        b.set_theme("light").unwrap();
        let places = b.places().await;
        assert!(places.iter().any(|p| p.current), "{places:?}");
        assert!(places.iter().all(|p| p.min_gb >= 4 && !p.data_dir.is_empty()));
        // Not a full path, a path inside the program folder: refused, nothing written.
        assert!(b.accept_consent(4, Some("models".into())).await.is_err());
        assert!(b.accept_consent(4, Some(b.paths.engine_dir.join("..").join("data").to_string_lossy().into_owned())).await.is_err());
        assert!(!b.paths.data_dir.exists());
        // The same folder as now: an ordinary consent.
        let same = b.paths.data_dir.to_string_lossy().into_owned();
        let tmp2 = tempfile::tempdir().unwrap();
        let b2 = backend(tmp2.path());
        assert_eq!(b2.accept_consent(4, Some(same.replace(&tmp.path().to_string_lossy().into_owned(), &tmp2.path().to_string_lossy()))).await.unwrap(), ConsentOutcome { relaunch: false });
        // Another folder: consent and settings there, the pointer here, relaunch.
        let chosen = tmp.path().join("big disk").join("AInode");
        assert_eq!(b.accept_consent(4, Some(chosen.to_string_lossy().into_owned())).await.unwrap(), ConsentOutcome { relaunch: true });
        assert_eq!(state::located(&b.paths.data_dir), Some(chosen.clone()), "the pointer in the default dir");
        let there = b.paths.with_data_dir(chosen.clone());
        assert!(state::load_consent(&there).is_some(), "the consent in the chosen folder");
        assert_eq!(state::load_settings(&there).theme, "light", "what the UI set before consent went along");
        assert!(state::load_consent(&b.paths).is_none(), "no consent in the default dir");
        // Once the data is there, the folder is no longer up for choice.
        let b3 = Backend::new(there.clone(), "0.1.0".into());
        assert!(b3.accept_consent(4, Some(tmp.path().join("other").to_string_lossy().into_owned())).await.unwrap_err().contains("first start only"));
    }

    #[cfg(unix)]
    #[test]
    fn delete_refuses_outside_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let root = b.paths.checkout_root();
        let victim = b.paths.modeldir().join("store");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::create_dir_all(root.join("abc")).unwrap();
        std::fs::write(root.join("abc/x.gguf"), b"x").unwrap();
        for bad in [
            victim.clone(),
            root.clone(),
            root.join(".."),
            root.join("abc/../../store"),
            b.paths.data_dir.clone(),
            PathBuf::from("/"),
        ] {
            assert!(b.delete_checkout_dir(&bad).is_err(), "deleted {}", bad.display());
        }
        std::os::unix::fs::symlink(&victim, root.join("link")).unwrap();
        assert!(b.delete_checkout_dir(&root.join("link")).is_err());
        assert!(victim.exists());
        b.delete_checkout_dir(&root.join("abc")).unwrap();
        assert!(!root.join("abc").exists());
        b.delete_checkout_dir(&root.join("gone")).unwrap();
    }

    #[tokio::test]
    async fn uri_commands_refuse_non_catalog_uris_and_a_stopped_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let alien = "btx://pq1notours";
        for r in [b.download(alien).await, b.cancel(alien).await, b.remove(alien).await, b.reveal(alien).await] {
            assert!(r.unwrap_err().contains("catalog"));
        }
        let ours = &catalog::bundled().models[0].uri;
        assert!(b.download(ours).await.unwrap_err().contains("not running"));
        assert!(b.chat_start(ours).await.unwrap_err().contains("not running"));
    }

    #[test]
    fn a_runnable_copy_must_fit_the_consented_total() {
        let c = catalog::bundled();
        let big = c.models.iter().find(|m| m.id == "granite-4.2-3b-q4km").unwrap();
        let need = big.gguf_size().unwrap();
        // 25 GB allowed, the 3B in the store (2.2 GB): its copy fits.
        assert!(copy_fits(big, 25_000_000_000, big.size_bytes).is_ok());
        // Exactly full after the copy is still allowed; one byte more is not.
        assert!(copy_fits(big, 10_000_000_000, 10_000_000_000 - need).is_ok());
        let err = copy_fits(big, 10_000_000_000, 10_000_000_001 - need).unwrap_err();
        assert!(err.contains("Granite 4.2 3B") && err.contains("of the 10.0 GB you allowed"), "{err}");
    }

    #[test]
    fn room_check_counts_evictable_but_not_pinned_or_active() {
        let c = catalog::bundled();
        let big = c.models.iter().find(|m| m.id == "granite-4.2-3b-q4km").unwrap();
        let small = c.models.iter().find(|m| m.id == "granite-4.0-350m-q4km").unwrap();
        let gib = 1u64 << 30;
        // 1 GiB store, empty: the 2.2 GB model cannot fit, the 237 MB one can.
        let err = check_room(big, &[], &[], gib, 0).unwrap_err();
        assert!(err.contains("Granite 4.2 3B needs 2.2 GB"), "{err}");
        assert!(check_room(small, &[], &[], gib, 0).is_ok());
        // 3 GiB store holding the small model: pinned, it still leaves room.
        let held = Listed {
            uri: small.uri.clone(),
            model_id: "m".into(),
            bytes: small.size_bytes,
            complete: true,
            pinned: true,
            ..Default::default()
        };
        assert!(check_room(big, std::slice::from_ref(&held), &[], 3 * gib, small.size_bytes).is_ok());
        // 2.3 GB store full of a pinned model: no room; unpinned: evictable.
        let quota = 2_300_000_000;
        assert!(check_room(big, std::slice::from_ref(&held), &[], quota, small.size_bytes).is_err());
        let loose = Listed { pinned: false, ..held.clone() };
        assert!(check_room(big, std::slice::from_ref(&loose), &[], quota, small.size_bytes).is_ok());
        // ...unless it is downloading right now.
        let job = Job { job_id: "j".into(), status: "running".into(), model_id: "m".into(), ..Default::default() };
        assert!(check_room(big, std::slice::from_ref(&loose), &[job], quota, small.size_bytes).is_err());
    }

    #[tokio::test]
    async fn open_url_refuses_non_https() {
        assert!(open_url("file:///etc/hosts").await.is_err());
        assert!(open_url("http://example.com").await.is_err());
    }
}
