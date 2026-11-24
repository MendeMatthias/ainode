//! A BTX node on the same computer, seen from AInode (read-only, loopback).
//!
//! easyNode serves a small "witness" on 127.0.0.1:3081, on by default since
//! 2026-09-16 (easynode `btx_core::witness`), and one of its routes is the
//! Esplora-style `GET /blocks/tip/height`: the node's height as a bare number,
//! no password. AInode asks it only while Status is open, after consent, and
//! writes nothing. Reading the node's RPC instead would mean reading its
//! cookie, a password file AInode has no business opening.
//!
//! WSL2 forwards loopback both ways: on the Windows PC that runs easyNode in
//! WSL, the port answers from Windows (measured 2026-10-16) and from the
//! other distributions, so AInode for Windows and AInode for Linux in WSL both
//! see the node. Nothing listening (the usual case) costs one refused
//! connection.
//!
//! Windows only: when its graphics card is lost for 15 minutes, easyNode
//! restarts WSL (`wsl --shutdown`), which ends AInode's engine too. Its
//! restart script first appends a line to `%TEMP%\easynode-wsl-restart.log`
//! (easynode `btx_core::wsl::restart_wsl_script`), measured 8 s before the
//! shutdown on 2026-10-14:
//!   `2026-10-14T17:01:26Z easyNode lost its GPU block verifier; restarting WSL`
//! AInode reads the newest such line when its engine stops, so the home line
//! can say what happened instead of showing an engine error.

use std::time::Duration;

use serde::Serialize;

/// easyNode's witness (`WITNESS_ADDR` in easynode's state.rs).
pub const WITNESS_PORT: u16 = 3081;
const ASK_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Neighbour {
    /// The local BTX node's height, when one answers.
    pub node_height: Option<u64>,
    /// When easyNode last said it was restarting WSL (unix seconds), if
    /// within RESTART_NOTE_FRESH. Windows only.
    pub wsl_restart_at: Option<u64>,
}

/// easyNode's WSL comes back in about 11 s and the node in about a minute;
/// a note older than this has nothing to do with an engine stopping now.
pub const RESTART_NOTE_FRESH: u64 = 15 * 60;
#[cfg_attr(not(windows), allow(dead_code))]
const RESTART_LOG_NAME: &str = "easynode-wsl-restart.log";
const RESTART_MARK: &str = " easyNode lost its GPU block verifier; restarting WSL";

pub async fn look() -> Neighbour {
    Neighbour { node_height: node_height(WITNESS_PORT).await, wsl_restart_at: wsl_restart_at() }
}

/// Nothing: before consent AInode asks nothing.
pub fn nothing() -> Neighbour {
    Neighbour { node_height: None, wsl_restart_at: None }
}

#[cfg(windows)]
fn wsl_restart_at() -> Option<u64> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let log = std::env::temp_dir().join(RESTART_LOG_NAME);
    fresh_restart(&read_tail(&log, 16 * 1024)?, now)
}

#[cfg(not(windows))]
fn wsl_restart_at() -> Option<u64> {
    None
}

/// The last `max` bytes of a file that only ever grows by appended lines.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn read_tail(path: &std::path::Path, max: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(max))).ok()?;
    let mut b = Vec::new();
    f.take(max).read_to_end(&mut b).ok()?;
    Some(String::from_utf8_lossy(&b).into_owned())
}

/// The newest restart note in `text`, if it is at most RESTART_NOTE_FRESH old
/// (and not from the future, beyond a minute of clock skew).
#[cfg_attr(not(windows), allow(dead_code))]
fn fresh_restart(text: &str, now: u64) -> Option<u64> {
    let t = text.lines().rev().find_map(|l| l.trim_end().strip_suffix(RESTART_MARK).and_then(unix_of))?;
    (t <= now + 60 && now.saturating_sub(t) <= RESTART_NOTE_FRESH).then_some(t)
}

/// `2026-10-14T17:01:26Z` as unix seconds (PowerShell's sortable UTC form).
pub(crate) fn unix_of(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let (y, m) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + se).ok()
}

/// The reverse of `unix_of`: unix seconds as `2026-10-14T17:01:26Z`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn utc_stamp(t: u64) -> String {
    let (days, secs) = ((t / 86_400) as i64, t % 86_400);
    // Civil date from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
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

/// The WSL restart handshake (docs/plan-ainode-easynode.md, idea 1): while
/// easyNode restarts WSL, AInode waits and calls no wsl.exe at all (a call
/// then would start WSL's VM again under easyNode's feet, memory of
/// 2026-10-14), instead of counting failed starts. easyNode's script notes
/// "restarting WSL", shuts WSL down 3 s later and notes "relaunch started"
/// about 11 s after that (measured 2026-10-14: 17:01:26, :34, :43). AInode
/// waits until 15 s after the relaunch, or at most EASYNODE_RESTART_MAX after
/// the first note when no relaunch line comes (three failed attempts).
pub const EASYNODE_RESTART_MAX: u64 = 180;
const RELAUNCH_MARK: &str = " relaunch started, attempt ";

/// Until when (unix seconds) AInode waits for easyNode's restart, if one is
/// under way at `now`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn easynode_restart_until(text: &str, now: u64) -> Option<u64> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let i = lines.iter().rposition(|l| l.ends_with(RESTART_MARK))?;
    let began = unix_of(lines[i].strip_suffix(RESTART_MARK)?)?;
    let relaunched = lines[i + 1..].iter().find_map(|l| {
        let (stamp, rest) = l.split_once(' ')?;
        format!(" {rest}").starts_with(RELAUNCH_MARK).then(|| unix_of(stamp)).flatten()
    });
    let until = relaunched.map_or(began + EASYNODE_RESTART_MAX, |r| (r + 15).min(began + EASYNODE_RESTART_MAX));
    (began <= now + 60 && now < until).then_some(until)
}

/// Windows: until when easyNode's WSL restart lasts, if one is under way.
#[cfg(windows)]
pub fn easynode_restarting() -> Option<u64> {
    let log = std::env::temp_dir().join(RESTART_LOG_NAME);
    easynode_restart_until(&read_tail(&log, 16 * 1024)?, crate::state::now_unix())
}

/// Is a BTX node answering on this computer? For the chat's thread count.
pub async fn node_here() -> bool {
    node_height(WITNESS_PORT).await.is_some()
}

async fn node_height(port: u16) -> Option<u64> {
    match crate::chat::http(port, "GET", "/blocks/tip/height", None, None, ASK_TIMEOUT).await {
        Ok((200, body)) => height_of(&body),
        _ => None,
    }
}

/// A bare positive number and nothing else; anything else is not a node.
fn height_of(body: &str) -> Option<u64> {
    let t = body.trim();
    if t.is_empty() || t.len() > 12 || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse().ok().filter(|h| *h > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn easynodes_timestamps_read_as_unix_seconds() {
        assert_eq!(unix_of("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_of("2026-10-14T17:01:26Z"), Some(1_791_997_286));
        assert_eq!(unix_of("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        for bad in ["", "2026-10-14 17:01:26Z", "2026-10-14T17:01:26", "2026-13-14T17:01:26Z", "2026-10-14T25:01:26Z", "x026-10-14T17:01:26Z"] {
            assert_eq!(unix_of(bad), None, "{bad:?}");
        }
    }

    const CRLF: &str = "\r\n";

    #[test]
    fn only_a_fresh_restart_note_counts() {
        // The real log of 2026-10-14 on the PC that runs the signer.
        // PowerShell's Add-Content ends each line with CRLF.
        let log = [
            "2026-10-14T17:01:26Z easyNode lost its GPU block verifier; restarting WSL",
            "2026-10-14T17:01:34Z wsl --shutdown exited 0",
            "2026-10-14T17:01:43Z relaunch started, attempt 1",
        ]
        .map(|l| format!("{l}{CRLF}"))
        .concat();
        let log = log.as_str();
        let at = 1_791_997_286;
        assert_eq!(fresh_restart(log, at + 30), Some(at));
        assert_eq!(fresh_restart(log, at + RESTART_NOTE_FRESH), Some(at));
        assert_eq!(fresh_restart(log, at + RESTART_NOTE_FRESH + 1), None, "too old");
        assert_eq!(fresh_restart(log, at - 120), None, "from the future");
        assert_eq!(fresh_restart(&format!("2026-10-14T17:01:34Z wsl --shutdown exited 0{CRLF}"), at), None);
        assert_eq!(fresh_restart("", at), None);
        // The newest note wins.
        let two = format!("{log}2026-10-14T17:31:26Z easyNode lost its GPU block verifier; restarting WSL{CRLF}");
        assert_eq!(fresh_restart(&two, at + 1800 + 5), Some(at + 1800));
    }

    #[test]
    fn utc_stamps_round_trip() {
        for t in [0, 1_709_208_000, 1_791_997_286, 1_792_510_805, 4_102_444_799] {
            assert_eq!(unix_of(&utc_stamp(t)), Some(t), "{}", utc_stamp(t));
        }
        assert_eq!(utc_stamp(1_791_997_286), "2026-10-14T17:01:26Z");
    }

    #[test]
    fn ainode_waits_while_easynode_restarts_wsl() {
        let at = 1_791_997_286; // 2026-10-14T17:01:26Z, the real log below
        let began = format!("2026-10-14T17:01:26Z easyNode lost its GPU block verifier; restarting WSL{CRLF}");
        let shut = format!("{began}2026-10-14T17:01:34Z wsl --shutdown exited 0{CRLF}");
        let back = format!("{shut}2026-10-14T17:01:43Z relaunch started, attempt 1{CRLF}");
        // Under way: from the first note until 15 s after the relaunch.
        assert_eq!(easynode_restart_until(&began, at + 1), Some(at + EASYNODE_RESTART_MAX));
        assert_eq!(easynode_restart_until(&shut, at + 10), Some(at + EASYNODE_RESTART_MAX));
        assert_eq!(easynode_restart_until(&back, at + 20), Some(at + 17 + 15));
        assert_eq!(easynode_restart_until(&back, at + 17 + 15), None, "over");
        // No relaunch line (three failed attempts): at most EASYNODE_RESTART_MAX.
        assert_eq!(easynode_restart_until(&shut, at + EASYNODE_RESTART_MAX), None);
        // An old restart, or none, means nothing to wait for.
        assert_eq!(easynode_restart_until(&back, at + 3600), None);
        assert_eq!(easynode_restart_until("", at), None);
        // A newer restart after an older finished one counts on its own.
        let again = format!("{back}2026-10-14T18:00:00Z easyNode lost its GPU block verifier; restarting WSL{CRLF}");
        let t2 = at + 58 * 60 + 34;
        assert_eq!(easynode_restart_until(&again, t2 + 5), Some(t2 + EASYNODE_RESTART_MAX));
    }

    #[test]
    fn the_tail_is_the_end_of_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(RESTART_LOG_NAME);
        std::fs::write(&p, format!("{}tail", "x".repeat(100))).unwrap();
        assert_eq!(read_tail(&p, 4).as_deref(), Some("tail"));
        assert_eq!(read_tail(&p, 1000).map(|s| s.len()), Some(104));
        assert_eq!(read_tail(&tmp.path().join("none"), 4), None);
    }

    #[test]
    fn only_a_bare_height_counts() {
        assert_eq!(height_of("249862"), Some(249862));
        assert_eq!(height_of("249862\n"), Some(249862));
        for bad in ["", "0", "-1", "not found", "{\"tip\":1}", "12 34", "1234567890123"] {
            assert_eq!(height_of(bad), None, "{bad:?}");
        }
    }

    #[tokio::test]
    async fn reads_a_witness_and_nothing_else() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            for reply in ["HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n249862", "HTTP/1.1 404 Not Found\r\n\r\nnot found"] {
                let (mut c, _) = l.accept().await.unwrap();
                let mut buf = [0u8; 1024];
                let n = c.read(&mut buf).await.unwrap();
                assert!(String::from_utf8_lossy(&buf[..n]).starts_with("GET /blocks/tip/height HTTP/1.1\r\n"));
                c.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        assert_eq!(node_height(port).await, Some(249862));
        assert_eq!(node_height(port).await, None);
    }

    #[tokio::test]
    async fn no_node_is_none_and_quick() {
        // A port nothing listens on: taken, then released.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let t0 = std::time::Instant::now();
        assert_eq!(node_height(port).await, None);
        assert!(t0.elapsed() < ASK_TIMEOUT + Duration::from_millis(500));
    }
}
