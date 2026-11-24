//! The Network view (0.7.0): what the BTX model network offers beyond
//! AInode's own store, read from the helper the app already runs. Mende: "the
//! bounties thing and whatever BTX dev itself suggests". Two halves:
//!
//! - **Bounties and release campaigns**, read-only: `searchbounties`
//!   (NETWORK scope) and `getfundablemodels` after one models-only network
//!   search. AInode never funds, awards or pays; there is no wallet in it.
//! - **The human path** of upstream's HUMANS.md (lines 193-216, read): open a
//!   pasted `btx://` link (`getmodelsharecard`, `showmodel`), fetch that
//!   model free (`getmodel FREE_ONLY`), and search what the network offers
//!   (`searchmodels`). A model fetched from a link is kept as files the user
//!   can open; AInode chats only with the models in its own store, which it
//!   checked and measured.
//!
//! Every link is checked by `validate::model_uri` before the helper sees it
//! (the helper reads a local file when handed a path, and an alias can wedge
//! it, btxchain/btx#225). The two calls that wedge on odd ids,
//! `getmodelreleaseeconomics` and `cacheencryptedmodel`, are never made.

use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use super::Backend;
use crate::catalog::{self, Listed, STALL_NOTE_MS};
use crate::validate;

/// A NETWORK search fans out to every peer the helper knows and waits for
/// them (helper.cpp:5346-5464, read); give it time, the poll path skips
/// meanwhile (Rpc::try_call).
const NETWORK_TIMEOUT: Duration = Duration::from_secs(25);

/// Opening the view makes three engine calls in a row: the models-only search,
/// the campaign list, the bounty search. Each waits for the engine alone, and
/// while one is on the wire the status poll shows its last snapshot. Their
/// limits add up to NETWORK_TIMEOUT, so the view can hold the engine for 25 s
/// in all, not three times that. Measured on 2026-11-23 against a host that
/// does not exist: 1.5 s per search (the engine's own limit per host), 3.0 s
/// for the three, the poll never held up. These limits are for an engine that
/// does not answer at all.
const VIEW_SEARCH_TIMEOUT: Duration = Duration::from_secs(10);
const VIEW_LIST_TIMEOUT: Duration = Duration::from_secs(5);
const VIEW_BOUNTY_TIMEOUT: Duration = Duration::from_secs(10);

/// One fetched link as the Network view shows it.
#[derive(Debug, Clone, Serialize)]
pub struct LinkModel {
    pub uri: String,
    pub title: String,
    /// none | downloading | ready
    pub local: &'static str,
    pub progress: f64,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
}

/// The start-up search of one engine run (network_start_search): which run,
/// when it ended (ms since the epoch), and the engine's answer or its error.
#[derive(Debug, Clone)]
pub struct StartSearch {
    pub run: u64,
    pub at_ms: u64,
    pub answer: Result<Value, String>,
}

/// What the page gets from the start-up search: the run, when, and the answer.
fn start_search_out(d: &StartSearch) -> Result<Value, String> {
    match &d.answer {
        Ok(v) => Ok(json!({ "run": d.run, "at_ms": d.at_ms, "answer": v })),
        Err(e) => Err(e.clone()),
    }
}

/// One host of the list the engine asks when it searches (its index peers).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IndexPeer {
    /// host:port, as validate::host_port gave it.
    pub endpoint: String,
    /// One of AInode's bundled hosts (resources/hosts.json).
    pub builtin: bool,
}

/// Why the engine would never ask this host when it searches, if it never
/// would: the reason, in words for the person who typed it. None: it may be
/// asked. The v0.34.16 engine dials a search host with loopback not allowed
/// and drops any address `IsForbiddenOutboundDialAddr` refuses and any port
/// `IsForbiddenControlPort` lists (upstream model_nat.cpp:29-34, 103-112;
/// helper.cpp:1926-1940). Measured on 2026-11-24
/// (lab/evidence/2026-11-24/091-sweep/06-which-hosts-the-engine-dials.txt):
/// 127/8, 0/8, 169.254/16, 100.64/10, 198.18/15 and the node ports came back
/// at once, a public address waited 1.5 s. LAN ranges (10/8, 172.16/12,
/// 192.168/16) are dialled and stay allowed. A name is not resolved here:
/// one that leads to such an address is simply not asked (the view says so).
pub fn never_asked(endpoint: &str) -> Option<String> {
    let (host, port) = endpoint.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    const NODE_PORTS: [u16; 22] = [
        8332, 18332, 18443, 18444, 28332, 28333, 8333, 18333, 38332, 38333, 18766, 18445, 8334, 28334, 19334, 19335, 19443,
        19444, 29334, 29335, 48332, 48333,
    ];
    let this_computer = || Some(format!("{endpoint} is this computer. The engine never asks a host on this computer when it searches, so it cannot go in this list."));
    let not_dialled = || Some(format!("{endpoint} is in an address range BTX's engine never dials for a search (link-local, shared or test-only addresses), so it cannot go in this list."));
    let lower = host.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") {
        return this_computer();
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(a)) => {
            let o = a.octets();
            if o[0] == 127 || o[0] == 0 {
                return this_computer();
            }
            if (o[0] == 169 && o[1] == 254) || (o[0] == 100 && (64..=127).contains(&o[1])) || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) {
                return not_dialled();
            }
        }
        Ok(std::net::IpAddr::V6(a)) => {
            let s = a.segments();
            if a.is_loopback() || a.is_unspecified() {
                return this_computer();
            }
            // ::a.b.c.d, the old IPv4-compatible form: not an address anything dials.
            if s[..6] == [0; 6] {
                return if s[6] >> 8 == 127 || s[6] == 0 { this_computer() } else { not_dialled() };
            }
            if (s[0] == 0xfe80 && s[1..4] == [0; 3]) || (s[0] & 0xfe00) == 0xfc00 {
                return not_dialled();
            }
        }
        Err(_) => {}
    }
    match port.parse::<u16>() {
        Ok(n) if NODE_PORTS.contains(&n) => Some(format!(
            "Port {n} belongs to a BTX or Bitcoin node, and BTX's engine never asks a search host on it, so {endpoint} cannot go in this list."
        )),
        _ => None,
    }
}

/// At most this many hosts in the list: each one is asked at every search.
/// The v0.34.16 engine asks the hosts of a search one after another, waits
/// up to 1.5 s for each (`SEARCH_PEER_TIMEOUT_MS`), and asks at most 8 in all,
/// the index peers first (`SEARCH_FANOUT_MAX`, helper.cpp:7786-7795). Measured
/// on 2026-11-24 with hosts that do not answer: 1 took 1.6 s, 3 took 4.6 s, 8
/// took 12.2 s and pushed the model host out of the search. With 4, a search
/// whose added hosts are all down still ends in about 6 s plus the model host,
/// inside VIEW_SEARCH_TIMEOUT, and the model host is always asked.
pub const INDEX_PEERS_MAX: usize = 4;
/// How a start-up search refusal that sent nothing begins; the page may ask
/// again later in the same run (startsearch.ts).
pub const NOT_SENT: &str = "Not sent: ";
/// What each host that does not answer costs a search, for the refusal's words.
const SEARCH_PEER_WAIT: &str = "1.5 seconds";

/// What opening a link shows: the share card, what the helper knows of the
/// model (empty unless it is local or indexed), and whether AInode's store
/// has it.
#[derive(Debug, Clone, Serialize)]
pub struct LinkCard {
    pub uri: String,
    /// The store's id when this link is one of AInode's own models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_id: Option<String>,
    pub card: Value,
    pub show: Value,
}

impl Backend {
    // ── The count on the chip, without opening the view ────────────────────

    /// One models-only network search with no words, once per engine run,
    /// so the home screen's chip can say how many hosts answered. The page
    /// asks for it when the engine runs and the window is shown (main.ts);
    /// the backend makes sure there is never a second search in the same
    /// run, whoever asks and however often: a later call gets the first
    /// answer (or its error) again. No timer: a new run comes only from an
    /// engine start (the user's, the keeper's, a host switch). Before
    /// consent there is no engine, so no search.
    ///
    /// A refusal in which nothing went to the network starts with
    /// `NOT_SENT` and does not use up the run's search: the engine is not
    /// running, its new run is not handed over yet, or its gate was held
    /// (a download or an export) past the limit. A search that was sent and
    /// failed is kept as the run's answer and not repeated.
    pub async fn network_start_search(&self) -> Result<Value, String> {
        self.start_search_within(VIEW_SEARCH_TIMEOUT).await
    }

    pub(super) async fn start_search_within(&self, limit: Duration) -> Result<Value, String> {
        self.require_running().map_err(|e| format!("{NOT_SENT}{e}"))?;
        // The run is counted only after the new helper has its index peers;
        // until then there is none to search for (second review, finding 2).
        let Some(run) = self.handed_over_run() else {
            return Err(format!("{NOT_SENT}the engine is still starting"));
        };
        // Held across the search: a second caller waits and then reads the answer.
        let mut slot = self.start_search.lock().await;
        if let Some(done) = slot.as_ref().filter(|d| d.run == run) {
            return start_search_out(done);
        }
        let answer = match self
            .engine
            .rpc
            .call_with("searchmodels", json!([{ "text": "", "scope": "NETWORK", "limit": 50, "filters": { "object_kind": "MODEL" } }]), limit)
            .await
        {
            Err(e @ crate::rpc::RpcError::Busy(_)) => return Err(format!("{NOT_SENT}{e}")),
            r => r.map_err(|e| e.to_string()),
        };
        let done = StartSearch { run, at_ms: crate::state::now_unix() * 1000, answer };
        let out = start_search_out(&done);
        *slot = Some(done);
        out
    }

    // ── The hosts the engine asks when it searches (index peers) ───────────
    //
    // The engine's search always goes to the model host it was given
    // (`-modelpeer`): measured 2026-11-24 with no index peer, the start-up
    // search asked 1 host and 1 answered (lab/evidence/2026-11-24). So the
    // list holds only hosts the user added; it starts empty, and the view
    // shows the model host above it as "your model host". Nothing is asked
    // that the user cannot see in the app.

    /// The user's list: every entry checked again, in one form, each once,
    /// at most INDEX_PEERS_MAX. A hand-edited settings.json cannot make the
    /// engine ask more hosts, or one host twice.
    ///
    /// Two kinds are dropped too, whatever settings.json holds: the model host
    /// (asked at every search anyway; listed, it was shown where the page says
    /// it cannot be, and counted twice), and a host on this computer (the
    /// engine never asks a loopback host when it searches).
    pub fn index_peers(&self) -> Vec<IndexPeer> {
        // During a failover the host switched to is the model host for the
        // run: not shown or given as a search host then, but kept saved.
        let model = self.active_peer();
        let builtin = crate::catalog::bundled_hosts();
        self.saved_index_peers()
            .into_iter()
            .filter(|e| Some(e) != model.as_ref())
            .map(|endpoint| IndexPeer { builtin: builtin.contains(&endpoint), endpoint })
            .collect()
    }

    /// The list as it is kept: checked, one form each, without the model host
    /// the user chose (not a failover's, which lasts one run) and without a
    /// host the engine never asks, at most INDEX_PEERS_MAX.
    fn saved_index_peers(&self) -> Vec<String> {
        let raw = self.settings.lock().unwrap().index_peers.clone().unwrap_or_default();
        let chosen = self.chosen_peer();
        let mut out: Vec<String> = Vec::new();
        for e in raw {
            let Ok(endpoint) = validate::host_port(&e) else { continue };
            if out.contains(&endpoint) || Some(&endpoint) == chosen.as_ref() || never_asked(&endpoint).is_some() {
                continue;
            }
            out.push(endpoint);
            if out.len() == INDEX_PEERS_MAX {
                break;
            }
        }
        out
    }

    /// Add a host to the list. Checked here, in the backend: only a host
    /// name or an IP literal and a port reach the engine (validate::host_port,
    /// the same rule as for a model host). A running engine is told first;
    /// only when it took the host is the list saved. So after a failure the
    /// view, the saved list and the engine still agree.
    pub async fn index_peer_add(&self, hostport: &str) -> Result<Vec<IndexPeer>, String> {
        let p = validate::host_port(hostport)?;
        let _change = self.index_lock.lock().await;
        let mut list = self.saved_index_peers();
        if self.active_peer().as_deref() == Some(p.as_str()) || self.chosen_peer().as_deref() == Some(p.as_str()) {
            return Err(format!("{p} is your model host: every search goes to it already"));
        }
        if let Some(why) = never_asked(&p) {
            return Err(why);
        }
        if list.contains(&p) {
            return Err(format!("{p} is in the list already"));
        }
        if list.len() >= INDEX_PEERS_MAX {
            return Err(format!(
                "The list holds at most {INDEX_PEERS_MAX} hosts: each one that does not answer makes every search wait {SEARCH_PEER_WAIT} longer. Remove one first."
            ));
        }
        let told = self.engine.is_running();
        if told {
            self.engine
                .rpc
                .call("addmodelindex", json!([p]))
                .await
                .map_err(|e| format!("AInode could not reach its engine ({e}), so {p} was not added. Try again in a moment."))?;
        }
        list.push(p.clone());
        let before = self.settings.lock().unwrap().index_peers.replace(list);
        if let Err(e) = self.persist_settings() {
            self.settings.lock().unwrap().index_peers = before;
            if told {
                let _ = self.engine.rpc.call("removemodelindex", json!([p])).await;
            }
            return Err(e);
        }
        Ok(self.index_peers())
    }

    /// Remove a host from the list. A running engine is told first; if it
    /// cannot be reached the host stays in the list, saved and shown, because
    /// the engine would go on asking it.
    pub async fn index_peer_remove(&self, hostport: &str) -> Result<Vec<IndexPeer>, String> {
        let p = validate::host_port(hostport)?;
        let _change = self.index_lock.lock().await;
        let mut list = self.saved_index_peers();
        if !list.contains(&p) {
            return Err(format!("{p} is not in the list"));
        }
        let told = self.engine.is_running();
        if told {
            self.engine
                .rpc
                .call("removemodelindex", json!([p]))
                .await
                .map_err(|e| format!("AInode could not reach its engine ({e}), so {p} stays in the list and is still asked. Try again in a moment."))?;
        }
        list.retain(|x| x != &p);
        let before = self.settings.lock().unwrap().index_peers.replace(list);
        if let Err(e) = self.persist_settings() {
            self.settings.lock().unwrap().index_peers = before;
            if told {
                let _ = self.engine.rpc.call("addmodelindex", json!([p])).await;
            }
            return Err(e);
        }
        Ok(self.index_peers())
    }

    /// A new engine run: the engine is given its index peers first, then the
    /// run is counted, so the run's one search never goes out without them.
    /// Called only after a start that spawned a helper (start_once).
    /// The run to report and search for: the backend's count, once it has
    /// caught up with the helpers the engine started (Engine::spawns). None
    /// while the engine is not running or a new helper is still being handed
    /// its index peers.
    pub(crate) fn handed_over_run(&self) -> Option<u64> {
        let run = self.engine_runs.load(std::sync::atomic::Ordering::SeqCst);
        (self.engine.is_running() && run > 0 && run == self.engine.spawns()).then_some(run)
    }

    pub(super) async fn after_engine_start(&self) {
        self.give_index_peers().await;
        self.engine_runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// The engine forgets its index peers when it stops (measured,
    /// lab/evidence/2026-11-24/index-peers), so they are given again after
    /// every start. A host that fails is noted and skipped: the engine still
    /// searches its model host.
    async fn give_index_peers(&self) {
        let _change = self.index_lock.lock().await;
        // What the saved list drops for good (the model host the user chose,
        // a host the engine never asks, a bad or doubled entry) leaves
        // settings.json too. A failover's host is only left out of this run:
        // after the run the user's own host is the model host again, and the
        // failover's host is back in the list (second review, finding 1).
        let kept = self.saved_index_peers();
        let list = self.index_peers();
        let saved = self.settings.lock().unwrap().index_peers.clone();
        if saved.is_some_and(|s| s != kept) {
            self.settings.lock().unwrap().index_peers = Some(kept);
            if let Err(e) = self.persist_settings() {
                eprintln!("[network] could not save the checked host list: {e}");
            }
        }
        for p in list {
            if let Err(e) = self.engine.rpc.call("addmodelindex", json!([p.endpoint])).await {
                eprintln!("[network] could not give the engine the index peer {}: {e}", p.endpoint);
            }
        }
    }

    async fn network_call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.network_call_within(method, params, NETWORK_TIMEOUT).await
    }

    async fn network_call_within(&self, method: &str, params: Value, limit: Duration) -> Result<Value, String> {
        self.require_running()?;
        self.engine.rpc.call_with(method, params, limit).await.map_err(|e| e.to_string())
    }

    /// Bounties the network knows of (searchbounties, NETWORK). Read-only.
    /// `getbounty` only knows bounties this helper published itself, so the
    /// search cards are what the view shows.
    pub async fn network_bounties(&self) -> Result<Value, String> {
        self.network_call_within("searchbounties", json!([{ "scope": "NETWORK", "limit": 50 }]), VIEW_BOUNTY_TIMEOUT).await
    }

    /// Models raising funds to be released (getfundablemodels, nearly funded
    /// first), after one network refresh. A campaign some other helper
    /// published is in that list only once its signed record has arrived, and
    /// the list itself asks nobody (measured on 0.34.16, docs/modelnet/5-discovery.md).
    /// So one models-only NETWORK search goes first; its own answer rides along
    /// as `refresh`: the models the hosts returned and the engine's counters of
    /// who was asked and who answered. If the search fails the list is still
    /// read; `refresh` is then null and `refresh_error` says why, so the view
    /// can say that the search failed instead of showing an empty answer.
    pub async fn network_campaigns(&self) -> Result<Value, String> {
        let refresh = self
            .network_call_within(
                "searchmodels",
                json!([{ "text": "", "scope": "NETWORK", "limit": 50, "filters": { "object_kind": "MODEL" } }]),
                VIEW_SEARCH_TIMEOUT,
            )
            .await;
        let mut list = self.network_call_within("getfundablemodels", json!([]), VIEW_LIST_TIMEOUT).await?;
        if let Some(o) = list.as_object_mut() {
            match refresh {
                Ok(v) => {
                    o.insert("refresh".into(), v);
                }
                Err(e) => {
                    o.insert("refresh".into(), Value::Null);
                    o.insert("refresh_error".into(), Value::String(e));
                }
            }
        }
        Ok(list)
    }

    /// searchmodels over the network, with the scope always explicit
    /// (upstream's docs disagree on the default, helper.cpp:6335-6344), and
    /// for models only: a bounty's record also matches a model search and
    /// would come back as a model with a link.
    pub async fn network_search(&self, text: &str) -> Result<Value, String> {
        let t = text.trim();
        if t.is_empty() || t.chars().count() > 200 || t.chars().any(char::is_control) {
            return Err("Type a few words to search for, at most 200 characters".into());
        }
        self.network_call("searchmodels", json!([{ "text": t, "scope": "NETWORK", "limit": 30, "filters": { "object_kind": "MODEL" } }])).await
    }

    /// A pasted link: checked, then its share card and what the helper knows.
    pub async fn link_open(&self, uri: &str) -> Result<LinkCard, String> {
        let uri = validate::model_uri(uri)?;
        let card = self.network_call("getmodelsharecard", json!([uri])).await?;
        // showmodel answers NOT_FOUND for a model that is neither local nor
        // indexed; that is a normal answer here, not an error.
        let show = self.network_call("showmodel", json!([uri])).await.unwrap_or(Value::Null);
        let store_id = self.catalog.models.iter().find(|e| e.uri == uri && e.offered).map(|e| e.id.clone());
        Ok(LinkCard { uri, store_id, card, show })
    }

    /// Fetch a linked model free. One of AInode's own goes through the
    /// store's download (room check, pinning); any other is a plain
    /// getmodel FREE_ONLY, never pinned, so the engine may evict it for a
    /// model from the store.
    pub async fn link_get(&self, uri: &str, title: &str) -> Result<(), String> {
        let uri = validate::model_uri(uri)?;
        if let Some(e) = self.catalog.models.iter().find(|e| e.uri == uri) {
            if !e.offered {
                return Err(format!("AInode tested {} and stopped offering it: in a conversation it repeats itself.", e.name));
            }
            return self.download(&uri).await;
        }
        self.require_running()?;
        let _g = self.jobs_lock.lock().await;
        let snap = self.fresh().await?;
        if let Some(l) = snap.listed.iter().find(|l| l.uri == uri) {
            if l.complete || self.link_job(&uri, Some(l), &snap.jobs).is_some() {
                self.remember_link(&uri, title)?;
                return Ok(());
            }
        }
        let r = self.engine.rpc.call("getmodel", json!([uri, "FREE_ONLY"])).await.map_err(|e| e.to_string())?;
        match (r.get("job_id").and_then(Value::as_str), r.get("status").and_then(Value::as_str)) {
            (Some(id), _) => {
                self.pending.lock().unwrap().insert(uri.clone(), id.to_string());
                self.started.lock().unwrap().insert(uri.clone(), id.to_string());
            }
            (None, Some("local")) => {}
            (None, Some("not_local")) if r.get("peers").and_then(Value::as_u64) == Some(0) => {
                return Err("No model host is configured yet. Add a host:port under Status.".into());
            }
            (None, other) => return Err(format!("The engine did not start a download ({})", other.unwrap_or("no status"))),
        }
        self.remember_link(&uri, title)?;
        self.invalidate();
        Ok(())
    }

    fn remember_link(&self, uri: &str, title: &str) -> Result<(), String> {
        let title: String = title.chars().filter(|c| !c.is_control()).take(120).collect();
        {
            let mut s = self.settings.lock().unwrap();
            if let Some(e) = s.links.iter_mut().find(|e| e.uri == uri) {
                if !title.is_empty() {
                    e.title = title;
                }
            } else {
                s.links.push(crate::state::LinkEntry { uri: uri.to_string(), title });
            }
        }
        self.persist_settings()
    }

    fn link_job<'a>(&self, uri: &str, listed: Option<&Listed>, jobs: &'a [catalog::Job]) -> Option<&'a catalog::Job> {
        let pending = self.pending.lock().unwrap().get(uri).cloned();
        jobs.iter().filter(|j| j.active()).find(|j| {
            pending.as_deref() == Some(j.job_id.as_str()) || listed.is_some_and(|l| !l.model_id.is_empty() && l.model_id == j.model_id)
        })
    }

    /// The links the user fetched, with their progress.
    pub async fn links(&self) -> Vec<LinkModel> {
        let snap = self.snapshot().await;
        let entries = self.settings.lock().unwrap().links.clone();
        entries
            .into_iter()
            .map(|e| {
                let listed = snap.listed.iter().find(|l| l.uri == e.uri);
                let job = self.link_job(&e.uri, listed, &snap.jobs);
                let failed = self
                    .started
                    .lock()
                    .unwrap()
                    .get(&e.uri)
                    .and_then(|id| snap.jobs.iter().find(|j| &j.job_id == id && j.status == "failed"))
                    .and_then(|j| j.error.clone())
                    .filter(|err| err != "cancelled" && err != "stopped");
                let (local, progress) = match (job, listed) {
                    (Some(_), l) => ("downloading", l.map(catalog::progress).unwrap_or(0.0)),
                    (None, Some(l)) if l.complete => ("ready", 1.0),
                    (None, Some(l)) => ("none", catalog::progress(l)),
                    (None, None) => ("none", 0.0),
                };
                LinkModel {
                    uri: e.uri,
                    title: e.title,
                    local,
                    progress,
                    bytes: listed.map(|l| l.bytes).unwrap_or(0),
                    error: if local == "none" { failed } else { None },
                    waiting: job
                        .filter(|j| j.stalled_for_ms >= STALL_NOTE_MS)
                        .and_then(|j| j.last_err.clone())
                        .filter(|x| !x.trim().is_empty()),
                }
            })
            .collect()
    }

    /// Show a fetched link's files in the file manager: exportmodelpath makes them
    /// readable (a second copy, E19), if it fits the space the user allowed.
    pub async fn link_reveal(&self, uri: &str) -> Result<(), String> {
        let uri = validate::model_uri(uri)?;
        self.require_running()?;
        let snap = self.fresh().await?;
        let listed = snap.listed.iter().find(|l| l.uri == uri).ok_or_else(|| format!("That model is not on this {}", crate::os::DEVICE))?;
        if !listed.complete {
            return Err("It is not fully downloaded yet".into());
        }
        let have = !listed.artifact_id.is_empty() && self.paths.checkout_root().join(&listed.artifact_id).is_dir();
        if !have {
            let st = self.get_state().await;
            let total = st.consent.map(|c| c.total_bytes).ok_or("No storage size has been chosen yet")?;
            if st.usage.total_bytes.saturating_add(listed.bytes) > total {
                return Err("Not enough room in the space you allowed for a copy you can open. Remove something or allow more.".into());
            }
        }
        let r = self.engine.rpc.call("exportmodelpath", json!([uri])).await;
        self.forget_runnable_bytes();
        let dir = r
            .map_err(|e| e.to_string())?
            .get("path")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
            .map(crate::state::from_engine)
            .ok_or("The engine returned no path")?;
        if !validate::strictly_inside(&self.paths.checkout_root(), &dir) {
            return Err(format!("The engine returned a path outside AInode's folder: {}", dir.display()));
        }
        self.exported.lock().unwrap().insert(uri.clone(), dir.clone());
        crate::os::open_folder(&dir).await
    }

    /// Forget a fetched link: stop its download, unhost it, delete the copy
    /// that could be opened. Its pieces are reclaimed when space is needed
    /// (upstream has no delete, E20).
    pub async fn link_remove(&self, uri: &str) -> Result<(), String> {
        let uri = validate::model_uri(uri)?;
        if self.catalog.models.iter().any(|e| e.uri == uri) {
            return Err("That is one of AInode's own models: remove it from its card".into());
        }
        if self.engine.is_running() {
            let _g = self.jobs_lock.lock().await;
            let snap = self.fresh().await?;
            let listed = snap.listed.iter().find(|l| l.uri == uri).cloned();
            if let Some(j) = self.link_job(&uri, listed.as_ref(), &snap.jobs) {
                let _ = self.engine.rpc.call("cancelmodeljob", json!([j.job_id])).await;
            }
            if listed.is_some() {
                let _ = self.engine.rpc.call("unhostmodel", json!([uri])).await;
            }
            if let Some(a) = listed.map(|l| l.artifact_id).filter(|a| !a.is_empty()) {
                self.delete_checkout_dir(&self.paths.checkout_root().join(a))?;
            }
        }
        if let Some(d) = self.exported.lock().unwrap().remove(&uri) {
            self.delete_checkout_dir(&d)?;
        }
        self.pending.lock().unwrap().remove(&uri);
        self.settings.lock().unwrap().links.retain(|e| e.uri != uri);
        self.forget_runnable_bytes();
        self.invalidate();
        self.persist_settings()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Paths;

    fn backend(tmp: &std::path::Path) -> Backend {
        let paths = Paths {
            data_dir: tmp.join("AInode"),
            engine_dir: tmp.join("engine"),
            socket: std::env::temp_dir().join(format!("ainn-u{}-{}/m.sock", std::process::id(), tmp.file_name().unwrap().to_string_lossy())),
        };
        Backend::new(paths, "0.1.0".into())
    }

    /// A stand-in for the engine on the backend's socket: answers every call
    /// and keeps the method and its first argument, in order.
    #[cfg(unix)]
    fn fake_engine(sock: &std::path::Path) -> std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(sock);
        let listener = tokio::net::UnixListener::bind(sock).unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let s2 = seen.clone();
        tokio::spawn(async move {
            loop {
                let (mut c, _) = listener.accept().await.unwrap();
                let s3 = s2.clone();
                tokio::spawn(async move {
                    let mut req = Vec::new();
                    c.read_to_end(&mut req).await.unwrap();
                    let v: Value = serde_json::from_slice(&req[..req.len() - 1]).unwrap();
                    let method = v["method"].as_str().unwrap_or("").to_string();
                    let arg = v["params"][0].as_str().map(str::to_string).unwrap_or_else(|| v["params"][0].to_string());
                    s3.lock().unwrap().push((method.clone(), arg));
                    let result = if method == "searchmodels" {
                        json!({ "results": [], "coverage": { "connected_peers_queried": 1, "index_peers_queried": 1, "responses_received": 2, "timed_out": 0 } })
                    } else {
                        json!({ "schema_version": 2 })
                    };
                    let reply = json!({ "result": result, "error": null, "id": 1 });
                    c.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
                });
            }
        });
        seen
    }

    #[cfg(unix)]
    fn calls(seen: &std::sync::Mutex<Vec<(String, String)>>, method: &str) -> Vec<String> {
        seen.lock().unwrap().iter().filter(|(m, _)| m == method).map(|(_, a)| a.clone()).collect()
    }

    /// A backend with consent given, so settings are written to disk.
    fn backend_with_consent(tmp: &std::path::Path) -> Backend {
        let b = backend(tmp);
        std::fs::create_dir_all(&b.paths.data_dir).unwrap();
        std::fs::write(b.paths.consent_file(), br#"{"total_bytes":4000000000,"engine_bytes":1073741824,"accepted_at":1}"#).unwrap();
        b
    }

    /// A harmless process to stand in for the helper's pid.
    #[cfg(unix)]
    fn stand_in() -> std::process::Child {
        std::process::Command::new("/bin/sleep").arg("60").spawn().unwrap()
    }

    #[test]
    fn the_list_fits_the_engines_search() {
        // v0.34.16: at most 8 hosts a search, index peers first; 1.5 s for each
        // host that does not answer, one after another (INDEX_PEERS_MAX's note).
        const ENGINE_FANOUT: usize = 8;
        const PER_HOST: Duration = Duration::from_millis(1500);
        assert!(INDEX_PEERS_MAX + 1 <= ENGINE_FANOUT, "the model host would be pushed out of the search");
        // Every added host and the model host down: still inside the start-up and view search limit.
        assert!(PER_HOST * (INDEX_PEERS_MAX as u32 + 1) < VIEW_SEARCH_TIMEOUT);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_listed_host_that_becomes_the_model_host_leaves_the_list() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let seen = fake_engine(&b.paths.socket);
        b.index_peer_add("index.example:29447").await.unwrap();
        b.index_peer_add("192.0.2.1:29447").await.unwrap();
        // "Use a host" under Status, with a host that is in the list.
        b.add_peer("192.0.2.1:29447").await.unwrap();
        assert_eq!(b.active_peer().as_deref(), Some("192.0.2.1:29447"));
        assert_eq!(b.index_peers(), vec![IndexPeer { endpoint: "index.example:29447".into(), builtin: false }]);
        assert_eq!(b.settings.lock().unwrap().index_peers, Some(vec!["index.example:29447".to_string()]));
        b.engine.mark_running_for_test(true);
        b.after_engine_start().await;
        assert_eq!(calls(&seen, "addmodelindex"), vec!["index.example:29447"]);
        assert!(b.index_peer_add("192.0.2.1:29447").await.unwrap_err().contains("model host"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_saved_list_with_the_model_host_or_this_computer_is_cleaned_on_load() {
        // A hand-edited file, or one written before these rules: the model
        // host (the engine session found it given to the engine and counted
        // twice) and loopback hosts (never asked by the engine).
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let model = b.active_peer().unwrap();
        b.settings.lock().unwrap().index_peers = Some(vec![
            model.clone(), "127.0.0.1:29447".into(), "localhost:1".into(), "[::1]:1".into(), "index.example:29447".into(),
        ]);
        assert_eq!(b.index_peers(), vec![IndexPeer { endpoint: "index.example:29447".into(), builtin: false }]);
        let seen = fake_engine(&b.paths.socket);
        b.engine.mark_running_for_test(true);
        b.after_engine_start().await;
        assert_eq!(calls(&seen, "addmodelindex"), vec!["index.example:29447"]);
        // Dropped from what is saved too, so a later host switch does not bring them back.
        assert_eq!(b.settings.lock().unwrap().index_peers, Some(vec!["index.example:29447".to_string()]));
    }

    /// Hosts the engine never asks when it searches are refused on add, with
    /// the reason, and dropped from a saved list (second review, finding 7;
    /// the ranges measured in 06-which-hosts-the-engine-dials.txt).
    #[cfg(unix)]
    #[tokio::test]
    async fn a_host_the_engine_never_asks_is_refused_with_the_reason() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let seen = fake_engine(&b.paths.socket);
        b.engine.mark_running_for_test(true);
        let this_computer = [
            "127.0.0.1:29447", "127.9.9.9:1", "0.1.2.3:1", "[::1]:29447", "[::ffff:127.0.0.1]:1", "[::127.0.0.1]:1", "0.0.0.0:1", "[::]:1",
            "localhost:29447", "LOCALHOST:1", "a.localhost:1",
        ];
        let never_dialled = [
            "169.254.169.254:29447", "100.64.0.1:29447", "100.127.255.254:1", "198.18.0.1:1", "198.19.255.254:1", "[fe80::1]:1", "[fd00::1]:1",
            "[fc00::1]:1", "[::ffff:169.254.1.1]:1", "[::203.0.113.1]:1",
        ];
        let node_ports = ["192.0.2.1:8332", "192.0.2.1:19335", "192.0.2.1:19334", "host.example:8333"];
        for h in this_computer {
            let e = b.index_peer_add(h).await.unwrap_err();
            assert!(e.contains("is this computer") && e.contains("never asks"), "{h}: {e}");
        }
        for h in never_dialled {
            let e = b.index_peer_add(h).await.unwrap_err();
            assert!(e.contains("never dials for a search"), "{h}: {e}");
        }
        for h in node_ports {
            let e = b.index_peer_add(h).await.unwrap_err();
            assert!(e.contains("belongs to a BTX or Bitcoin node"), "{h}: {e}");
        }
        assert!(b.index_peers().is_empty());
        assert!(calls(&seen, "addmodelindex").is_empty());
        // Dialled, so allowed: LAN ranges (upstream's own test: 10.0.0.1), public
        // and documentation addresses, a name that only contains the word.
        for ok in ["10.0.0.1:29447", "172.16.0.1:1", "192.168.1.2:1", "192.0.2.1:29447", "[2001:db8::1]:1", "localhost.example:1", "host.example:29447"] {
            assert_eq!(never_asked(&validate::host_port(ok).unwrap()), None, "{ok}");
        }
        // A saved list: such entries are dropped.
        b.settings.lock().unwrap().index_peers = Some(vec!["169.254.1.1:1".into(), "192.0.2.1:8332".into(), "10.0.0.1:29447".into()]);
        assert_eq!(b.index_peers(), vec![IndexPeer { endpoint: "10.0.0.1:29447".into(), builtin: false }]);
    }

    /// Second review, finding 1: a failover to a host that is in the search
    /// list leaves it out for that run only. It stays saved, and is back once
    /// the user's own model host is the model host again.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_failover_does_not_delete_a_host_the_user_added() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend_with_consent(tmp.path());
        let seen = fake_engine(&b.paths.socket);
        // The user's own model host, and the bundled host in the search list.
        b.add_peer("203.0.113.5:29447").await.unwrap();
        let bundled = crate::catalog::bundled_hosts().into_iter().next().expect("a bundled host");
        b.index_peer_add(&bundled).await.unwrap();
        b.index_peer_add("192.0.2.1:29447").await.unwrap();
        // The keeper fails over to the bundled host (maybe_failover).
        *b.host_override.lock().unwrap() = Some(bundled.clone());
        assert_eq!(b.active_peer().as_deref(), Some(bundled.as_str()));
        b.engine.mark_running_for_test(true);
        b.after_engine_start().await;
        // For this run: not shown, not given as a search host.
        assert_eq!(calls(&seen, "addmodelindex"), vec!["192.0.2.1:29447"]);
        assert!(b.index_peers().iter().all(|p| p.endpoint != bundled));
        // Kept: in memory and in settings.json.
        let saved = b.settings.lock().unwrap().index_peers.clone().unwrap();
        assert_eq!(saved, vec![bundled.clone(), "192.0.2.1:29447".to_string()]);
        let on_disk: Value = serde_json::from_slice(&std::fs::read(b.paths.settings_file()).unwrap()).unwrap();
        assert_eq!(on_disk["index_peers"], json!([bundled.clone(), "192.0.2.1:29447"]));
        // The override ends (an app restart, or add_peer): back in the list.
        *b.host_override.lock().unwrap() = None;
        assert_eq!(b.index_peers().len(), 2);
    }

    /// Second review, finding 2: while a new helper runs but has not been
    /// handed its index peers, the run is not reported and no start-up search
    /// goes out, so there is never a search for the old run in the new
    /// process, or one before the hand-over.
    #[cfg(unix)]
    #[tokio::test]
    async fn no_start_up_search_before_the_hand_over() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let seen = fake_engine(&b.paths.socket);
        b.index_peer_add("192.0.2.1:29447").await.unwrap();
        // Running is visible; the hand-over has not happened yet.
        b.engine.mark_running_for_test(true);
        assert_eq!(b.handed_over_run(), None);
        let e = b.network_start_search().await.unwrap_err();
        assert!(e.starts_with(NOT_SENT), "{e}");
        assert!(calls(&seen, "searchmodels").is_empty());
        b.after_engine_start().await;
        assert_eq!(b.handed_over_run(), Some(1));
        assert_eq!(b.network_start_search().await.unwrap()["run"], 1);
        // Run 1 had its search; a restart whose hand-over is under way.
        b.engine.mark_running_for_test(false);
        b.engine.mark_running_for_test(true);
        assert_eq!(b.handed_over_run(), None, "run 1 must not be reported for the new helper");
        assert!(b.network_start_search().await.unwrap_err().starts_with(NOT_SENT));
        // A poll at the very moment of the hand-over: one search for run 2, after the peers.
        let (_, r) = tokio::join!(b.after_engine_start(), b.network_start_search());
        let again = b.network_start_search().await.unwrap();
        if let Ok(r) = r {
            assert_eq!(r, again);
        }
        assert_eq!(again["run"], 2);
        assert_eq!(calls(&seen, "searchmodels").len(), 2);
        let order: Vec<String> = seen.lock().unwrap().iter().map(|(m, _)| m.clone()).collect();
        assert_eq!(order, vec!["addmodelindex", "searchmodels", "addmodelindex", "searchmodels"]);
    }

    /// Second review, finding 4: a refusal that sent nothing (the gate held by
    /// a download or an export) does not use up the run's search; a search
    /// that was sent and failed does (a_failed_start_up_search_is_not_repeated_in_its_run).
    #[cfg(unix)]
    #[tokio::test]
    async fn a_busy_engine_is_not_the_runs_answer() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let seen = fake_engine(&b.paths.socket);
        b.engine.mark_running_for_test(true);
        b.after_engine_start().await;
        let held = b.engine.rpc.hold_gate_for_test().await;
        let e = b.start_search_within(Duration::from_millis(50)).await.unwrap_err();
        assert!(e.starts_with(NOT_SENT) && e.contains("busy"), "{e}");
        drop(held);
        assert!(calls(&seen, "searchmodels").is_empty());
        let r = b.network_start_search().await.unwrap();
        assert_eq!(r["run"], 1);
        assert!(r["answer"]["coverage"].is_object());
        assert_eq!(calls(&seen, "searchmodels").len(), 1);
    }

    /// Every network search the app makes names its scope and asks for
    /// models only (restored from 0.9.0's test, as the second review asked):
    /// the start-up search, the view's refresh and a search with words.
    #[cfg(unix)]
    #[tokio::test]
    async fn every_network_search_is_for_models_only() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let seen = fake_engine(&b.paths.socket);
        b.engine.mark_running_for_test(true);
        b.after_engine_start().await;
        b.network_start_search().await.unwrap();
        let _ = b.network_campaigns().await;
        let _ = b.network_search("granite").await;
        let sent: Vec<String> = seen.lock().unwrap().iter().filter(|(m, _)| m == "searchmodels").map(|(_, a)| a.clone()).collect();
        assert_eq!(sent.len(), 3, "{sent:?}");
        for a in sent {
            let q: Value = serde_json::from_str(&a).unwrap();
            assert_eq!(q["scope"], "NETWORK", "{a}");
            assert_eq!(q["filters"]["object_kind"], "MODEL", "{a}");
        }
    }

    /// Second review, finding 8: "Use a host" and adding the same host to the
    /// search list at the same moment never leave the model host in the list.
    #[tokio::test]
    async fn using_a_host_and_listing_it_at_once_leaves_it_out_of_the_list() {
        for _ in 0..20 {
            let tmp = tempfile::tempdir().unwrap();
            let b = backend(tmp.path());
            let (a, u) = tokio::join!(b.index_peer_add("192.0.2.7:29447"), b.add_peer("192.0.2.7:29447"));
            u.unwrap();
            if let Err(e) = a {
                assert!(e.contains("model host"), "{e}");
            }
            assert_eq!(b.active_peer().as_deref(), Some("192.0.2.7:29447"));
            assert!(b.index_peers().is_empty());
            assert!(!b.settings.lock().unwrap().index_peers.clone().unwrap_or_default().contains(&"192.0.2.7:29447".to_string()));
        }
    }

    /// The count on the chip: exactly one search per engine run, however
    /// often the page asks, and one more after each restart, never before.
    /// The index peers are given before the run is counted.
    #[cfg(unix)]
    #[tokio::test]
    async fn one_start_up_search_per_engine_run() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let seen = fake_engine(&b.paths.socket);
        // No engine: no search, and nothing reaches the socket.
        assert!(b.network_start_search().await.unwrap_err().contains("not running"));
        assert!(seen.lock().unwrap().is_empty());
        // A host the user added while the engine was stopped: saved, nothing sent.
        b.index_peer_add("index.example:29447").await.unwrap();
        assert!(seen.lock().unwrap().is_empty());

        // The first start: the index peers are given, then the run counts.
        b.engine.mark_running_for_test(true);
        b.after_engine_start().await;
        assert_eq!(calls(&seen, "addmodelindex"), vec!["index.example:29447"]);
        let first = b.network_start_search().await.unwrap();
        assert_eq!(first["run"], 1);
        assert_eq!(first["answer"]["coverage"]["responses_received"], 2);
        // The hand-over came before the search.
        let order: Vec<String> = seen.lock().unwrap().iter().map(|(m, _)| m.clone()).collect();
        assert_eq!(order, vec!["addmodelindex", "searchmodels"]);
        // Asked again, from the page or anything else: the same answer, no new search.
        for _ in 0..5 {
            assert_eq!(b.network_start_search().await.unwrap(), first);
        }
        let (x, y) = tokio::join!(b.network_start_search(), b.network_start_search());
        assert_eq!((x.unwrap(), y.unwrap()), (first.clone(), first.clone()));
        assert_eq!(calls(&seen, "searchmodels").len(), 1);
        // The one search names its scope, asks for models only and carries no words.
        let req = seen.lock().unwrap().iter().find(|(m, _)| m == "searchmodels").unwrap().1.clone();
        let q: Value = serde_json::from_str(&req).unwrap();
        assert_eq!((q["text"].as_str(), q["scope"].as_str(), q["filters"]["object_kind"].as_str()), (Some(""), Some("NETWORK"), Some("MODEL")));

        // A restart (the user's, the keeper's, a failover): the peers again, one search more.
        b.engine.mark_running_for_test(false);
        assert!(b.network_start_search().await.is_err());
        b.engine.mark_running_for_test(true);
        b.after_engine_start().await;
        assert_eq!(calls(&seen, "addmodelindex"), vec!["index.example:29447", "index.example:29447"]);
        assert_eq!(b.network_start_search().await.unwrap()["run"], 2);
        b.network_start_search().await.unwrap();
        assert_eq!(calls(&seen, "searchmodels").len(), 2);
    }

    /// Review finding 1: start_engine on an engine that runs (a second click,
    /// the keeper and the user at once, the boot's autostart) is not a new
    /// run: no new search goes out, and the peers are not given again.
    #[cfg(unix)]
    #[tokio::test]
    async fn starting_a_running_engine_again_is_not_a_new_run() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend_with_consent(tmp.path());
        let seen = fake_engine(&b.paths.socket);
        b.index_peer_add("index.example:29447").await.unwrap();
        let mut child = stand_in();
        b.engine.pretend_started_for_test(child.id()).await;
        b.after_engine_start().await;
        b.network_start_search().await.unwrap();
        for _ in 0..3 {
            b.start_engine().await.unwrap();
        }
        let (x, y) = tokio::join!(b.start_engine(), b.start_engine());
        x.unwrap();
        y.unwrap();
        assert_eq!(b.get_state().await.network.engine_run, 1);
        b.network_start_search().await.unwrap();
        assert_eq!(calls(&seen, "searchmodels").len(), 1, "a start on a running engine searched again");
        assert_eq!(calls(&seen, "addmodelindex").len(), 1, "a start on a running engine gave the peers again");
        let _ = child.kill();
        let _ = child.wait();
    }

    /// A failed start-up search is not tried again in the same run: the
    /// chip then shows what it shows without a count. A failover starts a
    /// new run, which gets its own search.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_start_up_search_is_not_repeated_in_its_run() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        // No socket at all: the call fails.
        b.engine.mark_running_for_test(true);
        b.after_engine_start().await;
        let e1 = b.network_start_search().await.unwrap_err();
        assert!(!e1.starts_with(NOT_SENT), "a call that failed at the engine is the run's answer: {e1}");
        let seen = fake_engine(&b.paths.socket);
        // The engine answers now, but this run already had its one search.
        assert_eq!(b.network_start_search().await.unwrap_err(), e1);
        assert!(calls(&seen, "searchmodels").is_empty());
        // Given another host a moment later (failover restarts the engine): a new run, a new search.
        b.engine.mark_running_for_test(false);
        b.engine.mark_running_for_test(true);
        b.after_engine_start().await;
        assert_eq!(b.network_start_search().await.unwrap()["run"], 2);
        assert_eq!(calls(&seen, "searchmodels").len(), 1);
    }

    /// Only a host name or an IP literal and a port reach the engine, checked
    /// in the backend, in one form; a rejected entry changes nothing.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_index_peer_list_takes_only_a_host_and_a_port() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        let seen = fake_engine(&b.paths.socket);
        b.engine.mark_running_for_test(true);
        // Decision 8: the list starts empty; the model host is asked anyway.
        assert!(b.index_peers().is_empty());
        let long = format!("{}.example:29447", "a".repeat(300));
        for bad in [
            "", " ", "host.example", "host.example:", ":29447", "host.example:0", "host.example:65536", "host.example:-1", "host.example:29447x",
            "host example:29447", "host.example:29447\nremovemodelindex", "host.example:29447;rm -rf /", "$(touch /tmp/x):29447", "`id`:29447",
            "\"host\":29447", "host.example:29447/path", "http://host.example:29447", "user@host.example:29447", "host_example:29447",
            "2001:db8::1:29447", "[2001:db8::1]29447", "[not-v6]:29447", "host.example:29447 extra", "\u{0}:29447", &long,
            "0x7f.1:29447", "127.1:29447", "2130706433:29447", "host.127:29447", "a.0xff:29447",
            "h\u{e9}st.example:29447", "\u{e9}:1",
        ] {
            assert!(b.index_peer_add(bad).await.is_err(), "accepted {bad:?}");
            assert!(b.index_peer_remove(bad).await.is_err(), "remove accepted {bad:?}");
        }
        assert!(b.index_peers().is_empty());
        assert!(seen.lock().unwrap().is_empty(), "a rejected entry reached the engine");
        // Good ones, in one form; given to the running engine at once.
        let list = b.index_peer_add("  Index.Example:29447 ").await.unwrap();
        assert_eq!(list, vec![IndexPeer { endpoint: "index.example:29447".into(), builtin: false }]);
        b.index_peer_add("[2001:DB8:0::1]:29447").await.unwrap();
        b.index_peer_add("192.0.2.1:29447").await.unwrap();
        // The same hosts written otherwise are the same entries.
        assert!(b.index_peer_add("INDEX.example:29447").await.unwrap_err().contains("already"));
        assert!(b.index_peer_add("[2001:db8::1]:29447").await.unwrap_err().contains("already"));
        assert!(b.index_peer_add("[::ffff:192.0.2.1]:29447").await.unwrap_err().contains("already"));
        // The model host is asked anyway: not a second time through the list.
        let model_host = b.active_peer().unwrap();
        assert!(b.index_peer_add(&model_host).await.unwrap_err().contains("model host"));
        assert_eq!(calls(&seen, "addmodelindex"), vec!["index.example:29447", "[2001:db8::1]:29447", "192.0.2.1:29447"]);
        b.index_peer_remove("192.0.2.1:29447").await.unwrap();
        assert_eq!(calls(&seen, "removemodelindex"), vec!["192.0.2.1:29447"]);
        for i in 0..INDEX_PEERS_MAX {
            let _ = b.index_peer_add(&format!("h{i}.example:29447")).await;
        }
        assert_eq!(b.index_peers().len(), INDEX_PEERS_MAX);
        assert!(b.index_peer_add("one-more.example:29447").await.unwrap_err().contains("at most"));
    }

    /// Review finding 5: two adds at once both end up in the list and in
    /// the engine.
    #[cfg(unix)]
    #[tokio::test]
    async fn two_adds_at_once_are_both_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend_with_consent(tmp.path());
        let seen = fake_engine(&b.paths.socket);
        b.engine.mark_running_for_test(true);
        let (x, y, z) = tokio::join!(b.index_peer_add("a.example:1"), b.index_peer_add("b.example:1"), b.index_peer_add("c.example:1"));
        (x.unwrap(), y.unwrap(), z.unwrap());
        let mut got: Vec<String> = b.index_peers().into_iter().map(|p| p.endpoint).collect();
        got.sort();
        assert_eq!(got, vec!["a.example:1", "b.example:1", "c.example:1"]);
        let saved: Value = serde_json::from_slice(&std::fs::read(b.paths.settings_file()).unwrap()).unwrap();
        assert_eq!(saved["index_peers"].as_array().unwrap().len(), 3);
        assert_eq!(calls(&seen, "addmodelindex").len(), 3);
    }

    /// Review finding 4: when the engine cannot be reached, nothing changes:
    /// the list shown, the list saved and the engine agree.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_unreachable_engine_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend_with_consent(tmp.path());
        b.index_peer_add("kept.example:1").await.unwrap();
        // Running, but nothing answers on the socket.
        b.engine.mark_running_for_test(true);
        let e = b.index_peer_remove("kept.example:1").await.unwrap_err();
        assert!(e.contains("stays in the list"), "{e}");
        assert_eq!(b.index_peers().len(), 1);
        let saved: Value = serde_json::from_slice(&std::fs::read(b.paths.settings_file()).unwrap()).unwrap();
        assert_eq!(saved["index_peers"], json!(["kept.example:1"]));
        let e = b.index_peer_add("new.example:1").await.unwrap_err();
        assert!(e.contains("was not added"), "{e}");
        assert_eq!(b.index_peers().len(), 1);
        let saved: Value = serde_json::from_slice(&std::fs::read(b.paths.settings_file()).unwrap()).unwrap();
        assert_eq!(saved["index_peers"], json!(["kept.example:1"]));
        // A retry once the engine answers works, and does what it says.
        let seen = fake_engine(&b.paths.socket);
        assert!(b.index_peer_remove("kept.example:1").await.unwrap().is_empty());
        assert_eq!(calls(&seen, "removemodelindex"), vec!["kept.example:1"]);
    }

    /// The list survives a restart of the app; a 0.9.0 settings file without
    /// it loads empty; a hand-edited file cannot widen it.
    #[tokio::test]
    async fn the_index_peer_list_survives_a_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend_with_consent(tmp.path());
        // A 0.9.0 settings file: no index_peers field.
        std::fs::write(b.paths.settings_file(), br#"{"theme":"dark","extra_peers":[],"autostart":true,"think":true,"count_me":false}"#).unwrap();
        let b = backend(tmp.path());
        assert!(b.index_peers().is_empty(), "no host the user did not choose");
        assert!(!b.settings.lock().unwrap().count_me, "the rest of the 0.9.0 file is read as before");
        b.index_peer_add("index.example:29447").await.unwrap();
        let again = backend(tmp.path());
        assert_eq!(again.index_peers(), vec![IndexPeer { endpoint: "index.example:29447".into(), builtin: false }]);
        let raw: Value = serde_json::from_slice(&std::fs::read(again.paths.settings_file()).unwrap()).unwrap();
        assert_eq!(raw["index_peers"], json!(["index.example:29447"]));
        // Entries edited by hand into something else are not given to the engine.
        std::fs::write(again.paths.settings_file(), br#"{"index_peers":["index.example:29447","$(id):1","x"]}"#).unwrap();
        assert_eq!(backend(tmp.path()).index_peers().len(), 1);
        // Review finding 2: 500 valid entries give INDEX_PEERS_MAX; duplicates in any spelling give one.
        let many: Vec<String> = (0..500).map(|i| format!("h{i}.example:1")).collect();
        std::fs::write(again.paths.settings_file(), serde_json::to_vec(&json!({ "index_peers": many })).unwrap()).unwrap();
        assert_eq!(backend(tmp.path()).index_peers().len(), INDEX_PEERS_MAX);
        std::fs::write(
            again.paths.settings_file(),
            br#"{"index_peers":["A.example:1","a.example:1","a.EXAMPLE:1","[2001:DB8::1]:1","[2001:db8:0::1]:1","192.0.2.1:1","192.0.2.1:01"]}"#,
        )
        .unwrap();
        let list: Vec<String> = backend(tmp.path()).index_peers().into_iter().map(|p| p.endpoint).collect();
        assert_eq!(list, vec!["a.example:1", "[2001:db8::1]:1", "192.0.2.1:1"]);
    }

    /// Review finding 3: a wrong value in index_peers does not cost the
    /// user every other setting.
    #[tokio::test]
    async fn a_bad_index_peer_value_keeps_the_other_settings() {
        for bad in [r#"[1]"#, r#""x""#, r#"{"a":1}"#, r#"["ok.example:1", 2]"#, "null", "true"] {
            let tmp = tempfile::tempdir().unwrap();
            let b = backend_with_consent(tmp.path());
            let file = format!(
                r#"{{"theme":"dark","extra_peers":["my.host.example:29447"],"autostart":false,"count_me":false,"links":[{{"uri":"btx://x","title":"t"}}],"index_peers":{bad}}}"#
            );
            std::fs::write(b.paths.settings_file(), file).unwrap();
            let b = backend(tmp.path());
            let s = b.settings.lock().unwrap().clone();
            assert_eq!(s.extra_peers, vec!["my.host.example:29447"], "{bad}");
            assert!(!s.autostart && !s.count_me, "{bad}");
            assert_eq!(s.links.len(), 1, "{bad}");
            assert_eq!(s.index_peers, None, "{bad}");
            assert!(b.index_peers().is_empty());
        }
    }

    #[tokio::test]
    async fn nothing_reaches_the_helper_unchecked_or_while_stopped() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(tmp.path());
        // A path or junk is refused before the engine is even asked.
        for bad in ["/etc/passwd", "btx://pq1notours", "smoke"] {
            let e = b.link_open(bad).await.unwrap_err();
            assert!(!e.contains("not running"), "{bad}: {e}");
            assert!(b.link_get(bad, "").await.is_err());
        }
        let good = crate::catalog::bundled().models[0].uri.clone();
        assert!(b.link_open(&good).await.unwrap_err().contains("not running"));
        assert!(b.network_bounties().await.unwrap_err().contains("not running"));
        assert!(b.network_campaigns().await.unwrap_err().contains("not running"));
        assert!(b.network_search("granite").await.unwrap_err().contains("not running"));
        assert!(b.network_search(" ").await.is_err());
        assert!(b.link_remove(&good).await.unwrap_err().contains("own models"));
        let dropped = crate::catalog::bundled().models.into_iter().find(|m| !m.offered).unwrap();
        assert!(b.link_get(&dropped.uri, "").await.unwrap_err().contains("stopped offering"));
        assert!(b.links().await.is_empty());
    }

    /// Every engine method name the app's own code can send: each `"name"`
    /// string in the non-test part of every file under src/. Comment lines
    /// and each file's `mod tests` are left out.
    fn app_sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let src = std::fs::read_to_string(&p).unwrap();
                    let code = src.rsplit_once("\nmod tests").map(|(a, _)| a).unwrap_or(&src);
                    let code: String = code.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
                    out.push((p.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap().display().to_string(), code));
                }
            }
        }
        let mut out = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
        out
    }

    /// AInode has no wallet and the view is read-only. No file of the app
    /// names a call that funds, pledges, publishes, signs, submits, awards,
    /// refunds or reports to the chain, nor one of the calls that hang the
    /// helper on an odd id (getmodelreleaseeconomics, cacheencryptedmodel,
    /// and, found on 2026-11-23, getmodeleconomyentry).
    #[test]
    fn no_file_of_the_app_names_a_money_call_or_a_call_that_hangs() {
        let files = app_sources();
        // The scan sees the real code: every file is there, and it finds the
        // calls the view does make, in this module.
        assert!(files.len() >= 20, "only {} files scanned", files.len());
        let me = &files.iter().find(|(f, _)| f.ends_with("backend/network.rs")).expect("network.rs scanned").1;
        for made in ["searchbounties", "getfundablemodels", "searchmodels", "getmodelsharecard", "showmodel", "getmodel"] {
            assert!(me.contains(&format!("\"{made}\"")), "the scan no longer finds {made}, which the view calls");
        }
        const FORBIDDEN: &[&str] = &[
            // hang the helper on an odd id
            "getmodelreleaseeconomics", "cacheencryptedmodel", "getmodeleconomyentry",
            // release campaigns: plan, fund, pledge, create, claim, refund
            "preparefundmodelrelease", "preparemodelfunding", "signmodelfunding", "submitmodelfunding", "pledgemodelrelease", "createmodelrelease",
            "claimmodelrelease", "refundmodelrelease", "buildmodelhtlcclaim", "buildmodelhtlcrefund", "recordmodelfundingoutpoint",
            // bounties: draft, publish, pledge, fund, submit, evaluate, award, claim, refund, report
            "createbountydraft", "publishbounty", "revisebounty", "pledgebounty", "freezebountyfundinground", "preparebountyfunding", "signbountyfunding",
            "submitbountyfunding", "commitbountysubmission", "revealbountysubmission", "nominatebountyevaluator", "publishbountyevaluation",
            "proposebountyaward", "approvebountyaward", "preparebountyaward", "signbountyaward", "submitbountyaward", "preparebountyclaim",
            "signbountyclaim", "submitbountyclaim", "preparebountyrefund", "signbountyrefund", "submitbountyrefund", "observebountychain",
            "ingestchainfundingobservation",
            // a wallet's own calls
            "sendtoaddress", "sendrawtransaction", "signrawtransactionwithwallet", "walletprocesspsbt",
        ];
        for (file, code) in &files {
            for name in FORBIDDEN {
                assert!(!code.contains(&format!("\"{name}\"")), "{file} names the engine call {name}");
            }
        }
        // Every network search names its scope and asks for models only:
        // as many model filters as searches (and every_network_search_is_for_models_only).
        assert!(me.matches("\"searchmodels\"").count() >= 3);
        assert_eq!(me.matches("\"searchmodels\"").count(), me.matches("\"object_kind\": \"MODEL\"").count());
        assert!(me.matches("\"scope\": \"NETWORK\"").count() > me.matches("\"searchmodels\"").count());
        // In this module also: nothing that makes the engine serve or index.
        for name in ["hostmodel", "publishmodelsearchrecord", "importmodelindex"] {
            assert!(!me.contains(&format!("\"{name}\"")), "the Network view's module names {name}");
        }

    }

    /// The scan above can fail: given a file that names a forbidden call, the
    /// same check finds it, and it does not mistake a comment or a longer name.
    #[test]
    fn the_scan_finds_what_it_looks_for() {
        let hit = "fn f() { rpc.call(\"submitbountyfunding\", json!([])) }";
        assert!(hit.contains(&format!("\"{}\"", "submitbountyfunding")));
        let longer = "rpc.call(\"unhostmodel\", json!([uri]))";
        assert!(!longer.contains(&format!("\"{}\"", "hostmodel")));
        let files = app_sources();
        let me = &files.iter().find(|(f, _)| f.ends_with("backend/network.rs")).unwrap().1;
        assert!(!me.contains("the_scan_finds_what_it_looks_for"), "the tests module is left out of the scan");
        assert!(!me.contains("//!"), "comment lines are left out of the scan");
    }

    /// Opening the view can hold the engine for NETWORK_TIMEOUT in all.
    #[test]
    fn the_views_three_calls_share_one_limit() {
        assert_eq!(VIEW_SEARCH_TIMEOUT + VIEW_LIST_TIMEOUT + VIEW_BOUNTY_TIMEOUT, NETWORK_TIMEOUT);
    }
}
