//! Windows, 0.8.4: what AInode does when WSL itself is stuck (backend/wslheal.rs),
//! each path through the real backend, the real engine and a real scratch WSL
//! distribution. A stuck WSL cannot be made safely (only a graphics driver
//! reset did it, 2026-10-20), so the debug build's test seam injects WSL's
//! own error texts (`wsl::fake`), and a "restart" stops only the scratch
//! distribution: WSL itself, the user's `AInode` and every other
//! distribution keep running. Ignored by default. In PowerShell:
//!
//!   $env:AINODE_WSL_DISTRO = "AInode-t084"      # never "AInode"
//!   $env:TEMP = $env:TMP = <a scratch folder>    # easyNode's and AInode's notes
//!   $env:AINODE_TEST_ENGINE_DIR = "$PWD\resources\engine"
//!   $env:AINODE_TEST_DATA_DIR = <a scratch folder>
//!   cargo test --test wsl_stuck_live -- --ignored --nocapture --test-threads 1
#![cfg(windows)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ainode_lib::backend::Backend;
use ainode_lib::state::Paths;
use ainode_lib::wsl;

fn set(k: &str, v: Option<&str>) {
    match v {
        Some(v) => std::env::set_var(k, v),
        None => std::env::remove_var(k),
    }
}

fn notes() -> String {
    std::fs::read_to_string(std::env::temp_dir().join(wsl::NOTE_LOG)).unwrap_or_default()
}

fn app_log(paths: &Paths) -> String {
    std::fs::read_to_string(paths.chat_pidfile().with_file_name("ainode.log")).unwrap_or_default()
}

fn stamp(t: u64) -> String {
    // Unix seconds as easyNode's script writes them (UTC, `s` format plus Z).
    let days = t / 86_400;
    let secs = t % 86_400;
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

async fn registered(name: &str) -> bool {
    let out = tokio::process::Command::new(r"C:\Windows\System32\wsl.exe").args(["--list", "--quiet"]).env("WSL_UTF8", "1").output().await.unwrap();
    String::from_utf8_lossy(&out.stdout).replace('\0', "").lines().any(|l| l.trim() == name)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a scratch WSL distribution name (AINODE_WSL_DISTRO), a scratch TEMP and AINODE_TEST_ENGINE_DIR"]
async fn every_path_of_a_stuck_wsl() {
    tokio::spawn(flow()).await.unwrap();
}

async fn flow() {
    let distro = std::env::var("AINODE_WSL_DISTRO").expect("set AINODE_WSL_DISTRO to a scratch name");
    assert_ne!(distro, "AInode", "never the installed app's distribution");
    assert_eq!(wsl::distro(), distro);
    let temp = std::env::temp_dir();
    assert!(!temp.to_string_lossy().ends_with(r"AppData\Local\Temp") && !temp.to_string_lossy().ends_with(r"AppData\Local\Temp\"), "a scratch TEMP, not the user's: {}", temp.display());
    let engine_dir = PathBuf::from(std::env::var("AINODE_TEST_ENGINE_DIR").expect("set AINODE_TEST_ENGINE_DIR"));
    let data_dir = PathBuf::from(std::env::var("AINODE_TEST_DATA_DIR").expect("set AINODE_TEST_DATA_DIR")).join(format!("t084-{}", std::process::id()));
    let paths = Paths { data_dir: data_dir.clone(), engine_dir, socket: temp.join("ainode-t084").join("modeld.sock") };
    paths.check().unwrap();
    // The app makes its runtime folder at launch (lib.rs open_main_window); ainode.log is there.
    std::fs::create_dir_all(paths.socket_dir()).unwrap();
    let b = Backend::new(paths.clone(), "0.0.0-test".into());
    b.accept_consent(4, None).await.unwrap();

    // ── 0. A clean start: the scratch distribution is made, the engine runs. ──
    set("AINODE_FAKE_WSL_STUCK", None);
    let t = Instant::now();
    b.start_engine().await.unwrap();
    println!("0. clean start: engine running in {:?}", t.elapsed());
    assert!(b.engine.is_running());
    b.stop_engine().await.unwrap();

    // ── 1. Stuck (the mounts answer Input/output error), only AInode's distribution
    //       running: AInode restarts WSL by itself and the engine runs. ──
    set("AINODE_FAKE_WSL_STUCK", Some("io"));
    set("AINODE_FAKE_WSL_OTHERS", Some("-"));
    wsl::fake::arm();
    let t = Instant::now();
    let r = b.start_engine().await;
    println!("1. alone, io: {:?} in {:?}; engine {}", r, t.elapsed(), b.engine.state().name());
    assert!(r.is_ok() && b.engine.is_running());
    assert!(notes().contains("AInode found WSL stuck; restarting WSL (alone)"), "{}", notes());
    assert!(app_log(&paths).contains("AInode restarted WSL"), "{}", app_log(&paths));
    assert_eq!(b.get_state().await.wsl.unwrap().stuck_others, None);
    b.stop_engine().await.unwrap();

    // ── 2. Stuck again within 30 minutes, alone: not by itself a second time. ──
    wsl::fake::arm();
    let r = b.start_engine().await;
    println!("2. alone again, too soon: {r:?}");
    assert_eq!(r.unwrap_err(), wsl::WSL_STILL_STUCK);
    assert_eq!(notes().matches("restarting WSL (alone)").count(), 1, "one restart only: {}", notes());
    let st = b.get_state().await;
    assert_eq!(st.engine.state, "error");
    assert_eq!(st.engine.error.as_deref(), Some(wsl::WSL_STILL_STUCK));

    // ── 3. Stuck, no distribution can start (CreateInstance), others running
    //       (the real list): never by itself; the button names them; AInode's
    //       distribution is NOT unregistered. ──
    std::fs::write(temp.join(wsl::NOTE_LOG), "").unwrap(); // a new day
    set("AINODE_FAKE_WSL_STUCK", Some("create"));
    set("AINODE_FAKE_WSL_OTHERS", None);
    wsl::fake::arm();
    let real_others = wsl::others_of(&wsl::running().await.unwrap());
    println!("   running besides {distro}: {real_others:?}");
    assert!(!real_others.is_empty(), "this PC runs the BTX node's Ubuntu");
    let r = b.start_engine().await;
    println!("3. others, create: {r:?}");
    assert_eq!(r.unwrap_err(), wsl::WSL_STUCK_ASK);
    let st = b.get_state().await;
    let w = st.wsl.clone().unwrap();
    println!("   get_state: engine {} {:?}; wsl {:?}", st.engine.state, st.engine.error, w);
    assert_eq!(w.stuck_others.as_deref(), Some(&real_others[..]));
    assert!(registered(&distro).await, "the distribution is kept: WSL was stuck, not it");
    assert!(!notes().contains("restarting WSL"), "nothing restarted: {}", notes());

    // ── 4. The click, but another distribution started since the user read the
    //       names: refused, nothing restarted, the new list shown. ──
    let seen: Vec<String> = real_others[1..].to_vec();
    let r = b.restart_wsl(seen).await;
    println!("4. click with a name missing: {r:?}");
    assert!(r.unwrap_err().contains("started meanwhile"));
    assert!(!notes().contains("restarting WSL"));
    assert_eq!(b.get_state().await.wsl.unwrap().stuck_others.as_deref(), Some(&real_others[..]));

    // ── 5. The click with what the button named: restarted, the engine runs. ──
    let t = Instant::now();
    let r = b.restart_wsl(real_others.clone()).await;
    println!("5. click: {r:?} in {:?}; engine {}", t.elapsed(), b.engine.state().name());
    assert!(r.is_ok() && b.engine.is_running());
    assert!(notes().contains("restarting WSL (asked; also stops: "), "{}", notes());
    assert_eq!(b.get_state().await.wsl.unwrap().stuck_others, None);
    b.stop_engine().await.unwrap();

    // ── 6. A restart that does not help: plain words to restart the PC. ──
    std::fs::write(temp.join(wsl::NOTE_LOG), "").unwrap();
    set("AINODE_FAKE_WSL_STUCK", Some("io-sticky"));
    set("AINODE_FAKE_WSL_OTHERS", Some("-"));
    wsl::fake::arm();
    let r = b.start_engine().await;
    println!("6. alone, a restart does not help: {r:?}");
    assert_eq!(r.unwrap_err(), wsl::WSL_STILL_STUCK);

    // ── 7. easyNode restarts WSL: AInode waits (no failure counted, no restart
    //       of its own) and starts once easyNode's relaunch is in. ──
    set("AINODE_FAKE_WSL_STUCK", None);
    set("AINODE_FAKE_WSL_OTHERS", None);
    let log = temp.join("easynode-wsl-restart.log");
    std::fs::write(&log, format!("{} easyNode lost its GPU block verifier; restarting WSL\r\n", stamp(now()))).unwrap();
    let t = Instant::now();
    let b = std::sync::Arc::new(b);
    let b2 = b.clone();
    let start = tokio::spawn(async move { b2.start_engine().await });
    tokio::time::sleep(Duration::from_secs(4)).await;
    let st = b.get_state().await;
    println!("7. during easyNode's restart: engine {}, wsl {:?}", st.engine.state, st.wsl);
    assert_eq!(st.engine.state, "starting");
    assert!(st.wsl.unwrap().easynode);
    let mut text = std::fs::read_to_string(&log).unwrap();
    text.push_str(&format!("{} wsl --shutdown exited 0\r\n", stamp(now())));
    std::fs::write(&log, &text).unwrap();
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(!start.is_finished(), "still waiting for the relaunch");
    text.push_str(&format!("{} relaunch started, attempt 1\r\n", stamp(now())));
    std::fs::write(&log, &text).unwrap();
    let r = start.await.unwrap();
    println!("   started after {:?}: {r:?}", t.elapsed());
    assert!(r.is_ok() && b.engine.is_running());
    assert!(t.elapsed() >= Duration::from_secs(10 + 14), "waited 15 s past the relaunch: {:?}", t.elapsed());
    assert!(!b.get_state().await.wsl.unwrap().easynode);
    b.stop_engine().await.unwrap();

    // ── 8. The button left from a stuck WSL that then came back by itself:
    //       pressing it starts the engine and restarts nothing. ──
    std::fs::write(temp.join(wsl::NOTE_LOG), "").unwrap();
    std::fs::write(&log, "").unwrap();
    set("AINODE_FAKE_WSL_STUCK", Some("create"));
    wsl::fake::arm();
    let r = b.start_engine().await;
    assert_eq!(r.unwrap_err(), wsl::WSL_STUCK_ASK);
    let seen = b.get_state().await.wsl.unwrap().stuck_others.unwrap();
    set("AINODE_FAKE_WSL_STUCK", Some("none")); // WSL is fine again (the seam stays on: a restart stays fake)
    let r = b.restart_wsl(seen).await;
    println!("8. the old button after WSL came back: {r:?}, engine {}", b.engine.state().name());
    assert!(r.is_ok() && b.engine.is_running());
    assert!(!notes().contains("restarting WSL"), "nothing restarted: {}", notes());
    assert_eq!(b.get_state().await.wsl.unwrap().stuck_others, None);
    b.stop_engine().await.unwrap();

    // ── 9. easyNode begins its own WSL restart while AInode's probe runs
    //       (a slow probe, 6 s): AInode never restarts WSL, it waits. ──
    std::fs::write(temp.join(wsl::NOTE_LOG), "").unwrap();
    set("AINODE_FAKE_WSL_STUCK", Some("io"));
    set("AINODE_FAKE_WSL_OTHERS", Some("-"));
    set("AINODE_FAKE_WSL_PROBE_SECS", Some("6"));
    wsl::fake::arm();
    let b2 = b.clone();
    let start = tokio::spawn(async move { b2.start_engine().await });
    tokio::time::sleep(Duration::from_secs(2)).await;
    std::fs::write(&log, format!("{} easyNode lost its GPU block verifier; restarting WSL\r\n", stamp(now()))).unwrap();
    tokio::time::sleep(Duration::from_secs(8)).await;
    let st = b.get_state().await;
    println!("9. easyNode's note came during the probe: engine {}, wsl {:?}", st.engine.state, st.wsl);
    assert!(st.wsl.unwrap().easynode, "waiting for easyNode");
    std::fs::write(&log, format!("{}{} relaunch started, attempt 1\r\n", std::fs::read_to_string(&log).unwrap(), stamp(now()))).unwrap();
    set("AINODE_FAKE_WSL_STUCK", None); // easyNode's restart fixed it
    let r = start.await.unwrap();
    println!("   then: {r:?}; AInode's notes: {:?}", notes());
    assert!(r.is_ok() && b.engine.is_running());
    assert!(!notes().contains("restarting WSL"), "AInode did not restart WSL itself: {}", notes());
    set("AINODE_FAKE_WSL_PROBE_SECS", None);
    std::fs::write(&log, "").unwrap();
    b.stop_engine().await.unwrap();

    // ── 10. Quitting (or installing an update): nothing starts the engine and
    //        nothing restarts WSL any more. ──
    set("AINODE_FAKE_WSL_STUCK", Some("io"));
    wsl::fake::arm();
    b.shutdown().await;
    let r = b.start_engine().await;
    let c = b.restart_wsl(vec![]).await;
    println!("10. after shutdown: start {r:?}, click {c:?}");
    assert!(r.unwrap_err().contains("closing") && c.unwrap_err().contains("closing"));
    assert!(!notes().contains("restarting WSL"), "{}", notes());

    // ── 11. The fake list never counts without the seam on. ──
    set("AINODE_FAKE_WSL_STUCK", None);
    assert_eq!(wsl::fake::others(), None);
    set("AINODE_FAKE_WSL_OTHERS", None);

    // ── cleanup: the scratch distribution and data. ──
    wsl::terminate().await;
    let _ = tokio::process::Command::new(r"C:\Windows\System32\wsl.exe").args(["--unregister", &distro]).output().await;
    let _ = std::fs::remove_dir_all(&data_dir);
    println!("done: {distro} unregistered");
}
