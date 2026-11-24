//! Start AInode with the computer (Mende, 2026-10-17: "auto start with system
//! startup ... as much as it can", on by default, a switch in Status).
//!
//! Each system's own per-user place, nothing that needs an administrator:
//!   - Windows: the value `AInode` under
//!     `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (the uninstaller
//!     removes it, windows/hooks.nsh).
//!   - Linux: `~/.config/autostart/ainode.desktop` (XDG autostart, honoured
//!     by GNOME, KDE, Xfce and the others). For the AppImage the entry names
//!     the AppImage itself (`$APPIMAGE`), not its temporary mount. `TryExec`
//!     makes a desktop skip it once the program is gone. Inside WSL nothing
//!     reads it: there AInode for Linux starts with Windows through a
//!     Startup shortcut.
//!   - Mac: `~/Library/LaunchAgents/com.m2promende.ainode.plist`
//!     (RunAtLoad). Never for a copy run from the disk image or from
//!     Gatekeeper's translocated folder: that path is gone next time.
//!
//! The entry starts the app with `--at-login`, which opens its window
//! minimized (lib.rs): it sits in the background, online, and the engine
//! starts by itself as it does on any launch (the `autostart` setting).
//! Written only after consent (rule 1), and never by a debug build or a
//! test, so the dev bridge and `cargo test` leave the computer as it was.

use std::path::{Path, PathBuf};

/// The argument the entry passes: open minimized.
pub const AT_LOGIN: &str = "--at-login";

/// Was this process started by the entry?
pub fn launched_at_login() -> bool {
    std::env::args().any(|a| a == AT_LOGIN)
}

/// Make the computer's start entry match `on`: written (or rewritten, so it
/// names the program as it is now) or removed. Debug builds and tests do
/// nothing: `cargo test --release` once wrote the real user's LaunchAgent,
/// naming the test binary (the backend tests consent, 2026-10-17).
pub fn apply(on: bool) -> Result<(), String> {
    if cfg!(debug_assertions) || cfg!(test) {
        return Ok(());
    }
    let Some(program) = program() else {
        // A copy that cannot start itself next time (a disk image, a
        // translocated app): leave whatever is there alone.
        return Ok(());
    };
    apply_at(on, &program, &Place::user()?)
}

/// What the entry starts: the AppImage itself when this runs from one, else
/// this executable, and on the Mac only an installed app.
fn program() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        if let Some(a) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_absolute() && p.is_file()) {
            return Some(a);
        }
    }
    let exe = std::env::current_exe().ok()?;
    stable(&exe).then_some(exe)
}

/// Not a path that is gone after this run: a Mac disk image (/Volumes) or
/// Gatekeeper's App Translocation copy.
fn stable(exe: &Path) -> bool {
    let s = exe.to_string_lossy();
    !(cfg!(target_os = "macos") && (s.starts_with("/Volumes/") || s.contains("/AppTranslocation/")))
}

/// Where the entry lives: the real per-user place, or a test's own.
pub struct Place {
    #[cfg(windows)]
    key: String,
    #[cfg(not(windows))]
    dir: PathBuf,
}

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "AInode";

impl Place {
    #[cfg(windows)]
    fn user() -> Result<Place, String> {
        Ok(Place { key: RUN_KEY.into() })
    }

    #[cfg(target_os = "linux")]
    fn user() -> Result<Place, String> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .ok_or("no home folder")?;
        Ok(Place { dir: config.join("autostart") })
    }

    #[cfg(target_os = "macos")]
    fn user() -> Result<Place, String> {
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("no home folder")?;
        Ok(Place { dir: home.join("Library").join("LaunchAgents") })
    }
}

// ── Windows ──────────────────────────────────────────────────────────────

/// `"C:\...\AInode.exe" --at-login`, the program quoted (it may hold spaces).
#[cfg(windows)]
fn command_line(program: &Path) -> String {
    format!("\"{}\" {AT_LOGIN}", program.display())
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn apply_at(on: bool, program: &Path, place: &Place) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let (key, name) = (wide(&place.key), wide(VALUE_NAME));
    if on {
        let data = wide(&command_line(program));
        // SAFETY: NUL-terminated UTF-16 strings that live across the call.
        let r = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32)
        };
        if r != 0 {
            return Err(format!("could not add AInode to the programs that start with Windows (error {r})"));
        }
    } else {
        // SAFETY: as above. A value that is not there is fine (error 2).
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if r != 0 && r != 2 {
            return Err(format!("could not remove AInode from the programs that start with Windows (error {r})"));
        }
    }
    Ok(())
}

/// The command the entry holds now, if any (for the tests and Status).
#[cfg(windows)]
pub fn entry_at(place: &Place) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let (key, name) = (wide(&place.key), wide(VALUE_NAME));
    let mut buf = vec![0u16; 2048];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: the buffer and its length in bytes are passed together.
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len)
    };
    if r != 0 {
        return None;
    }
    let n = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n.min(buf.len())]))
}

// ── Linux ────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
const ENTRY_FILE: &str = "ainode.desktop";

/// A path as one argument of a desktop entry's Exec (the spec's quoting).
#[cfg(any(target_os = "linux", test))]
fn exec_arg(p: &str) -> String {
    if p.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c)) {
        return p.to_string();
    }
    let mut q = String::from("\"");
    for c in p.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            q.push('\\');
        }
        q.push(c);
    }
    q.push('"');
    q.replace('%', "%%")
}

#[cfg(any(target_os = "linux", test))]
fn desktop_entry(program: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=AInode\nComment=Open AI models over BTX, running on this computer\n\
         Exec={} {AT_LOGIN}\nTryExec={program}\nIcon=ainode\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exec_arg(program)
    )
}

#[cfg(target_os = "linux")]
fn apply_at(on: bool, program: &Path, place: &Place) -> Result<(), String> {
    let file = place.dir.join(ENTRY_FILE);
    if on {
        let text = desktop_entry(&program.to_string_lossy());
        if std::fs::read_to_string(&file).ok().as_deref() == Some(text.as_str()) {
            return Ok(());
        }
        std::fs::create_dir_all(&place.dir).map_err(|e| format!("could not make {}: {e}", place.dir.display()))?;
        crate::state::write_text_atomic(&file, &text)
    } else {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("could not remove {}: {e}", file.display())),
            _ => Ok(()),
        }
    }
}

#[cfg(target_os = "linux")]
pub fn entry_at(place: &Place) -> Option<String> {
    std::fs::read_to_string(place.dir.join(ENTRY_FILE)).ok()
}

// ── Mac ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
const ENTRY_FILE: &str = "com.m2promende.ainode.plist";

#[cfg(any(target_os = "macos", test))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(any(target_os = "macos", test))]
fn launch_agent(program: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key>\n\t<string>com.m2promende.ainode</string>\n\
         \t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{}</string>\n\t\t<string>{AT_LOGIN}</string>\n\t</array>\n\
         \t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>\n\
         \t<key>ProcessType</key>\n\t<string>Interactive</string>\n\
         </dict>\n</plist>\n",
        xml_escape(program)
    )
}

#[cfg(target_os = "macos")]
fn apply_at(on: bool, program: &Path, place: &Place) -> Result<(), String> {
    let file = place.dir.join(ENTRY_FILE);
    if on {
        let text = launch_agent(&program.to_string_lossy());
        if std::fs::read_to_string(&file).ok().as_deref() == Some(text.as_str()) {
            return Ok(());
        }
        std::fs::create_dir_all(&place.dir).map_err(|e| format!("could not make {}: {e}", place.dir.display()))?;
        crate::state::write_text_atomic(&file, &text)
    } else {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("could not remove {}: {e}", file.display())),
            _ => Ok(()),
        }
    }
}

#[cfg(target_os = "macos")]
pub fn entry_at(place: &Place) -> Option<String> {
    std::fs::read_to_string(place.dir.join(ENTRY_FILE)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_never_touches_the_computer() {
        // Also under `cargo test --release`: apply must be a no-op here.
        apply(true).unwrap();
        apply(false).unwrap();
    }

    #[test]
    fn desktop_entry_quotes_the_program_and_skips_a_missing_one() {
        let e = desktop_entry("/usr/bin/ainode");
        assert!(e.contains("\nExec=/usr/bin/ainode --at-login\n"), "{e}");
        assert!(e.contains("\nTryExec=/usr/bin/ainode\n"), "{e}");
        let e = desktop_entry("/home/a b/AInode 100%.AppImage");
        assert!(e.contains("\nExec=\"/home/a b/AInode 100%%.AppImage\" --at-login\n"), "{e}");
        assert_eq!(exec_arg("/x/$HOME`"), "\"/x/\\$HOME\\`\"");
    }

    #[test]
    fn launch_agent_names_the_program_and_runs_at_load() {
        let p = launch_agent("/Applications/AInode.app/Contents/MacOS/AInode");
        assert!(p.contains("<string>/Applications/AInode.app/Contents/MacOS/AInode</string>\n\t\t<string>--at-login</string>"), "{p}");
        assert!(p.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(launch_agent("/A & <B>").contains("<string>/A &amp; &lt;B&gt;</string>"));
    }

    #[cfg(not(windows))]
    #[test]
    fn writes_rewrites_and_removes_the_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let place = Place { dir: tmp.path().join("autostart") };
        let prog = tmp.path().join("ainode");
        std::fs::write(&prog, "x").unwrap();
        apply_at(true, &prog, &place).unwrap();
        assert!(entry_at(&place).unwrap().contains(&*prog.to_string_lossy()));
        // Moved program: the entry follows it.
        let moved = tmp.path().join("ainode2");
        apply_at(true, &moved, &place).unwrap();
        assert!(entry_at(&place).unwrap().contains(&*moved.to_string_lossy()));
        apply_at(false, &moved, &place).unwrap();
        assert_eq!(entry_at(&place), None);
        apply_at(false, &moved, &place).unwrap();
    }

    /// A scratch key of its own under HKCU, never the real Run key.
    #[cfg(windows)]
    #[test]
    fn writes_rewrites_and_removes_the_run_value() {
        let place = Place { key: format!(r"Software\AInodeTest\{}\Run", std::process::id()) };
        let prog = Path::new(r"C:\Users\a b\AppData\Local\AInode\ainode.exe");
        apply_at(true, prog, &place).unwrap();
        assert_eq!(entry_at(&place).as_deref(), Some(r#""C:\Users\a b\AppData\Local\AInode\ainode.exe" --at-login"#));
        let moved = Path::new(r"D:\AInode\ainode.exe");
        apply_at(true, moved, &place).unwrap();
        assert_eq!(entry_at(&place).as_deref(), Some(r#""D:\AInode\ainode.exe" --at-login"#));
        apply_at(false, moved, &place).unwrap();
        assert_eq!(entry_at(&place), None);
        apply_at(false, moved, &place).unwrap();
        // Leave nothing: the scratch keys go too.
        let parent = wide(&format!(r"Software\AInodeTest\{}", std::process::id()));
        let top = wide(r"Software\AInodeTest");
        unsafe {
            use windows_sys::Win32::System::Registry::{RegDeleteTreeW, HKEY_CURRENT_USER};
            RegDeleteTreeW(HKEY_CURRENT_USER, parent.as_ptr());
            windows_sys::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, top.as_ptr());
        }
    }
}
