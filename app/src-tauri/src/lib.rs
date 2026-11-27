//! AInode: a thin, honest shell around upstream's own btx-modeld plus a local
//! llama.cpp runtime for chat. The rules and every observed fact this code
//! relies on are in ../ENGINE-CONTRACT.md; read that first.
//!
//! Layout:
//!   rpc      the helper's unix JSON-RPC, one call in flight at a time
//!   engine   spawn / supervise / stop btx-modeld
//!   chat     one llama-server on loopback
//!   catalog  bundled catalog + hosts, joined with listmodels / getmodeljob
//!   state    paths, consent.json, settings.json, removed.json
//!   update   the updater, each offer bound to its version and platform
//!   own      the user's own files (text read from dropped PDF/Markdown)
//!   os       Finder / file manager and browser, per platform
//!   wsl      Windows: the engine inside AInode's own WSL distribution
//!   acl      Windows: the data folder's permissions, the user's alone
//!   backend  what each UI command does, Tauri-free (the integration test
//!            drives it against a real helper)
//!   commands the Tauri wrappers

#[cfg(windows)]
pub mod acl;
pub mod backend;
pub mod catalog;
pub mod chat;
pub mod chats;
mod commands;
pub mod disk;
pub mod engine;
pub mod login;
pub mod neighbour;
pub mod os;
pub mod own;
mod restart;
pub mod rpc;
pub mod state;
pub mod store_once;
pub mod update;
pub mod validate;
#[cfg(windows)]
pub mod wsl;

use std::sync::Arc;

use tauri::{Manager, RunEvent};

use crate::backend::Backend;
use crate::state::Paths;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    no_gpu_disk_caches();
    let app = tauri::Builder::default()
        // Must be the first plugin (its docs): it decides before anything
        // else starts whether this process is the app or just a messenger.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        // Every update check decides through update::comparator: an offer is
        // taken only when each file is signed under its release name.
        .plugin(
            tauri_plugin_updater::Builder::new()
                .default_version_comparator(update::comparator)
                .build(),
        )
        .manage(update::Pending::default())
        .setup(|app| {
            if let Err(e) = setup(app) {
                // A desktop launch has no console (windows_subsystem =
                // "windows"): without this, a failure here would be an app
                // that never appears.
                os::fatal(&format!("AInode could not start.\n\n{e}"));
                std::process::exit(1);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::accept_consent,
            commands::places,
            commands::relaunch,
            commands::start_engine,
            commands::restart_wsl,
            commands::stop_engine,
            commands::catalog,
            commands::download,
            commands::cancel,
            commands::remove,
            commands::reveal,
            commands::chat_start,
            commands::chat_stop,
            commands::set_theme,
            commands::set_pref,
            commands::set_face,
            commands::set_model_prefs,
            commands::knowledge_list,
            commands::knowledge_add,
            commands::knowledge_remove,
            commands::knowledge_passages,
            commands::network_bounties,
            commands::network_campaigns,
            commands::network_search,
            commands::network_start_search,
            commands::index_peers,
            commands::index_peer_add,
            commands::index_peer_remove,
            commands::link_open,
            commands::link_get,
            commands::links,
            commands::link_reveal,
            commands::link_remove,
            commands::chats_list,
            commands::chat_load,
            commands::chat_save,
            commands::chat_delete,
            commands::add_peer,
            commands::open_url,
            commands::update_check,
            commands::update_install,
            commands::system_info,
            commands::neighbour,
        ])
        .build(tauri::generate_context!())
        .expect("error while building AInode");

    app.run(|handle, event| {
        // Quit (Cmd+Q or the last window closing). Stop llama-server and the
        // helper before the process goes: the helper is a separate process
        // and would otherwise outlive us holding the socket and the store.
        // Bounded by Engine::stop (SIGKILL at 8 s).
        if let RunEvent::Exit = event {
            if let Some(b) = handle.try_state::<Arc<Backend>>() {
                let b = b.inner().clone();
                tauri::async_runtime::block_on(async move {
                    b.shutdown().await;
                    // Only AInode's own distribution, so WSL gives its memory back.
                    #[cfg(windows)]
                    wsl::terminate().await;
                });
            }
            // An update or a moved data folder asked for a restart (restart.rs).
            #[cfg(target_os = "linux")]
            restart::on_exit(handle);
        }
    });
}

/// Paths, the window (Linux and Windows), the backend and its launch cleanup.
fn setup(app: &mut tauri::App) -> Result<(), String> {
    let paths = Paths::resolve(app.path().resource_dir().ok().as_deref())?;
    update::set_data_dir(&paths.data_dir);
    #[cfg(any(target_os = "linux", windows))]
    open_main_window(app, &paths).map_err(|e| e.to_string())?;
    // Started with the computer (login.rs): stay out of the way, minimized.
    // Opening AInode again brings the window up (the single-instance plugin).
    if login::launched_at_login() {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.minimize();
        }
    }
    let backend = Arc::new(Backend::new(paths.clone(), app.package_info().version.to_string()));
    // After consent, the start entry follows the setting and names this
    // program as it is now (an AppImage moved, an update in a new folder).
    backend.apply_login();
    // A crashed earlier run may have left a socket or a helper behind.
    // Clean up; start nothing (the engine starts on the UI's request,
    // and never before consent.json exists).
    let b = backend.clone();
    tauri::async_runtime::spawn(async move { b.startup_cleanup().await });
    // The engine's self-heal and the check-in, whether or not the window's
    // page runs its timers (backend/keeper.rs).
    // Two loops: a long recovery never holds up the check-in.
    let b = backend.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            b.heal_tick().await;
            tokio::time::sleep(backend::KEEP_TICK).await;
        }
    });
    let b = backend.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            b.checkin_tick().await;
            tokio::time::sleep(backend::KEEP_TICK).await;
        }
    });
    update::selftest(app.handle(), &backend, &paths);
    app.manage(backend);
    Ok(())
}

/// Linux: WebKitGTK's network process writes salts and an HSTS database into
/// the data directory of the app's web context as soon as the window opens,
/// even for an incognito webview (observed 2026-10-13: a fresh launch, nothing
/// clicked, left `~/.local/share/com.m2promende.ainode/` behind). Rule 1:
/// nothing is written before consent. So the main window is created here
/// (`create: false` in tauri.linux.conf.json, the same window otherwise; a
/// test keeps the two in step) with its data directory in AInode's private
/// runtime folder beside the engine's socket: $XDG_RUNTIME_DIR, a per-user
/// tmpfs that is gone at logout.
///
/// Windows: WebView2 keeps its user data folder (EBWebView) the same way, by
/// default in `%LOCALAPPDATA%\com.m2promende.ainode`; it goes to the runtime
/// folder under `%TEMP%` instead (tauri.windows.conf.json).
#[cfg(any(target_os = "linux", windows))]
fn open_main_window(app: &tauri::App, paths: &Paths) -> Result<(), Box<dyn std::error::Error>> {
    engine::prepare_private_dir(&paths.socket_dir())?;
    let cfg = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .ok_or("no main window in the config")?
        .clone();
    #[cfg(windows)]
    let cfg = {
        let mut cfg = cfg;
        // The room for the window: the screen without the taskbar, in the
        // window's own units.
        let screen = app.primary_monitor().ok().flatten().map(|m| {
            let area = m.work_area().size;
            (area.width as f64 / m.scale_factor(), area.height as f64 / m.scale_factor())
        });
        let scale = windows_text_scale();
        (cfg.width, cfg.height, cfg.min_width, cfg.min_height) =
            grown_for_text_size(cfg.width, cfg.height, cfg.min_width, cfg.min_height, scale, screen);
        // A grown window is centred: where Windows would put it by itself,
        // its bottom edge can end up under the taskbar.
        if scale > 1.0 {
            cfg.center = true;
        }
        cfg
    };
    // WebDriver (tauri-driver, which sets TAURI_WEBVIEW_AUTOMATION) can only
    // attach to the web context Tauri makes, not to the ephemeral one wry
    // makes for an incognito webview. Under automation the window is not
    // incognito; its data still stays in the runtime folder.
    let automation = std::env::var("TAURI_WEBVIEW_AUTOMATION").as_deref() == Ok("true");
    tauri::WebviewWindowBuilder::from_config(app.handle(), &cfg)?
        .data_directory(paths.socket_dir().join("webview"))
        .incognito(cfg.incognito && !automation)
        .build()?;
    Ok(())
}

/// Windows: "Text size" under Accessibility (100 to 225 %). WebView2 draws
/// the whole page that much larger, but Tauri sizes the window by the
/// display's scale alone: at 143 % the page had 322 of its 460 px and the
/// lines at the bottom wrapped (measured 2026-11-25 on the PC, every version).
/// 1.0 when the setting is not there.
#[cfg(windows)]
fn windows_text_scale() -> f64 {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let key: Vec<u16> = "Software\\Microsoft\\Accessibility\0".encode_utf16().collect();
    let name: Vec<u16> = "TextScaleFactor\0".encode_utf16().collect();
    let mut value: u32 = 0;
    let mut len: u32 = 4;
    // SAFETY: both strings end in NUL, and `value` is the 4 bytes `len` names.
    let rc = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&mut value as *mut u32).cast(), &mut len)
    };
    if rc == 0 && (100..=225).contains(&value) {
        value as f64 / 100.0
    } else {
        1.0
    }
}

/// The window's size and smallest size for a text size of `scale`: wider by
/// that factor, so the page keeps the width it was designed for, and taller
/// by it too, but never larger than the screen leaves room for (`screen`:
/// the screen without the taskbar, width and height in the window's own
/// units; 40 of the height are the window's own title bar). A screen smaller
/// than the designed window never shrinks it.
#[cfg(any(windows, test))]
fn grown_for_text_size(w: f64, h: f64, min_w: Option<f64>, min_h: Option<f64>, scale: f64, screen: Option<(f64, f64)>) -> (f64, f64, Option<f64>, Option<f64>) {
    if !(scale > 1.0 && scale <= 2.25) {
        return (w, h, min_w, min_h);
    }
    let room_w = screen.map(|(sw, _)| sw.max(w)).unwrap_or(f64::MAX);
    let room_h = screen.map(|(_, sh)| (sh - 40.0).max(h)).unwrap_or(f64::MAX);
    let fit_w = |v: f64| (v * scale).round().min(room_w);
    let fit_h = |v: f64| (v * scale).round().min(room_h);
    (fit_w(w), fit_h(h), min_w.map(fit_w), min_h.map(fit_h))
}

/// Linux: the GPU drivers keep compiled shaders on disk (Mesa in
/// ~/.cache/mesa_shader_cache, NVIDIA in ~/.cache/nvidia) for every program
/// that draws with them, the webview included: a write before consent. The
/// app draws a few CSS and canvas layers, so compiling them at each start
/// costs nothing a user would notice. Set before GTK starts, so the webview's
/// processes inherit it.
#[cfg(target_os = "linux")]
fn no_gpu_disk_caches() {
    for (k, v) in [("MESA_SHADER_CACHE_DISABLE", "true"), ("MESA_GLSL_CACHE_DISABLE", "true"), ("__GL_SHADER_DISK_CACHE", "0")] {
        std::env::set_var(k, v);
    }
    // Fontconfig writes a per-user cache under $XDG_CACHE_HOME when the
    // system's is missing or stale (seen on a fresh Debian 13, 2026-10-14:
    // ~/.cache/fontconfig after a launch with nothing clicked). AInode keeps
    // no cache of its own, so for this process that folder is the private
    // runtime folder, a per-user tmpfs gone at logout.
    if let Some(run) = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).filter(|p| p.is_absolute() && p.is_dir()) {
        let cache = run.join("ainode").join("cache");
        if engine::prepare_private_dir(&cache).is_ok() {
            std::env::set_var("XDG_CACHE_HOME", cache);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_window_grows_with_windows_text_size_and_stops_at_the_screen() {
        use super::grown_for_text_size as g;
        // No setting, or one outside what Windows offers: the window as designed.
        assert_eq!(g(460.0, 760.0, Some(420.0), Some(640.0), 1.0, Some((1920.0, 1040.0))), (460.0, 760.0, Some(420.0), Some(640.0)));
        assert_eq!(g(460.0, 760.0, Some(420.0), Some(640.0), 9.0, Some((1920.0, 1040.0))), (460.0, 760.0, Some(420.0), Some(640.0)));
        // 143 % on a tall screen: the page keeps 460 px (658 / 1.43).
        assert_eq!(g(460.0, 760.0, Some(420.0), Some(640.0), 1.43, Some((2560.0, 1600.0))), (658.0, 1087.0, Some(601.0), Some(915.0)));
        // 225 % on a 1080 screen: as wide as the page needs, as tall as the screen has.
        assert_eq!(g(460.0, 760.0, Some(420.0), Some(640.0), 2.25, Some((1920.0, 1080.0))), (1035.0, 1040.0, Some(945.0), Some(1040.0)));
        // A laptop: 1080 at 125 % leaves 816 after the taskbar; 143 % text stops there less the title bar.
        assert_eq!(g(460.0, 760.0, Some(420.0), Some(640.0), 1.43, Some((1536.0, 816.0))), (658.0, 776.0, Some(601.0), Some(776.0)));
        // A screen shorter than the designed window never shrinks it.
        assert_eq!(g(460.0, 760.0, None, None, 1.5, Some((1280.0, 700.0))).1, 760.0);
        // A small panel (1280 x 800 at 150 %: 853 x 760 after the taskbar) at 225 % text:
        // no wider than the screen, and the smallest size with it.
        assert_eq!(g(460.0, 760.0, Some(420.0), Some(640.0), 2.25, Some((853.0, 760.0))), (853.0, 760.0, Some(853.0), Some(760.0)));
    }

    /// The Linux window is the shared one, created from code (see
    /// open_main_window): only `create` may differ.
    #[test]
    fn the_linux_and_windows_windows_are_the_shared_window() {
        let base: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let mut want = base["app"]["windows"][0].clone();
        want["create"] = serde_json::Value::Bool(false);
        for (name, conf) in [("tauri.linux.conf.json", include_str!("../tauri.linux.conf.json")), ("tauri.windows.conf.json", include_str!("../tauri.windows.conf.json"))] {
            let platform: serde_json::Value = serde_json::from_str(conf).unwrap();
            assert_eq!(platform["app"]["windows"], serde_json::json!([want.clone()]), "copy tauri.conf.json's window into {name}, with \"create\": false");
        }
    }

    /// The webview gets the four Tauri core commands the UI calls and no
    /// other (audit L5, #119): Tauri's own IPC check with the real
    /// capabilities/default.json, on every platform. The commands
    /// `core:default` allowed are refused by the ACL; the four pass it (what
    /// the mock runtime then answers does not matter here).
    // Not on Windows: see the dev-dependency in Cargo.toml.
    #[cfg(not(windows))]
    #[test]
    fn the_webview_gets_four_core_commands_and_no_more() {
        use tauri::ipc::{CallbackFn, InvokeBody};
        use tauri::webview::InvokeRequest;
        let app = tauri::test::mock_builder().build(tauri::generate_context!(test = true)).unwrap();
        let w = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        let acl_refuses = |cmd: &str, body: &serde_json::Value| -> bool {
            let r = tauri::test::get_ipc_response(
                &w,
                InvokeRequest {
                    cmd: cmd.into(),
                    callback: CallbackFn(0),
                    error: CallbackFn(1),
                    url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
                    body: InvokeBody::Json(body.clone()),
                    headers: Default::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_string(),
                },
            );
            matches!(&r, Err(e) if e.to_string().contains("not allowed"))
        };
        let none = serde_json::json!({});
        let label = serde_json::json!({ "label": "main" });
        let listen = serde_json::json!({ "event": "tauri://focus", "target": { "kind": "Any" }, "handler": 7 });
        let unlisten = serde_json::json!({ "event": "tauri://focus", "eventId": 1 });
        for (cmd, body) in [
            ("plugin:event|listen", &listen),
            ("plugin:event|unlisten", &unlisten),
            ("plugin:window|is_minimized", &label),
            ("plugin:window|start_dragging", &label),
        ] {
            assert!(!acl_refuses(cmd, body), "{cmd} must stay allowed: the UI calls it");
        }
        for (cmd, body) in [
            ("plugin:window|title", &label),
            ("plugin:window|minimize", &label),
            ("plugin:window|close", &label),
            ("plugin:window|set_title", &serde_json::json!({ "label": "main", "value": "x" })),
            ("plugin:window|internal_toggle_maximize", &label),
            ("plugin:app|version", &none),
            ("plugin:app|name", &none),
            ("plugin:webview|webview_position", &label),
            ("plugin:path|resolve_directory", &serde_json::json!({ "directory": 21 })),
            ("plugin:event|emit", &serde_json::json!({ "event": "x", "payload": null })),
            ("plugin:menu|new", &none),
            ("plugin:resources|close", &serde_json::json!({ "rid": 1 })),
            ("plugin:image|from_path", &serde_json::json!({ "path": "/etc/hosts" })),
        ] {
            assert!(acl_refuses(cmd, body), "{cmd} must be refused: core:default allowed it, the UI does not use it");
        }
    }
}
