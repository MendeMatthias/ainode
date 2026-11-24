//! Handing a file, a folder or a link to the desktop, per platform.
//!
//! macOS: `/usr/bin/open` (Finder, the default browser), exactly as before.
//! Linux: the file manager through the freedesktop `FileManager1.ShowItems`
//! D-Bus call (Nautilus, Dolphin, Nemo, Caja and Thunar answer it), else
//! `xdg-open` on the folder; links through `xdg-open`. Inside WSL, where the
//! distribution often has neither, Windows' URL handler when xdg-open fails.
//! Windows: Explorer (`/select,` for a file); links through the URL handler,
//! which opens the default browser. No console window.
//!
//! These children are the user's own desktop programs, so unlike the engine
//! they keep the user's environment (DISPLAY, the session bus, the theme).
//! Inside an AppImage that environment also carries the AppImage's private
//! library and module paths, which would load the AppImage's GTK into the
//! user's file manager; `desktop_env` takes those out first.

use std::path::Path;
#[cfg(target_os = "linux")]
use std::time::Duration;

/// What the app calls the machine it runs on, in the few messages the backend
/// writes itself. The UI has its own copy (src/platform.ts).
pub const DEVICE: &str = if cfg!(target_os = "macos") {
    "Mac"
} else if cfg!(windows) {
    "PC"
} else {
    "computer"
};

/// A last message when the app cannot start and has no window to say it in:
/// a native box, then stderr. Best effort: it must never be the thing that
/// fails.
pub fn fatal(msg: &str) {
    eprintln!("[ainode] {msg}");
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let wide = |s: &str| -> Vec<u16> { std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect() };
        let (text, title) = (wide(msg), wide("AInode"));
        // SAFETY: two NUL-terminated wide strings that outlive the call.
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!("display alert \"AInode\" message {} as critical", serde_json::json!(msg));
        let _ = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).status();
    }
    #[cfg(target_os = "linux")]
    {
        let text = format!("--text={msg}");
        let tries: [(&str, Vec<&str>); 3] = [
            ("zenity", vec!["--error", "--title=AInode", &text]),
            ("kdialog", vec!["--title", "AInode", "--error", msg]),
            ("xmessage", vec!["-center", msg]),
        ];
        for (prog, args) in tries {
            if std::process::Command::new(prog).args(&args).status().is_ok_and(|s| s.success()) {
                break;
            }
        }
    }
}

/// Show `file` selected in the file manager.
pub async fn reveal(file: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        run(tokio::process::Command::new("/usr/bin/open").arg("-R").arg(file), "open -R").await
    }
    #[cfg(target_os = "linux")]
    {
        let uri = file_uri(file);
        let mut c = tokio::process::Command::new("dbus-send");
        c.args(["--session", "--print-reply", "--dest=org.freedesktop.FileManager1"])
            .args(["/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems"])
            .arg(format!("array:string:{uri}"))
            .arg("string:");
        // No file manager on the bus (or no bus): open the folder instead.
        match run_for(&mut c, "dbus-send", Duration::from_secs(5)).await {
            Ok(()) => Ok(()),
            Err(_) => open_folder(file.parent().unwrap_or(file)).await,
        }
    }
    #[cfg(windows)]
    {
        // Explorer wants `/select,"<path>"` as one argument, quoted its way.
        let mut c = explorer();
        c.raw_arg(format!("/select,\"{}\"", file.display()));
        spawned(&mut c, "explorer").await
    }
}

/// Open a folder in the file manager.
pub async fn open_folder(dir: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        run(tokio::process::Command::new("/usr/bin/open").arg(dir), "open").await
    }
    #[cfg(target_os = "linux")]
    {
        match run(tokio::process::Command::new("xdg-open").arg(dir), "xdg-open").await {
            Err(e) if in_wsl() => match windows_path_of(dir).await {
                Some(w) => through_windows(&w).await.map_err(|_| e),
                None => Err(e),
            },
            r => r,
        }
    }
    #[cfg(windows)]
    {
        let mut c = explorer();
        c.arg(dir);
        spawned(&mut c, "explorer").await
    }
}

/// Open an https link (validated by the caller) in the default browser.
pub async fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        run(tokio::process::Command::new("/usr/bin/open").arg(url), "open").await
    }
    #[cfg(target_os = "linux")]
    {
        match run(tokio::process::Command::new("xdg-open").arg(url), "xdg-open").await {
            Err(e) if in_wsl() => through_windows(std::ffi::OsStr::new(url)).await.map_err(|_| e),
            r => r,
        }
    }
    #[cfg(windows)]
    {
        // The registered https handler: the default browser.
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let mut c = tokio::process::Command::new(std::path::PathBuf::from(root).join("System32").join("rundll32.exe"));
        c.arg("url.dll,FileProtocolHandler").arg(url);
        spawned(&mut c, "the browser").await
    }
}

/// The system, for Send feedback (plan 3.2): what a bug report needs and
/// nothing about the person. The UI puts it into a GitHub issue the user
/// reads before sending; the app sends nothing itself.
#[derive(Debug, Clone, Default, serde::Serialize, PartialEq)]
pub struct SystemInfo {
    /// "macOS 27.0.1", "Ubuntu 24.04.3 LTS", "Windows 11 (10.0.26100)".
    pub os: String,
    /// The machine model: "Mac17,2"; empty when unknown.
    pub machine: String,
    /// The processor: "Apple M5", "AMD Ryzen 7 5800X".
    pub chip: String,
    pub ram_bytes: u64,
}

pub async fn system_info() -> SystemInfo {
    tokio::task::spawn_blocking(|| SystemInfo { ram_bytes: crate::disk::ram_bytes(), ..system_info_blocking() })
        .await
        .unwrap_or_default()
}

/// What a short system program prints; empty on any failure.
#[allow(dead_code)]
fn output_of(prog: &str, args: &[&str]) -> String {
    let mut c = std::process::Command::new(prog);
    c.args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(crate::wsl::NO_WINDOW);
    }
    c.output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// The first line a short system program prints, trimmed; empty on any failure.
#[allow(dead_code)]
fn first_line(prog: &str, args: &[&str]) -> String {
    output_of(prog, args).lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string()
}

#[cfg(target_os = "macos")]
fn system_info_blocking() -> SystemInfo {
    let v = first_line("/usr/bin/sw_vers", &["-productVersion"]);
    SystemInfo {
        os: if v.is_empty() { "macOS".into() } else { format!("macOS {v}") },
        machine: first_line("/usr/sbin/sysctl", &["-n", "hw.model"]),
        chip: first_line("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"]),
        ram_bytes: 0,
    }
}

#[cfg(target_os = "linux")]
fn system_info_blocking() -> SystemInfo {
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    let os = os_release_name(&read("/etc/os-release"));
    let chip = read("/proc/cpuinfo")
        .lines()
        .find_map(|l| l.strip_prefix("model name").and_then(|r| r.split_once(':')).map(|(_, v)| v.trim().to_string()))
        .unwrap_or_default();
    SystemInfo { os: if os.is_empty() { "Linux".into() } else { os }, machine: read("/sys/devices/virtual/dmi/id/product_name").trim().to_string(), chip, ram_bytes: 0 }
}

/// PRETTY_NAME from /etc/os-release, unquoted.
#[cfg(any(target_os = "linux", test))]
pub fn os_release_name(text: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim().trim_matches('"').to_string())
        .unwrap_or_default()
}

#[cfg(windows)]
fn system_info_blocking() -> SystemInfo {
    // `ver` prints "Microsoft Windows [Version 10.0.26100.4652]".
    let ver = first_line("cmd", &["/d", "/c", "ver"]);
    let build = ver.split("Version").nth(1).map(|v| v.trim().trim_end_matches(']').trim().to_string()).unwrap_or_default();
    let major = if build.split('.').nth(2).and_then(|b| b.parse::<u32>().ok()).is_some_and(|b| b >= 22000) { "Windows 11" } else { "Windows" };
    // The product name Task Manager shows; PROCESSOR_IDENTIFIER is only
    // "Intel64 Family 6 Model 151 Stepping 2, GenuineIntel".
    let name = reg_processor_name(&output_of("reg", &["query", r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0", "/v", "ProcessorNameString"]));
    SystemInfo {
        os: if build.is_empty() { major.into() } else { format!("{major} ({build})") },
        machine: String::new(),
        chip: if name.is_empty() { std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default() } else { name },
        ram_bytes: 0,
    }
}

/// The value `reg query ... /v ProcessorNameString` prints:
/// "    ProcessorNameString    REG_SZ    12th Gen Intel(R) Core(TM) i9-12900K".
#[cfg(any(windows, test))]
pub fn reg_processor_name(text: &str) -> String {
    text.lines()
        .find_map(|l| l.trim_start().strip_prefix("ProcessorNameString"))
        .and_then(|r| r.trim_start().strip_prefix("REG_SZ"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod info_tests {
    #[test]
    fn os_release_names_are_read_plainly() {
        assert_eq!(super::os_release_name("NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.3 LTS\"\n"), "Ubuntu 24.04.3 LTS");
        assert_eq!(super::os_release_name("PRETTY_NAME=Debian GNU/Linux 13 (trixie)"), "Debian GNU/Linux 13 (trixie)");
        assert_eq!(super::os_release_name(""), "");
    }

    #[test]
    fn processor_names_are_read_from_reg_query() {
        let out = "\r\nHKEY_LOCAL_MACHINE\\HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0\r\n    ProcessorNameString    REG_SZ    12th Gen Intel(R) Core(TM) i9-12900K\r\n\r\n";
        assert_eq!(super::reg_processor_name(out), "12th Gen Intel(R) Core(TM) i9-12900K");
        assert_eq!(super::reg_processor_name("ERROR: The system was unable to find the specified registry key or value."), "");
        assert_eq!(super::reg_processor_name(""), "");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn this_pc_says_what_it_is() {
        let i = super::system_info().await;
        assert!(i.os.starts_with("Windows"), "{i:?}");
        assert!(!i.chip.is_empty() && !i.chip.contains(" Family "), "{i:?}");
        assert!(i.ram_bytes > 0);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn this_mac_says_what_it_is() {
        let i = super::system_info().await;
        assert!(i.os.starts_with("macOS "), "{i:?}");
        // hw.model: "Mac17,2", "MacBookPro18,1", and "VirtualMac2,1" on a
        // virtual Mac (GitHub's macOS runner).
        assert!(i.machine.contains("Mac"), "{i:?}");
        assert!(i.ram_bytes > 0);
    }
}

#[cfg(windows)]
fn explorer() -> tokio::process::Command {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    tokio::process::Command::new(std::path::PathBuf::from(root).join("explorer.exe"))
}

/// Windows: Explorer answers exit code 1 even when it did what it was asked,
/// so starting it is the success.
#[cfg(windows)]
async fn spawned(c: &mut tokio::process::Command, what: &str) -> Result<(), String> {
    c.creation_flags(crate::wsl::NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    c.spawn().map(|_| ()).map_err(|e| format!("{what}: {e}"))
}

#[cfg(target_os = "macos")]
async fn run(c: &mut tokio::process::Command, what: &str) -> Result<(), String> {
    let st = c.status().await.map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({st})"))
    }
}

#[cfg(target_os = "linux")]
async fn run(c: &mut tokio::process::Command, what: &str) -> Result<(), String> {
    // xdg-open returns once the handler is launched; a minute is only a
    // ceiling for a desktop that never answers.
    run_for(c, what, Duration::from_secs(60)).await
}

/// A Linux AInode inside WSL (its window through WSLg) often has no browser
/// or file manager in its distribution: xdg-open then answers exit 3 and Send
/// feedback, Learn links and Show file did nothing (the PC, 2026-10-16).
/// Windows' own URL handler, the one the Windows build uses (`rundll32
/// url.dll,FileProtocolHandler`), reached through WSL's interop, opens a link
/// in the default Windows browser and a folder (as `\\wsl.localhost\...`) in
/// Explorer. Measured from the PC's WSL: the browser asked for the link with
/// its query intact; `explorer.exe <link>` opened nothing there.
#[cfg(target_os = "linux")]
fn in_wsl() -> bool {
    Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()
}

/// rundll32: on PATH when the distribution appends Windows' PATH, else where
/// Windows keeps it.
#[cfg(target_os = "linux")]
fn windows_opener() -> Option<std::path::PathBuf> {
    let on_path = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("rundll32.exe")).find(|f| f.is_file()));
    on_path.or_else(|| Some(std::path::PathBuf::from("/mnt/c/Windows/System32/rundll32.exe")).filter(|f| f.is_file()))
}

/// Hand `arg` (a link, or a Windows path) to Windows. Its exit code says
/// nothing about the browser (as on Windows, `spawned`): starting it is the
/// success. stdin is closed: interop would pass the app's own on.
#[cfg(target_os = "linux")]
async fn through_windows(arg: &std::ffi::OsStr) -> Result<(), String> {
    let exe = windows_opener().ok_or("Windows' URL handler is not reachable from this Linux")?;
    let mut child = tokio::process::Command::new(exe)
        .arg("url.dll,FileProtocolHandler")
        .arg(arg)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("rundll32.exe: {e}"))?;
    // It exits as soon as it has handed the link on; it is never killed.
    let _ = tokio::time::timeout(Duration::from_secs(15), child.wait()).await;
    Ok(())
}

/// A Linux path as Windows sees it, from WSL's own `wslpath -w`.
#[cfg(target_os = "linux")]
async fn windows_path_of(p: &Path) -> Option<std::ffi::OsString> {
    let out = tokio::time::timeout(Duration::from_secs(10), tokio::process::Command::new("wslpath").arg("-w").arg(p).output())
        .await
        .ok()?
        .ok()?;
    let w = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !w.is_empty()).then(|| w.into())
}

#[cfg(target_os = "linux")]
async fn run_for(c: &mut tokio::process::Command, what: &str, t: Duration) -> Result<(), String> {
    c.env_clear()
        .envs(desktop_env(std::env::vars_os()))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let st = tokio::time::timeout(t, c.status())
        .await
        .map_err(|_| format!("{what} did not answer"))?
        .map_err(|e| format!("{what}: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({st})"))
    }
}

/// The user's environment without what an AppImage added for itself: every
/// variable naming the AppImage, and every entry of a path list that points
/// inside its mount (`$APPDIR`). Outside an AppImage it is the environment
/// unchanged.
#[cfg(target_os = "linux")]
pub(crate) fn desktop_env<I>(vars: I) -> Vec<(std::ffi::OsString, std::ffi::OsString)>
where
    I: IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
{
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let vars: Vec<_> = vars.into_iter().collect();
    let appdir = vars
        .iter()
        .find(|(k, _)| k == "APPDIR")
        .map(|(_, v)| v.as_bytes().strip_suffix(b"/").unwrap_or(v.as_bytes()).to_vec())
        .filter(|a| a.len() > 1);
    let Some(appdir) = appdir else { return vars };
    let inside = |p: &[u8]| p == appdir.as_slice() || (p.starts_with(&appdir) && p.get(appdir.len()) == Some(&b'/'));
    let own = ["APPDIR", "APPIMAGE", "ARGV0", "OWD", "APPIMAGE_SILENT_INSTALL"];
    vars.into_iter()
        .filter(|(k, _)| !own.iter().any(|o| k == o))
        .filter_map(|(k, v)| {
            let kept: Vec<&[u8]> = v.as_bytes().split(|&b| b == b':').filter(|p| !inside(p)).collect();
            if kept.iter().all(|p| p.is_empty()) && !v.is_empty() {
                return None;
            }
            Some((k, std::ffi::OsString::from_vec(kept.join(&b':'))))
        })
        .collect()
}

/// `file://` + the path, percent-encoded except for the unreserved characters
/// and `/` (RFC 3986), so a space or a non-ASCII name survives D-Bus.
#[cfg(target_os = "linux")]
fn file_uri(p: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut s = String::from("file://");
    for &b in p.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect()
    }

    #[test]
    fn outside_an_appimage_the_environment_is_unchanged() {
        let e = env(&[("PATH", "/usr/bin:/bin"), ("LD_LIBRARY_PATH", "/opt/x/lib"), ("DISPLAY", ":0")]);
        assert_eq!(desktop_env(e.clone()), e);
    }

    #[test]
    fn inside_an_appimage_its_own_paths_are_taken_out() {
        let e = env(&[
            ("APPDIR", "/tmp/.mount_AInodeX1/"),
            ("APPIMAGE", "/home/u/AInode.AppImage"),
            ("PATH", "/tmp/.mount_AInodeX1/usr/bin:/usr/bin:/bin"),
            ("LD_LIBRARY_PATH", "/tmp/.mount_AInodeX1/usr/lib"),
            ("XDG_DATA_DIRS", "/tmp/.mount_AInodeX1/usr/share:/usr/local/share:/usr/share"),
            ("GTK_PATH", "/tmp/.mount_AInodeX1/usr/lib/gtk-3.0"),
            ("DISPLAY", ":0"),
            ("HOME", "/tmp/.mount_AInodeX10"), // a different dir that merely shares the prefix
            ("EMPTY", ""),
        ]);
        let got = desktop_env(e);
        let get = |k: &str| got.iter().find(|(n, _)| n == k).map(|(_, v)| v.to_string_lossy().to_string());
        assert_eq!(get("PATH").as_deref(), Some("/usr/bin:/bin"));
        assert_eq!(get("XDG_DATA_DIRS").as_deref(), Some("/usr/local/share:/usr/share"));
        assert_eq!(get("LD_LIBRARY_PATH"), None);
        assert_eq!(get("GTK_PATH"), None);
        assert_eq!(get("APPDIR"), None);
        assert_eq!(get("APPIMAGE"), None);
        assert_eq!(get("DISPLAY").as_deref(), Some(":0"));
        assert_eq!(get("HOME").as_deref(), Some("/tmp/.mount_AInodeX10"));
        assert_eq!(get("EMPTY").as_deref(), Some(""));
    }

    #[test]
    fn file_uris_are_percent_encoded() {
        assert_eq!(file_uri(Path::new("/home/u/a b/ü.gguf")), "file:///home/u/a%20b/%C3%BC.gguf");
        assert_eq!(file_uri(Path::new("/x/y_z-1.~")), "file:///x/y_z-1.~");
    }

    #[tokio::test]
    async fn inside_wsl_windows_opens_what_xdg_open_cannot() {
        if !in_wsl() {
            return; // a Linux desktop: xdg-open is the whole story
        }
        assert!(windows_opener().is_some());
        let w = windows_path_of(Path::new("/tmp")).await.expect("wslpath -w /tmp");
        assert!(w.to_string_lossy().starts_with(r"\\wsl"), "{w:?}");
    }
}
