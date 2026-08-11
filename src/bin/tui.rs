//! screen-switch-tui — interactive config console for screen-switch-rs.
//!
//! Spatial monitor map with rounded cards, live cursor highlight, per-monitor
//! shortcuts, and auto-save to the config the background watcher hot-reloads.
//!
//! Mouse: click a card to select; click the shortcut again to capture a chord.
//! Esc cancels the current mode, then quits from Normal.

use std::io::{stdout, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::{Frame, Terminal};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

use screen_switch_rs::config::{self, Config};
use screen_switch_rs::keys;
use screen_switch_rs::monitors::{self, MonitorInfo};

/// Cursor-on-this-monitor accent.
const CURRENT_PINK: Color = Color::Rgb(0xF4, 0x45, 0xF7);
/// Selected-monitor outline (not white — soft cyan).
const SELECTED_BORDER: Color = Color::Rgb(0x6E, 0xC8, 0xFF);
const IDLE_BORDER: Color = Color::DarkGray;

/// Terminal cells are ~2× taller than wide; stretch X so a physical 16:9
/// monitor reads as widescreen in the TUI, not a near-square.
const CELL_ASPECT: f64 = 2.0;

const NORMAL_STATUS: &str = "click select · click shortcut again to capture · Esc quit";
const NORMAL_HELP: &str =
    "click/←→ select   click shortcut/Enter capture   e type   x clear   h/click hold   t test   r refresh   q quit";

/// Rows reserved under each card for the monitor name.
const LABEL_ROWS: u16 = 1;
/// Gap between scaled cards (cells).
const CARD_GAP: i32 = 1;

enum Mode {
    Normal,
    EditShortcut,
    EditHold,
    CaptureChord,
}

struct App {
    monitors: Vec<MonitorInfo>,
    cfg: Config,
    config_path: std::path::PathBuf,
    selected: Option<usize>,
    mode: Mode,
    input: String,
    status: String,
    current_device: String,
    watcher_running: bool,
    last_poll: Instant,
    /// Last laid-out card rects (screen cells), parallel to `monitors`.
    card_rects: Vec<Rect>,
    /// Last header area (for hold hit-testing).
    header_area: Rect,
}

impl App {
    fn new() -> Self {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let monitors = monitors::enumerate();
        let config_path = config::default_path();
        let mut cfg = Config::load_or_template(&config_path, &monitors);

        let mut changed = false;
        for m in &monitors {
            cfg.shortcuts.entry(m.device.clone()).or_insert_with(|| {
                changed = true;
                String::new()
            });
        }
        if cfg.hold_ms.is_none() {
            cfg.hold_ms = Some(keys::DEFAULT_HOLD_MS);
            changed = true;
        }
        if changed {
            let _ = cfg.save(&config_path);
        }

        let mut app = App {
            monitors,
            cfg,
            config_path,
            selected: None,
            mode: Mode::Normal,
            input: String::new(),
            status: NORMAL_STATUS.to_string(),
            current_device: String::new(),
            watcher_running: false,
            last_poll: Instant::now() - Duration::from_secs(10),
            card_rects: Vec::new(),
            header_area: Rect::default(),
        };
        app.poll(true);
        app
    }

    fn selected_device(&self) -> Option<String> {
        self.selected
            .and_then(|i| self.monitors.get(i))
            .map(|m| m.device.clone())
    }

    fn hold(&self) -> u64 {
        self.cfg.hold_ms.unwrap_or(keys::DEFAULT_HOLD_MS)
    }

    fn shortcut_for(&self, device: &str) -> String {
        self.cfg.shortcuts.get(device).cloned().unwrap_or_default()
    }

    fn save(&mut self) {
        match self.cfg.save(&self.config_path) {
            Ok(_) => {}
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn cancel_to_normal(&mut self) {
        self.mode = Mode::Normal;
        self.input.clear();
        self.status = "Cancelled.".to_string();
    }

    fn enter_capture(&mut self) {
        self.mode = Mode::CaptureChord;
        self.status = "Capture: press the shortcut chord now (Esc to cancel)".to_string();
    }

    fn enter_edit_hold(&mut self) {
        self.input = self.hold().to_string();
        self.mode = Mode::EditHold;
        self.status = "Hold ms (digits). Enter=save Esc=cancel".to_string();
    }

    fn poll(&mut self, force: bool) {
        if !force && self.last_poll.elapsed() < Duration::from_millis(400) {
            return;
        }
        self.last_poll = Instant::now();

        self.current_device = monitors::monitor_at_cursor()
            .and_then(monitors::device_name)
            .unwrap_or_default();

        self.watcher_running = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq screen-switch-rs.exe", "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("screen-switch-rs.exe"))
            .unwrap_or(false);
    }

    fn move_selection(&mut self, delta: isize) {
        if self.monitors.is_empty() {
            return;
        }
        let len = self.monitors.len() as isize;
        let cur = self.selected.unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len);
        self.selected = Some(next as usize);
    }

    fn set_selected_shortcut(&mut self, value: String) {
        if let Some(device) = self.selected_device() {
            self.cfg.shortcuts.insert(device, value);
            self.save();
        }
    }

    /// Esc ladder: edit modes cancel; Normal quits. Returns false to quit.
    fn handle_esc(&mut self) -> bool {
        match self.mode {
            Mode::Normal => false,
            Mode::EditShortcut | Mode::EditHold | Mode::CaptureChord => {
                self.cancel_to_normal();
                true
            }
        }
    }

    /// Returns false to quit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return true;
        }
        if key.code == KeyCode::Esc {
            return true;
        }
        match self.mode {
            Mode::Normal => self.handle_normal(key),
            Mode::EditShortcut => self.handle_edit_shortcut(key),
            Mode::EditHold => self.handle_edit_hold(key),
            Mode::CaptureChord => self.handle_capture(key),
        }
    }

    fn handle_normal(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') => return false,
            KeyCode::Left | KeyCode::Up | KeyCode::Char('h') | KeyCode::Char('k')
                if !matches!(key.code, KeyCode::Char('h')) =>
            {
                self.move_selection(-1);
            }
            KeyCode::Right | KeyCode::Down | KeyCode::Char('l') | KeyCode::Char('j') => {
                self.move_selection(1);
            }
            KeyCode::Enter => {
                if self.selected.is_some() {
                    self.enter_capture();
                } else {
                    self.status = "Select a monitor first.".to_string();
                }
            }
            KeyCode::Char('e') => {
                if let Some(dev) = self.selected_device() {
                    self.input = self.shortcut_for(&dev);
                    self.mode = Mode::EditShortcut;
                    self.status = "Type shortcut (e.g. ctrl+shift+home). Enter=save Esc=cancel"
                        .to_string();
                } else {
                    self.status = "Select a monitor first.".to_string();
                }
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                if self.selected.is_some() {
                    self.set_selected_shortcut(String::new());
                    self.status = "Cleared.".to_string();
                } else {
                    self.status = "Select a monitor first.".to_string();
                }
            }
            KeyCode::Char('h') => self.enter_edit_hold(),
            KeyCode::Char('t') => self.test_selected(),
            KeyCode::Char('r') => {
                self.monitors = monitors::enumerate();
                if let Some(i) = self.selected {
                    if i >= self.monitors.len() {
                        self.selected = self.monitors.len().checked_sub(1);
                    }
                }
                self.status = "Monitors refreshed.".to_string();
            }
            _ => {}
        }
        true
    }

    fn handle_edit_shortcut(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => {
                let val = self.input.trim().to_string();
                if val.is_empty() {
                    self.set_selected_shortcut(String::new());
                    self.mode = Mode::Normal;
                    self.status = "Cleared.".to_string();
                } else {
                    match keys::parse(&val) {
                        Ok(_) => {
                            self.set_selected_shortcut(val.clone());
                            self.mode = Mode::Normal;
                            self.status = format!("Saved '{val}'.");
                        }
                        Err(e) => self.status = format!("Invalid: {e}"),
                    }
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => self.input.push(c),
            _ => {}
        }
        true
    }

    fn handle_edit_hold(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => match self.input.trim().parse::<u64>() {
                Ok(v) if v > 0 && v <= 5000 => {
                    self.cfg.hold_ms = Some(v);
                    self.save();
                    self.mode = Mode::Normal;
                    self.status = format!("Hold set to {v}ms.");
                }
                _ => self.status = "Enter a number between 1 and 5000.".to_string(),
            },
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) if c.is_ascii_digit() => self.input.push(c),
            _ => {}
        }
        true
    }

    fn handle_capture(&mut self, key: KeyEvent) -> bool {
        if let Some(chord) = chord_from_event(&key) {
            self.set_selected_shortcut(chord.clone());
            self.mode = Mode::Normal;
            self.status = format!("Captured '{chord}'.");
        }
        true
    }

    fn handle_mouse(&mut self, m: MouseEvent, area: Rect) {
        if !matches!(m.kind, MouseEventKind::Down(_)) {
            return;
        }

        if !matches!(self.mode, Mode::Normal) {
            self.cancel_to_normal();
            return;
        }

        let pos = Position::new(m.column, m.row);

        if hold_hit(self.header_area, self.hold(), pos) {
            self.enter_edit_hold();
            return;
        }

        if let Some(idx) = card_at(&self.card_rects, pos) {
            if self.selected == Some(idx) {
                self.enter_capture();
            } else {
                self.selected = Some(idx);
                self.status = NORMAL_STATUS.to_string();
            }
            return;
        }

        // Click open space (map area, not a card) clears selection.
        let chunks = ui_chunks(area);
        if chunks[1].contains(pos) {
            self.selected = None;
            self.status = NORMAL_STATUS.to_string();
        }
    }

    fn test_selected(&mut self) {
        let Some(dev) = self.selected_device() else {
            return;
        };
        let spec = self.shortcut_for(&dev);
        if spec.trim().is_empty() {
            self.status = "No shortcut set on this monitor.".to_string();
            return;
        }
        match keys::parse(&spec) {
            Ok((mods, k)) => {
                let hold = self.hold();
                let (di, de, _, _) = keys::send(&mods, k, hold);
                self.status = format!("Tested '{spec}' (injected {di}/{de}).");
            }
            Err(e) => self.status = format!("Invalid shortcut: {e}"),
        }
    }
}

fn chord_from_event(k: &KeyEvent) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let m = k.modifiers;
    let mut shift = m.contains(KeyModifiers::SHIFT);

    let key = match k.code {
        KeyCode::Char(c) => {
            let lower = c.to_ascii_lowercase();
            if c.is_ascii_uppercase() {
                shift = true;
            }
            lower.to_string()
        }
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        _ => return None,
    };

    if m.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if m.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }
    if shift {
        parts.push("shift".to_string());
    }
    if m.contains(KeyModifiers::SUPER) {
        parts.push("win".to_string());
    }
    parts.push(key);
    Some(parts.join("+"))
}

fn ui_chunks(area: Rect) -> [Rect; 4] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(8),    // monitor map
            Constraint::Length(1), // status
            Constraint::Length(1), // help
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2], chunks[3]]
}

fn card_at(rects: &[Rect], pos: Position) -> Option<usize> {
    rects.iter().position(|r| r.contains(pos))
}

fn hold_hit(header_area: Rect, hold_ms: u64, pos: Position) -> bool {
    if header_area.width == 0 || !header_area.contains(pos) {
        return false;
    }
    if pos.y != header_area.y {
        return false;
    }
    let before_len = "screen-switch-rs    watcher: ".len() + 7 + "    ".len();
    let hold_text = format!("hold: {hold_ms}ms");
    let start = header_area.x + before_len as u16;
    let end = start + hold_text.len() as u16;
    pos.x >= start && pos.x < end
}

/// Scale physical monitor rects into `area`, leaving `LABEL_ROWS` under each card.
///
/// X is stretched by [`CELL_ASPECT`] so physical 16:9 monitors look widescreen
/// in the terminal (cells are roughly twice as tall as they are wide).
fn layout_monitor_cards(monitors: &[MonitorInfo], area: Rect) -> Vec<Rect> {
    if monitors.is_empty() || area.width < 4 || area.height < 4 {
        return vec![Rect::default(); monitors.len()];
    }

    let map_h = area.height.saturating_sub(LABEL_ROWS);
    if map_h < 3 {
        return vec![Rect::default(); monitors.len()];
    }

    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for m in monitors {
        let (l, t, r, b) = m.rect;
        min_x = min_x.min(l);
        min_y = min_y.min(t);
        max_x = max_x.max(r);
        max_y = max_y.max(b);
    }
    let world_w = (max_x - min_x).max(1) as f64;
    let world_h = (max_y - min_y).max(1) as f64;

    let pad = 1u16;
    let avail_w = area.width.saturating_sub(pad * 2).max(1) as f64;
    let avail_h = map_h.saturating_sub(pad * 2).max(1) as f64;

    // Stretch X so cell aspect doesn't squash widescreen monitors into squares.
    let scale_x = (avail_w / (world_w * CELL_ASPECT)).min(avail_h / world_h) * CELL_ASPECT;
    let scale_y = scale_x / CELL_ASPECT;

    let used_w = world_w * scale_x;
    let used_h = world_h * scale_y;
    let ox = area.x as f64 + pad as f64 + ((avail_w - used_w) / 2.0).max(0.0);
    let oy = area.y as f64 + pad as f64 + ((avail_h - used_h) / 2.0).max(0.0);

    let max_cell_x = (area.x + area.width) as i32;
    let max_cell_y = (area.y + map_h) as i32;

    monitors
        .iter()
        .map(|m| {
            let (l, t, r, b) = m.rect;
            let mut x = (ox + (l - min_x) as f64 * scale_x).round() as i32;
            let mut y = (oy + (t - min_y) as f64 * scale_y).round() as i32;
            let mut right = (ox + (r - min_x) as f64 * scale_x).round() as i32;
            let mut bottom = (oy + (b - min_y) as f64 * scale_y).round() as i32;

            x += CARD_GAP;
            y += CARD_GAP;
            right -= CARD_GAP;
            bottom -= CARD_GAP;

            // Prefer a ~16:9 cell footprint when the scaled rect is too square.
            let mut w = (right - x).max(10);
            let mut h = (bottom - y).max(3);
            let target_h = ((w as f64) / (16.0 / 9.0)).round() as i32;
            if h > target_h && target_h >= 3 {
                let shrink = h - target_h;
                y += shrink / 2;
                h = target_h;
            } else if h < target_h {
                // Grow width instead when height-limited — keep widescreen feel.
                let target_w = ((h as f64) * (16.0 / 9.0)).round() as i32;
                if target_w > w {
                    let grow = target_w - w;
                    x -= grow / 2;
                    w = target_w;
                }
            }

            // One extra row for a roomier card interior.
            h += 1;
            y = (y - 0).max(area.y as i32); // keep y; grow downward first

            if x + w > max_cell_x {
                w = (max_cell_x - x).max(3);
            }
            if y + h > max_cell_y {
                // Prefer growing upward if we hit the bottom edge.
                let overflow = y + h - max_cell_y;
                y = (y - overflow).max(area.y as i32);
                h = (max_cell_y - y).max(3);
            }
            x = x.clamp(area.x as i32, max_cell_x - 3);
            y = y.clamp(area.y as i32, max_cell_y - 3);

            Rect::new(x as u16, y as u16, w as u16, h as u16)
        })
        .collect()
}

fn monitor_label(m: &MonitorInfo) -> String {
    if m.primary {
        format!("Monitor {} · primary", m.number)
    } else {
        format!("Monitor {}", m.number)
    }
}

fn color_swatch(color: Color) -> Span<'static> {
    Span::styled("██", Style::default().fg(color))
}

fn draw(frame: &mut Frame, app: &mut App) {
    let chunks = ui_chunks(frame.area());
    app.header_area = chunks[0];

    // ── Header (borderless): settings + color key ────────────────────────
    let header_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(chunks[0]);

    let watcher = if app.watcher_running {
        Span::styled("running", Style::default().fg(Color::Green))
    } else {
        Span::styled("stopped", Style::default().fg(Color::Red))
    };
    let hold_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::UNDERLINED);
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "screen-switch-rs",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("    watcher: "),
        watcher,
        Span::raw("    "),
        Span::styled(format!("hold: {}ms", app.hold()), hold_style),
    ]));
    frame.render_widget(title, header_rows[0]);

    let muted = Style::default().fg(Color::DarkGray);
    let key = Paragraph::new(Line::from(vec![
        color_swatch(SELECTED_BORDER),
        Span::styled(" selected   ", muted),
        color_swatch(CURRENT_PINK),
        Span::styled(" cursor here   ", muted),
        color_swatch(IDLE_BORDER),
        Span::styled(" idle", muted),
    ]));
    frame.render_widget(key, header_rows[1]);

    // ── Monitor map ──────────────────────────────────────────────────────
    let cards = layout_monitor_cards(&app.monitors, chunks[1]);
    app.card_rects = cards.clone();

    if app.monitors.is_empty() {
        frame.render_widget(
            Paragraph::new("No monitors detected.")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            chunks[1],
        );
    }

    for (i, m) in app.monitors.iter().enumerate() {
        let Some(&card) = cards.get(i) else {
            continue;
        };
        if card.width < 3 || card.height < 3 {
            continue;
        }

        let is_current = m.device == app.current_device;
        let is_selected = app.selected == Some(i);

        let border_color = if is_selected {
            SELECTED_BORDER
        } else if is_current {
            CURRENT_PINK
        } else {
            IDLE_BORDER
        };

        let mut border_style = Style::default().fg(border_color);
        if is_selected {
            border_style = border_style.add_modifier(Modifier::BOLD);
        }

        frame.render_widget(Clear, card);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style);
        let inner = block.inner(card);
        frame.render_widget(block, card);

        let sc = app.shortcut_for(&m.device);
        let (sc_text, sc_style) = if sc.trim().is_empty() {
            (
                "(unset)".to_string(),
                if is_selected {
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            )
        } else if is_current {
            (
                sc,
                Style::default()
                    .fg(CURRENT_PINK)
                    .add_modifier(if is_selected {
                        Modifier::BOLD | Modifier::UNDERLINED
                    } else {
                        Modifier::BOLD
                    }),
            )
        } else if is_selected {
            (
                sc,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
        } else {
            (sc, Style::default().fg(Color::Green))
        };

        if inner.height > 0 && inner.width > 0 {
            let text_y = if inner.height > 1 {
                inner.y + inner.height / 2
            } else {
                inner.y
            };
            let text_area = Rect::new(inner.x, text_y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Span::styled(sc_text, sc_style)).alignment(Alignment::Center),
                text_area,
            );
        }

        let label_y = card.y.saturating_add(card.height);
        if label_y < chunks[1].y.saturating_add(chunks[1].height) {
            let label_style = if is_current {
                Style::default()
                    .fg(CURRENT_PINK)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default()
                    .fg(SELECTED_BORDER)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let label_w = card.width.max(monitor_label(m).len() as u16);
            let label_x = card.x;
            let max_w = chunks[1]
                .x
                .saturating_add(chunks[1].width)
                .saturating_sub(label_x);
            let label_area = Rect::new(label_x, label_y, label_w.min(max_w), 1);
            frame.render_widget(
                Paragraph::new(Span::styled(monitor_label(m), label_style))
                    .alignment(Alignment::Center),
                label_area,
            );
        }
    }

    // ── Status (borderless) ──────────────────────────────────────────────
    let status_line = match app.mode {
        Mode::EditShortcut => format!("Shortcut: {}_", app.input),
        Mode::EditHold => format!("Hold ms: {}_", app.input),
        Mode::CaptureChord => "Press the shortcut chord now… Esc cancel".to_string(),
        Mode::Normal => app.status.clone(),
    };
    let status_style = match app.mode {
        Mode::Normal => Style::default().fg(Color::Gray),
        _ => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    };
    frame.render_widget(Paragraph::new(status_line).style(status_style), chunks[2]);

    // ── Help (borderless) ────────────────────────────────────────────────
    let help_text = match app.mode {
        Mode::Normal => NORMAL_HELP,
        Mode::EditShortcut => "type shortcut   Enter save   Esc cancel",
        Mode::EditHold => "type ms (1–5000)   Enter save   Esc cancel",
        Mode::CaptureChord => "press chord to capture   Esc cancel",
    };
    frame.render_widget(
        Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray)),
        chunks[3],
    );
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
}

fn main() -> std::io::Result<()> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal: Terminal<CrosstermBackend<Stdout>> = Terminal::new(backend)?;

    let mut app = App::new();
    let result = run(&mut terminal, &mut app);

    restore_terminal();
    result
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> std::io::Result<()> {
    loop {
        app.poll(false);
        terminal.draw(|f| draw(f, app))?;

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if key.code == KeyCode::Esc && key.kind == KeyEventKind::Press => {
                    if !app.handle_esc() {
                        return Ok(());
                    }
                }
                Event::Key(key) => {
                    if !app.handle_key(key) {
                        return Ok(());
                    }
                }
                Event::Mouse(m) => {
                    let size = terminal.size()?;
                    let area = Rect::new(0, 0, size.width, size.height);
                    app.handle_mouse(m, area);
                }
                _ => {}
            }
        }
    }
}
