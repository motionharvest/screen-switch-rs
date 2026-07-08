// No console window: run as a background GUI-subsystem process. All output goes
// to %LOCALAPPDATA%\ScreenSwitchRs\log.txt.
#![windows_subsystem = "windows"]

//! screen-switch-rs
//!
//! Watches the mouse cursor and fires a per-monitor keyboard shortcut whenever
//! the cursor moves onto a different monitor (once per crossing).
//!
//! Detection uses a tight dedicated-thread poll (physical coordinates, Per-
//! Monitor-V2 DPI aware). Unlike a message-loop timer or a managed low-level
//! hook, this loop cannot be starved or silently removed, so crossings are not
//! missed regardless of speed or how the monitors are arranged.

mod config;
mod keys;
mod monitors;

use std::ffi::c_void;
use std::io::Write;
use std::mem::size_of;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

use config::Config;

const POLL_INTERVAL_MS: u64 = 8;
const CONFIG_RELOAD_SECS: u64 = 2;

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let elevated = is_elevated();
    let monitors = monitors::enumerate();
    log("========================================");
    log(&format!(
        "screen-switch-rs started. Elevated: {}. Detected {} monitor(s):",
        elevated,
        monitors.len()
    ));
    if !elevated {
        log("WARNING: not elevated. Keys injected into an elevated foreground window");
        log("         (e.g. an app shown as [Administrator]) are SILENTLY DROPPED by");
        log("         Windows UIPI even though injection reports success. Use install.ps1");
        log("         or run.ps1 to run elevated if a target app runs as admin.");
    }
    for m in &monitors {
        let flag = if m.primary { " (primary)" } else { "" };
        let (l, t, r, b) = m.rect;
        log(&format!(
            "  Monitor {}{}  device={}  rect=({l},{t})-({r},{b})",
            m.number, flag, m.device
        ));
    }

    let config_path = config::default_path();
    let mut cfg = Config::load_or_template(&config_path, &monitors);
    log(&format!("Config: {}", config_path.display()));
    log("Edit config.json to set a shortcut per device (e.g. \"ctrl+shift+home\"). Reloads automatically.");
    print_config(&cfg);

    let mut current = monitors::monitor_at_cursor().unwrap_or(0);
    let mut config_mtime = mtime(&config_path);
    let mut last_reload_check = Instant::now();

    log("Watching for monitor switches... (Ctrl+C to quit)");

    loop {
        thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

        if let Some(hmon) = monitors::monitor_at_cursor() {
            if hmon != current {
                current = hmon;
                on_monitor_change(hmon, &cfg);
            }
        }

        if last_reload_check.elapsed() >= Duration::from_secs(CONFIG_RELOAD_SECS) {
            last_reload_check = Instant::now();
            let m = mtime(&config_path);
            if m != config_mtime {
                config_mtime = m;
                if let Some(new_cfg) = Config::load(&config_path) {
                    cfg = new_cfg;
                    log("(config reloaded)");
                    print_config(&cfg);
                }
            }
        }
    }
}

fn on_monitor_change(hmon: isize, cfg: &Config) {
    let device = monitors::device_name(hmon).unwrap_or_default();

    match cfg.shortcuts.get(&device) {
        Some(spec) if !spec.trim().is_empty() => match keys::parse(spec) {
            Ok((mods, key)) => {
                let fg = foreground_title();
                let hold = cfg.hold_ms.unwrap_or(keys::DEFAULT_HOLD_MS);
                let (di, de, ui, ue) = keys::send(&mods, key, hold);
                log(&format!(
                    "Switched to {device} -> sent '{spec}' (hold {hold}ms) to foreground='{fg}' (down {di}/{de}, up {ui}/{ue})"
                ));
            }
            Err(e) => log(&format!("Switched to {device} -> bad shortcut '{spec}': {e}")),
        },
        Some(_) => log(&format!("Switched to {device} -> no shortcut set")),
        None => log(&format!("Switched to {device} -> device not in config")),
    }
}

fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        let _ = CloseHandle(token);
        ok.is_ok() && elevation.TokenIsElevated != 0
    }
}

fn foreground_title() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == HWND::default() {
            return String::new();
        }
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

fn print_config(cfg: &Config) {
    log(&format!(
        "Hold time: {}ms",
        cfg.hold_ms.unwrap_or(keys::DEFAULT_HOLD_MS)
    ));
    log("Current shortcuts:");
    if cfg.shortcuts.is_empty() {
        log("  (none)");
    }
    for (device, spec) in &cfg.shortcuts {
        let shown = if spec.trim().is_empty() { "(unset)" } else { spec };
        log(&format!("  {device} = {shown}"));
    }
}

fn mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

fn log(message: &str) {
    let st = unsafe { GetLocalTime() };
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}  {}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, message
    );
    println!("{line}");

    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        let dir = Path::new(&base).join("ScreenSwitchRs");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("log.txt"))
        {
            let _ = writeln!(f, "{line}");
        }
    }
}
