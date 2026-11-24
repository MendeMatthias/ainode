//! Windows: after AInode restarted WSL on the user's click, the other
//! distributions it stopped are started again the way a boot starts them,
//! and only those (wsl::start_again, backend/wslheal.rs). Through the real
//! backend and engine with the debug build's test seam: a stuck WSL is
//! injected, the button names scratch distributions the test made (and
//! Ubuntu, which the seam never touches), and the "restart" stops only the
//! scratch ones. WSL itself and every distribution of the user's keep
//! running. Ignored by default. In PowerShell:
//!
//!   $env:AINODE_WSL_DISTRO = "AInode-t085"      # never "AInode"
//!   $env:TEMP = $env:TMP = <a scratch folder>    # AInode's restart notes
//!   $env:AINODE_TEST_ENGINE_DIR = "$PWD\resources\engine"
//!   $env:AINODE_TEST_DATA_DIR = <a scratch folder>
//!   cargo test --test wsl_again_live -- --ignored --nocapture --test-threads 1
//!
//! It leaves AInode-t085b running and held, to show the hold outlives the
//! test process; the caller (tools\t085\wsl-again-live.ps1) checks that and
//! removes every AInode-t085 distribution.
#![cfg(windows)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ainode_lib::backend::Backend;
use ainode_lib::state::Paths;
use ainode_lib::wsl;

const A: &str = "AInode-t085a"; // a boot hook that counts its boots
const B: &str = "AInode-t085b";
const C: &str = "AInode-t085c"; // made, never running: must stay stopped
const Z: &str = "AInode-t085z"; // named on the button, does not exist: cannot start again

fn set(k: &str, v: Option<&str>) {
    match v {
        Some(v) => std::env::set_var(k, v),
        None => std::env::remove_var(k),
    }
}

fn notes() -> String {
    std::fs::read_to_string(std::env::temp_dir().join(wsl::NOTE_LOG)).unwrap_or_default()
}

fn wsl_exe(args: &[&str], input: Option<&str>) -> String {
    use std::io::Write;
    let mut c = std::process::Command::new(r"C:\Windows\System32\wsl.exe");
    c.args(args).env("WSL_UTF8", "1").stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    c.stdin(if input.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() });
    let mut p = c.spawn().unwrap();
    if let Some(i) = input {
        p.stdin.take().unwrap().write_all(i.as_bytes()).unwrap();
    }
    let o = p.wait_with_output().unwrap();
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).replace('\0', "")
}

fn running() -> Vec<String> {
    wsl_exe(&["--list", "--running", "--quiet"], None).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

fn is_running(name: &str) -> bool {
    running().iter().any(|d| d == name)
}

fn boots(name: &str) -> usize {
    wsl_exe(&["-d", name, "-u", "root", "--exec", "cat", "/root/boot.log"], None).lines().filter(|l| l.trim().parse::<u64>().is_ok()).count()
}

/// The wsl.exe processes holding `name` (their command lines).
fn holders(name: &str) -> Vec<String> {
    let ps = format!(
        "Get-CimInstance Win32_Process -Filter \"Name='wsl.exe'\" | Where-Object {{ $_.CommandLine -like '*-d {name} *sleep*' }} | ForEach-Object {{ $_.CommandLine }}"
    );
    let o = std::process::Command::new("powershell.exe").args(["-NoProfile", "-Command", &ps]).output().unwrap();
    String::from_utf8_lossy(&o.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a scratch WSL distribution name (AINODE_WSL_DISTRO), a scratch TEMP and AINODE_TEST_ENGINE_DIR"]
async fn the_restart_brings_back_exactly_what_it_stopped() {
    tokio::spawn(flow()).await.unwrap();
}

async fn flow() {
    let distro = std::env::var("AINODE_WSL_DISTRO").expect("set AINODE_WSL_DISTRO to a scratch name");
    assert_eq!(distro, "AInode-t085", "this test's own scratch name");
    assert_eq!(wsl::distro(), distro);
    let temp = std::env::temp_dir();
    assert!(!temp.to_string_lossy().trim_end_matches('\\').ends_with(r"AppData\Local\Temp"), "a scratch TEMP, not the user's: {}", temp.display());
    let engine_dir = PathBuf::from(std::env::var("AINODE_TEST_ENGINE_DIR").expect("set AINODE_TEST_ENGINE_DIR"));
    let data_dir = PathBuf::from(std::env::var("AINODE_TEST_DATA_DIR").expect("set AINODE_TEST_DATA_DIR")).join(format!("t085-{}", std::process::id()));
    let user_running = || {
        let mut v: Vec<String> = running().into_iter().filter(|d| !d.starts_with("AInode-t")).collect();
        v.sort();
        v
    };
    let user_before = user_running();
    println!("the user's distributions running: {user_before:?}");

    // ── The scratch distributions: A counts its boots (wsl.conf [boot]), A and B
    //    run (as the user's Linux would), C is made and stopped. ──
    let rootfs = engine_dir.join("wsl").join("rootfs.tar.gz");
    for d in [A, B, C] {
        let dir = temp.join("vhd").join(d);
        std::fs::create_dir_all(&dir).unwrap();
        println!("import {d}: {}", wsl_exe(&["--import", d, &dir.to_string_lossy(), &rootfs.to_string_lossy(), "--version", "2"], None).trim());
    }
    wsl_exe(&["-d", A, "-u", "root", "--exec", "sh", "-c", "cat > /etc/wsl.conf; printf '#!/bin/sh\\ndate +%%s >> /root/boot.log\\n' > /root/boot.sh"], Some("[boot]\ncommand=/bin/sh /root/boot.sh\n"));
    for d in [A, B, C] {
        wsl_exe(&["--terminate", d], None);
    }
    wsl_exe(&["-d", A, "--exec", "sleep", "2"], None);
    wsl_exe(&["-d", B, "--exec", "sleep", "2"], None);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(is_running(A) && is_running(B) && !is_running(C), "{:?}", running());
    assert_eq!(boots(A), 1, "A booted once");

    let paths = Paths { data_dir: data_dir.clone(), engine_dir, socket: temp.join("ainode-t085").join("modeld.sock") };
    paths.check().unwrap();
    std::fs::create_dir_all(paths.socket_dir()).unwrap();
    let b = Backend::new(paths.clone(), "0.0.0-test".into());
    b.accept_consent(4, None).await.unwrap();
    set("AINODE_FAKE_WSL_STUCK", None);
    b.start_engine().await.unwrap();
    b.stop_engine().await.unwrap();

    // ── 1. WSL stuck; the button names A, B, Z and Ubuntu (the seam's list). ──
    set("AINODE_FAKE_WSL_STUCK", Some("io"));
    set("AINODE_FAKE_WSL_OTHERS", Some(&format!("{A},{B},{Z},Ubuntu")));
    wsl::fake::arm();
    let r = b.start_engine().await;
    assert_eq!(r.unwrap_err(), wsl::WSL_STUCK_ASK);
    let seen = b.get_state().await.wsl.unwrap().stuck_others.unwrap();
    println!("1. the button names {seen:?}");
    assert_eq!(seen, vec![A, B, Z, "Ubuntu"]);

    // ── 2. The click: A and B stop with the "restart" and run again, A's boot
    //       hook ran again; Z cannot start (it does not exist); C, which was not
    //       running, stays stopped; Ubuntu is never touched by the seam. ──
    let t = Instant::now();
    let r = b.restart_wsl(seen.clone()).await;
    let st = b.get_state().await;
    let again = st.wsl.clone().unwrap().again;
    println!("2. click: {r:?} in {:?}; engine {}; again {again:?}", t.elapsed(), st.engine.state);
    println!("   notes:\n{}", notes());
    assert!(r.is_ok() && b.engine.is_running());
    let again = again.expect("the home line's facts");
    assert_eq!(again.back, vec![A, B]);
    assert_eq!(again.not_back, vec![Z]);
    assert!(is_running(A) && is_running(B), "{:?}", running());
    assert!(!is_running(C), "C was not running before: never started");
    assert_eq!(boots(A), 2, "A's boot hook ran at the start again");
    let n = notes();
    assert!(n.contains(&format!("stops: {A},{B},{Z},Ubuntu")), "{n}");
    assert!(n.contains(&format!("(also: {A},{B},{Z})")), "the seam stopped only the scratch ones: {n}");
    assert!(n.contains("(test seam) not started, not a scratch distribution: Ubuntu"), "{n}");
    assert!(n.contains(&format!("relaunch started: {A},{B},{Z}")), "{n}");
    assert!(n.contains(&format!("not running again: {Z} (")), "{n}");
    assert!(n.contains(&format!("relaunch done; running again: {A},{B}; not running again: {Z} (held 60 s; .wslconfig keeps idle distributions)")), "{n}");
    let logged = wsl::restart_in(&n).unwrap();
    println!("   read back as easyNode could: {logged:?}");
    assert_eq!((logged.running_again.clone(), logged.not_running_again.clone()), (vec![A.to_string(), B.to_string()], vec![Z.to_string()]));
    assert!(logged.done.is_some());
    // This PC's .wslconfig keeps idle distributions: the brief hold ends, A and B run on.
    println!("   holders now: {:?}", holders(A));
    tokio::time::sleep(Duration::from_secs(wsl::HOLD_BRIEF + 15)).await;
    println!("   {} s later: holders {:?}; running {:?}", wsl::HOLD_BRIEF + 15, holders(A), running());
    assert!(holders(A).is_empty(), "the brief hold ended");
    assert!(is_running(A) && is_running(B) && !is_running(C));
    b.stop_engine().await.unwrap();

    // ── 3. Where WSL stops idle distributions (no .wslconfig under this
    //       USERPROFILE): the hold lasts until the distribution stops. ──
    let home = temp.join("home-no-wslconfig");
    std::fs::create_dir_all(&home).unwrap();
    let real_home = std::env::var("USERPROFILE").unwrap();
    set("USERPROFILE", Some(&home.to_string_lossy()));
    assert!(!wsl::keeps_idle());
    std::fs::write(temp.join(wsl::NOTE_LOG), "").unwrap();
    set("AINODE_FAKE_WSL_OTHERS", Some(&format!("{A},{B}")));
    wsl::fake::arm();
    assert_eq!(b.start_engine().await.unwrap_err(), wsl::WSL_STUCK_ASK);
    let r = b.restart_wsl(vec![A.into(), B.into()]).await;
    set("USERPROFILE", Some(&real_home));
    println!("3. click, no instanceIdleTimeout=-1: {r:?}; notes:\n{}", notes());
    assert!(r.is_ok());
    assert!(notes().contains(&format!("relaunch done; running again: {A},{B}; not running again: - (held until each stops)")), "{}", notes());
    assert_eq!(boots(A), 3);
    tokio::time::sleep(Duration::from_secs(wsl::HOLD_BRIEF + 15)).await;
    let (ha, hb) = (holders(A), holders(B));
    println!("   {} s later: holders A {ha:?}, B {hb:?}", wsl::HOLD_BRIEF + 15);
    // One hold shows as two wsl.exe with the same command line (wsl.exe starts a second one).
    assert!(!ha.is_empty() && !hb.is_empty(), "still held");
    // The hold ends with its distribution (WSL's next restart, or the PC's).
    wsl_exe(&["--terminate", A], None);
    tokio::time::sleep(Duration::from_secs(5)).await;
    println!("   A terminated: holders A {:?}", holders(A));
    assert!(holders(A).is_empty(), "the hold ended with A");
    b.stop_engine().await.unwrap();

    // ── 4. The user's own distributions were never stopped or started. ──
    set("AINODE_FAKE_WSL_STUCK", None);
    set("AINODE_FAKE_WSL_OTHERS", None);
    let user_after = user_running();
    println!("4. the user's distributions running: {user_after:?}");
    assert_eq!(user_after, user_before);

    // ── cleanup: everything but B (held, for the caller to see it outlive us). ──
    b.shutdown().await;
    for d in [A, C, distro.as_str()] {
        wsl_exe(&["--unregister", d], None);
    }
    let _ = std::fs::remove_dir_all(&data_dir);
    println!("done: {A}, {C}, {distro} unregistered; {B} left running and held: {:?}", holders(B));
}
