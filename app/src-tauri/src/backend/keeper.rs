//! The app's own timers, in Rust (0.8.4): the check-in with easybtx.com and
//! the engine's self-heal.
//!
//! Both used to run in the page (main.ts). The Linux AInode under WSLg
//! stopped checking in while the PC sat idle (2026-10-20 11:45 to 15:40, back
//! at the first input), though its process and engine kept running. A
//! webview's timers are its engine's to throttle or stop (a hidden page, a
//! display that sleeps); these must not be. They do not depend on the
//! window: a tokio task started at launch (lib.rs) runs them every TICK.
//! The page keeps what only matters while someone looks: reading the count,
//! the update checks, the lines and buttons.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use serde::Serialize;
use tokio::time::Instant;

use super::Backend;
use crate::engine::EngineState;
use crate::state;

pub const TICK: Duration = Duration::from_secs(5);

/// Check in every 5 minutes (easybtx.com counts an install online for 15).
pub const CHECKIN_EVERY: Duration = Duration::from_secs(5 * 60);
const CHECKIN_URL: &str = "https://easybtx.com/api/ainode-checkin";
const CHECKIN_TIMEOUT: Duration = Duration::from_secs(20);
/// The first check-in after launch waits this long, so it says what the engine does
/// (started by the page as it opens), not that it has not started yet.
pub const FIRST_CHECKIN: Duration = Duration::from_secs(30);

/// After the engine stops on its own (it crashed, or on Windows WSL was
/// restarted under it), start it again while the user wants it on
/// (`autostart`, which Stop turns off): after 20 s, 1 min and 3 min, at most
/// 3 times in 10 minutes. The first wait lets whoever restarted WSL boot it
/// first. After that the Retry button is the user's. (As logic.ts had it.)
pub const RESTART_WAITS: [Duration; 3] = [Duration::from_secs(20), Duration::from_secs(60), Duration::from_secs(180)];
pub const RESTART_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct Keeper {
    errored_since: StdMutex<Option<Instant>>,
    restarts: StdMutex<Vec<Instant>>,
    /// A restart is coming (the home line says so instead of an error).
    pending: AtomicBool,
    checkin_due: StdMutex<Option<Instant>>,
    /// The last check-in: when (unix seconds) and whether easybtx.com took it.
    last_checkin: StdMutex<Option<(u64, bool)>>,
}

/// What the page shows about these timers (get_state `keeper`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KeeperOut {
    /// The engine stopped on its own and AInode starts it again soon.
    pub restart_pending: bool,
    /// The last check-in, unix seconds, and whether it was accepted.
    pub checked_in_at: Option<u64>,
    pub checkin_ok: Option<bool>,
}

#[derive(Debug, PartialEq)]
pub enum Due {
    Now,
    Wait,
    No,
}

/// Start the engine again now? (`past`: this app's earlier automatic restarts.)
pub fn restart_due(wanted: bool, error: Option<&str>, now: Instant, errored_since: Instant, past: &[Instant]) -> Due {
    let Some(error) = error.filter(|_| wanted) else { return Due::No };
    let recent = past.iter().filter(|t| now.duration_since(**t) < RESTART_WINDOW).count();
    // The first restart needs a stop that looks like a crash; once a sequence
    // has begun, a failed start (WSL still coming back) is retried too.
    let e = error.to_ascii_lowercase();
    if recent == 0 && !["signal", "exited", "stopped"].iter().any(|w| e.contains(w)) {
        return Due::No;
    }
    if recent >= RESTART_WAITS.len() {
        return Due::No;
    }
    if now.duration_since(errored_since) >= RESTART_WAITS[recent] {
        Due::Now
    } else {
        Due::Wait
    }
}

/// The check-in easybtx.com takes (site src/lib/ainodeCheckin.mjs, schema 1):
/// nothing about what anyone asks a model, no file, no address, no key.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Checkin {
    pub schema: u8,
    pub node_id: String,
    pub agent: String,
    pub platform: &'static str,
    pub engine: &'static str,
    pub models: u64,
    pub hosts: u64,
}

pub fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

impl Keeper {
    pub fn out(&self) -> KeeperOut {
        let last = *self.last_checkin.lock().unwrap();
        KeeperOut { restart_pending: self.pending.load(Ordering::SeqCst), checked_in_at: last.map(|l| l.0), checkin_ok: last.map(|l| l.1) }
    }
}

impl Backend {
    /// One round of the self-heal. lib.rs runs it every TICK for the app's
    /// life, in its own loop: a long recovery (easyNode's WSL restart, an
    /// import) never holds up the check-in.
    pub async fn heal_tick(&self) {
        self.heal_engine().await;
    }

    /// One round of the check-in, in a loop of its own (lib.rs).
    pub async fn checkin_tick(&self) {
        self.maybe_checkin().await;
    }

    async fn heal_engine(&self) {
        let now = Instant::now();
        let st = self.engine.state();
        let error = match st {
            // Quitting or installing an update (closed): never start it again.
            EngineState::Error(e) if !self.engine.is_closed() => e,
            _ => {
                // Any other state ends the error the waits count from.
                *self.keeper.errored_since.lock().unwrap() = None;
                self.keeper.pending.store(false, Ordering::SeqCst);
                return;
            }
        };
        let since = *self.keeper.errored_since.lock().unwrap().get_or_insert(now);
        let wanted = state::load_consent(&self.paths).is_some() && self.settings.lock().unwrap().autostart;
        let past = self.keeper.restarts.lock().unwrap().clone();
        let due = restart_due(wanted, Some(&error), now, since, &past);
        self.keeper.pending.store(due != Due::No, Ordering::SeqCst);
        if due != Due::Now {
            return;
        }
        self.keeper.restarts.lock().unwrap().push(now);
        eprintln!("[keeper] the engine stopped ({error}); starting it again");
        if let Err(e) = self.start_engine().await {
            eprintln!("[keeper] the restart failed: {e}");
        }
        // The next wait counts from this attempt.
        *self.keeper.errored_since.lock().unwrap() = None;
        self.keeper.pending.store(false, Ordering::SeqCst);
    }

    /// What this app tells easybtx.com now; None when it does not check in
    /// (before consent, or "count me" off).
    pub async fn checkin_body(&self) -> Option<Checkin> {
        state::load_consent(&self.paths)?;
        if !self.settings.lock().unwrap().count_me {
            return None;
        }
        let node_id = self.community_id()?;
        // Never `catalog()`: its failover could switch hosts and restart the engine.
        let models = self.catalog_quiet().await.iter().filter(|m| m.local == "ready").count() as u64;
        Some(Checkin {
            schema: 1,
            node_id,
            agent: format!("ainode/{}", self.version),
            platform: platform(),
            engine: self.get_state_engine_name(),
            models,
            hosts: u64::from(self.active_peer().is_some()),
        })
    }

    /// The engine's state as get_state names it (Windows: restarting WSL is starting).
    fn get_state_engine_name(&self) -> &'static str {
        let st = self.engine.state();
        #[cfg(windows)]
        if self.wsl_heal.busy() && st != EngineState::Running {
            return EngineState::Starting.name();
        }
        st.name()
    }

    async fn maybe_checkin(&self) {
        let now = Instant::now();
        {
            let mut due = self.keeper.checkin_due.lock().unwrap();
            match *due {
                None => {
                    *due = Some(now + FIRST_CHECKIN);
                    return;
                }
                Some(d) if now < d => return,
                Some(_) => {}
            }
        }
        // Not yet (no consent) or not at all ("count me" off): look again at
        // the next tick, so a consent or a switch turned on counts at once.
        let Some(body) = self.checkin_body().await else { return };
        *self.keeper.checkin_due.lock().unwrap() = Some(now + CHECKIN_EVERY);
        let ok = post_checkin(&body).await;
        if let Err(e) = &ok {
            eprintln!("[keeper] check-in: {e}");
        }
        *self.keeper.last_checkin.lock().unwrap() = Some((state::now_unix(), ok.is_ok()));
    }
}

async fn post_checkin(body: &Checkin) -> Result<(), String> {
    // As the updater does: rustls with the ring provider, the system's roots.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder().timeout(CHECKIN_TIMEOUT).build().map_err(|e| e.to_string())?;
    // A debug build's test may catch it locally instead (never counted).
    let url = state::dev_env("AINODE_CHECKIN_URL").and_then(|v| v.into_string().ok()).unwrap_or_else(|| CHECKIN_URL.into());
    let r = client
        .post(url)
        .header("content-type", "application/json")
        .header("x-ainode", "ainode-checkin-v1")
        .body(serde_json::to_vec(body).map_err(|e| e.to_string())?)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!("easybtx.com answered {}", r.status()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_after_20_s_1_min_3_min_then_the_user() {
        let t0 = Instant::now();
        let s = |n: u64| Duration::from_secs(n);
        let crash = Some("The engine was stopped by signal 9");
        assert_eq!(restart_due(true, crash, t0 + s(19), t0, &[]), Due::Wait);
        assert_eq!(restart_due(true, crash, t0 + s(20), t0, &[]), Due::Now);
        // Not wanted (Stop pressed, no consent), or no error: never.
        assert_eq!(restart_due(false, crash, t0 + s(60), t0, &[]), Due::No);
        assert_eq!(restart_due(true, None, t0 + s(60), t0, &[]), Due::No);
        // A first failure that is not a crash (no space, a broken install): the user's.
        assert_eq!(restart_due(true, Some("There is not enough free disk space"), t0 + s(60), t0, &[]), Due::No);
        // Once a sequence began, a failed start is retried: 1 min, then 3 min.
        let one = [t0];
        assert_eq!(restart_due(true, Some("WSL did not answer"), t0 + s(59), t0, &one), Due::Wait);
        assert_eq!(restart_due(true, Some("WSL did not answer"), t0 + s(60), t0, &one), Due::Now);
        let two = [t0, t0 + s(60)];
        assert_eq!(restart_due(true, crash, t0 + s(60) + s(179), t0 + s(60), &two), Due::Wait);
        assert_eq!(restart_due(true, crash, t0 + s(60) + s(180), t0 + s(60), &two), Due::Now);
        // Three in 10 minutes: then the Retry button.
        let three = [t0, t0 + s(60), t0 + s(240)];
        assert_eq!(restart_due(true, crash, t0 + s(300), t0 + s(240), &three), Due::No);
        // Ten minutes later the count starts again.
        assert_eq!(restart_due(true, crash, t0 + s(840) + s(20), t0 + s(840), &three), Due::Now);
    }

    /// The real endpoint, with a check-in it must refuse (schema 0): the
    /// transport works (TLS, the header), and nothing is counted.
    #[tokio::test]
    #[ignore = "reaches easybtx.com"]
    async fn easybtx_answers_and_refuses_a_bad_checkin() {
        let bad = Checkin { schema: 0, node_id: "0".repeat(32), agent: "ainode/0.0.0".into(), platform: platform(), engine: "running", models: 0, hosts: 0 };
        let e = post_checkin(&bad).await.unwrap_err();
        assert!(e.contains("422"), "refused as invalid, not a network error: {e}");
    }

    #[test]
    fn the_checkin_is_the_sites_schema_and_nothing_else() {
        let c = Checkin { schema: 1, node_id: "0".repeat(32), agent: "ainode/0.8.4".into(), platform: platform(), engine: "running", models: 1, hosts: 1 };
        let v = serde_json::to_value(&c).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["agent", "engine", "hosts", "models", "node_id", "platform", "schema"]);
        assert!(["macos", "windows", "linux"].contains(&c.platform));
    }
}
