//! screen-switch-tui — interactive config console for screen-switch-rs.
//!
//! Shows detected monitors (live-highlighting the one under the cursor), lets
//! you set a per-monitor shortcut and the chord hold time, test-fire a shortcut,
//! and auto-saves to the same config.json the background watcher hot-reloads.

use std::io::{stdout, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::{Frame, Terminal};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

use screen_switch_rs::config::{self, Config};
use screen_switch_rs::keys;
use screen_switch_rs::monitors::{self, MonitorInfo};

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
    table: TableState,
    mode: Mode,
    input: String,
    status: String,
    current_device: String,
    watcher_running: bool,
    last_poll: Instant,
}

impl App {
    fn new() -> Self {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let monitors = monitors::enumerate();
        let config_path = config::default_path();
        let mut cfg = Config::load_or_template(&config_path, &monitors);

        // Ensure every detected monitor has an entry so it shows up editable.
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

        let mut table = TableState::default();
        if !monitors.is_empty() {
            table.select(Some(0));
        }

        let mut app = App {
            monitors,
            cfg,
            config_path,
            table,
            mode: Mode::Normal,
            input: String::new(),
            status: "Ready. Enter: capture chord  e: type  x: clear  h: hold  t: test  q: quit"
                .to_string(),
            current_device: String::new(),
            watcher_running: false,
            last_poll: Instant::now() - Duration::from_secs(10),
        };
        app.poll(true);
        app
    }

    fn selected(&self) -> usize {
        self.table.selected().unwrap_or(0)
    }

    fn selected_device(&self) -> Option<String> {
        self.monitors.get(self.selected()).map(|m| m.device.clone())
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
        let cur = self.selected() as isize;
        let next = (cur + delta).rem_euclid(len);
        self.table.select(Some(next as usize));
    }

    fn move_by(&mut self, keycode: KeyCode) {
        match keycode {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            _ => {}
        }
    }

    fn set_selected_shortcut(&mut self, value: String) {
        if let Some(device) = self.selected_device() {
            self.cfg.shortcuts.insert(device, value);
            self.save();
        }
    }

    /// Returns false to quit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
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
            KeyCode::Char('q') | KeyCode::Esc => return false,
            KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k') => {
                self.move_by(key.code)
            }
            KeyCode::Enter => {
                self.mode = Mode::CaptureChord;
                self.status = "Capture: press the shortcut chord now (Esc to cancel)".to_string();
            }
            KeyCode::Char('e') => {
                if let Some(dev) = self.selected_device() {
                    self.input = self.shortcut_for(&dev);
                    self.mode = Mode::EditShortcut;
                    self.status = "Type shortcut (e.g. ctrl+shift+home). Enter=save Esc=cancel"
                        .to_string();
                }
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                self.set_selected_shortcut(String::new());
                self.status = "Cleared.".to_string();
            }
            KeyCode::Char('h') => {
                self.input = self.hold().to_string();
                self.mode = Mode::EditHold;
                self.status = "Hold ms (digits). Enter=save Esc=cancel".to_string();
            }
            KeyCode::Char('t') => self.test_selected(),
            KeyCode::Char('r') => {
                self.monitors = monitors::enumerate();
                if self.selected() >= self.monitors.len() {
                    self.table
                        .select(self.monitors.len().checked_sub(1));
                }
                self.status = "Monitors refreshed.".to_string();
            }
            _ => {}
        }
        true
    }

    fn handle_edit_shortcut(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.status = "Cancelled.".to_string();
            }
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
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.status = "Cancelled.".to_string();
            }
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
        if key.code == KeyCode::Esc {
            self.mode = Mode::Normal;
            self.status = "Cancelled.".to_string();
            return true;
        }
        if let Some(chord) = chord_from_event(&key) {
            self.set_selected_shortcut(chord.clone());
            self.mode = Mode::Normal;
            self.status = format!("Captured '{chord}'.");
        }
        true
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

fn draw(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(frame.area());

    // Title bar.
    let watcher = if app.watcher_running {
        Span::styled("running", Style::default().fg(Color::Green))
    } else {
        Span::styled("stopped", Style::default().fg(Color::Red))
    };
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "screen-switch-rs",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  config console    watcher: "),
        watcher,
        Span::raw(format!("    hold: {}ms", app.hold())),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, chunks[0]);

    // Monitor table.
    let header = Row::new(vec!["#", "Device", "Shortcut", "Here"]).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    let rows: Vec<Row> = app
        .monitors
        .iter()
        .map(|m| {
            let here = if m.device == app.current_device {
                "●"
            } else {
                ""
            };
            let label = if m.primary {
                format!("Monitor {} (primary)", m.number)
            } else {
                format!("Monitor {}", m.number)
            };
            let sc = app.shortcut_for(&m.device);
            let sc = if sc.trim().is_empty() {
                Span::styled("(unset)", Style::default().fg(Color::DarkGray))
            } else {
                Span::styled(sc, Style::default().fg(Color::Green))
            };
            Row::new(vec![
                Cell::from(label),
                Cell::from(m.device.clone()),
                Cell::from(sc),
                Cell::from(Span::styled(here, Style::default().fg(Color::Cyan))),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(22),
        Constraint::Percentage(35),
        Constraint::Percentage(40),
        Constraint::Length(6),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Monitors "),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    frame.render_stateful_widget(table, chunks[1], &mut app.table);

    // Edit / status line.
    let mid = match app.mode {
        Mode::EditShortcut => {
            Paragraph::new(format!("Shortcut: {}_", app.input)).block(edit_block("Edit shortcut"))
        }
        Mode::EditHold => {
            Paragraph::new(format!("Hold ms: {}_", app.input)).block(edit_block("Edit hold"))
        }
        Mode::CaptureChord => Paragraph::new("Press the shortcut chord now...")
            .block(edit_block("Capture chord")),
        Mode::Normal => Paragraph::new(app.status.clone())
            .block(Block::default().borders(Borders::ALL).title(" Status ")),
    };
    frame.render_widget(mid, chunks[2]);

    // Help bar.
    let help = Paragraph::new(
        "↑/↓ select   Enter capture   e type   x clear   h hold   t test   r refresh   q quit",
    )
    .style(Style::default().fg(Color::DarkGray))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(help, chunks[3]);
}

fn edit_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(" {title} "))
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);
}

fn main() -> std::io::Result<()> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
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
            if let Event::Key(key) = event::read()? {
                if !app.handle_key(key) {
                    return Ok(());
                }
            }
        }
    }
}
