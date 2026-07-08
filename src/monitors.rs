//! Monitor enumeration and cursor-to-monitor resolution (physical coordinates).

use std::ffi::c_void;
use std::mem::size_of;

use windows::Win32::Foundation::{BOOL, LPARAM, POINT, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};

const MONITORINFOF_PRIMARY: u32 = 0x0000_0001;

#[derive(Clone, Debug)]
pub struct MonitorInfo {
    pub number: u32,
    pub device: String,
    pub rect: (i32, i32, i32, i32),
    pub primary: bool,
}

/// Enumerate monitors, numbered primary-first then left-to-right.
pub fn enumerate() -> Vec<MonitorInfo> {
    let mut handles: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(enum_proc),
            LPARAM(&mut handles as *mut _ as isize),
        );
    }

    let mut infos: Vec<MonitorInfo> = handles.iter().filter_map(|&h| info_for(h)).collect();
    infos.sort_by(|a, b| {
        b.primary
            .cmp(&a.primary)
            .then(a.rect.0.cmp(&b.rect.0))
            .then(a.rect.1.cmp(&b.rect.1))
            .then(a.device.cmp(&b.device))
    });
    for (i, m) in infos.iter_mut().enumerate() {
        m.number = (i + 1) as u32;
    }
    infos
}

extern "system" fn enum_proc(h: HMONITOR, _hdc: HDC, _rc: *mut RECT, data: LPARAM) -> BOOL {
    unsafe {
        let list = &mut *(data.0 as *mut Vec<isize>);
        list.push(h.0 as isize);
    }
    TRUE
}

fn info_for(handle: isize) -> Option<MonitorInfo> {
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;

    let ok = unsafe {
        GetMonitorInfoW(
            HMONITOR(handle as *mut c_void),
            &mut mi as *mut _ as *mut MONITORINFO,
        )
    };
    if !ok.as_bool() {
        return None;
    }

    let device = device_string(&mi.szDevice);
    let r = mi.monitorInfo.rcMonitor;
    let primary = (mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;

    Some(MonitorInfo {
        number: 0,
        device,
        rect: (r.left, r.top, r.right, r.bottom),
        primary,
    })
}

/// HMONITOR under the current cursor position, as a raw isize identity.
pub fn monitor_at_cursor() -> Option<isize> {
    let mut point = POINT::default();
    let ok = unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) };
    if ok.is_err() {
        return None;
    }
    let hmon = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    if hmon.0.is_null() {
        None
    } else {
        Some(hmon.0 as isize)
    }
}

/// Resolve the device name (e.g. `\\.\DISPLAY1`) for a monitor handle.
pub fn device_name(handle: isize) -> Option<String> {
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let ok = unsafe {
        GetMonitorInfoW(
            HMONITOR(handle as *mut c_void),
            &mut mi as *mut _ as *mut MONITORINFO,
        )
    };
    if !ok.as_bool() {
        return None;
    }
    Some(device_string(&mi.szDevice))
}

fn device_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
