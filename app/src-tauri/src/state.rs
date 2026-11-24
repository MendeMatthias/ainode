//! Where things live, and the small JSON files the app owns in $DATA.
//!
//! Rule 1 of the contract: nothing is written to disk before the user consents
//! to a size. So the settings the UI can change before consent (theme, extra
//! peers) are held in memory and reach `settings.json` only once
//! `consent.json` exists; `accept_consent` is the first write the app ever
//! makes.

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const GIB: u64 = 1 << 30;
/// The consent number is DECIMAL gigabytes, the unit Finder shows, so "25 GB"
/// on the consent screen is the same 25 GB the user sees in Finder.
pub const GB: u64 = 1_000_000_000;
pub const MIN_CONSENT_GB: u32 = 4;
/// The longest unix socket path, without the NUL: sun_path is 104 bytes on
/// macOS and 108 on Linux.
#[cfg(target_os = "macos")]
const MAX_SOCKET_PATH: usize = 103;
#[cfg(all(unix, not(target_os = "macos")))]
const MAX_SOCKET_PATH: usize = 107;

#[derive(Debug, Clone)]
pub struct Paths {
    /// $DATA: ~/Library/Application Support/AInode on macOS,
    /// $XDG_DATA_HOME/AInode (~/.local/share/AInode) on Linux
    pub data_dir: PathBuf,
    /// resources/engine: btx-modeld, llama/ (Linux and Windows: lib/ too)
    pub engine_dir: PathBuf,
    /// $TMPDIR/ainode/modeld.sock on macOS, $XDG_RUNTIME_DIR/ainode/modeld.sock
    /// on Linux (see default_socket)
    pub socket: PathBuf,
}

impl Paths {
    /// Production paths. `resource_dir` is Tauri's; the bundle config
    /// (`"resources": ["resources/**/*"]`) keeps the `resources/` prefix.
    ///
    /// Overrides, honoured in debug builds only (`dev_env`):
    ///   AINODE_ENGINE_DIR  a staged engine dir (scripts/stage-engine.sh output)
    ///   AINODE_DATA_DIR    a throwaway $DATA, so a dev run never touches the real one
    ///   AINODE_SOCKET      the helper's socket path
    pub fn resolve(resource_dir: Option<&Path>) -> Result<Paths, String> {
        let engine_dir = match env_path("AINODE_ENGINE_DIR") {
            Some(p) => p,
            None => resource_dir
                .ok_or("no resource dir and AINODE_ENGINE_DIR unset")?
                .join("resources")
                .join("engine"),
        };
        let data_dir = match env_path("AINODE_DATA_DIR") {
            Some(p) => p,
            None => {
                // The default, unless the user chose another drive or folder
                // at the first start (location.json in the default dir).
                let default = default_data_dir()?;
                let _ = DEFAULT_DATA_DIR.set(default.clone());
                located(&default).unwrap_or(default)
            }
        };
        let socket = match env_path("AINODE_SOCKET") {
            Some(p) => p,
            None => default_socket(),
        };
        let p = Paths { data_dir, engine_dir, socket };
        p.check()?;
        Ok(p)
    }

    /// Windows: the helper's socket is inside WSL (wsl.rs `SOCKET`, short by
    /// construction); `socket` here only names the folder for the chat's
    /// files, so the unix length limit does not apply to it.
    pub fn check(&self) -> Result<(), String> {
        #[cfg(unix)]
        if self.socket.as_os_str().len() > MAX_SOCKET_PATH {
            return Err(format!(
                "socket path is {} bytes, this system allows {MAX_SOCKET_PATH}: {}",
                self.socket.as_os_str().len(),
                self.socket.display()
            ));
        }
        Ok(())
    }

    /// The same paths with the data somewhere else.
    pub fn with_data_dir(&self, data_dir: PathBuf) -> Paths {
        Paths { data_dir, engine_dir: self.engine_dir.clone(), socket: self.socket.clone() }
    }

    pub fn modeldir(&self) -> PathBuf {
        self.data_dir.join("modelnet")
    }
    /// Where exportmodelpath materialises runnable files:
    /// `<modeldir>/checkout/<artifact_id>/` (observed: its `path` reply).
    pub fn checkout_root(&self) -> PathBuf {
        self.modeldir().join("checkout")
    }
    pub fn consent_file(&self) -> PathBuf {
        self.data_dir.join("consent.json")
    }
    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }
    pub fn removed_file(&self) -> PathBuf {
        self.data_dir.join("removed.json")
    }
    /// Models kept once: their pieces deleted after the runnable copy was
    /// verified (store_once.rs). uri -> artifact id.
    pub fn stored_once_file(&self) -> PathBuf {
        self.data_dir.join("stored-once.json")
    }
    pub fn socket_dir(&self) -> PathBuf {
        self.socket.parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir)
    }
    pub fn chat_pidfile(&self) -> PathBuf {
        self.socket_dir().join("llama-server.pid")
    }
    pub fn modeld_pidfile(&self) -> PathBuf {
        self.socket_dir().join("btx-modeld.pid")
    }
    pub fn modeld(&self) -> PathBuf {
        self.engine_dir.join("btx-modeld")
    }
    pub fn llama_server(&self) -> PathBuf {
        self.engine_dir.join("llama").join(if cfg!(windows) { "llama-server.exe" } else { "llama-server" })
    }
}

#[cfg(unix)]
fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from).ok_or_else(|| "HOME is not set".to_string())
}

/// Windows: `%LOCALAPPDATA%\AInode\data`, the user's own, never roamed. The
/// per-user installer puts the program in `%LOCALAPPDATA%\AInode` and its
/// uninstaller removes only the files it installed, so the data is kept in a
/// folder of its own beside them, made at consent.
#[cfg(windows)]
pub fn default_data_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).ok_or("LOCALAPPDATA is not set")?;
    Ok(PathBuf::from(base).join("AInode").join("data"))
}

/// Windows: the engine's socket is inside WSL (wsl.rs); this folder, in the
/// user's `%TEMP%` like macOS's `$TMPDIR`, holds the chat's pid, key and log
/// and the webview's own files.
#[cfg(windows)]
pub fn default_socket() -> PathBuf {
    std::env::temp_dir().join("ainode").join("modeld.sock")
}

/// A path the engine reported (exportmodelpath). On Windows the engine runs
/// inside WSL and reports `/mnt/c/...`: this is the same file as Windows sees
/// it. Elsewhere it is the path itself.
pub fn from_engine(p: &str) -> PathBuf {
    #[cfg(windows)]
    if let Some(w) = crate::wsl::windows_path(p) {
        return w;
    }
    PathBuf::from(p)
}

#[cfg(target_os = "macos")]
pub fn default_data_dir() -> Result<PathBuf, String> {
    Ok(home()?.join("Library/Application Support/AInode"))
}

/// The XDG base directory spec: $XDG_DATA_HOME if it is an absolute path
/// (a relative one must be ignored), else ~/.local/share.
#[cfg(target_os = "linux")]
pub fn default_data_dir() -> Result<PathBuf, String> {
    let base = match std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()) {
        Some(b) => b,
        None => home()?.join(".local/share"),
    };
    Ok(base.join("AInode"))
}

#[cfg(target_os = "macos")]
pub fn default_socket() -> PathBuf {
    // $TMPDIR is per user and short on macOS.
    std::env::temp_dir().join("ainode").join("modeld.sock")
}

/// $XDG_RUNTIME_DIR (/run/user/<uid>: per user, 0700, short) when the
/// session has one. Without it /tmp is shared by every user, so the folder
/// carries our uid, and engine::prepare_private_dir refuses one that another
/// user made.
#[cfg(target_os = "linux")]
pub fn default_socket() -> PathBuf {
    if let Some(run) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_absolute() && p.is_dir()) {
        return run.join("ainode").join("modeld.sock");
    }
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    std::env::temp_dir().join(format!("ainode-{uid}")).join("modeld.sock")
}

fn env_path(k: &str) -> Option<PathBuf> {
    dev_env(k).map(PathBuf::from)
}

/// An `AINODE_*` override, honoured in debug builds only. A release app must
/// not let anything that can set its environment (`launchctl setenv`) point
/// it at other binaries, another data dir, or extra peers.
pub fn dev_env(k: &str) -> Option<std::ffi::OsString> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os(k).filter(|v| !v.is_empty())
}

// ── a dev run stays off the installed app ───────────────────────────────────
//
// The dev bridge and the live tests run the same backend as the app. Its
// start-up cleanup (engine.rs `clear_socket`, chat.rs) works on the socket's
// FOLDER: the helper's and llama-server's pid files and the chat key live
// there, and a helper that answers on the socket is taken for a leftover and
// stopped. So a dev run must be given a socket folder and a data folder that
// are not an installed AInode's, however the paths are spelled.

/// Where an installed AInode on this computer has its engine socket and its
/// data: the default folder and, when the user moved the data at the first
/// start, the folder `location.json` names as well.
#[derive(Debug, Clone)]
pub struct LiveInstall {
    pub socket: PathBuf,
    pub data_dirs: Vec<PathBuf>,
}

impl LiveInstall {
    pub fn of(socket: PathBuf, default_data_dir: Option<PathBuf>) -> LiveInstall {
        let mut data_dirs = Vec::new();
        if let Some(d) = default_data_dir {
            if let Some(moved) = located(&d) {
                data_dirs.push(moved);
            }
            data_dirs.push(d);
        }
        LiveInstall { socket, data_dirs }
    }

    /// This computer's.
    pub fn here() -> LiveInstall {
        LiveInstall::of(default_socket(), default_data_dir().ok())
    }
}

/// `p` with every link on the way resolved, also when its last parts do not
/// exist yet, and also through a link whose target does not exist yet (the
/// run would create it). None when it cannot be resolved.
fn resolved(p: &Path) -> Option<PathBuf> {
    let mut cur = p.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    for _ in 0..64 {
        if let Ok(c) = cur.canonicalize() {
            return Some(rest.iter().rev().fold(c, |acc, n| acc.join(n)));
        }
        if let Ok(target) = std::fs::read_link(&cur) {
            cur = match cur.parent() {
                Some(dir) if target.is_relative() => dir.join(target),
                _ => target,
            };
            continue;
        }
        let name = cur.file_name().map(|n| n.to_os_string())?;
        let dir = cur.parent().map(Path::to_path_buf)?;
        rest.push(name);
        cur = dir;
    }
    None
}

fn overlap(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// Why a dev run with these paths must not start, if it would reach an
/// installed AInode's engine or data. Checked before anything is touched.
pub fn dev_run_refusal(paths: &Paths, live: &LiveInstall) -> Option<String> {
    use std::path::Component;
    for (what, p) in [("the socket", &paths.socket), ("the data folder", &paths.data_dir)] {
        if !p.is_absolute() {
            return Some(format!("{what} is a relative path ({}): give the full path", p.display()));
        }
        if p.components().any(|c| c == Component::ParentDir) {
            return Some(format!("{what} has `..` in its path ({})", p.display()));
        }
    }
    let Some(sock_dir) = resolved(&paths.socket_dir()) else {
        return Some(format!("the socket's folder cannot be resolved ({})", paths.socket_dir().display()));
    };
    if let Some(live_dir) = live.socket.parent().and_then(resolved) {
        if overlap(&sock_dir, &live_dir) {
            return Some(format!(
                "the socket is in the installed app's own socket folder ({}): its pid files and chat key live there",
                live_dir.display()
            ));
        }
    }
    let Some(data) = resolved(&paths.data_dir) else {
        return Some(format!("the data folder cannot be resolved ({})", paths.data_dir.display()));
    };
    for l in &live.data_dirs {
        if let Some(l) = resolved(l) {
            if overlap(&data, &l) {
                return Some(format!("the data folder is the installed app's own, or inside it, or holds it ({})", l.display()));
            }
        }
    }
    None
}

// ── location.json ───────────────────────────────────────────────────────────

/// Where the data lives when the user chose another drive or folder at the
/// first start (backend `accept_consent`): a pointer in the DEFAULT data dir,
/// the one place every launch can find. Absent: the default itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub data_dir: PathBuf,
}

pub const LOCATION_FILE: &str = "location.json";

/// The default data dir `Paths::resolve` started from, where the pointer goes.
/// Unset outside the app (tests build their Paths by hand): then the current
/// data dir stands in.
pub static DEFAULT_DATA_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// The data dir named by the pointer in `default_dir`, when there is one and
/// it names an absolute path.
pub fn located(default_dir: &Path) -> Option<PathBuf> {
    let raw = std::fs::read(default_dir.join(LOCATION_FILE)).ok()?;
    let l: Location = serde_json::from_slice(&raw).ok()?;
    l.data_dir.is_absolute().then_some(l.data_dir)
}

pub fn write_location(default_dir: &Path, data_dir: &Path) -> Result<(), String> {
    create_private_dir_all(default_dir)?;
    write_json_atomic(&default_dir.join(LOCATION_FILE), &Location { data_dir: data_dir.to_path_buf() })
}

// ── consent.json ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Consent {
    pub total_bytes: u64,
    pub engine_bytes: u64,
    pub accepted_at: u64,
    /// Informational. Optional so that a consent written by another version
    /// is still a consent: the size is what the user agreed to.
    #[serde(default)]
    pub app_version: String,
}

impl Consent {
    /// `-modelstorage=<engine_gib>GiB`: the helper takes whole GiB.
    pub fn engine_gib(&self) -> u64 {
        self.engine_bytes / GIB
    }
}

/// What AInode already holds on disk: the engine's piece store, and that
/// plus the runnable copies. A new size is measured against it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Held {
    pub engine: u64,
    pub total: u64,
}

/// The sizes the user may choose now, in whole decimal GB, as (min, max).
/// At least what AInode holds, with the piece store fitting its half; at
/// most what it holds plus 80% of the free space. With nothing held this is
/// the first-run rule. When the disk is too full, max < min.
pub fn consent_limits(free_bytes: u64, held: Held) -> (u32, u32) {
    let gb = u128::from(GB);
    let need_total = u128::from(held.total).div_ceil(gb);
    // floor(g * GB / 2 / GiB) >= ceil(engine / GiB)  <=>  g >= ceil(2 * GiB * k / GB)
    let k = u128::from(held.engine).div_ceil(u128::from(GIB));
    let need_engine = (2 * u128::from(GIB) * k).div_ceil(gb);
    let min = need_total.max(need_engine).max(u128::from(MIN_CONSENT_GB));
    let max = (u128::from(free_bytes) * 8 / 10 + u128::from(held.total)) / gb;
    let clamp = |v: u128| u32::try_from(v).unwrap_or(u32::MAX);
    (clamp(min), clamp(max))
}

/// The consent math. `total_gb` decimal GB is the WHOLE footprint the user
/// agrees to; half goes to the engine's fixed piece store and half is left
/// for runnable copies, because a runnable checkout is a second full copy,
/// not a hardlink (contract E19), so a model costs ~2x its size. Used at
/// first run (nothing held) and whenever the user changes the size.
pub fn consent_for(total_gb: u32, free_bytes: u64, held: Held, now: u64, app_version: &str) -> Result<Consent, String> {
    if total_gb < MIN_CONSENT_GB {
        return Err(format!("Choose at least {MIN_CONSENT_GB} GB"));
    }
    let (min_gb, max_gb) = consent_limits(free_bytes, held);
    if total_gb < min_gb {
        let have = held.total as f64 / GB as f64;
        return Err(format!(
            "AInode already holds {have:.1} GB. Choose {min_gb} GB or more, or remove a model first."
        ));
    }
    let total_bytes = u64::from(total_gb) * GB;
    // What is not held yet <= 80% of free, in integers: grow * 10 <= free * 8.
    if u128::from(total_bytes.saturating_sub(held.total)) * 10 > u128::from(free_bytes) * 8 {
        return Err(format!(
            "{total_gb} GB needs more than 80% of the free space on this disk. Choose {max_gb} GB or less."
        ));
    }
    let engine_gib = total_bytes / 2 / GIB;
    Ok(Consent {
        total_bytes,
        engine_bytes: engine_gib * GIB,
        accepted_at: now,
        app_version: app_version.to_string(),
    })
}

pub fn load_consent(paths: &Paths) -> Option<Consent> {
    let raw = std::fs::read(paths.consent_file()).ok()?;
    let c: Consent = serde_json::from_slice(&raw).ok()?;
    // A hand-edited file that would start the helper with a zero quota is no
    // consent at all.
    (c.engine_gib() >= 1).then_some(c)
}

// ── settings.json ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub extra_peers: Vec<String>,
    /// The hosts the engine asks when it searches (its index peers, upstream's
    /// `addmodelindex`), added by the user. None or empty: no host besides the
    /// model host, which every search asks anyway. The engine forgets them at
    /// every restart, so the app gives them again after each start
    /// (backend/network.rs). Read leniently and checked again on load
    /// (Backend::index_peers): one form each, no model host, no host the
    /// engine never asks, at most INDEX_PEERS_MAX.
    #[serde(default, deserialize_with = "lenient_strings", skip_serializing_if = "Option::is_none")]
    pub index_peers: Option<Vec<String>>,
    /// Opening the app starts the engine, unless the user pressed stop.
    #[serde(default = "yes")]
    pub autostart: bool,
    /// Let the model reason before answering (the chat's Think switch). On by
    /// default since 0.2.1, on Mende's word ("it should always think"): the 3B
    /// went from 10 to 11.5 of 12 on the reasoning set and its visible answers
    /// from about 215 to 30 characters, at about 3x the wait.
    #[serde(default = "yes")]
    pub think: bool,
    /// The face in the chat's loading scene: "random" (one of the three each
    /// time), or one of them pinned. Mende picked the three on 2026-10-13.
    #[serde(default = "default_face")]
    pub face: String,
    /// Per model (catalog id): the settings button on its card (0.7.0).
    /// A model without an entry uses `think` above and the defaults.
    #[serde(default)]
    pub models: std::collections::BTreeMap<String, ModelPrefs>,
    /// Keep conversations on this Mac (chats.rs). Off unless the user turns
    /// it on in Status: without it nothing of a chat is ever written.
    #[serde(default)]
    pub keep_chats: bool,
    /// Chat on the graphics card when llama.cpp finds one (Metal, Vulkan).
    /// Off: llama-server runs with `--device none`, the processor only, so a
    /// graphics card that mines or runs a node stays free (measured on the
    /// Windows PC, 2026-10-14: with it off, an RTX 3060 beside the Vulkan
    /// backend saw no memory and no process from llama-server).
    #[serde(default = "yes")]
    pub gpu: bool,
    /// Be counted among the AInodes online (easybtx.com/api/ainode-checkin):
    /// every 5 minutes while the app is open, after consent only. Off: the
    /// app still shows the number, it just is not in it.
    #[serde(default = "yes")]
    pub count_me: bool,
    /// Start AInode with the computer, its window minimized (login.rs). On
    /// by default (Mende, 2026-10-17: "auto start with system startup ...
    /// a user can turn this off in the settings"). The entry is written only
    /// after consent.
    #[serde(default = "yes")]
    pub start_at_login: bool,
    /// A random id for this install in that count (32 hex), made at the first
    /// check-in after consent. Not derived from anything about the computer.
    #[serde(default)]
    pub community_id: Option<String>,
    /// Models fetched from a pasted btx:// link (backend/network.rs).
    #[serde(default)]
    pub links: Vec<LinkEntry>,
}

/// A link the user fetched, remembered so it is listed after a restart (and
/// only links the user asked for are).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkEntry {
    pub uri: String,
    /// The name the network gave it when it was opened, if any.
    #[serde(default)]
    pub title: String,
}

/// One model's chat settings (Mende, 2026-10-13: "a settings button per
/// model"). Think on or off for this model, how long its answers may be, and
/// how freely it writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPrefs {
    pub think: bool,
    /// "short", "normal" or "long" (valid_length).
    #[serde(default = "default_length")]
    pub length: String,
    /// llama.cpp sampling temperature, 0.0 to 1.5. The app's default is 0.6.
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

fn default_length() -> String {
    "normal".into()
}

fn default_temperature() -> f32 {
    0.6
}

pub fn valid_length(l: &str) -> bool {
    matches!(l, "short" | "normal" | "long")
}

impl ModelPrefs {
    /// Refuses what the UI never sends, so a bad call cannot write a
    /// settings file the chat would then misread.
    pub fn check(&self) -> Result<(), String> {
        if !valid_length(&self.length) {
            return Err(format!("unknown answer length {:?}", self.length));
        }
        if !(0.0..=1.5).contains(&self.temperature) || self.temperature.is_nan() {
            return Err(format!("temperature {} is outside 0 to 1.5", self.temperature));
        }
        Ok(())
    }
}

fn default_theme() -> String {
    "system".into()
}

/// A list of strings, or "absent" for anything else. A wrong value in this
/// one field (a number, a string, a list with a number in it) must not make
/// the whole settings file unreadable: load_settings would fall back to the
/// defaults and the next write would lose every other setting.
fn lenient_strings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Array(items) => items.into_iter().map(|x| x.as_str().map(str::to_string)).collect(),
        _ => None,
    })
}

fn default_face() -> String {
    "random".into()
}

pub fn valid_face(f: &str) -> bool {
    matches!(f, "random" | "swarm" | "glyphs" | "chip")
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings { theme: default_theme(), extra_peers: Vec::new(), index_peers: None, autostart: true, think: true, face: default_face(), models: Default::default(), keep_chats: false, gpu: true, count_me: true, start_at_login: true, community_id: None, links: Vec::new() }
    }
}

pub fn valid_theme(t: &str) -> bool {
    matches!(t, "system" | "dark" | "light")
}

pub fn load_settings(paths: &Paths) -> Settings {
    let mut s: Settings = std::fs::read(paths.settings_file())
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();
    if !valid_theme(&s.theme) {
        s.theme = default_theme();
    }
    s
}

// ── removed.json ────────────────────────────────────────────────────────────
//
// unhostmodel unpins and unseeds but frees nothing, and there is no delete
// RPC in v0.34.15 (contract E20). Observed 2026-10-12: after unhostmodel the
// model is STILL in listmodels with `complete: true`, so without a record of
// the user's "Remove" the catalog would keep showing it as ready. This file
// is that record. The pieces stay evictable in the store; a later download of
// the same uri is answered from them (getmodel replies `status: "local"`) and
// clears the entry.

/// stored-once.json: the models whose pieces AInode deleted (store_once.rs).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredOnce {
    #[serde(default)]
    pub models: std::collections::BTreeMap<String, String>,
}

pub fn load_stored_once(paths: &Paths) -> StoredOnce {
    std::fs::read(paths.stored_once_file())
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Removed {
    #[serde(default)]
    pub uris: Vec<String>,
}

pub fn load_removed(paths: &Paths) -> Removed {
    std::fs::read(paths.removed_file())
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

/// Write `value` as JSON to `path` atomically: a uniquely named temp file in
/// the same directory (pid + per-process counter, so two writers never share
/// one), fsync it, rename over the target, then fsync the directory so the
/// rename itself survives a power cut. A crash leaves the old file or the new
/// one, never half of one.
///
/// Mac and Linux: the file is 0600 and a folder made for it 0700. These are
/// the user's settings, kept chats and the text of their own files; with the
/// default umask they were 0644 in a 0755 folder, readable by every other
/// user wherever the home folder is not private (a data folder on another
/// drive, an older Ubuntu's 0755 homes). Windows: the folder's ACL.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    write_bytes_atomic(path, &bytes, true)
}

/// The same for a text file (login.rs: the start entry), with the default
/// permissions: the system reads it as it reads any other start entry.
pub fn write_text_atomic(path: &Path, text: &str) -> Result<(), String> {
    write_bytes_atomic(path, text.as_bytes(), false)
}

/// Mac and Linux, at each start: an existing data folder made private. 0.8.2
/// and earlier made it 0755 with 0644 files, and `write_json_atomic` only
/// tightens a file it rewrites, so the consent, the kept chats and the text of
/// the user's own files stayed readable by every local user wherever the
/// folder is not private (second security audit, 2026-10-19). The folder,
/// `chats/` and `knowledge/` become 0700, AInode's `.json` files in them
/// 0600. `modelnet` is the engine's (0700 already); a link is never followed;
/// never the home folder or a root (AInode's folder is a folder of its own,
/// as on Windows, acl.rs). Returns how many entries it changed.
#[cfg(unix)]
pub fn make_private(data_dir: &Path) -> Result<usize, String> {
    use std::os::unix::fs::PermissionsExt;
    if data_dir.parent().is_none() || home().ok().as_deref() == Some(data_dir) {
        return Ok(0);
    }
    let mut changed = 0;
    let mut tighten = |p: &Path, dir: bool| -> Result<(), String> {
        let Ok(m) = std::fs::symlink_metadata(p) else { return Ok(()) };
        if (dir && !m.is_dir()) || (!dir && !m.is_file()) || m.permissions().mode() & 0o077 == 0 {
            return Ok(());
        }
        let mode = if dir { 0o700 } else { 0o600 };
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).map_err(|e| format!("{}: {e}", p.display()))?;
        changed += 1;
        Ok(())
    };
    for d in [data_dir.to_path_buf(), data_dir.join("chats"), data_dir.join("knowledge")] {
        tighten(&d, true)?;
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                tighten(&p, false)?;
            }
        }
    }
    Ok(changed)
}

/// `dir` and any missing parent, each new one 0700 on Mac and Linux (an
/// existing folder keeps its mode).
pub fn create_private_dir_all(dir: &Path) -> Result<(), String> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir).map_err(|e| format!("create {}: {e}", dir.display()))
}

fn write_bytes_atomic(path: &Path, bytes: &[u8], private: bool) -> Result<(), String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().ok_or("no parent dir")?;
    if private {
        create_private_dir_all(dir)?;
    } else {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("state");
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{seq}.tmp", std::process::id()));
    let res = (|| {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        // A directory cannot be opened as a file on Windows; NTFS logs the
        // rename itself.
        #[cfg(unix)]
        std::fs::File::open(dir)?.sync_all()?;
        Ok::<(), std::io::Error>(())
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("write {}: {e}", path.display()));
    }
    Ok(())
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREE: u64 = 500 * GB;

    #[cfg(unix)]
    #[test]
    fn an_older_data_folder_is_made_private() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let tmp = tempfile::tempdir().unwrap();
        let mode = |p: &Path| std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777;
        let set = |p: &Path, m: u32| std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
        // As 0.8.2 left it: umask 022.
        let data = tmp.path().join("AInode");
        for d in ["chats", "knowledge", "modelnet"] {
            std::fs::create_dir_all(data.join(d)).unwrap();
            set(&data.join(d), 0o755);
        }
        set(&data, 0o755);
        let files = ["consent.json", "settings.json", "chats/0123456789abcdef.json", "knowledge/fedcba9876543210.json"];
        for f in files {
            std::fs::write(data.join(f), b"{}").unwrap();
            set(&data.join(f), 0o644);
        }
        std::fs::write(data.join("modelnet/catalog.json"), b"{}").unwrap();
        set(&data.join("modelnet/catalog.json"), 0o644);
        let outside = tmp.path().join("outside.json");
        std::fs::write(&outside, b"{}").unwrap();
        set(&outside, 0o644);
        symlink(&outside, data.join("link.json")).unwrap();

        assert_eq!(make_private(&data).unwrap(), 7);
        for d in [&data, &data.join("chats"), &data.join("knowledge")] {
            assert_eq!(mode(d), 0o700, "{}", d.display());
        }
        for f in files {
            assert_eq!(mode(&data.join(f)), 0o600, "{f}");
        }
        // The engine's folder, and a link's target, are not AInode's to change.
        assert_eq!(mode(&data.join("modelnet")), 0o755);
        assert_eq!(mode(&data.join("modelnet/catalog.json")), 0o644);
        assert_eq!(mode(&outside), 0o644);
        // Once private, nothing more to do; a folder that does not exist is no error.
        assert_eq!(make_private(&data).unwrap(), 0);
        assert_eq!(make_private(&tmp.path().join("missing")).unwrap(), 0);
        // Never a root or the home folder.
        assert_eq!(make_private(Path::new("/")).unwrap(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn data_files_are_private_and_start_entries_are_not() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let data = tmp.path().join("share").join("AInode");
        write_json_atomic(&data.join("chats").join("0123456789abcdef.json"), &serde_json::json!({ "a": 1 })).unwrap();
        assert_eq!(mode(&data.join("chats").join("0123456789abcdef.json")), 0o600);
        for d in [tmp.path().join("share"), data.clone(), data.join("chats")] {
            assert_eq!(mode(&d), 0o700, "{}", d.display());
        }
        // A rewrite of a file an older AInode left 0644 makes it 0600.
        let old = data.join("settings.json");
        std::fs::write(&old, b"{}").unwrap();
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_json_atomic(&old, &serde_json::json!({})).unwrap();
        assert_eq!(mode(&old), 0o600);
        // An existing folder keeps its mode.
        let open = tmp.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        create_private_dir_all(&open).unwrap();
        assert_eq!(mode(&open), 0o755);
        // The start entry is read by the system: the umask decides, as before.
        let entry = tmp.path().join("autostart").join("ainode.desktop");
        write_text_atomic(&entry, "[Desktop Entry]\n").unwrap();
        let plain = tmp.path().join("autostart").join("plain");
        std::fs::write(&plain, b"x").unwrap();
        assert_eq!(mode(&entry), mode(&plain));
    }

    #[test]
    fn consent_math_decimal_gb_and_floor_gib() {
        let c = consent_for(25, FREE, Held::default(), 7, "0.1.0").unwrap();
        assert_eq!(c.total_bytes, 25_000_000_000);
        // 25e9 / 2 / 2^30 = 11.64 -> 11 GiB, passed as -modelstorage=11GiB.
        assert_eq!(c.engine_gib(), 11);
        assert_eq!(c.engine_bytes, 11 * GIB);
        assert_eq!(c.accepted_at, 7);
        // The smallest allowed consent still gives the helper a non-zero quota.
        assert_eq!(consent_for(4, FREE, Held::default(), 0, "x").unwrap().engine_gib(), 1);
        assert_eq!(consent_for(5, FREE, Held::default(), 0, "x").unwrap().engine_gib(), 2);
    }

    #[test]
    fn consent_bounds() {
        assert!(consent_for(3, FREE, Held::default(), 0, "x").is_err());
        assert!(consent_for(0, FREE, Held::default(), 0, "x").is_err());
        // Exactly 80% of free is allowed, one byte over is not.
        let free = 10 * GB; // 80% = 8 GB
        assert!(consent_for(8, free, Held::default(), 0, "x").is_ok());
        assert!(consent_for(9, free, Held::default(), 0, "x").is_err());
        assert!(consent_for(8, free - 1, Held::default(), 0, "x").is_err());
        // No overflow at the top of u32.
        assert!(consent_for(u32::MAX, FREE, Held::default(), 0, "x").is_err());
        assert!(consent_for(u32::MAX, u64::MAX, Held::default(), 0, "x").is_ok());
        let msg = consent_for(9, free, Held::default(), 0, "x").unwrap_err();
        assert!(msg.contains("8 GB or less"), "{msg}");
    }

    #[test]
    fn a_change_is_measured_against_what_is_held() {
        // 5 GB held, 2.4 GB of it in the store, 196 GB free. Up to 156 + 5 =
        // 161 GB. Down to 7 GB, not 5: the store's quota is whole GiB (3 GiB
        // for 2.4 GB) and is half the total, so 6.4 GB rounds up to 7.
        let held = Held { engine: 2_400_000_000, total: 5_000_000_000 };
        let free = 196 * GB;
        assert_eq!(consent_limits(free, held), (7, 161));
        assert!(consent_for(7, free, held, 0, "x").is_ok());
        assert!(consent_for(6, free, held, 0, "x").is_err());
        assert!(consent_for(161, free, held, 0, "x").is_ok());
        assert!(consent_for(162, free, held, 0, "x").unwrap_err().contains("161 GB or less"));
        assert!(consent_for(200, free, held, 0, "x").is_err(), "200 GB on a Mac with 196 GB free");
        // Never below what is held: the user is told to remove a model first.
        let msg = consent_for(4, free, held, 0, "x").unwrap_err();
        assert!(msg.contains("already holds 5.0 GB") && msg.contains("7 GB or more"), "{msg}");
        // The piece store must fit its half: 10 GiB in the store needs 22 GB
        // (floor(22e9 / 2 / 2^30) = 10 GiB), even if the total is small.
        let store_heavy = Held { engine: 10 * GIB, total: 10 * GIB };
        assert_eq!(consent_limits(FREE, store_heavy).0, 22);
        assert!(consent_for(21, FREE, store_heavy, 0, "x").is_err());
        assert!(consent_for(22, FREE, store_heavy, 0, "x").unwrap().engine_bytes >= 10 * GIB);
        // With nothing held it is the first-run rule.
        assert_eq!(consent_limits(10 * GB, Held::default()), (MIN_CONSENT_GB, 8));
        // A full disk leaves no valid size: max below min.
        let (lo, hi) = consent_limits(GB, Held { engine: 0, total: 20 * GB });
        assert!(hi >= lo, "what is held can always be kept: {lo}..{hi}");
        let (lo, hi) = consent_limits(0, Held::default());
        assert!(hi < lo, "{lo}..{hi}");
    }

    #[test]
    fn think_is_on_unless_the_user_turned_it_off() {
        assert!(Settings::default().think);
        let old: Settings = serde_json::from_str(r#"{"theme":"dark","extra_peers":[]}"#).unwrap();
        assert!(old.think, "a settings file from before 0.3.0 has no think field");
        let off: Settings = serde_json::from_str(r#"{"theme":"dark","extra_peers":[],"think":false}"#).unwrap();
        assert!(!off.think);
        assert_eq!(off.face, "random", "a settings file from before 0.6.0 has no face field");
        assert!(valid_face("chip") && !valid_face("ellipse"));
        assert!(off.models.is_empty(), "a settings file from before 0.7.0 has no per-model settings");
    }

    #[test]
    fn atomic_write_round_trips_and_leaves_no_temp() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("deep/settings.json");
        let s = Settings { theme: "dark".into(), extra_peers: vec!["[::1]:29447".into()], ..Default::default() };
        write_json_atomic(&p, &s).unwrap();
        let back: Settings = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(back, s);
        let names: Vec<_> = std::fs::read_dir(p.parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn concurrent_atomic_writes_never_tear() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("removed.json");
        std::thread::scope(|sc| {
            for i in 0..8 {
                let p = &p;
                sc.spawn(move || {
                    for j in 0..20 {
                        let r = Removed { uris: vec![format!("u{i}-{j}"); 50] };
                        write_json_atomic(p, &r).unwrap();
                    }
                });
            }
        });
        let back: Removed = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(back.uris.len(), 50);
        let left: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left.len(), 1, "temp files left behind: {left:?}");
    }

    #[test]
    fn settings_tolerate_missing_and_bad_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths {
            data_dir: tmp.path().to_path_buf(),
            engine_dir: tmp.path().join("e"),
            socket: tmp.path().join("s.sock"),
        };
        assert_eq!(load_settings(&paths), Settings::default());
        std::fs::write(paths.settings_file(), br#"{"theme":"neon"}"#).unwrap();
        assert_eq!(load_settings(&paths).theme, "system");
        std::fs::write(paths.consent_file(), br#"{"total_bytes":1,"engine_bytes":0,"accepted_at":0,"app_version":"x"}"#).unwrap();
        assert!(load_consent(&paths).is_none(), "zero-quota consent is no consent");
        // Written by another version, without app_version: still a consent.
        std::fs::write(paths.consent_file(), br#"{"total_bytes":4000000000,"engine_bytes":1073741824,"accepted_at":1}"#).unwrap();
        assert_eq!(load_consent(&paths).map(|c| c.engine_gib()), Some(1));
    }

    #[test]
    fn the_location_pointer_names_another_data_dir_or_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let default = tmp.path().join("default");
        assert_eq!(located(&default), None, "no pointer: the default");
        let chosen = tmp.path().join("elsewhere");
        write_location(&default, &chosen).unwrap();
        assert_eq!(located(&default), Some(chosen.clone()));
        let p = Paths { data_dir: default.clone(), engine_dir: tmp.path().join("e"), socket: tmp.path().join("s.sock") };
        assert_eq!(p.with_data_dir(chosen.clone()).consent_file(), chosen.join("consent.json"));
        // A relative or broken pointer is no pointer.
        std::fs::write(default.join(LOCATION_FILE), br#"{"data_dir":"relative/x"}"#).unwrap();
        assert_eq!(located(&default), None);
        std::fs::write(default.join(LOCATION_FILE), b"not json").unwrap();
        assert_eq!(located(&default), None);
    }

    /// The dev-run guard, against scratch stand-ins for an installed AInode
    /// (never the real folders): `live` is a folder tree in a tempdir.
    #[cfg(unix)]
    mod dev_run {
        use super::super::*;
        use std::os::unix::fs::symlink;

        struct Fx {
            _tmp: tempfile::TempDir,
            root: PathBuf,
            live: LiveInstall,
        }

        fn fx() -> Fx {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            for d in ["live-sock", "live-data/modelnet", "dev-sock", "dev-data"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            let live = LiveInstall::of(root.join("live-sock/modeld.sock"), Some(root.join("live-data")));
            Fx { _tmp: tmp, root, live }
        }

        fn paths(socket: PathBuf, data_dir: PathBuf) -> Paths {
            Paths { data_dir, engine_dir: PathBuf::from("/e"), socket }
        }

        #[test]
        fn folders_of_its_own_pass() {
            let f = fx();
            let p = paths(f.root.join("dev-sock/modeld.sock"), f.root.join("dev-data"));
            assert_eq!(dev_run_refusal(&p, &f.live), None);
            // Folders that do not exist yet pass too.
            let p = paths(f.root.join("new/s/modeld.sock"), f.root.join("new/data"));
            assert_eq!(dev_run_refusal(&p, &f.live), None);
        }

        #[test]
        fn another_file_name_in_the_live_socket_folder_is_refused() {
            let f = fx();
            let p = paths(f.root.join("live-sock/other.sock"), f.root.join("dev-data"));
            let why = dev_run_refusal(&p, &f.live).expect("refused");
            assert!(why.contains("socket folder"), "{why}");
            // The live socket itself, and a folder inside the live one.
            assert!(dev_run_refusal(&paths(f.root.join("live-sock/modeld.sock"), f.root.join("dev-data")), &f.live).is_some());
            assert!(dev_run_refusal(&paths(f.root.join("live-sock/sub/modeld.sock"), f.root.join("dev-data")), &f.live).is_some());
        }

        #[test]
        fn a_socket_folder_that_links_to_the_live_one_is_refused() {
            let f = fx();
            symlink(f.root.join("live-sock"), f.root.join("alias-sock")).unwrap();
            let p = paths(f.root.join("alias-sock/modeld.sock"), f.root.join("dev-data"));
            assert!(dev_run_refusal(&p, &f.live).is_some());
        }

        #[test]
        fn a_data_folder_that_links_into_the_live_one_is_refused() {
            let f = fx();
            // The folder itself is a link to the live folder.
            symlink(f.root.join("live-data"), f.root.join("alias-data")).unwrap();
            assert!(dev_run_refusal(&paths(f.root.join("dev-sock/modeld.sock"), f.root.join("alias-data")), &f.live).is_some());
            // A link to a folder inside it.
            symlink(f.root.join("live-data/modelnet"), f.root.join("alias-sub")).unwrap();
            assert!(dev_run_refusal(&paths(f.root.join("dev-sock/modeld.sock"), f.root.join("alias-sub")), &f.live).is_some());
            // A link to a folder inside it that does not exist yet: the run
            // would create it there.
            symlink(f.root.join("live-data/not-yet"), f.root.join("alias-new")).unwrap();
            assert!(dev_run_refusal(&paths(f.root.join("dev-sock/modeld.sock"), f.root.join("alias-new")), &f.live).is_some());
            // The same with a relative link.
            symlink("live-data/also-not-yet", f.root.join("alias-rel")).unwrap();
            assert!(dev_run_refusal(&paths(f.root.join("dev-sock/modeld.sock"), f.root.join("alias-rel")), &f.live).is_some());
            // Plainly inside, and a folder that holds the live one.
            assert!(dev_run_refusal(&paths(f.root.join("dev-sock/modeld.sock"), f.root.join("live-data/x")), &f.live).is_some());
            assert!(dev_run_refusal(&paths(f.root.join("dev-sock/modeld.sock"), f.root.clone()), &f.live).is_some());
        }

        #[test]
        fn a_live_app_whose_data_was_moved_is_recognised() {
            let f = fx();
            let moved = f.root.join("other-drive/AInode");
            std::fs::create_dir_all(&moved).unwrap();
            write_location(&f.root.join("live-data"), &moved).unwrap();
            let live = LiveInstall::of(f.live.socket.clone(), Some(f.root.join("live-data")));
            assert_eq!(live.data_dirs, vec![moved.clone(), f.root.join("live-data")]);
            let sock = f.root.join("dev-sock/modeld.sock");
            assert!(dev_run_refusal(&paths(sock.clone(), moved.clone()), &live).is_some(), "the moved folder");
            assert!(dev_run_refusal(&paths(sock.clone(), moved.join("modelnet")), &live).is_some(), "inside it");
            assert!(dev_run_refusal(&paths(sock.clone(), f.root.join("live-data")), &live).is_some(), "the default folder still");
            assert_eq!(dev_run_refusal(&paths(sock, f.root.join("dev-data")), &live), None);
        }

        #[test]
        fn relative_paths_are_refused() {
            let f = fx();
            // A bare file name would be read against the folder the run
            // starts in, which may be the live socket folder.
            let why = dev_run_refusal(&paths(PathBuf::from("modeld.sock"), f.root.join("dev-data")), &f.live).expect("refused");
            assert!(why.contains("relative"), "{why}");
            assert!(dev_run_refusal(&paths(f.root.join("dev-sock/modeld.sock"), PathBuf::from("data")), &f.live).is_some());
            // `..` could walk back into the live folders.
            let dotdot = f.root.join("dev-sock/../live-sock/modeld.sock");
            assert!(dev_run_refusal(&paths(dotdot, f.root.join("dev-data")), &f.live).is_some());
        }
    }

    #[test]
    fn socket_path_length_is_checked() {
        let p = Paths {
            data_dir: "/d".into(),
            engine_dir: "/e".into(),
            socket: PathBuf::from(format!("/{}/modeld.sock", "x".repeat(100))),
        };
        // Windows: the socket is inside WSL; this path only names a folder.
        assert_eq!(p.check().is_err(), cfg!(unix));
    }
}
