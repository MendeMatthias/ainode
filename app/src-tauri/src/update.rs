//! Updates: the Tauri updater plugin, with every offer bound to its version.
//!
//! The feed is `latest.json` on the latest release of
//! MendeMatthias/ainode-releases (tauri.conf.json `plugins.updater`). It lists
//! one entry per platform: `darwin-aarch64` (the .app tarball),
//! `windows-x86_64` (the setup.exe), `linux-x86_64` (the AppImage) and
//! `linux-x86_64-deb` (the .deb; the plugin looks for `<os>-<arch>-deb` first
//! when the running app was installed from one).
//!
//! # The binding (from easyBTX's update_binding.rs, easynode #148)
//!
//! The plugin checks that the feed's version is newer and that the downloaded
//! bytes carry a valid signature from the key in tauri.conf.json. Nothing ties
//! the two together: the key signs files, not versions, so an old build,
//! genuinely signed, would pass under any version a feed claims, and a Linux
//! machine could be handed the Windows installer. A minisign signature carries
//! a trusted comment, `timestamp:…\tfile:<name>`, that the key signs together
//! with the file, and the release scripts sign every file under its release
//! name, `AInode_<version><platform suffix>`. So an offer is taken only when
//! every entry of the feed is signed under exactly the name its platform and
//! the feed's version call for. The name is read here before anything is
//! downloaded; the plugin authenticates it with the signature at download.
//!
//! The app also remembers the highest version it has run (`update.json` in
//! the data folder, written only after consent, rule 1) and takes nothing
//! below it, so a replayed old feed is refused even on a Mac rolled back by
//! hand. A pre-release never raises the mark.
//!
//! A refusal is kept and shown in Status as a refusal with its reason, never
//! as "up to date": a feed the app refuses stops every update, and the
//! download on easybtx.com/ai always works by hand.
//!
//! A feed with no file for this platform offers this computer nothing: the
//! plugin looks the file up before it asks the comparator whether the release
//! is newer (tauri-plugin-updater 2.13 `check`), so a release published for
//! the Mac first failed every check on Windows and Linux ("None of the
//! fallback platforms ... were found"). That is "nothing newer here".
//!
//! An update never touches the data folder: it replaces the app only. Before
//! it does, the chat server and the engine are stopped the way Quit stops
//! them, so the helper never has its files replaced under it.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use base64::Engine as _;
use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_updater::{RemoteRelease, RemoteReleaseInner, Update, UpdaterExt};

use crate::state;

/// Every file of this app is signed under a name that starts with this.
pub const ASSET_PREFIX: &str = "AInode_";

/// Each platform key a feed may carry, and the end of the name its file is
/// signed under. The release scripts mint the same names (a test reads them).
pub const PLATFORM_SUFFIXES: [(&str, &str); 4] = [
    ("darwin-aarch64", "_aarch64.app.tar.gz"),
    ("linux-x86_64", "_amd64.AppImage"),
    ("linux-x86_64-deb", "_amd64.deb"),
    ("windows-x86_64", "_x64-setup.exe"),
];

/// The name the file for `platform` at `version` must have been signed under.
pub fn expected_signed_name(platform: &str, version: &str) -> Option<String> {
    PLATFORM_SUFFIXES
        .iter()
        .find(|(key, _)| *key == platform)
        .map(|(_, suffix)| format!("{ASSET_PREFIX}{version}{suffix}"))
}

/// The file name a Tauri signature was made over: the `file:` field of its
/// trusted comment. A Tauri signature is base64 of a whole minisign signature
/// file, decoded here the way the plugin decodes it before verifying.
pub fn signed_name(signature: &str) -> Option<String> {
    let decoded = base64::engine::general_purpose::STANDARD.decode(signature).ok()?;
    let text = std::str::from_utf8(&decoded).ok()?;
    let sig = minisign_verify::Signature::decode(text).ok()?;
    sig.trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("file:"))
        .map(str::to_string)
}

/// Why a release is not bound to `version`, or `None` when every entry is.
pub fn binding_refusal(version: &str, data: &RemoteReleaseInner) -> Option<String> {
    let platforms = match data {
        RemoteReleaseInner::Static { platforms } => platforms,
        RemoteReleaseInner::Dynamic(_) => return Some("the feed does not list its platforms".into()),
    };
    if platforms.is_empty() {
        return Some("the feed lists no platforms".into());
    }
    let mut keys: Vec<&String> = platforms.keys().collect();
    keys.sort();
    for key in keys {
        let Some(expected) = expected_signed_name(key, version) else {
            return Some(format!("it lists {key}, which no release of AInode has"));
        };
        match signed_name(&platforms[key].signature) {
            Some(name) if name == expected => {}
            Some(name) => return Some(format!("its {key} file is signed as {name}, not {expected}")),
            None => return Some(format!("its {key} signature does not say what file it signs")),
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Offer,
    NotNewer,
    Refuse(String),
}

/// The rule, pure. `high_water` is the highest version this install has run.
pub fn decide(current: &Version, high_water: Option<&Version>, release: &RemoteRelease) -> Decision {
    if release.version <= *current {
        return Decision::NotNewer;
    }
    if high_water.is_some_and(|mark| release.version < *mark) {
        return Decision::NotNewer;
    }
    match binding_refusal(&release.version.to_string(), &release.data) {
        None => Decision::Offer,
        Some(reason) => Decision::Refuse(reason),
    }
}

/// The data folder, for the mark (set once in lib.rs setup).
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
/// The last refusal: the comparator can only say yes or no.
static REFUSAL: Mutex<Option<String>> = Mutex::new(None);

pub fn set_data_dir(dir: &Path) {
    let _ = DATA_DIR.set(dir.to_path_buf());
}

/// The plugin's version comparator (lib.rs): every check goes through it.
pub fn comparator(current: Version, release: RemoteRelease) -> bool {
    let mark = DATA_DIR.get().and_then(|d| high_water(d));
    match decide(&current, mark.as_ref(), &release) {
        Decision::Offer => true,
        Decision::NotNewer => false,
        Decision::Refuse(reason) => {
            let reason = format!("AInode {} was refused: {reason}", release.version);
            eprintln!("[update] {reason}");
            *REFUSAL.lock().unwrap_or_else(|e| e.into_inner()) = Some(reason);
            false
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Mark {
    #[serde(default)]
    high_water: Option<String>,
}

fn mark_file(data_dir: &Path) -> PathBuf {
    data_dir.join("update.json")
}

pub fn high_water(data_dir: &Path) -> Option<Version> {
    let text = std::fs::read_to_string(mark_file(data_dir)).ok()?;
    let m: Mark = serde_json::from_str(&text).ok()?;
    m.high_water.and_then(|v| Version::parse(&v).ok())
}

/// The new mark after running `running`, or `None` to leave it.
pub fn raised_mark(stored: Option<&Version>, running: &Version) -> Option<Version> {
    if !running.pre.is_empty() {
        return None;
    }
    match stored {
        Some(mark) if mark >= running => None,
        _ => Some(running.clone()),
    }
}

/// The version this AInode was updated from: the mark it found, when that is
/// below the running version. `None` on a first install (no mark yet) or when
/// nothing changed.
pub fn updated_from(stored: Option<&Version>, running: &Version) -> Option<String> {
    match stored {
        Some(mark) if running.pre.is_empty() && mark < running => Some(mark.to_string()),
        _ => None,
    }
}

/// Read once, before the first check raises the mark: what the UI says once,
/// "Updated to AInode X".
static UPDATED_FROM: OnceLock<Option<String>> = OnceLock::new();

/// Can this computer install an update with nobody there? Everywhere but a
/// `.deb`, whose install asks for a password (pkexec); an AppImage replaces
/// itself (Mende, 2026-10-18: "update automatically ... so users don't have to
/// click restart").
pub fn installs_by_itself() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some()
}

/// Record the running version as the mark if it is the highest yet. Only once
/// consent.json exists (rule 1); called at each check, so the first check
/// after consent records it.
pub fn remember_running_version(paths: &state::Paths, running: &Version) {
    if state::load_consent(paths).is_none() {
        return;
    }
    if let Some(mark) = raised_mark(high_water(&paths.data_dir).as_ref(), running) {
        let m = Mark { high_water: Some(mark.to_string()) };
        if let Err(e) = state::write_json_atomic(&mark_file(&paths.data_dir), &m) {
            eprintln!("[update] could not record the running version: {e}");
        }
    }
}

// ── The commands ───────────────────────────────────────────────────────────

/// The update found by the last check, held until the user restarts.
#[derive(Default)]
pub struct Pending(pub tokio::sync::Mutex<Option<Update>>);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UpdateOut {
    /// The newer version on offer, if any.
    pub version: Option<String>,
    pub notes: Option<String>,
    /// Why the feed was refused, if it was.
    pub refused: Option<String>,
    /// It may be installed without a click (installs_by_itself).
    pub by_itself: bool,
    /// The version this AInode was updated from, on the checks after an update.
    pub updated_from: Option<String>,
}

pub async fn check(app: &tauri::AppHandle, paths: &state::Paths) -> Result<UpdateOut, String> {
    if let Ok(v) = Version::parse(&app.package_info().version.to_string()) {
        UPDATED_FROM.get_or_init(|| updated_from(high_water(&paths.data_dir).as_ref(), &v));
        remember_running_version(paths, &v);
    }
    *REFUSAL.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let updater = app.updater().map_err(|e| format!("The update check could not start: {e}"))?;
    let found = match updater.check().await {
        Ok(found) => found,
        Err(e) if not_for_this_platform(&e) => {
            eprintln!("[update] the feed has no file for this platform: {e}");
            None
        }
        Err(e) => return Err(format!("The update check failed: {e}")),
    };
    let refused = REFUSAL.lock().unwrap_or_else(|e| e.into_inner()).take();
    let out = UpdateOut {
        version: found.as_ref().map(|u| u.version.clone()),
        notes: found.as_ref().and_then(|u| u.body.clone()),
        refused,
        by_itself: installs_by_itself(),
        updated_from: UPDATED_FROM.get().cloned().flatten(),
    };
    if let Some(p) = app.try_state::<Pending>() {
        *p.0.lock().await = found;
    }
    Ok(out)
}

/// The plugin's answer for a feed that lists no file for this platform.
fn not_for_this_platform(e: &tauri_plugin_updater::Error) -> bool {
    matches!(e, tauri_plugin_updater::Error::TargetNotFound(_) | tauri_plugin_updater::Error::TargetsNotFound(_))
}

/// Download the pending update, stop the chat and the engine, install it and
/// start the new AInode. On Windows the installer takes over and the process
/// ends inside `install`.
pub async fn install(app: &tauri::AppHandle, backend: &crate::backend::Backend) -> Result<(), String> {
    let update = {
        let p = app.try_state::<Pending>().ok_or("no update state")?;
        let guard = p.0.lock().await;
        guard.clone().ok_or("No update is waiting. Check for updates first.")?
    };
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| format!("The update could not be downloaded: {e}"))?;
    backend.shutdown().await;
    #[cfg(windows)]
    crate::wsl::terminate().await;
    // The installer inherits our output: never let it write into a pipe
    // nobody reads (restart.rs, quiet_dead_output).
    #[cfg(target_os = "linux")]
    crate::restart::quiet_dead_output();
    if let Err(e) = update.install(bytes) {
        // AInode stays open as it was: its engine may start again (the page
        // starts it, or the self-heal).
        backend.engine.reopen();
        return Err(install_failed(&e.to_string(), installs_by_itself()));
    }
    crate::restart::now(app)
}


/// What the page says when the install failed. Where AInode replaces its
/// own file (Mac, Windows, AInode as an AppImage) a failed install leaves it
/// as it was. A `.deb` is installed by the system's package tool, which can
/// stop halfway: then the running AInode still works, but the next start may
/// not, so the message says what to do.
fn install_failed(why: &str, by_itself: bool) -> String {
    if by_itself {
        format!("The update could not be installed: {why}. AInode is unchanged and stays open.")
    } else {
        format!(
            "The update could not be installed: {why}. AInode stays open. The package may be left half installed: \
             if AInode does not start next time, install the .deb again from easybtx.com/ai."
        )
    }
}

/// The scratch update test (plan 3.1, "prove before release"), debug builds
/// only: with AINODE_UPDATE_SELFTEST set, check at launch and install what
/// is found, with no window clicked, printing each step. Run it with
/// AINODE_DATA_DIR and AINODE_SOCKET pointing at scratch places, or its
/// launch cleanup would stop the real AInode's engine on the shared socket.
pub fn selftest(app: &tauri::AppHandle, backend: &std::sync::Arc<crate::backend::Backend>, paths: &state::Paths) {
    if state::dev_env("AINODE_UPDATE_SELFTEST").is_none() {
        return;
    }
    let (app, backend, paths) = (app.clone(), backend.clone(), paths.clone());
    eprintln!("[selftest] running {} from {}", app.package_info().version, std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default());
    tauri::async_runtime::spawn(async move {
        match check(&app, &paths).await {
            Ok(u) => {
                eprintln!("[selftest] check: {u:?}");
                if u.version.is_some() {
                    match install(&app, &backend).await {
                        Ok(()) => eprintln!("[selftest] installed"),
                        Err(e) => eprintln!("[selftest] install refused: {e}"),
                    }
                }
            }
            Err(e) => eprintln!("[selftest] check failed: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_install_says_unchanged_only_where_that_is_sure() {
        let own = install_failed("no space", true);
        assert!(own.contains("no space") && own.contains("AInode is unchanged"));
        let deb = install_failed("dpkg stopped", false);
        assert!(deb.contains("dpkg stopped") && !deb.contains("unchanged"));
        assert!(deb.contains("half installed") && deb.contains("install the .deb again"));
    }

    #[test]
    fn updated_from_only_after_an_update() {
        let v = |s: &str| Version::parse(s).unwrap();
        assert_eq!(updated_from(Some(&v("0.8.1")), &v("0.8.2")), Some("0.8.1".into()));
        assert_eq!(updated_from(Some(&v("0.8.2")), &v("0.8.2")), None);
        assert_eq!(updated_from(None, &v("0.8.2")), None, "a first install says nothing");
        assert_eq!(updated_from(Some(&v("0.8.3")), &v("0.8.2")), None);
        assert_eq!(updated_from(Some(&v("0.8.1")), &v("0.8.2-rc.1")), None);
    }

    /// A structurally valid Tauri signature carrying a chosen file name. Its
    /// key id and signature bytes are zeros: it decodes, it would never
    /// verify, which is why the name is read BEFORE the download rather than
    /// trusted after it.
    fn fake_sig(name: &str) -> String {
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut body = b"ED".to_vec();
        body.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        body.extend_from_slice(&[0u8; 64]);
        let raw = format!(
            "untrusted comment: signature from tauri secret key\n{}\ntrusted comment: timestamp:0\tfile:{name}\n{}\n",
            b64.encode(body),
            b64.encode([0u8; 64]),
        );
        b64.encode(raw)
    }

    fn feed(version: &str, platforms: &[(&str, String)]) -> RemoteRelease {
        let mut p = serde_json::Map::new();
        for (key, sig) in platforms {
            p.insert(
                key.to_string(),
                serde_json::json!({
                    "signature": sig,
                    "url": format!("https://github.com/MendeMatthias/ainode-releases/releases/download/v{version}/x"),
                }),
            );
        }
        serde_json::from_value(serde_json::json!({
            "version": version, "notes": "n", "pub_date": "2026-10-14T00:00:00Z", "platforms": p,
        }))
        .expect("a feed the plugin itself would parse")
    }

    fn ver(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn bound(version: &str, keys: &[&str]) -> RemoteRelease {
        let entries: Vec<(&str, String)> =
            keys.iter().map(|k| (*k, fake_sig(&expected_signed_name(k, version).unwrap()))).collect();
        feed(version, &entries)
    }

    #[test]
    fn names_are_the_release_names() {
        assert_eq!(expected_signed_name("darwin-aarch64", "0.8.0").unwrap(), "AInode_0.8.0_aarch64.app.tar.gz");
        assert_eq!(expected_signed_name("windows-x86_64", "0.8.0").unwrap(), "AInode_0.8.0_x64-setup.exe");
        assert_eq!(expected_signed_name("linux-x86_64", "0.8.0").unwrap(), "AInode_0.8.0_amd64.AppImage");
        assert_eq!(expected_signed_name("linux-x86_64-deb", "0.8.0").unwrap(), "AInode_0.8.0_amd64.deb");
        assert_eq!(expected_signed_name("darwin-x86_64", "0.8.0"), None);
        assert_eq!(signed_name(&fake_sig("AInode_0.8.0_aarch64.app.tar.gz")).as_deref(), Some("AInode_0.8.0_aarch64.app.tar.gz"));
        assert_eq!(signed_name("not base64 at all"), None);
    }

    #[test]
    fn a_feed_with_every_platform_bound_is_offered() {
        let r = bound("0.8.0", &["darwin-aarch64", "windows-x86_64", "linux-x86_64", "linux-x86_64-deb"]);
        assert_eq!(decide(&ver("0.7.9"), None, &r), Decision::Offer);
        assert_eq!(decide(&ver("0.8.0"), None, &r), Decision::NotNewer);
        assert_eq!(decide(&ver("0.8.1"), None, &r), Decision::NotNewer);
    }

    #[test]
    fn the_bundlers_own_name_binds_nothing() {
        let r = feed("0.8.0", &[("darwin-aarch64", fake_sig("AInode.app.tar.gz"))]);
        assert_eq!(
            decide(&ver("0.7.9"), None, &r),
            Decision::Refuse("its darwin-aarch64 file is signed as AInode.app.tar.gz, not AInode_0.8.0_aarch64.app.tar.gz".into())
        );
    }

    #[test]
    fn an_old_build_under_a_new_version_is_refused() {
        let r = feed("0.9.0", &[("darwin-aarch64", fake_sig("AInode_0.8.0_aarch64.app.tar.gz"))]);
        assert!(matches!(decide(&ver("0.8.0"), None, &r), Decision::Refuse(_)));
    }

    #[test]
    fn another_platforms_file_is_refused() {
        // The Windows installer listed for Linux: one bad entry refuses the release.
        let r = feed("0.8.0", &[
            ("darwin-aarch64", fake_sig("AInode_0.8.0_aarch64.app.tar.gz")),
            ("linux-x86_64", fake_sig("AInode_0.8.0_x64-setup.exe")),
        ]);
        assert_eq!(
            decide(&ver("0.7.9"), None, &r),
            Decision::Refuse("its linux-x86_64 file is signed as AInode_0.8.0_x64-setup.exe, not AInode_0.8.0_amd64.AppImage".into())
        );
        let other_app = feed("0.8.0", &[("darwin-aarch64", fake_sig("easyBTX_0.8.0_aarch64.app.tar.gz"))]);
        assert!(matches!(decide(&ver("0.7.9"), None, &other_app), Decision::Refuse(_)));
    }

    #[test]
    fn unknown_platforms_and_empty_feeds_are_refused() {
        let r = feed("0.8.0", &[("darwin-x86_64", fake_sig("AInode_0.8.0_x64.app.tar.gz"))]);
        assert_eq!(decide(&ver("0.7.9"), None, &r), Decision::Refuse("it lists darwin-x86_64, which no release of AInode has".into()));
        assert_eq!(decide(&ver("0.7.9"), None, &feed("0.8.0", &[])), Decision::Refuse("the feed lists no platforms".into()));
    }

    #[test]
    fn a_feed_without_this_platform_offers_nothing_here() {
        use tauri_plugin_updater::Error;
        assert!(not_for_this_platform(&Error::TargetsNotFound(vec!["windows-x86_64-nsis".into(), "windows-x86_64".into()])));
        assert!(not_for_this_platform(&Error::TargetNotFound("linux-x86_64".into())));
        assert!(!not_for_this_platform(&Error::ReleaseNotFound));
    }

    #[test]
    fn nothing_below_the_mark_is_taken() {
        let r = bound("0.8.0", &["darwin-aarch64"]);
        assert_eq!(decide(&ver("0.7.1"), Some(&ver("0.8.1")), &r), Decision::NotNewer);
        assert_eq!(decide(&ver("0.7.1"), Some(&ver("0.8.0")), &r), Decision::Offer);
    }

    #[test]
    fn a_pre_release_never_raises_the_mark() {
        assert_eq!(raised_mark(None, &ver("0.8.0")), Some(ver("0.8.0")));
        assert_eq!(raised_mark(Some(&ver("0.8.0")), &ver("0.7.9")), None);
        assert_eq!(raised_mark(Some(&ver("0.8.0")), &ver("0.9.0-rc.1")), None);
        assert_eq!(raised_mark(Some(&ver("0.8.0")), &ver("0.8.1")), Some(ver("0.8.1")));
    }

    #[test]
    fn the_mark_is_written_only_after_consent() {
        let dir = tempfile::tempdir().unwrap();
        let paths = state::Paths {
            data_dir: dir.path().to_path_buf(),
            engine_dir: dir.path().join("engine"),
            socket: dir.path().join("s.sock"),
        };
        remember_running_version(&paths, &ver("0.8.0"));
        assert!(!mark_file(dir.path()).exists(), "no write before consent (rule 1)");
        let c = state::consent_for(10, 100 * state::GB, state::Held::default(), 0, "0.8.0").unwrap();
        state::write_json_atomic(&paths.consent_file(), &c).unwrap();
        remember_running_version(&paths, &ver("0.8.0"));
        assert_eq!(high_water(dir.path()), Some(ver("0.8.0")));
    }

    /// The scripts that sign the files use the same names as this module.
    #[test]
    fn the_release_scripts_sign_under_these_names() {
        let mac = include_str!("../../scripts/release-macos.sh");
        assert!(mac.contains("AInode_${VERSION}_aarch64.app.tar.gz"), "release-macos.sh signs the tarball under its release name");
        let feed = include_str!("../../scripts/updater-feed.py");
        for (key, suffix) in PLATFORM_SUFFIXES {
            assert!(feed.contains(&format!("\"{key}\": \"{suffix}\"")), "updater-feed.py knows {key} as {suffix}");
        }
    }
}
