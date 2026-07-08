//! Shortcut parsing and hardware-like keystroke injection.

use std::mem::size_of;
use std::thread;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
};

const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12; // Alt
const VK_LWIN: u16 = 0x5B;

/// Default chord hold time. Long enough that a polling key-detector
/// (e.g. GetAsyncKeyState on a 50-100ms timer) reliably samples the press.
pub const DEFAULT_HOLD_MS: u64 = 90;

/// Parse a shortcut like `"ctrl+shift+home"` into (modifier VKs, main-key VK).
pub fn parse(spec: &str) -> Result<(Vec<u16>, u16), String> {
    let mut mods: Vec<u16> = Vec::new();
    let mut key: Option<u16> = None;

    for raw in spec.split('+') {
        let part = raw.trim().to_lowercase();
        if part.is_empty() {
            continue;
        }
        match part.as_str() {
            "ctrl" | "control" => push_unique(&mut mods, VK_CONTROL),
            "alt" | "menu" => push_unique(&mut mods, VK_MENU),
            "shift" => push_unique(&mut mods, VK_SHIFT),
            "win" | "super" | "meta" | "cmd" => push_unique(&mut mods, VK_LWIN),
            other => {
                let vk = vk_from_name(other)
                    .ok_or_else(|| format!("unknown key '{other}'"))?;
                if key.is_some() {
                    return Err("more than one non-modifier key".to_string());
                }
                key = Some(vk);
            }
        }
    }

    key.map(|k| (mods, k)).ok_or_else(|| "no main key".to_string())
}

fn push_unique(v: &mut Vec<u16>, item: u16) {
    if !v.contains(&item) {
        v.push(item);
    }
}

/// Press modifiers + key together, hold `hold_ms`, then release in reverse order.
/// Returns (down_injected, down_expected, up_injected, up_expected).
pub fn send(mods: &[u16], key: u16, hold_ms: u64) -> (u32, usize, u32, usize) {
    let mut down: Vec<INPUT> = Vec::with_capacity(mods.len() + 1);
    for &m in mods {
        down.push(make_input(m, false));
    }
    down.push(make_input(key, false));

    let mut up: Vec<INPUT> = Vec::with_capacity(mods.len() + 1);
    up.push(make_input(key, true));
    for &m in mods.iter().rev() {
        up.push(make_input(m, true));
    }

    let cb = size_of::<INPUT>() as i32;
    let down_ok = unsafe { SendInput(&down, cb) };
    thread::sleep(Duration::from_millis(hold_ms));
    let up_ok = unsafe { SendInput(&up, cb) };

    (down_ok, down.len(), up_ok, up.len())
}

fn make_input(vk: u16, key_up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;

    let mut flags = KEYBD_EVENT_FLAGS(0);
    // Provide both the virtual key and the scan code (no SCANCODE flag) so both
    // VK-based listeners (RegisterHotKey, GetAsyncKeyState) and scan-based
    // consumers see a consistent event.
    let _ = KEYEVENTF_SCANCODE;
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn is_extended(vk: u16) -> bool {
    matches!(
        vk,
        0x2D // Insert
            | 0x2E // Delete
            | 0x24 // Home
            | 0x23 // End
            | 0x21 // PageUp
            | 0x22 // PageDown
            | 0x25 // Left
            | 0x26 // Up
            | 0x27 // Right
            | 0x28 // Down
            | 0x5B // LWin
            | 0x5C // RWin
            | 0xA3 // RControl
            | 0xA5 // RMenu
            | 0x90 // NumLock
            | 0x2C // PrintScreen
    )
}

/// Map a key name (already lowercased) to a virtual-key code.
fn vk_from_name(name: &str) -> Option<u16> {
    // Single letters a-z
    if name.len() == 1 {
        let c = name.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_uppercase() as u16);
        }
        if c.is_ascii_digit() {
            return Some(c as u16);
        }
    }

    // Function keys f1..f24
    if let Some(rest) = name.strip_prefix('f') {
        if let Ok(n) = rest.parse::<u16>() {
            if (1..=24).contains(&n) {
                return Some(0x70 + (n - 1)); // VK_F1 = 0x70
            }
        }
    }

    let vk = match name {
        "home" => 0x24,
        "end" => 0x23,
        "insert" | "ins" => 0x2D,
        "delete" | "del" => 0x2E,
        "pageup" | "pgup" | "prior" => 0x21,
        "pagedown" | "pgdn" | "next" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "space" | "spacebar" => 0x20,
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "backspace" | "bksp" => 0x08,
        "capslock" => 0x14,
        "printscreen" | "prtsc" => 0x2C,
        "pause" => 0x13,
        "numlock" => 0x90,
        "scrolllock" => 0x91,
        "minus" | "-" => 0xBD,
        "plus" | "=" => 0xBB,
        "comma" | "," => 0xBC,
        "period" | "." => 0xBE,
        "slash" | "/" => 0xBF,
        "backslash" | "\\" => 0xDC,
        "semicolon" | ";" => 0xBA,
        "quote" | "'" => 0xDE,
        "lbracket" | "[" => 0xDB,
        "rbracket" | "]" => 0xDD,
        "grave" | "`" => 0xC0,
        _ => return None,
    };
    Some(vk)
}
