//! The bundled model catalog and bootstrap hosts, the helper's view of them
//! (listmodels + getmodeljob), and the join the UI sees.
//!
//! catalog.json and hosts.json are compiled in (include_str!), not read from
//! the bundle at runtime: the backend's behaviour depends on them (which uris
//! it will ever pass to the helper, which gguf it runs), so they ship inside
//! the same signed binary as the code that trusts them and a unit test proves
//! they parse.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

const CATALOG_JSON: &str = include_str!("../resources/catalog.json");
const HOSTS_JSON: &str = include_str!("../resources/hosts.json");

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    pub models: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub licence: String,
    pub licence_url: String,
    pub source_url: String,
    pub summary: String,
    pub uri: String,
    pub size_bytes: u64,
    pub files: Vec<CatalogFile>,
    /// File name of the runnable gguf inside the checkout dir.
    pub gguf: String,
    /// Offered in the app. Granite 4.0 350M is kept for the engine and the
    /// public host (a 6-second smoke test) but not offered: in Mende's own
    /// chat it answered every question with the same canned line, and no
    /// instruction fixed it (2026-10-12). A model that is not offered still
    /// shows while any of it is on disk, so it can be removed (`shown`).
    #[serde(default = "offered_default")]
    pub offered: bool,
    /// The smallest Mac memory, in GB, the model runs well on: its weights
    /// plus the chat's working memory must fit next to macOS. The app does
    /// not offer Get on a Mac with less. 0: no limit.
    #[serde(default)]
    pub ram_gb: u32,
    /// What it is good at, a few words for the store ("chat, writing, code").
    #[serde(default)]
    pub good_at: String,
}

fn offered_default() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

impl CatalogEntry {
    pub fn gguf_size(&self) -> Option<u64> {
        self.files.iter().find(|f| f.path == self.gguf).map(|f| f.size)
    }
    pub fn gguf_sha256(&self) -> Option<&str> {
        self.files.iter().find(|f| f.path == self.gguf).map(|f| f.sha256.as_str())
    }
}

pub fn bundled() -> Catalog {
    serde_json::from_str(CATALOG_JSON).expect("resources/catalog.json is compiled in and unit-tested")
}

/// hosts.json: `{"hosts": ["host:port", ...]}`. Empty until the first public
/// host is proven reachable (contract).
#[derive(Debug, Clone, Deserialize)]
struct Hosts {
    #[serde(default)]
    hosts: Vec<String>,
}

pub fn bundled_hosts() -> Vec<String> {
    serde_json::from_str::<Hosts>(HOSTS_JSON).expect("resources/hosts.json is compiled in and unit-tested").hosts
}

// ── The helper's view ───────────────────────────────────────────────────────

/// One `listmodels` entry, only the fields the app reads. Everything else the
/// helper sends (admission, share card, ratio...) is ignored.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Listed {
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub artifact_id: String,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub files: Vec<ListedFile>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListedFile {
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub piece_size: u64,
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub ranges: Vec<PieceRange>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct PieceRange {
    pub first: u64,
    pub count: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Job {
    #[serde(default)]
    pub job_id: String,
    /// queued | running | done | failed, and `cancelled` between a
    /// cancelmodeljob and the worker noticing (helper.cpp:7570, read).
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub created_ms: i64,
    /// Time since the job last committed a piece (getmodeljob, observed).
    #[serde(default)]
    pub stalled_for_ms: u64,
    /// The transport error behind the latest retry, e.g. "tls io … unexpected
    /// eof" while the host is down (docs/experiments.md E10). Empty when fine.
    #[serde(default)]
    pub last_err: Option<String>,
}

impl Job {
    /// A resumed job (after a helper restart) reports `queued` while it
    /// downloads (contract), so queued counts as active.
    pub fn active(&self) -> bool {
        matches!(self.status.as_str(), "queued" | "running")
    }
}

/// Committed bytes of one file from its piece ranges.
///
/// Not the contract's `min(file_size, count * piece_size)` per range: that
/// overcounts whenever a range ends in the file's short last piece and the
/// file has more than one range. Observed mid-download: ranges [{0,1},{6,2}]
/// over a 57-piece file; once such a file completes as [{0,1},{1,56}] the
/// per-range formula gives 4 MiB + 56 x 4 MiB = 239,075,328 > the 236,985,760
/// byte file. Each range here is clipped to the file's real end instead.
pub fn file_committed_bytes(f: &ListedFile) -> u64 {
    if f.piece_size == 0 {
        return if f.complete { f.file_size } else { 0 };
    }
    f.ranges
        .iter()
        .map(|r| {
            let start = r.first.saturating_mul(f.piece_size).min(f.file_size);
            let end = r.first.saturating_add(r.count).saturating_mul(f.piece_size).min(f.file_size);
            end - start
        })
        .fold(0u64, u64::saturating_add)
        .min(f.file_size)
}

/// Bytes of this model actually in the store.
pub fn committed_bytes(m: &Listed) -> u64 {
    if m.complete {
        return m.bytes;
    }
    m.files.iter().map(file_committed_bytes).fold(0, u64::saturating_add)
}

/// Per-model progress 0..1 from listmodels ranges. NOT from getmodeljob's
/// `bytes_committed`, which counts the whole store, not the job (contract).
pub fn progress(m: &Listed) -> f64 {
    if m.complete {
        return 1.0;
    }
    if m.bytes == 0 {
        return 0.0;
    }
    let have = committed_bytes(m);
    // Every piece can be in while the model is still being verified (observed:
    // ~0.6 s of `complete: false` with all ranges full). Hold below 1 so the
    // UI never shows 100% next to "downloading".
    (have as f64 / m.bytes as f64).clamp(0.0, 0.999)
}

// ── The join ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub licence: String,
    pub licence_url: String,
    pub size_bytes: u64,
    pub uri: String,
    pub summary: String,
    pub source_url: String,
    /// The smallest Mac memory it runs well on, in GB (0: no limit).
    pub ram_gb: u32,
    /// What it is good at, for the store.
    pub good_at: String,
    /// False for a model kept only so its copy can be removed (`shown`).
    pub offered: bool,
    /// none | downloading | ready
    pub local: &'static str,
    pub progress: f64,
    /// A runnable copy (checkout dir) is on disk. Independent of `local`: a
    /// copy outlives its pieces if the store ever drops them, and it still
    /// takes disk the user must be able to Remove.
    pub runnable: bool,
    /// Why the last download of this model stopped, when it failed (not when
    /// the user cancelled it). Not in the contract's shape; optional.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Set while a download is stuck retrying (no piece for STALL_NOTE_MS and a
    /// transport error): the engine keeps trying for a long time without
    /// failing (E10), so without this the UI would just sit still.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
}

/// A slow line can go minutes between pieces with no error at all (E5c: first
/// piece at 117 s), so silence alone is not a problem; an error plus silence is.
pub const STALL_NOTE_MS: u64 = 20_000;

/// Everything the join needs, gathered by the caller.
pub struct View<'a> {
    pub listed: &'a [Listed],
    pub jobs: &'a [Job],
    /// uri -> job_id returned by our own getmodel, for the moment before
    /// listmodels knows the model (observed: the first poll after getmodel
    /// has the job but no listmodels entry).
    pub pending: &'a HashMap<String, String>,
    /// uri -> the last job our own getmodel started, kept after it ends. A
    /// fetch that fails before the engine lists the model (a host without the
    /// model: "manifest failed") is only findable through this.
    pub started: &'a HashMap<String, String>,
    pub removed: &'a HashSet<String>,
    /// Does a runnable gguf exist for this entry (and artifact id if known)?
    pub runnable: &'a (dyn Fn(&CatalogEntry, Option<&str>) -> bool + Sync),
}

pub fn join(catalog: &Catalog, v: &View) -> Vec<CatalogModel> {
    catalog.models.iter().map(|e| join_one(e, v)).collect()
}

/// The rows the app shows: offered models, plus any other model that still
/// holds disk (pieces, a download in flight, or a runnable copy). Hiding
/// those would leave the user no Remove for space the budget still counts.
pub fn shown(catalog: &Catalog, rows: Vec<CatalogModel>) -> Vec<CatalogModel> {
    rows.into_iter()
        .filter(|m| {
            let offered = catalog.models.iter().any(|e| e.id == m.id && e.offered);
            offered || m.local != "none" || m.progress > 0.0 || m.runnable
        })
        .collect()
}

pub fn active_jobs_for<'a>(uri: &str, listed: Option<&Listed>, v: &'a View) -> Vec<&'a Job> {
    let pending = v.pending.get(uri);
    v.jobs
        .iter()
        .filter(|j| j.active())
        .filter(|j| {
            pending.is_some_and(|p| *p == j.job_id)
                || listed.is_some_and(|l| !l.model_id.is_empty() && l.model_id == j.model_id)
        })
        .collect()
}

fn join_one(e: &CatalogEntry, v: &View) -> CatalogModel {
    let listed = v.listed.iter().find(|l| l.uri == e.uri);
    let active_jobs = active_jobs_for(&e.uri, listed, v);
    let active = !active_jobs.is_empty();
    let waiting = active_jobs
        .iter()
        .filter(|j| j.stalled_for_ms >= STALL_NOTE_MS)
        .find_map(|j| j.last_err.clone().filter(|err| !err.trim().is_empty()));
    let removed = v.removed.contains(&e.uri);

    let (local, progress) = if active {
        ("downloading", listed.map(progress).unwrap_or(0.0))
    } else if removed {
        ("none", 0.0)
    } else if let Some(l) = listed {
        if l.complete {
            ("ready", 1.0)
        } else {
            // Cancelled or failed part-way: pieces stay (cancelmodeljob is a
            // pause, btx-model `pause`). Show how far it got; a new download
            // resumes from there.
            ("none", progress(l))
        }
    } else {
        ("none", 0.0)
    };

    let runnable = (v.runnable)(e, listed.map(|l| l.artifact_id.as_str()));

    // The newest terminal job for this model, if it failed for a reason other
    // than the user's own cancel.
    let error = if local == "none" && !removed {
        let ours = v
            .started
            .get(&e.uri)
            .and_then(|id| v.jobs.iter().find(|j| &j.job_id == id && j.status == "failed"));
        ours.or_else(|| {
            listed.and_then(|l| {
                v.jobs
                    .iter()
                    .filter(|j| j.model_id == l.model_id && j.status == "failed")
                    .max_by_key(|j| j.created_ms)
            })
        })
        .and_then(|j| j.error.clone())
        .filter(|err| err != "cancelled" && err != "stopped")
    } else {
        None
    };

    CatalogModel {
        id: e.id.clone(),
        name: e.name.clone(),
        publisher: e.publisher.clone(),
        licence: e.licence.clone(),
        licence_url: e.licence_url.clone(),
        size_bytes: e.size_bytes,
        uri: e.uri.clone(),
        summary: e.summary.clone(),
        source_url: e.source_url.clone(),
        ram_gb: e.ram_gb,
        good_at: e.good_at.clone(),
        offered: e.offered,
        local,
        progress,
        runnable,
        error,
        waiting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real listmodels entry captured from btx-modeld v0.34.15 0.16 s into
    /// a FREE_ONLY retrieve of the 350M model from the lab host (2026-10-12).
    const PARTIAL: &str = include_str!("../tests/fixtures/listmodels-partial.json");
    const URI_350M: &str = "btx://pq33nsv5hxj5mgrs4r4256h80g0y6n7uqhkv0j56cs3nrfq2c9jt686pm507apg2ygtvhd7uc43maj58gc8hu";

    fn partial() -> Listed {
        let v: serde_json::Value = serde_json::from_str(PARTIAL).unwrap();
        serde_json::from_value(v["models"][0].clone()).unwrap()
    }

    #[test]
    fn bundled_files_parse_and_are_consistent() {
        let c = bundled();
        assert!(!c.models.is_empty());
        for m in &c.models {
            assert!(m.uri.starts_with("btx://"), "{}", m.uri);
            let sum: u64 = m.files.iter().map(|f| f.size).sum();
            assert_eq!(sum, m.size_bytes, "{}: files must add up to size_bytes", m.id);
            assert!(m.gguf_size().is_some(), "{}: gguf {} not in files", m.id, m.gguf);
            assert_eq!(m.gguf_sha256().unwrap().len(), 64);
            // The store shows what each offered model is good at.
            assert!(!m.offered || !m.good_at.trim().is_empty(), "{}: offered without good_at", m.id);
        }
        for h in bundled_hosts() {
            crate::validate::host_port(&h).unwrap_or_else(|e| panic!("hosts.json {h}: {e}"));
        }
    }

    #[test]
    fn progress_from_captured_partial_ranges() {
        let m = partial();
        assert_eq!(m.uri, URI_350M);
        assert!(!m.complete);
        // file 0 (LICENSE, 11,358 B) complete in one short piece; file 1 has
        // pieces 0, 6, 7 of 57: 3 x 4 MiB.
        assert_eq!(file_committed_bytes(&m.files[0]), 11_358);
        assert_eq!(file_committed_bytes(&m.files[1]), 3 * 4_194_304);
        let p = progress(&m);
        let want = (11_358 + 3 * 4_194_304) as f64 / 236_997_118.0;
        assert!((p - want).abs() < 1e-12, "{p} vs {want}");
        assert!(p > 0.05 && p < 0.06);
    }

    #[test]
    fn progress_clips_the_short_last_piece_across_split_ranges() {
        let mut m = partial();
        // All 57 pieces, split in two ranges, model not yet verified.
        m.files[1].ranges = vec![PieceRange { first: 0, count: 1 }, PieceRange { first: 1, count: 56 }];
        assert_eq!(file_committed_bytes(&m.files[1]), 236_985_760);
        assert_eq!(progress(&m), 0.999, "held below 1 until complete");
        m.complete = true;
        assert_eq!(progress(&m), 1.0);
        // Nonsense ranges past the end never count beyond the file.
        m.files[1].ranges = vec![PieceRange { first: 90, count: 5 }, PieceRange { first: 0, count: 500 }];
        assert_eq!(file_committed_bytes(&m.files[1]), 236_985_760);
    }

    /// getmodeljob from the same poll as PARTIAL: the running job joins to the
    /// listed model by model_id, and its `bytes_committed` (the whole store,
    /// per the contract) is not what progress uses.
    #[test]
    fn captured_job_joins_by_model_id() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/getmodeljob-running.json")).unwrap();
        let jobs: Vec<Job> = serde_json::from_value(v["jobs"].clone()).unwrap();
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].active());
        let listed = [partial()];
        let (p, r) = (HashMap::new(), HashSet::new());
        let rows = join(&bundled(), &view(&listed, &jobs, &p, &r));
        let row = row(&rows, URI_350M);
        assert_eq!(row.local, "downloading");
        assert!((row.progress - progress(&listed[0])).abs() < 1e-12);
    }

    #[test]
    fn a_fetch_that_failed_before_the_model_was_listed_still_says_why() {
        // Seen on the first real use: Get on a model the host did not have.
        // The engine never lists the model, so only our own job id finds it.
        let c = bundled();
        let mut failed = job("j-350m", "failed", "8c670652");
        failed.error = Some("manifest failed".into());
        let jobs = vec![failed];
        let none: HashMap<String, String> = HashMap::new();
        let started: HashMap<String, String> = [(URI_350M.to_string(), "j-350m".to_string())].into();
        let removed = HashSet::new();
        let v = View { listed: &[], jobs: &jobs, pending: &none, started: &started, removed: &removed, runnable: &|_, _| false };
        let rows = join(&c, &v);
        let r = row(&rows, URI_350M);
        assert_eq!((r.local, r.error.as_deref()), ("none", Some("manifest failed")));
        // A job we did not start, for a model nobody lists, stays silent.
        let v = View { started: &none, ..v };
        assert_eq!(row(&join(&c, &v), URI_350M).error, None);
    }

    #[test]
    fn waiting_note_needs_both_silence_and_an_error() {
        let (c, p, r) = (bundled(), HashMap::new(), HashSet::new());
        let m = partial();
        let listed = [m.clone()];
        let with = |stalled: u64, err: Option<&str>| {
            let mut j = job("j", "running", &m.model_id);
            j.stalled_for_ms = stalled;
            j.last_err = err.map(str::to_string);
            let jobs = [j];
            row(&join(&c, &view(&listed, &jobs, &p, &r)), URI_350M).waiting.clone()
        };
        // Host down for 25 s, the E10 error: say so.
        let eof = "tls io ssl_error=1 errno=0 error:0A000126:SSL routines::unexpected eof while reading";
        assert_eq!(with(25_000, Some(eof)).as_deref(), Some(eof));
        // A slow line: minutes between pieces with no error (E5c). Not a problem.
        assert_eq!(with(200_000, None), None);
        assert_eq!(with(200_000, Some("  ")), None);
        // A momentary error that pieces already moved past.
        assert_eq!(with(5_000, Some(eof)), None);
    }

    fn view<'a>(
        listed: &'a [Listed],
        jobs: &'a [Job],
        pending: &'a HashMap<String, String>,
        removed: &'a HashSet<String>,
    ) -> View<'a> {
        let started: &'a HashMap<String, String> = Box::leak(Box::default());
        View { listed, jobs, pending, started, removed, runnable: &|_, _| false }
    }

    fn with_copy<'a>(v: View<'a>) -> View<'a> {
        View { runnable: &|_, _| true, ..v }
    }

    #[test]
    fn a_model_not_offered_shows_only_while_it_holds_disk() {
        let c = bundled();
        assert!(!c.models.iter().find(|m| m.uri == URI_350M).unwrap().offered);
        let (p, r) = (HashMap::new(), HashSet::new());
        let ids = |rows: Vec<CatalogModel>| -> Vec<String> { shown(&c, rows).into_iter().map(|m| m.uri).collect() };

        // Nothing on disk: the 350M is not offered, so it is not shown.
        assert!(!ids(join(&c, &view(&[], &[], &p, &r))).contains(&URI_350M.to_string()));
        // Pieces of a paused download: shown, so they can be removed.
        let listed = [partial()];
        assert!(ids(join(&c, &view(&listed, &[], &p, &r))).contains(&URI_350M.to_string()));
        // Only a runnable copy left: shown.
        assert!(ids(join(&c, &with_copy(view(&[], &[], &p, &r)))).contains(&URI_350M.to_string()));
        // Removed: gone again.
        let removed: HashSet<_> = [URI_350M.to_string()].into();
        assert!(!ids(join(&c, &view(&listed, &[], &p, &removed))).contains(&URI_350M.to_string()));
        // Offered models always show.
        assert!(ids(join(&c, &view(&[], &[], &p, &r))).iter().any(|u| u != URI_350M));
    }

    fn job(id: &str, status: &str, model_id: &str) -> Job {
        Job { job_id: id.into(), status: status.into(), model_id: model_id.into(), ..Default::default() }
    }

    fn row<'a>(rows: &'a [CatalogModel], uri: &str) -> &'a CatalogModel {
        rows.iter().find(|r| r.uri == uri).unwrap()
    }

    #[test]
    fn join_states() {
        let c = bundled();
        let none_p = HashMap::new();
        let none_r = HashSet::new();

        // Nothing local.
        let rows = join(&c, &view(&[], &[], &none_p, &none_r));
        assert!(rows.iter().all(|r| r.local == "none" && r.progress == 0.0 && !r.runnable));

        // Just after getmodel: job known by id, listmodels empty yet.
        let pending: HashMap<_, _> = [(URI_350M.to_string(), "j1".to_string())].into();
        let jobs = [job("j1", "running", "abc")];
        let rows = join(&c, &view(&[], &jobs, &pending, &none_r));
        assert_eq!(row(&rows, URI_350M).local, "downloading");

        // A resumed job reports queued while it downloads: still downloading.
        let m = partial();
        let jobs = [job("j2", "queued", &m.model_id)];
        let listed = [m.clone()];
        let rows = join(&c, &view(&listed, &jobs, &none_p, &none_r));
        let r = row(&rows, URI_350M);
        assert_eq!(r.local, "downloading");
        assert!(r.progress > 0.05 && r.progress < 0.06);
        assert!(!r.runnable);

        // Cancelled part-way: not downloading, progress kept, no error shown.
        let mut cancelled = job("j2", "failed", &m.model_id);
        cancelled.error = Some("cancelled".into());
        let jobs = [cancelled];
        let rows = join(&c, &view(&listed, &jobs, &none_p, &none_r));
        let r = row(&rows, URI_350M);
        assert_eq!((r.local, r.error.clone()), ("none", None));
        assert!(r.progress > 0.0);

        // Failed for a real reason: the reason is surfaced.
        let mut failed = job("j3", "failed", &m.model_id);
        failed.error = Some("retrieve failed".into());
        let jobs = [failed];
        let rows = join(&c, &view(&listed, &jobs, &none_p, &none_r));
        assert_eq!(row(&rows, URI_350M).error.as_deref(), Some("retrieve failed"));

        // Complete -> ready; runnable once a checkout exists.
        let mut done = m.clone();
        done.complete = true;
        let listed = [done];
        let rows = join(&c, &view(&listed, &[], &none_p, &none_r));
        let r = row(&rows, URI_350M);
        assert_eq!((r.local, r.progress, r.runnable), ("ready", 1.0, false));
        let rows = join(&c, &with_copy(view(&listed, &[], &none_p, &none_r)));
        assert!(row(&rows, URI_350M).runnable);

        // Removed by the user: listmodels still says complete (observed after
        // unhostmodel), the catalog must not.
        let removed: HashSet<_> = [URI_350M.to_string()].into();
        let rows = join(&c, &view(&listed, &[], &none_p, &removed));
        let r = row(&rows, URI_350M);
        assert_eq!((r.local, r.progress, r.runnable), ("none", 0.0, false));

        // Gone from the store (evicted) but its runnable copy is still on
        // disk: reported, so the UI can offer Remove.
        let rows = join(&c, &with_copy(view(&[], &[], &none_p, &none_r)));
        let r = row(&rows, URI_350M);
        assert_eq!((r.local, r.runnable), ("none", true));
    }
}
