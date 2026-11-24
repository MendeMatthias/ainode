//! Windows: when WSL itself is stuck, start the engine again where that is
//! safe (0.8.4, after 2026-10-20 on the PC).
//!
//! A graphics driver reset after an update reboot left WSL stuck: every
//! distribution's Windows drives answered "Input/output error" and no stopped
//! distribution could start (CreateInstance 0x800705b4). Only `wsl --shutdown`
//! helped, and that stops every Linux on the PC (there: the BTX node's
//! Ubuntu). So, when a start fails that way (`wsl::is_stuck`):
//!
//! 1. easyNode restarting WSL (its note, neighbour.rs): AInode waits for it,
//!    calls no wsl.exe meanwhile, and starts again after (`wait_for_easynode`,
//!    also before every start).
//! 2. A second look (`wsl::probe`) must find WSL stuck too.
//! 3. AInode's distribution is the only one running, and AInode has not
//!    restarted WSL by itself in the last AUTO_SPACING: it restarts WSL itself
//!    and starts the engine again. No click.
//! 4. Other distributions run (a BTX node, the user's own Linux), or WSL
//!    cannot say which: never by itself. The UI shows one button naming them
//!    (`WslOut::stuck_others`); `restart_wsl` does it on that click only, and
//!    only when nothing else has started since the user read the names.
//! 5. After that restart, every distribution it stopped is started again the
//!    way a boot starts it (`wsl::start_again`), and the home line says so
//!    (`WslOut::again`). Never one that was not running before.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;

use super::Backend;
use crate::{neighbour, state, wsl};

/// AInode restarts WSL by itself at most once in this long (its own log, so
/// it holds across app restarts too): a WSL that is stuck again right after a
/// restart needs the user, not a loop.
pub const AUTO_SPACING: u64 = 30 * 60;

/// What the home screen shows about WSL (get_state `wsl`, Windows only).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct WslOut {
    /// AInode is restarting WSL now (by itself, or on the user's click).
    pub restarting: bool,
    /// AInode is waiting while easyNode restarts WSL.
    pub easynode: bool,
    /// WSL is stuck and other distributions run: the names the button shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stuck_others: Option<Vec<String>>,
    /// A BTX node answers on this PC (it stops with WSL too).
    pub node_here: bool,
    /// After AInode's last restart of WSL (this run): what it started again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub again: Option<Again>,
}

/// The other distributions a restart stopped, started again (or not).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Again {
    /// When starting them again was over (unix seconds).
    pub at: u64,
    pub back: Vec<String>,
    pub not_back: Vec<String>,
}

#[derive(Default)]
pub struct WslHeal {
    restarting: AtomicBool,
    easynode: AtomicBool,
    ask: std::sync::Mutex<Option<(Vec<String>, bool)>>,
    again: std::sync::Mutex<Option<Again>>,
    /// One recovery at a time (the UI's retry and the button).
    lock: tokio::sync::Mutex<()>,
}

impl WslHeal {
    pub fn out(&self) -> WslOut {
        let ask = self.ask.lock().unwrap().clone();
        WslOut {
            restarting: self.restarting.load(Ordering::SeqCst),
            easynode: self.easynode.load(Ordering::SeqCst),
            node_here: ask.as_ref().is_some_and(|a| a.1),
            stuck_others: ask.map(|a| a.0),
            again: self.again.lock().unwrap().clone(),
        }
    }

    /// The engine runs again: nothing to ask any more.
    pub fn clear(&self) {
        *self.ask.lock().unwrap() = None;
    }

    /// The engine shows as starting while AInode restarts WSL or waits for easyNode.
    pub fn busy(&self) -> bool {
        self.restarting.load(Ordering::SeqCst) || self.easynode.load(Ordering::SeqCst)
    }
}

/// Clears a flag when the step that set it ends, however it ends.
struct Flag<'a>(&'a AtomicBool);
impl<'a> Flag<'a> {
    fn raise(f: &'a AtomicBool) -> Self {
        f.store(true, Ordering::SeqCst);
        Flag(f)
    }
}
impl Drop for Flag<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// What to do about a stuck WSL, decided from facts only (a test pins it).
#[derive(Debug, PartialEq)]
pub enum Decision {
    /// Restart WSL now, nobody asked.
    Alone,
    /// Ask with a button naming these.
    Ask(Vec<String>),
    /// AInode restarted WSL by itself a short while ago: tell the user.
    TooSoon,
    /// WSL cannot say what runs: say how to restart it, no button.
    Unknown,
}

pub fn decide(running: Result<&[String], &str>, last_auto: Option<u64>, now: u64) -> Decision {
    let Ok(running) = running else { return Decision::Unknown };
    let others = wsl::others_of(running);
    if !others.is_empty() {
        return Decision::Ask(others);
    }
    match last_auto {
        Some(t) if now.saturating_sub(t) < AUTO_SPACING => Decision::TooSoon,
        _ => Decision::Alone,
    }
}

/// What to start again after a restart: the others running just before it
/// (`others_before`, which the click covered), never a name the user read
/// that had stopped meanwhile, never one twice.
pub fn to_start_again(others_before: &[String]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for d in others_before {
        if !d.eq_ignore_ascii_case(wsl::distro()) && !v.iter().any(|x| x.eq_ignore_ascii_case(d)) {
            v.push(d.clone());
        }
    }
    v
}

/// The user's click consented to stopping `seen`: nothing else may run now.
pub fn click_covers(seen: &[String], others_now: &[String]) -> Result<(), Vec<String>> {
    let new: Vec<String> = others_now.iter().filter(|d| !seen.iter().any(|s| s.eq_ignore_ascii_case(d))).cloned().collect();
    if new.is_empty() {
        Ok(())
    } else {
        Err(new)
    }
}

impl Backend {
    /// Before any start: while easyNode restarts WSL, wait (no wsl.exe call),
    /// at most until its restart is over (neighbour::EASYNODE_RESTART_MAX).
    pub(super) async fn wait_for_easynode(&self) {
        let Some(mut until) = neighbour::easynode_restarting() else { return };
        let _f = Flag::raise(&self.wsl_heal.easynode);
        eprintln!("[wsl] easyNode is restarting WSL; waiting until it is done");
        loop {
            let now = state::now_unix();
            if now >= until {
                break;
            }
            tokio::time::sleep(Duration::from_secs((until - now).min(5))).await;
            match neighbour::easynode_restarting() {
                Some(u) => until = u,
                None => break,
            }
        }
    }

    /// A start failed with a stuck WSL's error (`first`). Recover where it is
    /// safe, else leave the question to the user. Ok when the engine runs.
    pub(super) async fn on_wsl_stuck(&self, first: String) -> Result<(), String> {
        let _one = self.wsl_heal.lock.lock().await;
        if self.engine.is_closed() {
            return Err(first);
        }
        // easyNode restarting WSL is the fix already under way: wait for it.
        if neighbour::easynode_restarting().is_some() {
            self.wait_for_easynode().await;
            return self.start_once().await;
        }
        match wsl::probe(&self.paths).await {
            wsl::Probe::Stuck => {}
            // Gone by itself, or something else: one more start shows which.
            _ => {
                self.wait_for_easynode().await;
                return self.start_once().await;
            }
        }
        // The probe took up to a minute: easyNode may have begun meanwhile.
        if neighbour::easynode_restarting().is_some() {
            self.wait_for_easynode().await;
            return self.start_once().await;
        }
        let running = wsl::running().await;
        let d = decide(running.as_deref().map_err(String::as_str), wsl::last_restart(), state::now_unix());
        eprintln!("[wsl] WSL is stuck ({}); {d:?}", first.lines().next().unwrap_or(""));
        match d {
            Decision::Alone => {
                self.chat.note("WSL was stuck (its drives answered Input/output error); AInode's distribution was the only one running, so AInode restarted WSL");
                self.restart_and_start("alone", true, Vec::new()).await
            }
            Decision::Ask(others) => {
                let node = neighbour::node_here().await;
                *self.wsl_heal.ask.lock().unwrap() = Some((others, node));
                Err(wsl::WSL_STUCK_ASK.into())
            }
            Decision::TooSoon => Err(wsl::WSL_STILL_STUCK.into()),
            Decision::Unknown => Err(wsl::WSL_STUCK.into()),
        }
    }

    /// The button: the user read `seen` (the other distributions) and chose
    /// to restart WSL. Refused when another one started since; no restart
    /// at all when WSL came back by itself meanwhile.
    pub async fn restart_wsl(&self, seen: Vec<String>) -> Result<(), String> {
        let r = self.restart_wsl_inner(seen).await;
        if let Err(e) = &r {
            // The status line says what is true now, not the first error.
            self.engine.failed(e);
        }
        r
    }

    async fn restart_wsl_inner(&self, seen: Vec<String>) -> Result<(), String> {
        let _one = self.wsl_heal.lock.lock().await;
        if self.engine.is_running() {
            self.wsl_heal.clear();
            return Ok(());
        }
        if self.engine.is_closed() {
            return Err("AInode is closing".into());
        }
        self.wait_for_easynode().await;
        if wsl::probe(&self.paths).await != wsl::Probe::Stuck {
            eprintln!("[wsl] WSL answers again: starting without a restart");
            self.wsl_heal.clear();
            return self.start_once().await;
        }
        let running = wsl::running().await?;
        let others = wsl::others_of(&running);
        if let Err(new) = click_covers(&seen, &others) {
            *self.wsl_heal.ask.lock().unwrap() = Some((others, neighbour::node_here().await));
            return Err(format!("{} started meanwhile and would stop too. Read the names again, then press Restart.", wsl::names_text(&new)));
        }
        let stops = if others.is_empty() { "nothing".to_string() } else { wsl::names_text(&others) };
        self.chat.note(&format!("WSL was stuck; restarted on the user's click (also stopped: {stops})"));
        self.restart_and_start(&format!("asked; also stops: {stops}"), false, to_start_again(&others)).await
    }

    /// `by_itself`: nobody clicked (wsl::restart then needs its note written).
    /// `others`: the other distributions running now; they stop too, and are
    /// started again right after.
    async fn restart_and_start(&self, why: &str, by_itself: bool, others: Vec<String>) -> Result<(), String> {
        if self.engine.is_closed() {
            return Err("AInode is closing".into());
        }
        self.wsl_heal.clear();
        // Last look before the shutdown: never in the middle of easyNode's restart.
        if neighbour::easynode_restarting().is_some() {
            self.wait_for_easynode().await;
            return self.start_once().await;
        }
        *self.wsl_heal.again.lock().unwrap() = None;
        let r = {
            let _f = Flag::raise(&self.wsl_heal.restarting);
            wsl::restart(why, by_itself, &others).await?;
            // WSL's service settles before the next distribution starts.
            tokio::time::sleep(Duration::from_secs(3)).await;
            // The others first (a BTX node's boot hook), AInode's engine meanwhile.
            let (again, r) = tokio::join!(wsl::start_again(&others), self.start_once());
            if !others.is_empty() {
                let not_back: Vec<String> = again.not_running.iter().map(|(n, _)| n.clone()).collect();
                let back = wsl::names_text(&again.running);
                self.chat.note(&format!(
                    "after the WSL restart: running again: {}; not running again: {}",
                    if back.is_empty() { "none" } else { &back },
                    if not_back.is_empty() { "none".to_string() } else { wsl::names_text(&not_back) }
                ));
                *self.wsl_heal.again.lock().unwrap() = Some(Again { at: state::now_unix(), back: again.running, not_back });
            }
            r
        };
        match r {
            Err(e) if wsl::is_stuck(&e) => Err(wsl::WSL_STILL_STUCK.into()),
            r => r,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn alone_restarts_by_itself_others_are_asked() {
        let now = 1_792_510_805;
        let ours = wsl::distro();
        // Only AInode's distribution, or none at all: by itself.
        assert_eq!(decide(Ok(&v(&[ours])), None, now), Decision::Alone);
        assert_eq!(decide(Ok(&[]), None, now), Decision::Alone);
        // Anything else running: the button, naming them (2026-10-20 on the PC).
        assert_eq!(
            decide(Ok(&v(&["Ubuntu", ours, "AInode-debian13", "AInode-build"])), None, now),
            Decision::Ask(v(&["Ubuntu", "AInode-debian13", "AInode-build"]))
        );
        // Asked even right after a restart: the user decides.
        assert_eq!(decide(Ok(&v(&["Ubuntu"])), Some(now - 60), now), Decision::Ask(v(&["Ubuntu"])));
        // By itself at most once in AUTO_SPACING.
        assert_eq!(decide(Ok(&v(&[ours])), Some(now - 60), now), Decision::TooSoon);
        assert_eq!(decide(Ok(&v(&[ours])), Some(now - AUTO_SPACING), now), Decision::Alone);
        // WSL cannot say what runs: never by itself, no button.
        assert_eq!(decide(Err("wsl did not answer"), None, now), Decision::Unknown);
    }

    #[test]
    fn only_what_ran_before_is_started_again() {
        let ours = wsl::distro();
        // What ran just before the restart, AInode's own aside (its engine starts it).
        assert_eq!(to_start_again(&v(&["Ubuntu", ours, "AInode-debian13"])), v(&["Ubuntu", "AInode-debian13"]));
        assert_eq!(to_start_again(&v(&["Ubuntu", "ubuntu"])), v(&["Ubuntu"]));
        assert_eq!(to_start_again(&[]), Vec::<String>::new());
        // The user read Ubuntu and Kali; Kali stopped before the click: only Ubuntu ran, only Ubuntu again.
        let seen = v(&["Ubuntu", "Kali"]);
        let before = v(&["Ubuntu"]);
        assert_eq!(click_covers(&seen, &before), Ok(()));
        assert_eq!(to_start_again(&before), v(&["Ubuntu"]));
    }

    #[test]
    fn a_click_covers_only_what_the_user_read() {
        assert_eq!(click_covers(&v(&["Ubuntu", "Debian"]), &v(&["Ubuntu"])), Ok(()));
        assert_eq!(click_covers(&v(&["Ubuntu"]), &v(&["ubuntu"])), Ok(()));
        assert_eq!(click_covers(&v(&["Ubuntu"]), &v(&["Ubuntu", "Kali"])), Err(v(&["Kali"])));
        assert_eq!(click_covers(&[], &v(&["Ubuntu"])), Err(v(&["Ubuntu"])));
    }
}
