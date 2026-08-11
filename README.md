# screen-switch-rs

Watches the mouse cursor and fires a **per-monitor keyboard shortcut** whenever
the cursor moves onto a different monitor — once per crossing.

This is a native Rust rewrite of the C# `screen-switch-keypress` app. The C#
version kept missing crossings; the root causes were:

- **Managed low-level mouse hooks get silently removed** by Windows when the
  callback occasionally exceeds the ~300 ms `LowLevelHooksTimeout` (CLR/GC
  latency), after which no more moves arrive.
- The safety-net poll was a **WinForms timer**, which is message-loop-bound and
  gets starved under load.
- `SendInput` failed with `ERROR_INVALID_PARAMETER (87)` due to a 32-vs-40 byte
  `INPUT` union sizing bug and silently fell back to `keybd_event`.

## How this version is reliable

- **Detection: a tight dedicated-thread poll (~8 ms)** using `GetCursorPos` +
  `MonitorFromPoint` on physical coordinates. A plain thread loop cannot be
  starved by a message loop and cannot be silently unhooked, so it catches every
  crossing regardless of speed or monitor arrangement. Verified: 15/15 and
  further test crossings across three askew, mixed-DPI monitors, zero misses.
- **DPI: Per-Monitor-V2 aware** (`SetProcessDpiAwarenessContext`), so coordinates
  are correct even with mixed scaling (e.g. one monitor at 150%).
- **Injection: `SendInput` via the `windows` crate**, which builds the `INPUT`
  union at the correct size — no `Win32Error=87`. Modifiers + key are pressed
  together as a chord, held briefly, then released in reverse. Verified end to
  end against a registered global hotkey (`WM_HOTKEY` fired every time,
  `down 3/3, up 3/3`).

## Requirements

- Windows with the MSVC Rust toolchain (`cargo`, `rustc`).
- Building from WSL: these scripts invoke the Windows-side `cargo` and write the
  build output to a local Windows path for speed.

## Build

```powershell
.\build.ps1
```

Output: `%LOCALAPPDATA%\screen-switch-rs\target\release\screen-switch-rs.exe`

## Run (foreground, watch the logs)

```powershell
.\run.ps1
```

Runs elevated so injected keys can reach elevated target apps. The console logs
every detected switch and injection result.

## Config TUI

```powershell
.\tui.ps1
```

Opens an interactive console (`screen-switch-tui`) for configuring everything
without hand-editing JSON. It edits the same `config.json` the background watcher
hot-reloads, so changes take effect within ~2s while the service keeps running.

- Spatial map of monitors as rounded cards (name underneath); the monitor under
  the cursor is highlighted in pink (`#F445F7`). Selection uses a white outline.
- Click a card (or `←/→`) to select; click again (or `Enter`) to capture a chord.
- `e` type a shortcut string instead; `x` clears it.
- Click the header **hold** value (or `h`) to edit chord hold time (ms).
- `t` test-fire the selected monitor's shortcut.
- `r` re-scan monitors; `q` quits.
- `Esc` cancels capture/edit modes; from the normal view, `Esc` quits.

All edits auto-save immediately.

## Configure

On first run it writes `%LOCALAPPDATA%\ScreenSwitchRs\config.json` (shared by the
console, elevated, and installed launches), pre-filled with your detected monitor
device names and empty shortcuts:

```json
{
  "hold_ms": 90,
  "shortcuts": {
    "\\\\.\\DISPLAY1": "ctrl+shift+home",
    "\\\\.\\DISPLAY6": "ctrl+shift+insert"
  }
}
```

- `hold_ms` = how long the chord is held down. Increase it (e.g. 120–150) if a
  target that *polls* key state (many global key-detectors use `GetAsyncKeyState`
  on a timer) intermittently misses the press. Omit it to use the default (90).
- Key = monitor device name (printed at startup, e.g. `\\.\DISPLAY1`).
- Value = shortcut string; empty means "do nothing" for that monitor.
- The file is **reloaded automatically** (~every 2 s) — no restart needed.

Shortcut syntax: `+`-separated, modifiers first. Modifiers: `ctrl`, `alt`,
`shift`, `win`. Keys: `a`–`z`, `0`–`9`, `f1`–`f24`, `home`, `end`, `insert`,
`delete`, `pageup`, `pagedown`, `left/right/up/down`, `space`, `enter`, `tab`,
`esc`, and common punctuation. Example: `ctrl+shift+alt+r`.

## Install (auto-start at logon, no UAC prompt)

```powershell
.\install.ps1     # elevates, copies to a fixed location, registers a Scheduled Task
.\uninstall.ps1   # removes it
```

The Scheduled Task runs at logon with highest privileges, so there is no UAC
prompt on subsequent launches, and it always loads the current build location.

## Elevation matters

Windows UIPI blocks a lower-integrity process from injecting input into a
higher-integrity (elevated) window. If a target app runs **as administrator**
(e.g. shown as `[Administrator]`), a non-elevated build's `SendInput` will
*report success but the keys are silently dropped*. Run elevated (via
`install.ps1` or `run.ps1`) so injection reaches elevated apps. The startup log
records `Elevated: true/false`, and each fire logs the foreground window title.

## Logs

`%LOCALAPPDATA%\ScreenSwitchRs\log.txt` records startup (elevation, monitors),
every detected switch, the foreground window the shortcut was sent to, and the
`SendInput` result.

## Layout

Two binaries share a small library:

- `src/lib.rs` — re-exports the shared modules below.
- `src/config.rs` — JSON config load / template (BOM-tolerant).
- `src/monitors.rs` — monitor enumeration + cursor-to-monitor resolution.
- `src/keys.rs` — shortcut parsing + `SendInput` chord injection.
- `src/main.rs` — **watcher** binary: DPI setup, poll loop, config hot-reload
  (windowless, `#![windows_subsystem = "windows"]`).
- `src/bin/tui.rs` — **config console** binary (`ratatui`/`crossterm`).
