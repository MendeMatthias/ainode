//! Windows: upstream's Linux helper runs inside AInode's own WSL distribution
//! (docs/windows.md, route 1, measured on 2026-10-13).
//!
//! - The distribution is named `AInode`, imported from Canonical's Ubuntu Base
//!   tarball shipped in the engine folder (`wsl/rootfs.tar.gz`, sha256 in
//!   MANIFEST.json) into `%LOCALAPPDATA%\AInode\data\wsl`, on the first Start
//!   after consent (rule 1: it is a write). 1.1 s, 172 MB. It never touches
//!   the user's own distributions, needs no Store and no apt.
//! - The engine runs from the install folder through `/mnt/c`, as
//!   `wsl.exe -d AInode -u ainode -- <engine>/ainode-wsl serve <sock> -- <args>`
//!   (app/wsl-host): one process for the engine's life, its stdin/stdout the
//!   RPC pipe (rpc.rs `Pipe`), 0.2 ms a call.
//! - The models live on the Windows drive, `%LOCALAPPDATA%\AInode\data\modelnet`
//!   (`/mnt/c/...` for the helper): plain Windows files, the space AInode shows
//!   is the space Explorer shows, and llama-server.exe reads them natively.
//! - Only those two folders are visible inside (security audit M1): no drive
//!   is mounted whole (`[automount] enabled=false`), `/etc/fstab` mounts the
//!   engine folder read-only and the models folder read-write and never
//!   executable, each at the `/mnt/...` path the app hands the engine. A bug
//!   in the engine, fed by a hostile host, cannot reach the user's other
//!   files (their Startup folder, their documents). Windows programs cannot
//!   be started from inside either (`[interop] enabled=false`). Checked
//!   before every start (`mounts`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use tokio::process::Command;

use crate::engine::{child_env, StartArgs};
use crate::state::Paths;

/// AInode's own distribution. Never any other. A debug build may name a
/// scratch one (`AINODE_WSL_DISTRO`), so a test never touches the installed
/// app's.
pub fn distro() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        crate::state::dev_env("AINODE_WSL_DISTRO").and_then(|v| v.into_string().ok()).unwrap_or_else(|| "AInode".into())
    })
}
/// The user the engine runs as inside it.
const USER: &str = "ainode";
/// The helper's socket inside the distribution (ainode-wsl makes its folder
/// 0700). Short, as upstream needs.
pub const SOCKET: &str = "/tmp/ainode/modeld.sock";
/// `CREATE_NO_WINDOW`: no console window flashes up for wsl.exe.
pub const NO_WINDOW: u32 = 0x0800_0000;

/// `C:\Users\u\x` as the helper sees it: `/mnt/c/Users/u/x`. Only local drive
/// paths (also `\\?\C:\...`); anything else has no `/mnt` path.
pub fn linux_path(p: &Path) -> Option<String> {
    let s = p.to_str()?;
    let s = s.strip_prefix(r"\\?\").unwrap_or(s);
    let mut chars = s.chars();
    let (drive, colon) = (chars.next()?, chars.next()?);
    if !drive.is_ascii_alphabetic() || colon != ':' {
        return None;
    }
    let rest = &s[2..];
    if !(rest.is_empty() || rest.starts_with('\\') || rest.starts_with('/')) {
        return None;
    }
    let rest = rest.replace('\\', "/");
    let rest = rest.trim_end_matches('/');
    Some(format!("/mnt/{}{}", drive.to_ascii_lowercase(), rest))
}

/// The reverse: `/mnt/c/Users/u/x` as Windows sees it, `C:\Users\u\x`.
pub fn windows_path(p: &str) -> Option<PathBuf> {
    let rest = p.strip_prefix("/mnt/")?;
    let mut parts = rest.splitn(2, '/');
    let drive = parts.next()?;
    if drive.len() != 1 || !drive.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let tail = parts.next().unwrap_or("").replace('/', "\\");
    Some(PathBuf::from(format!("{}:\\{tail}", drive.to_ascii_uppercase())))
}

/// The helper's argv with the paths it sees (engine.rs `helper_args`, the
/// contract's flags, in the same order).
pub fn helper_args(paths: &Paths, a: &StartArgs) -> Result<Vec<String>, String> {
    let modeldir = linux_path(&paths.modeldir())
        .ok_or_else(|| format!("AInode's folder must be on a local drive: {}", paths.modeldir().display()))?;
    let mut v = vec![
        format!("-modeldir={modeldir}"),
        format!("-modelstorage={}GiB", a.engine_gib),
        "-modelseed=manual".to_string(),
        "-modelfollowpeers=0".to_string(),
        format!("-modelrpcsocket={SOCKET}"),
    ];
    v.extend(a.peers.iter().map(|p| format!("-modelpeer={p}")));
    Ok(v)
}

fn wsl_exe() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join("wsl.exe")
}

/// Every wsl.exe this module runs: a fixed environment, no console window,
/// UTF-8 output (`WSL_UTF8`, else it is UTF-16 on most builds), and a process
/// that dies with the future that waited for it, so a timeout leaves nothing
/// behind.
fn wsl() -> Command {
    let mut c = Command::new(wsl_exe());
    c.env_clear().envs(child_env()).env("WSL_UTF8", "1").creation_flags(NO_WINDOW).stdin(std::process::Stdio::null()).kill_on_drop(true);
    c
}

/// A wsl.exe call with a ceiling.
async fn run(t: Duration, c: &mut Command) -> Result<std::process::Output, String> {
    tokio::time::timeout(t, c.output())
        .await
        .map_err(|_| "Windows' Linux support (WSL) did not answer".to_string())?
        .map_err(|e| format!("wsl.exe: {e}"))
}

/// `wsl.exe -d AInode -u ainode --cd ~ --exec <engine>/ainode-wsl serve <SOCKET> -- <args>`.
/// `--exec`, not `--`: `--` hands the line to the user's shell, which would
/// read a `'`, `$` or `&` in the profile path (`C:\Users\O'Brien`) as syntax;
/// `--exec` passes each argument as it is.
pub fn engine_command(paths: &Paths, a: &StartArgs) -> Result<Command, String> {
    let host = linux_path(&paths.engine_dir.join("ainode-wsl"))
        .ok_or_else(|| format!("AInode must be installed on a local drive: {}", paths.engine_dir.display()))?;
    let mut c = wsl();
    c.args(["-d", distro(), "-u", USER, "--cd", "~", "--exec"]).arg(host).args(["serve", SOCKET, "--"]).args(helper_args(paths, a)?);
    Ok(c)
}

/// `ainode-wsl clear <SOCKET>` inside the distribution: a crashed run's
/// leftovers are gone before the app pins the helper's peers into
/// catalog.json (a leftover helper asked to stop saves the file once more on
/// its way out, with the peers it knew). `serve` clears again and finds
/// nothing. A leftover that cannot be cleared is the reason the start fails.
pub async fn clear_leftovers(paths: &Paths) -> Result<(), String> {
    let host = linux_path(&paths.engine_dir.join("ainode-wsl"))
        .ok_or_else(|| format!("AInode must be installed on a local drive: {}", paths.engine_dir.display()))?;
    let out = run(Duration::from_secs(60), wsl().args(["-d", distro(), "-u", USER, "--cd", "/", "--exec"]).arg(host).args(["clear", SOCKET])).await?;
    if out.status.success() {
        return Ok(());
    }
    let why = decode(&out.stderr);
    if let Some(plain) = explain(&why) {
        return Err(plain.into());
    }
    let line = why.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").trim_start_matches("ainode-wsl: ").to_string();
    Err(format!("Could not clear a leftover engine: {line}"))
}

/// What `wsl.exe` prints is UTF-16LE on most Windows versions and UTF-8 on
/// some; either way, as text, without wsl.exe's notes about wsl.conf.
fn decode(b: &[u8]) -> String {
    // UTF-16LE text of ASCII names: every second byte is zero.
    let text = if b.len() >= 2 && b.len().is_multiple_of(2) && b.iter().skip(1).step_by(2).filter(|&&x| x == 0).count() * 2 >= b.len() / 2 {
        let (pairs, _) = b.as_chunks::<2>();
        let u: Vec<u16> = pairs.iter().map(|&c| u16::from_le_bytes(c)).collect();
        String::from_utf16_lossy(&u)
    } else {
        String::from_utf8_lossy(b).into_owned()
    };
    if !text.lines().any(is_wsl_note) {
        return text;
    }
    text.lines().filter(|l| !is_wsl_note(l)).map(|l| format!("{l}\n")).collect()
}

/// Is WSL installed at all? Windows 11 (and Windows 10 since 22H2) ships a
/// stub `wsl.exe` in System32 that only knows `--install`, `--update`,
/// `--status` and `--help` until the real WSL is installed. Its whole message
/// table (read from `System32\en-US\wsl.exe.mui`, 2026-10-14) is its usage
/// text and "Press any key to install Windows Subsystem for Linux ... This
/// prompt will time out in 60 seconds": it never says "not installed", and a
/// bare call waits a minute for a key. It finds the real WSL where this does:
/// the MSI package under `%ProgramFiles%\WSL` (also named by
/// `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Lxss\MSI`), or the Store
/// package's `wsl.exe` alias under `%LOCALAPPDATA%\Microsoft\WindowsApps`.
/// Older Windows 10 has WSL as an inbox optional component:
/// `System32\lxss\LxssManager.dll`.
fn installed() -> bool {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let mut places = vec![program_files.join("WSL").join("wsl.exe"), root.join("System32").join("lxss").join("LxssManager.dll")];
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        places.push(
            PathBuf::from(local)
                .join("Microsoft")
                .join("WindowsApps")
                .join("MicrosoftCorporationII.WindowsSubsystemForLinux_8wekyb3d8bbwe")
                .join("wsl.exe"),
        );
    }
    places.iter().any(|p| p.is_file())
}

/// The distributions WSL knows, or why WSL cannot be used here.
async fn distributions() -> Result<Vec<String>, String> {
    if !installed() {
        return Err(NO_WSL.into());
    }
    let out = tokio::time::timeout(Duration::from_secs(60), wsl().args(["--list", "--quiet"]).output())
        .await
        .map_err(|_| "Windows' Linux support (WSL) did not answer".to_string())?
        .map_err(|_| NO_WSL.to_string())?;
    listed(out.status.success(), &format!("{}{}", decode(&out.stdout), decode(&out.stderr)))
}

/// What `wsl --list --quiet` printed, read as the list of distributions, or
/// as the reason there is none to list.
fn listed(success: bool, text: &str) -> Result<Vec<String>, String> {
    if !success {
        let all = text.to_ascii_lowercase();
        // The real wsl.exe with nothing installed yet answers non-zero ("...
        // has no installed distributions", with an `--install` hint): an
        // empty list. WSL itself missing: the stub's usage text or its
        // install prompt (see `installed`), or, on some builds, "is not
        // installed" / "not enabled".
        if all.contains("not installed") || all.contains("not enabled") || all.contains("usage: wsl.exe") || all.contains("press any key") {
            return Err(NO_WSL.into());
        }
        return Ok(Vec::new());
    }
    Ok(text.lines().map(|l| l.trim().trim_start_matches('\u{feff}').to_string()).filter(|l| !l.is_empty()).collect())
}

/// Plain words for the one thing a user may have to do once.
pub const NO_WSL: &str = "AInode needs Windows' built-in Linux support (WSL) for its engine. \
Open PowerShell as administrator, run: wsl --install --no-distribution, then restart the PC and press Start again.";

/// The distribution exists and is set up; import it the first time. Called
/// by engine.rs before every start, so after consent only.
pub async fn ensure_distro(paths: &Paths) -> Result<(), String> {
    if distributions().await?.iter().any(|d| d == distro()) {
        // Registered, but is it set up, and from this release's Ubuntu Base?
        // A run killed between the import and the user setup, a deleted disk
        // image, or an import that outlived its timeout leaves a name that
        // `-u ainode` can never use. A release with a newer Ubuntu Base (its
        // security fixes; security audit L4) brings a new pin, and 0.8.2 and
        // earlier recorded none. The distribution holds nothing of the
        // user's (the engine keeps its identities, pins and models in the
        // models folder on the Windows drive), so it is made again.
        let want = rootfs_pin(&paths.engine_dir);
        // A distribution that does not start because WSL itself is stuck is
        // not broken: never unregister it then (only a WSL restart helps).
        match (imported_from().await?, &want) {
            (Some(have), Some(want)) if &have == want => return mounts(paths).await,
            (Some(_), None) => return mounts(paths).await,
            (Some(have), Some(_)) => eprintln!("[wsl] {} was made from Ubuntu Base {have:?}; importing this release's", distro()),
            (None, _) => eprintln!("[wsl] the {} distribution is registered but not usable; importing it again", distro()),
        }
        let _ = unregister().await;
    }
    let rootfs = paths.engine_dir.join("wsl").join("rootfs.tar.gz");
    if !rootfs.is_file() {
        return Err(format!("engine file missing: {}", rootfs.display()));
    }
    let dir = paths.data_dir.join("wsl");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let out = tokio::time::timeout(
        Duration::from_secs(300),
        wsl().arg("--import").arg(distro()).arg(&dir).arg(&rootfs).args(["--version", "2"]).output(),
    )
    .await
    .map_err(|_| "Setting up AInode's Linux engine took too long".to_string())?
    .map_err(|e| format!("wsl --import: {e}"))?;
    if !out.status.success() {
        let why = format!("{}{}", decode(&out.stdout), decode(&out.stderr));
        if let Some(plain) = explain(&why) {
            return Err(plain.into());
        }
        return Err(format!("Could not set up AInode's Linux engine: {}", why.trim()));
    }
    // A user for the engine and the pin this was made from; `mounts` then
    // writes wsl.conf and fstab and restarts the distribution. Sparse: the
    // disk gives space back.
    let setup = format!("id {USER} >/dev/null 2>&1 || useradd -m -s /bin/sh {USER}; printf '%s' \"$1\" > {PIN_FILE}");
    let pin = rootfs_pin(&paths.engine_dir).unwrap_or_default();
    let out = run(Duration::from_secs(120), wsl().args(["-d", distro(), "-u", "root", "--cd", "/", "--exec", "sh", "-c", &setup, "sh", &pin])).await?;
    if !out.status.success() {
        let _ = unregister().await;
        let why = decode(&out.stderr);
        return Err(explain(&why).map(str::to_string).unwrap_or_else(|| format!("Could not set up AInode's Linux engine: {}", why.trim())));
    }
    terminate().await;
    let _ = run(Duration::from_secs(60), wsl().args(["--manage", distro(), "--set-sparse", "true"])).await;
    mounts(paths).await
}

/// `/etc/wsl.conf`: the engine's user by default, no Windows programs and no
/// Windows PATH, no drive mounted whole: `/etc/fstab` says what is. No WSLg
/// (second security audit, 2026-10-19: its X11, Wayland and PulseAudio
/// sockets were open to the engine's user, a way to the desktop, the
/// clipboard and the microphone) and no GPU libraries or driver store (the
/// chat runs on Windows; the engine needs no GPU). Measured on WSL 2.7.10:
/// each key removes its mounts for this distribution alone.
pub const WSL_CONF: &str = "[user]\ndefault=ainode\n[interop]\nenabled=false\nappendWindowsPath=false\n[automount]\nenabled=false\nmountFsTab=true\n[general]\nguiApplications=false\n[gpu]\nenabled=false\n";

/// A line wsl.exe itself prints about the distribution's wsl.conf, such as
/// "wsl: Unknown key 'gpu.enabled' in /etc/wsl.conf:12" on a WSL older than
/// a key AInode writes. A note, never the error a user is shown.
pub fn is_wsl_note(line: &str) -> bool {
    let l = line.trim_start_matches('\u{feff}').trim();
    l.starts_with("wsl: ") && l.contains("/etc/wsl.conf")
}

/// `/etc/fstab`: the engine folder read-only, the models folder read-write
/// and never executable (what a host sends is data), owned by the engine's
/// user (uid 1000: the only user `useradd` made in Ubuntu Base).
pub fn fstab(paths: &Paths) -> Result<String, String> {
    let line = |win: &Path, opts: &str| -> Result<String, String> {
        let linux = linux_path(win).ok_or_else(|| format!("AInode's folders must be on a local drive: {}", win.display()))?;
        let src = win.to_str().ok_or_else(|| format!("not a Unicode path: {}", win.display()))?;
        let src = src.strip_prefix(r"\\?\").unwrap_or(src).trim_end_matches('\\');
        Ok(format!("{} {} drvfs {opts},uid=1000,gid=1000 0 0\n", fstab_escape(src), fstab_escape(&linux)))
    };
    Ok(format!(
        "# AInode: only the engine and the models (security audit M1).\n{}{}",
        line(&paths.engine_dir, "ro,nosuid,nodev,noatime")?,
        line(&paths.modeldir(), "rw,nosuid,nodev,noexec,noatime")?
    ))
}

/// fstab's escapes for the characters that would split a field.
fn fstab_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\\' => r"\134".to_string(),
            ' ' => r"\040".to_string(),
            '\t' => r"\011".to_string(),
            '\n' => r"\012".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// As root inside the distribution, with arguments (never shell text):
/// wsl.conf, fstab, the engine's and the models' paths inside, and the same
/// two as /proc/mounts writes them (`\040` for a space).
/// - wsl.conf or fstab not as wanted: write them, say "restart".
/// - A Windows drive mounted anywhere else (a distribution still running
///   with an older wsl.conf that mounted every drive): say "restart". The
///   mount points must not be made then: they would land on that drive.
/// - Mount points missing (the first boot after the switch finds none, and
///   WSL then repeats "Processing /etc/fstab with mount -a failed" on every
///   session of that boot, which would become the engine's error line):
///   make them, say "restart".
/// - Otherwise mount what fstab lists and say "mounted" when both are
///   (Ubuntu Base has neither `mountpoint` nor `findmnt`).
const MOUNTS_SCRIPT: &str = r#"set -e
restart=
[ "$(cat /etc/wsl.conf 2>/dev/null)" = "$(printf '%s' "$1")" ] || { printf '%s' "$1" > /etc/wsl.conf; restart=1; }
[ "$(cat /etc/fstab 2>/dev/null)" = "$(printf '%s' "$2")" ] || { printf '%s' "$2" > /etc/fstab; restart=1; }
ours='BEGIN { a = ARGV[1]; b = ARGV[2]; ARGV[1] = ""; ARGV[2] = "" }'
awk "$ours"' $1 ~ /^[A-Za-z]:/ && $2 != a && $2 != b { found = 1 } END { exit !found }' "$5" "$6" /proc/mounts && restart=1
if [ -n "$restart" ]; then echo restart; exit 0; fi
if [ ! -d "$3" ] || [ ! -d "$4" ]; then mkdir -p "$3" "$4"; echo restart; exit 0; fi
mount -a
if awk "$ours"' $2 == a { x = 1 } $2 == b { y = 1 } END { exit !(x && y) }' "$5" "$6" /proc/mounts; then echo mounted; fi
"#;

/// The distribution sees only the engine and the models folder (wsl.conf,
/// fstab), and both are mounted. A change, a drive still mounted whole (an
/// earlier AInode's distribution) or missing mount points restart the
/// distribution; not mounted after that is an error, so the engine never
/// writes models into the distribution's own disk.
pub async fn mounts(paths: &Paths) -> Result<(), String> {
    // A drvfs mount needs its Windows folder.
    std::fs::create_dir_all(paths.modeldir()).map_err(|e| format!("create {}: {e}", paths.modeldir().display()))?;
    let fstab = fstab(paths)?;
    let engine = linux_path(&paths.engine_dir).ok_or("AInode must be installed on a local drive")?;
    let models = linux_path(&paths.modeldir()).ok_or("AInode's folder must be on a local drive")?;
    let (engine_seen, models_seen) = (fstab_escape(&engine), fstab_escape(&models));
    let check = || async {
        if let Some(t) = fake::stuck(fake::At::Mounts) {
            return Err(explain(t).unwrap_or(WSL_STUCK).to_string());
        }
        let out = run(
            Duration::from_secs(60),
            wsl().args(["-d", distro(), "-u", "root", "--cd", "/", "--exec", "sh", "-c", MOUNTS_SCRIPT, "sh"]).args([
                WSL_CONF,
                &fstab,
                &engine,
                &models,
                &engine_seen,
                &models_seen,
            ]),
        )
        .await?;
        if !out.status.success() {
            let why = format!("{}{}", decode(&out.stdout), decode(&out.stderr));
            return Err(explain(&why).map(str::to_string).unwrap_or_else(|| format!("AInode's Linux engine could not see its folders: {}", why.trim())));
        }
        let said = decode(&out.stdout);
        Ok((said.contains("restart"), said.contains("mounted")))
    };
    // At most two restarts: the new wsl.conf and fstab, then the mount points.
    for _ in 0..3 {
        match check().await? {
            (_, true) => return Ok(()),
            (true, false) => terminate().await,
            (false, false) => break,
        }
    }
    Err(format!("AInode's Linux engine could not see its folders ({engine}, {models})"))
}

/// The distribution starts and has the engine's user.
/// Inside the distribution: the sha256 of the Ubuntu Base it was made from.
const PIN_FILE: &str = "/etc/ainode-rootfs";

/// The sha256 of this release's Ubuntu Base, from the engine's MANIFEST.json
/// (stage-engine-windows.sh); `None` for an engine folder without one (a
/// dev build), which then keeps whatever distribution it finds.
pub fn rootfs_pin(engine_dir: &Path) -> Option<String> {
    let raw = std::fs::read(engine_dir.join("MANIFEST.json")).ok()?;
    let m: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    m["files"].as_array()?.iter().find(|f| f["path"] == "wsl/rootfs.tar.gz")?["sha256"].as_str().map(str::to_string)
}

/// The distribution starts as the engine's user: the pin it was made from
/// (empty when it recorded none, 0.8.2 and earlier). `None` when it does
/// not start or has no engine user; an error when WSL itself is stuck
/// (no distribution can start), which says nothing about this one.
async fn imported_from() -> Result<Option<String>, String> {
    if let Some(t) = fake::stuck(fake::At::Start) {
        return Err(explain(t).unwrap_or(WSL_STUCK).into());
    }
    let script = format!("cat {PIN_FILE} 2>/dev/null; true");
    // No answer in a minute is WSL hanging (the CreateInstance timeout is
    // 30 s), never a reason to unregister: `probe` reads it the same way.
    let Ok(out) = run(Duration::from_secs(60), wsl().args(["-d", distro(), "-u", USER, "--cd", "/", "--exec", "sh", "-c", &script])).await else {
        return Err(WSL_STUCK.into());
    };
    if out.status.success() {
        return Ok(Some(decode(&out.stdout).trim().to_string()));
    }
    let why = format!("{}{}", decode(&out.stdout), decode(&out.stderr));
    if is_stuck(&why) {
        return Err(WSL_STUCK.into());
    }
    Ok(None)
}

/// Plain words for the WSL failures a user can do something about, when a
/// message carries one of their codes; the raw text otherwise says nothing
/// to anyone.
pub fn explain(text: &str) -> Option<&'static str> {
    let t = text.to_ascii_lowercase();
    if t.contains("0x80370102") || t.contains("virtualization") || t.contains("hypervisor") {
        return Some(NO_VIRTUALIZATION);
    }
    if t.contains("0x8007019e") || t.contains("not installed") || t.contains("not enabled") || t.contains("usage: wsl.exe") {
        return Some(NO_WSL);
    }
    // WSL itself stuck (2026-10-20, the PC, after a graphics driver reset
    // restarted WSL's virtual machine): every distribution's Windows drives
    // answer "Input/output error" and no distribution starts any more
    // ("Wsl/Service/CreateInstance/0x800705b4", a timeout). Only a restart
    // of WSL helps, which stops the user's other distributions too: AInode
    // says so and leaves it to the user.
    if t.contains("input/output error") || (t.contains("createinstance") && (t.contains("0x800705b4") || t.contains("timeout period expired"))) {
        return Some(WSL_STUCK);
    }
    None
}

pub const WSL_STUCK: &str = "Windows' Linux support (WSL) has lost its connection to this PC's drives. This can happen after a graphics driver reset or a Windows update. \
Restart WSL: open PowerShell and run wsl --shutdown (it also stops any other Linux you run), or restart the PC. Then press Retry.";

pub const NO_VIRTUALIZATION: &str = "Windows' Linux support needs virtualization, which is switched off in this PC's firmware (BIOS/UEFI). Switch it on there, then press Start again.";

// ── WSL stuck: restart it where that is safe (0.8.4) ────────────────────────
// 2026-10-20 on the PC: after a graphics driver reset every distribution's
// Windows drives answered "Input/output error" and no stopped distribution
// could start (CreateInstance 0x800705b4); only `wsl --shutdown` helped. That
// stops every Linux on the PC, so AInode does it by itself only when its own
// distribution is the only one running, and otherwise only on the user's
// click on a button that names what else stops (backend/wslheal.rs).

/// Does this error mean WSL itself is stuck? WSL's own texts, or the plain
/// words AInode already put in their place.
pub fn is_stuck(text: &str) -> bool {
    text.contains(WSL_STUCK) || explain(text) == Some(WSL_STUCK)
}

/// Shown with the button, when other distributions run.
pub const WSL_STUCK_ASK: &str = "Windows' Linux (WSL) has lost this PC's drives, as can happen after a graphics driver reset or a Windows update. Only a restart of it helps.";
/// After AInode's restart of WSL, the drives are still gone.
pub const WSL_STILL_STUCK: &str = "Windows' Linux (WSL) still cannot reach this PC's drives after a restart. Restart the PC, then open AInode again.";

/// What a quick look inside AInode's distribution says about WSL.
#[derive(Debug, Clone, PartialEq)]
pub enum Probe {
    /// The engine's two folders can be read.
    Fine,
    /// The distribution does not start, or its drives answer "Input/output error".
    Stuck,
    /// Something else went wrong (shown as it is).
    Other(String),
}

/// Read the engine's two folders inside AInode's distribution, as root. The
/// confirmation before AInode restarts WSL: a start that failed once with a
/// stuck WSL's error must fail the same way again.
pub async fn probe(paths: &Paths) -> Probe {
    if fake::stuck(fake::At::Probe).is_some() {
        // A slow probe, as a real one can take a minute (a test's easyNode
        // note can then arrive during it).
        if let Some(s) = crate::state::dev_env("AINODE_FAKE_WSL_PROBE_SECS").and_then(|v| v.into_string().ok()?.parse::<u64>().ok()) {
            tokio::time::sleep(Duration::from_secs(s)).await;
        }
        return Probe::Stuck;
    }
    let (Some(engine), Some(models)) = (linux_path(&paths.engine_dir), linux_path(&paths.modeldir())) else {
        return Probe::Other("AInode's folders must be on a local drive".into());
    };
    // `ls <dir>/` reads the folder itself: a dead drvfs mount answers EIO. A
    // folder not mounted yet reads as the empty mount point: fine.
    let out = run(
        Duration::from_secs(60),
        wsl().args(["-d", distro(), "-u", "root", "--cd", "/", "--exec", "sh", "-c", r#"ls -- "$1/" "$2/" >/dev/null"#, "sh", &engine, &models]),
    )
    .await;
    match out {
        // No answer in a minute: the CreateInstance timeout is 30 s.
        Err(_) => Probe::Stuck,
        Ok(o) if o.status.success() => Probe::Fine,
        Ok(o) => {
            let why = format!("{}{}", decode(&o.stdout), decode(&o.stderr));
            if is_stuck(&why) {
                Probe::Stuck
            } else {
                Probe::Other(why.trim().to_string())
            }
        }
    }
}

/// The distributions running now (`wsl --list --running --quiet`). It asks
/// WSL's service only and never starts a distribution.
pub async fn running() -> Result<Vec<String>, String> {
    if let Some(others) = fake::others() {
        return Ok(std::iter::once(distro().to_string()).chain(others).collect());
    }
    running_now().await
}

/// What WSL says runs now, never the test seam's list.
async fn running_now() -> Result<Vec<String>, String> {
    let out = run(Duration::from_secs(60), wsl().args(["--list", "--running", "--quiet"])).await?;
    let text = format!("{}{}", decode(&out.stdout), decode(&out.stderr));
    running_from(out.status.success(), &text)
}

/// What `wsl --list --running --quiet` printed: the names, or none ("There
/// are no running distributions.", a non-zero exit on WSL 2.x).
fn running_from(success: bool, text: &str) -> Result<Vec<String>, String> {
    if !success {
        if text.to_ascii_lowercase().contains("no running distributions") {
            return Ok(Vec::new());
        }
        return Err(format!("wsl --list --running: {}", text.trim()));
    }
    Ok(text.lines().map(|l| l.trim().trim_start_matches('\u{feff}').to_string()).filter(|l| !l.is_empty()).collect())
}

/// The running distributions that are not AInode's own.
pub fn others_of(running: &[String]) -> Vec<String> {
    running.iter().filter(|d| !d.eq_ignore_ascii_case(distro())).cloned().collect()
}

/// "Ubuntu", "Ubuntu and Debian", "Ubuntu, Debian and AInode-debian13".
pub fn names_text(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Restart WSL: `wsl --shutdown` (every distribution stops; the next wsl.exe
/// call starts the VM again). The caller decided it may (wslheal.rs). Every
/// step is noted in `%TEMP%\ainode-wsl-restart.log`, the way easyNode notes
/// its own restarts. A debug build with the test seam on stops only its own
/// scratch distribution instead and clears the injected fault.
///
/// `by_itself`: nobody clicked. Then the first note must be written, or no
/// restart happens: that note is what keeps AInode from restarting WSL by
/// itself more than once in 30 minutes (`last_restart`). `stops`: the other
/// distributions running now, which stop too (`start_again` brings them back).
pub async fn restart(why: &str, by_itself: bool, stops: &[String]) -> Result<(), String> {
    if !note(&format!("AInode found WSL stuck; restarting WSL ({why})")) && by_itself {
        return Err(WSL_STUCK.into());
    }
    note(&format!("{STOPS_MARK}{}", list_of(stops)));
    if fake::on() && !fake::real_restart() {
        terminate().await;
        // The test's own scratch distributions stop as WSL's restart stops them.
        let mut also = Vec::new();
        for d in stops.iter().filter(|d| fake::scratch(d)) {
            terminate_one(d).await;
            also.push(d.clone());
        }
        let fixed = fake::restarted();
        note(&format!(
            "(test seam) {} terminated instead of wsl --shutdown (also: {}); fault {}",
            distro(),
            list_of(&also),
            if fixed { "cleared" } else { "kept" }
        ));
        return Ok(());
    }
    let out = run(Duration::from_secs(120), wsl().arg("--shutdown")).await;
    match out {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            note(&format!("wsl --shutdown exited {code}"));
            if o.status.success() {
                // A test's injected fault ends with a real restart too.
                if fake::on() {
                    fake::restarted();
                }
                Ok(())
            } else {
                Err(format!("wsl --shutdown: {}", format!("{}{}", decode(&o.stdout), decode(&o.stderr)).trim()))
            }
        }
        Err(e) => {
            note(&format!("wsl --shutdown: {e}"));
            Err(e)
        }
    }
}

/// AInode's own restart log beside easyNode's (`%TEMP%`).
pub const NOTE_LOG: &str = "ainode-wsl-restart.log";
pub const RESTART_MARK: &str = " AInode found WSL stuck; restarting WSL";
/// The rest of a restart in the log, for a reader such as easyNode
/// (docs/plan-ainode-easynode.md, idea 1 the other way round, `restart_in`):
/// each line is `<UTC stamp> <text>`; names are comma separated (WSL allows
/// no comma or space in a name), `-` for none; `stops` are the other
/// distributions, besides AInode's own.
///
/// ```text
/// 2026-10-21T15:20:00Z AInode found WSL stuck; restarting WSL (asked; also stops: Ubuntu and Kali)
/// 2026-10-21T15:20:00Z stops: Ubuntu,Kali
/// 2026-10-21T15:20:08Z wsl --shutdown exited 0
/// 2026-10-21T15:20:11Z relaunch started: Ubuntu,Kali
/// 2026-10-21T15:20:41Z not running again: Kali (what wsl.exe said)
/// 2026-10-21T15:20:41Z relaunch done; running again: Ubuntu; not running again: Kali
/// ```
///
/// A restart is under way from its first line until `relaunch done`; one
/// that failed (`wsl --shutdown` did not run) writes no `relaunch` line.
pub const STOPS_MARK: &str = "stops: ";
pub const RELAUNCH_MARK: &str = "relaunch started: ";
pub const NOT_AGAIN_MARK: &str = "not running again: ";
pub const DONE_MARK: &str = "relaunch done; running again: ";
const DONE_NOT: &str = "; not running again: ";

/// Append a line to AInode's restart log; false when it could not be written.
fn note(m: &str) -> bool {
    use std::io::Write;
    let line = format!("{} {m}\r\n", crate::neighbour::utc_stamp(crate::state::now_unix()));
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join(NOTE_LOG))
        .and_then(|mut f| f.write_all(line.as_bytes()))
        .is_ok()
}

/// When AInode last restarted WSL by itself (unix seconds), from its log.
pub fn last_restart() -> Option<u64> {
    let text = crate::neighbour::read_tail(&std::env::temp_dir().join(NOTE_LOG), 16 * 1024)?;
    last_restart_in(&text)
}

fn last_restart_in(text: &str) -> Option<u64> {
    text.lines().rev().find_map(|l| {
        let (stamp, rest) = l.split_once(' ')?;
        format!(" {rest}").starts_with(RESTART_MARK).then(|| crate::neighbour::unix_of(stamp)).flatten()
    })
}

/// `Ubuntu,AInode-debian13`, or `-` for none.
fn list_of(names: &[String]) -> String {
    if names.is_empty() {
        "-".into()
    } else {
        names.join(",")
    }
}

fn names_in(list: &str) -> Vec<String> {
    list.split(',').map(str::trim).filter(|s| !s.is_empty() && *s != "-").map(str::to_string).collect()
}

/// One restart as AInode's log tells it.
#[derive(Debug, Clone, PartialEq)]
pub struct LoggedRestart {
    /// Its first line (unix seconds).
    pub began: u64,
    /// The other distributions it stopped.
    pub stops: Vec<String>,
    /// When it was over (`relaunch done`), if it is.
    pub done: Option<u64>,
    pub running_again: Vec<String>,
    pub not_running_again: Vec<String>,
}

/// The newest restart in AInode's log text: what a reader such as easyNode
/// needs (is one under way, did it stop my distribution, is it back).
pub fn restart_in(text: &str) -> Option<LoggedRestart> {
    let lines: Vec<(&str, &str)> = text.lines().filter_map(|l| l.trim_end().split_once(' ')).collect();
    let i = lines.iter().rposition(|(_, rest)| format!(" {rest}").starts_with(RESTART_MARK))?;
    let began = crate::neighbour::unix_of(lines[i].0)?;
    let mut r = LoggedRestart { began, stops: Vec::new(), done: None, running_again: Vec::new(), not_running_again: Vec::new() };
    for (stamp, rest) in &lines[i + 1..] {
        if let Some(l) = rest.strip_prefix(STOPS_MARK) {
            r.stops = names_in(l);
        } else if let Some(l) = rest.strip_prefix(DONE_MARK) {
            let (back, not) = l.split_once(DONE_NOT).unwrap_or((l, "-"));
            r.running_again = names_in(back);
            r.not_running_again = names_in(not.split(" (").next().unwrap_or(not));
            r.done = crate::neighbour::unix_of(stamp);
        }
    }
    Some(r)
}

// ── After a restart on the user's click: start again what it stopped ───────
// 2026-10-20 on the PC (lab/evidence/2026-10-20/track-w-084/real-wsl-restart/):
// AInode was back 15 s after the click, but the BTX node's Ubuntu came back
// only when easyNode's boot task was started by hand, and AInode-debian13
// (the Linux AInode) only by hand. So each distribution the restart stopped
// is started again the way a boot starts it: a plain `wsl.exe -d <name>`,
// which lets its boot hook run (`[boot] command` in its wsl.conf, easyNode's
// miner-boot.sh), held by a sleeping wsl.exe as easyNode's boot task holds
// Ubuntu. Only those: never a distribution that was not running before.
//
// What holds a distribution open (the PC, 2026-10-13 and 2026-10-21): WSL
// stops a distribution about 15 s after the last wsl.exe attached to it
// ends, whatever runs inside (`instanceIdleTimeout`; a `sleep infinity`
// inside does not help), unless `%USERPROFILE%\.wslconfig` says `[general]
// instanceIdleTimeout=-1`, as on the PC since 2026-10-13 (a scratch
// distribution held 5 s still ran a minute later, its boot hook run). So
// with -1 a brief hold covers the start; otherwise the hold lasts until the
// distribution stops (the next WSL restart, or the PC's), and outlives
// AInode: a BTX node must not stop 15 s after the user quits AInode.

/// The brief hold, when WSL keeps idle distributions: the boot hook starts
/// meanwhile (easyNode's starts the node 25 s in).
pub const HOLD_BRIEF: u64 = 60;
/// A start that does not show as running within this long failed
/// (CreateInstance gives up after 30 s).
pub const START_WAIT: Duration = Duration::from_secs(60);

/// Does WSL keep a distribution running with nothing attached?
pub fn keeps_idle() -> bool {
    let Some(home) = std::env::var_os("USERPROFILE") else { return false };
    std::fs::read_to_string(PathBuf::from(home).join(".wslconfig")).is_ok_and(|t| keeps_idle_in(&t))
}

/// Only `instanceIdleTimeout=-1` under `[general]`, spelled as WSL reads
/// it; any other value, place or spelling means WSL may stop an idle
/// distribution, and the hold then lasts.
fn keeps_idle_in(wslconfig: &str) -> bool {
    let mut general = false;
    let mut keeps = false;
    for l in wslconfig.lines() {
        let l = l.trim().trim_start_matches('\u{feff}');
        if l.starts_with('[') {
            general = l == "[general]";
        } else if let (true, Some((k, v))) = (general, l.split_once('=')) {
            if k.trim() == "instanceIdleTimeout" {
                keeps = v.split(['#', ';']).next().unwrap_or("").trim() == "-1";
            }
        }
    }
    keeps
}

/// A name as WSL makes them: letters, digits, `.`, `_`, `-` (nothing a file
/// name or the log could misread).
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Inside, as the distribution's default user: sleep `$1` seconds, or with
/// 0 until the distribution stops.
const HOLD: &str = r#"if [ "$1" -gt 0 ]; then exec sleep "$1"; fi; while :; do sleep 3600; done"#;

/// A distribution being started again: its holding wsl.exe and its output.
struct Held {
    name: String,
    child: Option<std::process::Child>,
    out: PathBuf,
    error: Option<String>,
}

/// Start `name` the way a boot does and hold it (`secs`, 0 for until it
/// stops). The wsl.exe is AInode's child but not tied to it: it goes on
/// after AInode quits (no kill on drop, out of any job when Windows allows).
fn hold(name: &str, secs: u64) -> Held {
    use std::os::windows::process::CommandExt;
    let out = std::env::temp_dir().join(format!("ainode-wsl-start-{name}.txt"));
    let mut held = Held { name: name.to_string(), child: None, out: out.clone(), error: None };
    if !plain_name(name) {
        held.error = Some("not a name WSL gives".into());
        return held;
    }
    const BREAKAWAY: u32 = 0x0100_0000; // CREATE_BREAKAWAY_FROM_JOB
    let spawn = |flags: u32| -> std::io::Result<std::process::Child> {
        let f = std::fs::File::create(&out)?;
        std::process::Command::new(wsl_exe())
            .env_clear()
            .envs(child_env())
            .env("WSL_UTF8", "1")
            .creation_flags(NO_WINDOW | flags)
            .stdin(std::process::Stdio::null())
            .stdout(f.try_clone()?)
            .stderr(f)
            .args(["-d", name, "--cd", "/", "--exec", "sh", "-c", HOLD, "sh", &secs.to_string()])
            .spawn()
    };
    // A job that allows no breakaway refuses the flag: then a plain child.
    match spawn(BREAKAWAY).or_else(|_| spawn(0)) {
        Ok(c) => held.child = Some(c),
        Err(e) => held.error = Some(format!("wsl.exe: {e}")),
    }
    held
}

/// What starting again came to: the names running again, and the others
/// with what wsl.exe said.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StartedAgain {
    pub running: Vec<String>,
    pub not_running: Vec<(String, String)>,
}

/// After a restart: start each of `stopped` again (the other distributions
/// it stopped, nothing else) and wait until each runs or failed; every step
/// in the log. Under the test seam only the test's scratch distributions
/// (the only ones its restart stopped).
pub async fn start_again(stopped: &[String]) -> StartedAgain {
    let seam = fake::on() && !fake::real_restart();
    let (names, skipped): (Vec<String>, Vec<String>) = stopped.iter().cloned().partition(|d| !seam || fake::scratch(d));
    if !skipped.is_empty() {
        note(&format!("(test seam) not started, not a scratch distribution: {}", list_of(&skipped)));
    }
    let secs = if keeps_idle() { HOLD_BRIEF } else { 0 };
    note(&format!("{RELAUNCH_MARK}{}", list_of(&names)));
    let held: Vec<Held> = names.iter().map(|n| hold(n, secs)).collect();
    let r = settle(held, START_WAIT).await;
    for (n, why) in &r.not_running {
        let why: Vec<&str> = why.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        note(&format!("{NOT_AGAIN_MARK}{n} ({})", why.join(" ")));
    }
    let not: Vec<String> = r.not_running.iter().map(|(n, _)| n.clone()).collect();
    let held_for = match (r.running.is_empty(), secs) {
        (true, _) => String::new(),
        (false, 0) => " (held until each stops)".into(),
        (false, s) => format!(" (held {s} s; .wslconfig keeps idle distributions)"),
    };
    note(&format!("{DONE_MARK}{}{DONE_NOT}{}{held_for}", list_of(&r.running), list_of(&not)));
    r
}

/// Wait until each held distribution shows as running, or its wsl.exe ended
/// first (it could not start), or `wait` passed.
async fn settle(held: Vec<Held>, wait: Duration) -> StartedAgain {
    let mut r = StartedAgain::default();
    let mut pending = Vec::new();
    for mut h in held {
        match h.error.take() {
            Some(e) => r.not_running.push((h.name, e)),
            None => pending.push(h),
        }
    }
    let until = tokio::time::Instant::now() + wait;
    while !pending.is_empty() {
        let now = running_now().await.unwrap_or_default();
        let late = tokio::time::Instant::now() >= until;
        let mut left = Vec::new();
        for mut h in pending {
            let ended = h.child.as_mut().and_then(|c| c.try_wait().ok().flatten());
            if now.iter().any(|d| d.eq_ignore_ascii_case(&h.name)) {
                // Dropping the handle leaves the hold running.
                r.running.push(h.name);
            } else if let Some(status) = ended {
                let said = std::fs::read(&h.out).map(|b| decode(&b)).unwrap_or_default();
                let said = if said.trim().is_empty() { format!("wsl.exe ended with {}", status.code().unwrap_or(-1)) } else { said };
                r.not_running.push((h.name, said.trim().to_string()));
            } else if late {
                r.not_running.push((h.name, format!("not running {} s after its start", wait.as_secs())));
            } else {
                left.push(h);
            }
        }
        pending = left;
        if !pending.is_empty() {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
    r
}

/// The test seam (debug builds only, `state::dev_env`): a stuck WSL cannot
/// be made safely, so a test injects its errors, as WSL printed them on
/// 2026-10-20. `AINODE_FAKE_WSL_STUCK`:
/// - `io`: the mounts answer "Input/output error" (AInode's distribution
///   still running, as on 2026-10-20);
/// - `create`: no distribution starts (CreateInstance 0x800705b4);
/// - `io-sticky`, `create-sticky`: the same, and a restart does not help.
///
/// `AINODE_FAKE_WSL_OTHERS`: the other distributions "running", comma
/// separated (`-` for none), instead of asking WSL. With the seam on, a
/// restart stops only the scratch distribution (`AINODE_WSL_DISTRO`), never
/// WSL; it refuses to run against `AInode`.
pub mod fake {
    use std::sync::atomic::{AtomicBool, Ordering};

    pub const IO: &str = "mkdir: cannot stat '/mnt/c/x': Input/output error";
    pub const CREATE: &str = "This operation returned because the timeout period expired. \r\nError code: Wsl/Service/CreateInstance/0x800705b4";

    #[derive(Clone, Copy, PartialEq)]
    pub enum At {
        Start,
        Mounts,
        Probe,
    }

    static CLEARED: AtomicBool = AtomicBool::new(false);

    fn mode() -> Option<String> {
        let m = crate::state::dev_env("AINODE_FAKE_WSL_STUCK")?.into_string().ok()?;
        assert_ne!(super::distro(), "AInode", "the WSL test seam needs a scratch distribution (AINODE_WSL_DISTRO)");
        Some(m)
    }

    pub fn on() -> bool {
        mode().is_some()
    }

    /// `AINODE_FAKE_WSL_REAL_RESTART=1` (debug builds, with the seam on): the
    /// stuck WSL is injected, but the restart is the real `wsl --shutdown`,
    /// the release code path, for a test the owner agreed to (it stops every
    /// Linux on the PC). The injected fault ends with it.
    pub fn real_restart() -> bool {
        on() && crate::state::dev_env("AINODE_FAKE_WSL_REAL_RESTART").is_some()
    }

    /// The injected error at this point, while the fault lasts.
    pub fn stuck(at: At) -> Option<&'static str> {
        let m = mode()?;
        if CLEARED.load(Ordering::SeqCst) {
            return None;
        }
        match (m.trim_end_matches("-sticky"), at) {
            ("io", At::Mounts | At::Probe) => Some(IO),
            ("create", _) => Some(CREATE),
            _ => None,
        }
    }

    /// The fault is on again (a test's next case).
    pub fn arm() {
        CLEARED.store(false, Ordering::SeqCst);
    }

    /// A restart happened: the fault ends unless it is sticky. True when it ended.
    pub fn restarted() -> bool {
        let sticky = mode().is_some_and(|m| m.ends_with("-sticky"));
        if !sticky {
            CLEARED.store(true, Ordering::SeqCst);
        }
        !sticky
    }

    /// A test's own scratch distribution (`AInode-t...`): with the seam on, a
    /// "restart" stops these among the others and starts them again; any
    /// other name in the fake list (Ubuntu) is never touched.
    pub fn scratch(name: &str) -> bool {
        name.starts_with("AInode-t")
    }

    /// Only with the seam on: a "restart" then stops the scratch distribution
    /// alone, so a fake list can never decide a real `wsl --shutdown`.
    pub fn others() -> Option<Vec<String>> {
        if !on() {
            return None;
        }
        let v = crate::state::dev_env("AINODE_FAKE_WSL_OTHERS")?.into_string().ok()?;
        Some(v.split(',').map(str::trim).filter(|s| !s.is_empty() && *s != "-").map(str::to_string).collect())
    }
}

/// Stop AInode's distribution (only ours), so WSL can give its memory back
/// once the app quits.
pub async fn terminate() {
    terminate_one(distro()).await;
}

async fn terminate_one(name: &str) {
    let _ = tokio::time::timeout(Duration::from_secs(20), wsl().args(["--terminate", name]).output()).await;
}

async fn unregister() -> Result<(), String> {
    run(Duration::from_secs(60), wsl().args(["--unregister", distro()])).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_notes_about_wsl_conf_are_never_the_error_line() {
        let note = "wsl: Unknown key 'gpu.enabled' in /etc/wsl.conf:12";
        assert!(is_wsl_note(note) && is_wsl_note(&format!("\u{feff}{note}\r")));
        assert!(!is_wsl_note("wsl: the distribution AInode was not found") && !is_wsl_note("ainode-wsl: no socket"));
        let utf16: Vec<u8> = format!("{note}\r\nreal error\r\n").encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode(&utf16).lines().map(str::trim).find(|l| !l.is_empty()), Some("real error"));
        assert_eq!(decode(b"plain\n"), "plain\n");
    }

    #[test]
    fn windows_paths_map_to_mnt_and_back() {
        assert_eq!(linux_path(Path::new(r"C:\Users\Jo Doe\AppData\Local\AInode\modelnet")).as_deref(), Some("/mnt/c/Users/Jo Doe/AppData/Local/AInode/modelnet"));
        assert_eq!(linux_path(Path::new(r"\\?\D:\x\y\")).as_deref(), Some("/mnt/d/x/y"));
        assert_eq!(linux_path(Path::new(r"C:\")).as_deref(), Some("/mnt/c"));
        assert_eq!(linux_path(Path::new(r"\\server\share\x")), None);
        assert_eq!(linux_path(Path::new(r"C:relative")), None);
        assert_eq!(windows_path("/mnt/c/Users/Jo Doe/m/x.gguf"), Some(PathBuf::from(r"C:\Users\Jo Doe\m\x.gguf")));
        assert_eq!(windows_path("/home/ainode/x"), None);
        assert_eq!(windows_path("/mnt/wsl/x"), None);
    }

    #[test]
    fn the_stubs_usage_text_and_prompt_mean_no_wsl() {
        // The inbox stub's message table (System32\en-US\wsl.exe.mui, Windows 11 24H2).
        let usage = "Windows Subsystem for Linux\r\nCopyright (c) Microsoft Corporation. All rights reserved.\r\n\r\nUsage: wsl.exe [Argument]\r\n\r\nArguments:\r\n\r\n    --install\r\n        Install Windows Subsystem for Linux. If no options are specified,\r\n        the recommended features will be installed along with the default distribution.\r\n";
        assert_eq!(listed(false, usage), Err(NO_WSL.into()));
        assert_eq!(listed(false, "Press any key to install Windows Subsystem for Linux.\r\nPress CTRL-C or close this window to cancel.\r\n"), Err(NO_WSL.into()));
        assert_eq!(listed(false, "The Windows Subsystem for Linux is not installed. You can install by running 'wsl.exe --install'."), Err(NO_WSL.into()));
        // The real wsl.exe, nothing imported yet: an empty list, and the import goes ahead.
        assert_eq!(
            listed(false, "Windows Subsystem for Linux has no installed distributions.\r\nDistributions can be installed by visiting the Microsoft Store:\r\nhttps://aka.ms/wslstore\r\n"),
            Ok(Vec::new())
        );
        assert_eq!(listed(true, "\u{feff}Ubuntu\r\nAInode\r\n\r\n"), Ok(vec!["Ubuntu".to_string(), "AInode".to_string()]));
        // This PC has WSL (the tests run the engine in it).
        assert!(installed());
    }

    #[test]
    fn wsl_failure_codes_get_plain_words() {
        assert_eq!(explain("Wsl/Service/CreateInstance/CreateVm/HCS/HCS_E_HYPERVISOR_NOT_PRESENT 0x80370102"), Some(NO_VIRTUALIZATION));
        assert_eq!(explain("Error code: Wsl/InstallDistro/WSL_E_WSL_OPTIONAL_COMPONENT_REQUIRED 0x8007019e"), Some(NO_WSL));
        assert_eq!(explain("The engine exited with code 1"), None);
        // Both as seen on the PC on 2026-10-20, WSL stuck after a graphics driver reset.
        assert_eq!(
            explain("mkdir: cannot stat '/mnt/c/Users/user/AppData/Local/AInode/resources/engine': Input/output error mkdir: cannot stat '/mnt/d/AInode/modelnet': Input/output error"),
            Some(WSL_STUCK)
        );
        assert_eq!(
            explain("This operation returned because the timeout period expired. \r\nError code: Wsl/Service/CreateInstance/0x800705b4"),
            Some(WSL_STUCK)
        );
    }

    #[test]
    fn a_stuck_wsl_is_recognised_and_what_runs_is_named() {
        assert!(is_stuck(fake::IO) && is_stuck(fake::CREATE) && is_stuck(WSL_STUCK));
        assert!(!is_stuck("The engine exited with code 1") && !is_stuck(NO_VIRTUALIZATION));
        // `wsl --list --running --quiet`, as WSL 2.7.10 prints it (UTF-8 with WSL_UTF8=1).
        assert_eq!(running_from(true, "Ubuntu\r\nAInode\r\nAInode-debian13\r\n"), Ok(vec!["Ubuntu".into(), "AInode".into(), "AInode-debian13".into()]));
        assert_eq!(running_from(false, "There are no running distributions.\r\n"), Ok(vec![]));
        assert!(running_from(false, "Error code: Wsl/Service/0x8000ffff").is_err());
        let all: Vec<String> = ["Ubuntu", "ainode", "AInode-debian13"].map(String::from).to_vec();
        if distro() == "AInode" {
            assert_eq!(others_of(&all), vec!["Ubuntu".to_string(), "AInode-debian13".to_string()], "only AInode's own is not another");
        }
        assert_eq!(names_text(&[]), "");
        assert_eq!(names_text(&all[..1]), "Ubuntu");
        assert_eq!(names_text(&["Ubuntu".into(), "Debian".into()]), "Ubuntu and Debian");
        assert_eq!(names_text(&all), "Ubuntu, ainode and AInode-debian13");
    }

    #[test]
    fn ainodes_own_restart_note_is_read_back() {
        let log = "2026-10-20T13:20:00Z AInode found WSL stuck; restarting WSL (alone)\r\n2026-10-20T13:20:04Z wsl --shutdown exited 0\r\n";
        assert_eq!(last_restart_in(log), crate::neighbour::unix_of("2026-10-20T13:20:00Z"));
        assert_eq!(last_restart_in("2026-10-20T13:20:04Z wsl --shutdown exited 0\r\n"), None);
        assert_eq!(last_restart_in(""), None);
    }

    #[test]
    fn a_reader_such_as_easynode_finds_what_a_restart_stopped_and_started_again() {
        let began = "2026-10-21T15:20:00Z AInode found WSL stuck; restarting WSL (asked; also stops: Ubuntu and Kali)\r\n";
        let under_way = format!("{began}2026-10-21T15:20:00Z stops: Ubuntu,Kali\r\n2026-10-21T15:20:08Z wsl --shutdown exited 0\r\n2026-10-21T15:20:11Z relaunch started: Ubuntu,Kali\r\n");
        let r = restart_in(&under_way).unwrap();
        assert_eq!(r.began, crate::neighbour::unix_of("2026-10-21T15:20:00Z").unwrap());
        assert_eq!((r.stops.clone(), r.done), (vec!["Ubuntu".to_string(), "Kali".to_string()], None), "under way");
        let done = format!(
            "{under_way}2026-10-21T15:20:41Z not running again: Kali (There is no distribution with the supplied name.)\r\n2026-10-21T15:20:41Z relaunch done; running again: Ubuntu; not running again: Kali (held until each stops)\r\n"
        );
        let r = restart_in(&done).unwrap();
        assert_eq!(r.done, crate::neighbour::unix_of("2026-10-21T15:20:41Z"));
        assert_eq!((r.running_again, r.not_running_again), (vec!["Ubuntu".to_string()], vec!["Kali".to_string()]));
        // AInode alone (nothing else ran), and the newest restart wins.
        let alone = format!(
            "{done}2026-10-21T18:00:00Z AInode found WSL stuck; restarting WSL (alone)\r\n2026-10-21T18:00:00Z stops: -\r\n2026-10-21T18:00:06Z relaunch started: -\r\n2026-10-21T18:00:06Z relaunch done; running again: -; not running again: -\r\n"
        );
        let r = restart_in(&alone).unwrap();
        assert!(r.stops.is_empty() && r.running_again.is_empty() && r.not_running_again.is_empty() && r.done.is_some());
        // The 0.8.4 log (no such lines yet) still reads: a restart, nothing more known.
        let old = restart_in("2026-10-20T19:29:47Z AInode found WSL stuck; restarting WSL (asked; also stops: Ubuntu)\r\n2026-10-20T19:29:55Z wsl --shutdown exited 0\r\n").unwrap();
        assert!(old.stops.is_empty() && old.done.is_none());
        assert_eq!(restart_in(""), None);
        assert_eq!(last_restart_in(&alone), crate::neighbour::unix_of("2026-10-21T18:00:00Z"));
    }

    #[test]
    fn only_minus_one_under_general_keeps_an_idle_distribution() {
        // This PC's .wslconfig since 2026-10-13.
        let pc = "[general]\r\n# Never stop a distro for being idle.\r\ninstanceIdleTimeout=-1\r\n\r\n[wsl2]\r\nnetworkingMode=mirrored\r\nvmIdleTimeout=-1\r\nmemory=16GB\r\n";
        assert!(keeps_idle_in(pc));
        assert!(keeps_idle_in("\u{feff}[general]\ninstanceIdleTimeout = -1 # forever\n"));
        for no in [
            "",
            "[wsl2]\nvmIdleTimeout=-1\n",
            "[wsl2]\ninstanceIdleTimeout=-1\n",
            "[general]\ninstanceIdleTimeout=60000\n",
            "[general]\ninstanceidletimeout=-1\n",
            "[general]\n# instanceIdleTimeout=-1\n",
            "[general]\ninstanceIdleTimeout=-1\ninstanceIdleTimeout=15000\n",
        ] {
            assert!(!keeps_idle_in(no), "{no:?}");
        }
    }

    #[test]
    fn only_plain_names_are_started_and_logged() {
        for ok in ["Ubuntu", "Ubuntu-24.04", "AInode-debian13", "kali_linux"] {
            assert!(plain_name(ok), "{ok}");
        }
        for no in ["", "a b", "a,b", "a;rm", "..\\x", &"x".repeat(65)] {
            assert!(!plain_name(no), "{no:?}");
        }
        assert_eq!(list_of(&[]), "-");
        assert_eq!(names_in(&list_of(&["Ubuntu".into(), "Kali".into()])), vec!["Ubuntu".to_string(), "Kali".to_string()]);
        assert!(names_in("-").is_empty());
        assert!(fake::scratch("AInode-t085a") && !fake::scratch("AInode") && !fake::scratch("AInode-debian13") && !fake::scratch("Ubuntu"));
    }

    #[test]
    fn wsl_output_is_read_in_either_encoding() {
        let utf16: Vec<u8> = "Ubuntu\r\nAInode\r\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode(&utf16), "Ubuntu\r\nAInode\r\n");
        assert_eq!(decode(b"Ubuntu\nAInode\n"), "Ubuntu\nAInode\n");
    }

    #[test]
    fn only_the_engine_and_the_models_are_mounted() {
        let paths = Paths {
            data_dir: PathBuf::from(r"D:\AInode"),
            engine_dir: PathBuf::from(r"C:\Users\Jo Doe\AppData\Local\AInode\resources\engine"),
            socket: PathBuf::from(r"C:\Users\Jo Doe\AppData\Local\Temp\ainode\modeld.sock"),
        };
        assert_eq!(
            fstab(&paths).unwrap(),
            concat!(
                "# AInode: only the engine and the models (security audit M1).\n",
                r"C:\134Users\134Jo\040Doe\134AppData\134Local\134AInode\134resources\134engine /mnt/c/Users/Jo\040Doe/AppData/Local/AInode/resources/engine drvfs ro,nosuid,nodev,noatime,uid=1000,gid=1000 0 0",
                "\n",
                r"D:\134AInode\134modelnet /mnt/d/AInode/modelnet drvfs rw,nosuid,nodev,noexec,noatime,uid=1000,gid=1000 0 0",
                "\n",
            )
        );
        assert!(WSL_CONF.contains("[automount]\nenabled=false\nmountFsTab=true\n"));
        assert!(WSL_CONF.contains("[interop]\nenabled=false\n"));
        assert!(WSL_CONF.contains("[general]\nguiApplications=false\n"));
        assert!(WSL_CONF.contains("[gpu]\nenabled=false\n"));
        let unc = Paths { data_dir: PathBuf::from(r"\\server\share\AInode"), ..paths };
        assert!(fstab(&unc).is_err());
    }

    /// The switch on a distribution an earlier AInode made (every drive
    /// mounted, the old wsl.conf). Needs a scratch one: create it, then
    ///   $env:AINODE_WSL_DISTRO = "AInode-sectest-up"
    ///   $env:AINODE_TEST_ENGINE_DIR = <an engine folder>; $env:AINODE_TEST_DATA_DIR = <a folder>
    ///   cargo test --lib wsl::tests::an_older_distribution_is_switched -- --ignored
    #[tokio::test]
    #[ignore = "needs a scratch WSL distribution named by AINODE_WSL_DISTRO"]
    async fn an_older_distribution_is_switched() {
        assert_ne!(distro(), "AInode", "never the installed app's distribution");
        let paths = Paths {
            data_dir: PathBuf::from(std::env::var("AINODE_TEST_DATA_DIR").unwrap()),
            engine_dir: PathBuf::from(std::env::var("AINODE_TEST_ENGINE_DIR").unwrap()),
            socket: std::env::temp_dir().join("ainode-up").join("modeld.sock"),
        };
        let inside = |script: &'static str| async move {
            let out = run(Duration::from_secs(60), wsl().args(["-d", distro(), "-u", "root", "--cd", "/", "--exec", "sh", "-c", script])).await.unwrap();
            (decode(&out.stdout), decode(&out.stderr))
        };
        let drives = "awk '$1 ~ /^[A-Za-z]:/ {print $2}' /proc/mounts";
        let (before, _) = inside(drives).await;
        assert!(before.lines().any(|l| l == "/mnt/c"), "an older distribution mounts C: whole: {before}");
        assert_eq!(imported_from().await.unwrap().as_deref(), Some(""), "0.8.2 recorded no pin");
        ensure_distro(&paths).await.unwrap();
        assert_eq!(imported_from().await.unwrap(), rootfs_pin(&paths.engine_dir), "made again from this release's Ubuntu Base");
        let (after, err) = inside(drives).await;
        println!("before:
{before}after:
{after}");
        assert_eq!(after.lines().count(), 2, "{after}");
        assert_eq!(err.trim(), "", "a clean boot");
        ensure_distro(&paths).await.unwrap();
        assert_eq!(imported_from().await.unwrap(), rootfs_pin(&paths.engine_dir), "kept: same pin");
        terminate().await;
    }

    #[test]
    fn the_rootfs_pin_comes_from_the_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(rootfs_pin(tmp.path()), None, "no MANIFEST.json: a dev build");
        std::fs::write(
            tmp.path().join("MANIFEST.json"),
            r#"{"files":[{"path":"btx-modeld","sha256":"aa"},{"path":"wsl/rootfs.tar.gz","sha256":"4f1e"}]}"#,
        )
        .unwrap();
        assert_eq!(rootfs_pin(tmp.path()).as_deref(), Some("4f1e"));
    }

    #[test]
    fn the_helper_gets_the_contracts_flags_with_linux_paths() {
        let paths = Paths {
            data_dir: PathBuf::from(r"C:\Users\u\AppData\Local\AInode"),
            engine_dir: PathBuf::from(r"C:\Program Files\AInode\resources\engine"),
            socket: PathBuf::from(r"C:\Users\u\AppData\Local\Temp\ainode\modeld.sock"),
        };
        let a = StartArgs { engine_gib: 11, peers: vec!["h.example:29447".into()] };
        assert_eq!(
            helper_args(&paths, &a).unwrap(),
            vec![
                "-modeldir=/mnt/c/Users/u/AppData/Local/AInode/modelnet",
                "-modelstorage=11GiB",
                "-modelseed=manual",
                "-modelfollowpeers=0",
                "-modelrpcsocket=/tmp/ainode/modeld.sock",
                "-modelpeer=h.example:29447",
            ]
        );
    }
}
