//! Interactive terminal emulator with PTY support.
//!
//! Provides a real pseudo-terminal (conpty on Windows, pty on Unix) for
//! interactive shell sessions within the IDE.

use egui;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// Terminal cell character attributes.
#[derive(Debug, Clone, Copy, Default)]
pub struct CellAttrs {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub fg_color: Option<u8>,
    pub bg_color: Option<u8>,
    /// 24-bit foreground (`CSI 38;2;r;g;b`). Takes precedence over `fg_color`
    /// when set; modern CLIs (cargo, ripgrep, eza, linters) emit truecolor.
    pub fg_rgb: Option<[u8; 3]>,
    /// 24-bit background (`CSI 48;2;r;g;b`). Takes precedence over `bg_color`.
    pub bg_rgb: Option<[u8; 3]>,
}

impl CellAttrs {
    /// Resolve the effective foreground colour: 24-bit truecolor wins, then the
    /// indexed ANSI palette, then `default` (the theme's body text colour).
    pub fn resolved_fg(&self, default: egui::Color32) -> egui::Color32 {
        if let Some([r, g, b]) = self.fg_rgb {
            return egui::Color32::from_rgb(r, g, b);
        }
        self.fg_color.map(ansi_color).unwrap_or(default)
    }

    /// Resolve the effective background colour (truecolor, then indexed, then
    /// fully transparent so the panel background shows through).
    pub fn resolved_bg(&self) -> egui::Color32 {
        if let Some([r, g, b]) = self.bg_rgb {
            return egui::Color32::from_rgb(r, g, b);
        }
        self.bg_color
            .map(ansi_color)
            .unwrap_or(egui::Color32::TRANSPARENT)
    }
}

/// A single character cell in the terminal grid.
#[derive(Debug, Clone, Default)]
pub struct Cell {
    pub ch: char,
    pub attrs: CellAttrs,
    /// OSC 8 hyperlink target attached to this cell (`ESC ]8;;URI ST`), if
    /// any. Ctrl+clicking the cell opens it directly â€” no text heuristics.
    pub link: Option<Arc<str>>,
}

impl Cell {
    /// A blank cell: a space with default attributes. Erase/scroll operations
    /// must produce this (not `Cell::default()`, whose `ch` would be `'\0'`).
    pub fn blank() -> Self {
        Cell {
            ch: ' ',
            attrs: CellAttrs::default(),
            link: None,
        }
    }
}

/// Terminal buffer (grid of cells).
#[derive(Debug, Clone)]
pub struct TerminalBuffer {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<Vec<Cell>>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scrollback: VecDeque<Vec<Cell>>,
    pub scrollback_limit: usize,
    /// Current graphics attributes applied to newly written cells. Mutated by
    /// SGR (`ESC[...m`) sequences and persists across `process_output` calls
    /// because PTY output arrives in arbitrary chunks.
    pub cur_attrs: CellAttrs,
    /// OSC 8 hyperlink currently in effect, stamped onto newly written cells.
    /// Set by `ESC ]8;;URI â€¦` and cleared by an empty-URI sequence.
    pub cur_link: Option<Arc<str>>,
}

impl TerminalBuffer {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cells = vec![vec![Cell::blank(); cols]; rows];
        Self {
            cols,
            rows,
            cells,
            cursor_row: 0,
            cursor_col: 0,
            scrollback: VecDeque::new(),
            scrollback_limit: 5000,
            cur_attrs: CellAttrs::default(),
            cur_link: None,
        }
    }

    /// Write a character at the cursor position and advance.
    pub fn put_char(&mut self, ch: char, attrs: CellAttrs) {
        if ch == '\n' {
            self.newline();
            return;
        }
        if ch == '\r' {
            self.cursor_col = 0;
            return;
        }
        if ch == '\x08' {
            // Backspace
            if self.cursor_col > 0 {
                self.cursor_col -= 1;
            }
            return;
        }
        if ch == '\t' {
            let next_tab = ((self.cursor_col / 8) + 1) * 8;
            self.cursor_col = next_tab.min(self.cols - 1);
            return;
        }

        if self.cursor_col >= self.cols {
            self.newline();
        }
        self.cells[self.cursor_row][self.cursor_col] = Cell {
            ch,
            attrs,
            link: self.cur_link.clone(),
        };
        self.cursor_col += 1;
    }

    fn newline(&mut self) {
        self.cursor_col = 0;
        if self.cursor_row + 1 >= self.rows {
            self.scroll_up();
        } else {
            self.cursor_row += 1;
        }
    }

    fn scroll_up(&mut self) {
        let top_row = self.cells.remove(0);
        self.scrollback.push_back(top_row);
        if self.scrollback.len() > self.scrollback_limit {
            self.scrollback.pop_front();
        }
        self.cells.push(vec![Cell::blank(); self.cols]);
    }

    /// Clear the entire screen.
    pub fn clear(&mut self) {
        for row in &mut self.cells {
            for cell in row.iter_mut() {
                cell.ch = ' ';
                cell.attrs = CellAttrs::default();
                cell.link = None;
            }
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    /// Process raw output bytes from the PTY (ANSI subset).
    pub fn process_output(&mut self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                // ANSI escape sequence.
                if chars.peek() == Some(&'[') {
                    chars.next(); // consume '['
                    let mut params = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() || c == ';' || c == '?' {
                            params.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    let cmd = chars.next().unwrap_or('m');
                    self.handle_csi(&params, cmd);
                } else if chars.peek() == Some(&']') {
                    // OSC: ESC ] <code> ; â€¦ (terminated by BEL or ST).
                    chars.next(); // consume ']'
                    let mut payload = String::new();
                    let mut terminated = false;
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            terminated = true;
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next(); // consume the ST backslash
                            terminated = true;
                            break;
                        }
                        payload.push(c);
                    }
                    if terminated {
                        self.handle_osc(&payload);
                    }
                }
            } else {
                self.put_char(ch, self.cur_attrs);
            }
        }
    }

    /// Handle a collected OSC payload (after `ESC ]`). Only OSC 8 hyperlinks
    /// act on the grid: `8;<params>;<URI>` stamps subsequent cells with the
    /// URI, and an empty URI closes the link. Everything else (window titles,
    /// cwd reports, â€¦) is consumed and ignored â€” crucially *not* printed, so
    /// escape payloads never leak into the visible grid.
    fn handle_osc(&mut self, payload: &str) {
        let Some(rest) = payload.strip_prefix("8;") else {
            return;
        };
        // `params` is the first field; the URI is everything after the next
        // semicolon and may itself contain semicolons (query strings).
        let uri = match rest.find(';') {
            Some(i) => &rest[i + 1..],
            None => "",
        };
        self.cur_link = if uri.is_empty() {
            None
        } else {
            Some(Arc::from(uri))
        };
    }

    /// Handle CSI (Control Sequence Introducer) commands.
    fn handle_csi(&mut self, params: &str, cmd: char) {
        match cmd {
            'H' | 'f' => {
                // Cursor position: ESC[row;colH
                let parts: Vec<usize> = params.split(';').filter_map(|s| s.parse().ok()).collect();
                self.cursor_row = parts
                    .first()
                    .copied()
                    .unwrap_or(1)
                    .saturating_sub(1)
                    .min(self.rows - 1);
                self.cursor_col = parts
                    .get(1)
                    .copied()
                    .unwrap_or(1)
                    .saturating_sub(1)
                    .min(self.cols - 1);
            }
            'J' => {
                // Erase display: 0 = cursorâ†’end, 1 = startâ†’cursor, 2/3 = all.
                let n: usize = params.parse().unwrap_or(0);
                match n {
                    2 | 3 => self.clear(),
                    0 => {
                        // Clear from the cursor to the end of the screen.
                        let r = self.cursor_row;
                        let c = self.cursor_col;
                        for row in r..self.rows {
                            let start = if row == r { c } else { 0 };
                            for col in start..self.cols {
                                self.cells[row][col] = Cell::blank();
                            }
                        }
                    }
                    1 => {
                        // Clear from the start of the screen to the cursor.
                        let r = self.cursor_row;
                        let c = self.cursor_col;
                        for row in 0..=r.min(self.rows - 1) {
                            let end = if row == r {
                                (c + 1).min(self.cols)
                            } else {
                                self.cols
                            };
                            for col in 0..end {
                                self.cells[row][col] = Cell::blank();
                            }
                        }
                    }
                    _ => {}
                }
            }
            'K' => {
                // Erase line: 0 = cursorâ†’end, 1 = startâ†’cursor, 2 = whole line.
                let n: usize = params.parse().unwrap_or(0);
                let row = self.cursor_row;
                match n {
                    0 => {
                        for col in self.cursor_col..self.cols {
                            self.cells[row][col] = Cell::blank();
                        }
                    }
                    1 => {
                        let end = (self.cursor_col + 1).min(self.cols);
                        for col in 0..end {
                            self.cells[row][col] = Cell::blank();
                        }
                    }
                    2 => {
                        for col in 0..self.cols {
                            self.cells[row][col] = Cell::blank();
                        }
                    }
                    _ => {}
                }
            }
            'A' => {
                let n: usize = params.parse().unwrap_or(1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            'B' => {
                let n: usize = params.parse().unwrap_or(1);
                self.cursor_row = (self.cursor_row + n).min(self.rows - 1);
            }
            'C' => {
                let n: usize = params.parse().unwrap_or(1);
                self.cursor_col = (self.cursor_col + n).min(self.cols - 1);
            }
            'D' => {
                let n: usize = params.parse().unwrap_or(1);
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            'm' => {
                self.apply_sgr(params);
            }
            _ => {} // ignore unknown
        }
    }

    /// Apply an SGR (Select Graphic Rendition) sequence to `cur_attrs`.
    /// Handles reset, intensity/italic/underline flags, the 16 ANSI colours
    /// (standard + bright), and the `38;5;n` / `48;5;n` 256-colour selectors.
    fn apply_sgr(&mut self, params: &str) {
        let codes: Vec<u16> = if params.is_empty() {
            vec![0]
        } else {
            params
                .split(';')
                .map(|s| s.parse::<u16>().unwrap_or(0))
                .collect()
        };
        let mut i = 0;
        while i < codes.len() {
            match codes[i] {
                0 => self.cur_attrs = CellAttrs::default(),
                1 => self.cur_attrs.bold = true,
                2 => self.cur_attrs.dim = true,
                3 => self.cur_attrs.italic = true,
                4 => self.cur_attrs.underline = true,
                22 => {
                    self.cur_attrs.bold = false;
                    self.cur_attrs.dim = false;
                }
                23 => self.cur_attrs.italic = false,
                24 => self.cur_attrs.underline = false,
                30..=37 => self.cur_attrs.fg_color = Some((codes[i] - 30) as u8),
                39 => {
                    self.cur_attrs.fg_color = None;
                    self.cur_attrs.fg_rgb = None;
                }
                40..=47 => self.cur_attrs.bg_color = Some((codes[i] - 40) as u8),
                49 => {
                    self.cur_attrs.bg_color = None;
                    self.cur_attrs.bg_rgb = None;
                }
                90..=97 => self.cur_attrs.fg_color = Some((codes[i] - 90 + 8) as u8),
                100..=107 => self.cur_attrs.bg_color = Some((codes[i] - 100 + 8) as u8),
                38 | 48 => {
                    // Extended colour: `38;5;n` (indexed) or `38;2;r;g;b` (RGB).
                    let is_fg = codes[i] == 38;
                    if codes.get(i + 1) == Some(&5) {
                        if let Some(&n) = codes.get(i + 2) {
                            let idx = n.min(255) as u8;
                            if is_fg {
                                self.cur_attrs.fg_color = Some(idx);
                                self.cur_attrs.fg_rgb = None;
                            } else {
                                self.cur_attrs.bg_color = Some(idx);
                                self.cur_attrs.bg_rgb = None;
                            }
                        }
                        i += 2;
                    } else if codes.get(i + 1) == Some(&2) {
                        // 24-bit truecolor: `38;2;r;g;b` / `48;2;r;g;b`.
                        let clamp = |v: Option<&u16>| v.copied().unwrap_or(0).min(255) as u8;
                        let r = clamp(codes.get(i + 2));
                        let g = clamp(codes.get(i + 3));
                        let b = clamp(codes.get(i + 4));
                        if is_fg {
                            self.cur_attrs.fg_rgb = Some([r, g, b]);
                            self.cur_attrs.fg_color = None;
                        } else {
                            self.cur_attrs.bg_rgb = Some([r, g, b]);
                            self.cur_attrs.bg_color = None;
                        }
                        i += 4;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// Render terminal content as text lines (for egui display).
    pub fn render_lines(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|row| {
                row.iter()
                    .map(|c| c.ch)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// Rows the panel should paint: retained scrollback (bounded to
    /// `max_scrollback`, most-recent) followed by the live grid. Returning cell
    /// rows (not strings) so the renderer keeps per-cell colours. This is what
    /// lets the user scroll up through output that has already scrolled off the
    /// grid, matching mainstream integrated terminals.
    pub fn display_rows(&self, max_scrollback: usize) -> Vec<&Vec<Cell>> {
        let skip = self.scrollback.len().saturating_sub(max_scrollback);
        self.scrollback
            .iter()
            .skip(skip)
            .chain(self.cells.iter())
            .collect()
    }
}

/// A named terminal shell profile.
/// Users can create multiple profiles (e.g. PowerShell, cmd, WSL, bash)
/// and switch between them when opening new terminal tabs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TerminalProfile {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    /// Optional working directory override.
    pub cwd: Option<String>,
    /// Optional environment variable overrides (KEY=VALUE).
    pub env: std::collections::HashMap<String, String>,
}

impl TerminalProfile {
    /// Default profile for the current platform.
    #[cfg(target_os = "windows")]
    pub fn default_platform() -> Self {
        Self {
            name: "PowerShell".to_string(),
            program: "powershell.exe".to_string(),
            args: vec!["-NoLogo".to_string()],
            cwd: None,
            env: std::collections::HashMap::new(),
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn default_platform() -> Self {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let name = std::path::Path::new(&shell)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Shell".to_string());
        Self {
            name,
            program: shell,
            args: vec![],
            cwd: None,
            env: std::collections::HashMap::new(),
        }
    }

    /// Built-in profiles for Windows.
    #[cfg(target_os = "windows")]
    pub fn builtin_profiles() -> Vec<Self> {
        vec![
            Self {
                name: "PowerShell".to_string(),
                program: "powershell.exe".to_string(),
                args: vec!["-NoLogo".to_string()],
                cwd: None,
                env: std::collections::HashMap::new(),
            },
            Self {
                name: "Command Prompt".to_string(),
                program: "cmd.exe".to_string(),
                args: vec![],
                cwd: None,
                env: std::collections::HashMap::new(),
            },
        ]
    }

    #[cfg(not(target_os = "windows"))]
    pub fn builtin_profiles() -> Vec<Self> {
        let mut profiles = vec![Self::default_platform()];
        for shell in &["/bin/bash", "/bin/sh", "/bin/zsh"] {
            if std::path::Path::new(shell).exists() {
                let name = std::path::Path::new(shell)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !profiles.iter().any(|p| p.name == name) {
                    profiles.push(Self {
                        name: name.clone(),
                        program: shell.to_string(),
                        args: vec![],
                        cwd: None,
                        env: std::collections::HashMap::new(),
                    });
                }
            }
        }
        profiles
    }
}

/// Terminal session state.
#[derive(Clone)]
pub struct TerminalState {
    pub buffer: Arc<Mutex<TerminalBuffer>>,
    pub input_line: String,
    pub title: String,
    pub running: bool,
    /// History of commands entered.
    pub history: Vec<String>,
    pub history_idx: usize,
    /// Grid coordinate where the current mouse selection was started (press),
    /// or `None` when no selection is in progress.
    pub sel_anchor: Option<GridPos>,
    /// Grid coordinate the selection has been dragged to (release/latest hover).
    pub sel_focus: Option<GridPos>,
    /// The finalized, normalized `(start, end)` selection with `end` exclusive,
    /// used both to paint the highlight and to drive copy. `None` = no selection.
    pub selection: Option<(GridPos, GridPos)>,
    /// A link the user Ctrl+clicked, awaiting consumption by the app shell
    /// (which owns editor/browser access); taken and cleared each frame.
    pub pending_link: Option<LinkKind>,
    /// Find-in-terminal bar: visibility, live query, and the cached match
    /// list (`find_matches`, row-major) with the highlighted match index
    /// (`find_active`, `usize::MAX` = none seated yet).
    pub find_open: bool,
    pub find_query: String,
    pub find_matches: Vec<GridPos>,
    pub find_active: usize,
    /// One-shot: request keyboard focus for the find field next render.
    pub find_just_opened: bool,
    /// One-shot: row the renderer should scroll into view for the active match.
    pub find_scroll_to: Option<usize>,
    /// Sender for writing to the PTY's stdin.
    pub pty_writer: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            buffer: Arc::new(Mutex::new(TerminalBuffer::new(120, 30))),
            input_line: String::new(),
            title: "Terminal".to_string(),
            running: false,
            history: Vec::new(),
            history_idx: 0,
            sel_anchor: None,
            sel_focus: None,
            selection: None,
            pending_link: None,
            find_open: false,
            find_query: String::new(),
            find_matches: Vec::new(),
            find_active: usize::MAX,
            find_just_opened: false,
            find_scroll_to: None,
            pty_writer: None,
        }
    }
}

impl TerminalState {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(TerminalBuffer::new(cols, rows))),
            ..Default::default()
        }
    }

    /// Send input to the PTY.
    pub fn send_input(&mut self, input: &str) {
        if let Some(ref writer) = self.pty_writer {
            if let Ok(mut w) = writer.lock() {
                let _ = w.write_all(input.as_bytes());
                let _ = w.flush();
            }
        }
    }

    /// Send a command (adds newline and records in history).
    pub fn send_command(&mut self, cmd: &str) {
        if !cmd.is_empty() {
            self.history.push(cmd.to_string());
            self.history_idx = self.history.len();
        }
        self.send_input(&format!("{}\n", cmd));
    }

    /// Navigate command history up.
    pub fn history_up(&mut self) {
        if self.history_idx > 0 {
            self.history_idx -= 1;
            self.input_line = self
                .history
                .get(self.history_idx)
                .cloned()
                .unwrap_or_default();
        }
    }

    /// Navigate command history down.
    pub fn history_down(&mut self) {
        if self.history_idx < self.history.len() {
            self.history_idx += 1;
            self.input_line = self
                .history
                .get(self.history_idx)
                .cloned()
                .unwrap_or_default();
        }
    }

    /// Open the find bar and focus its field on the next render.
    pub fn open_find(&mut self) {
        self.find_open = true;
        self.find_just_opened = true;
    }

    /// Close the find bar and drop every trace of it — including the
    /// selection, since a match highlight *is* the selection by design.
    pub fn close_find(&mut self) {
        self.find_open = false;
        self.find_query.clear();
        self.find_matches.clear();
        self.find_active = usize::MAX;
        self.find_just_opened = false;
        self.find_scroll_to = None;
        self.sel_anchor = None;
        self.sel_focus = None;
        self.selection = None;
    }

    /// Re-run the query against the visible rows (scrollback tail plus the
    /// live grid). A changed match set revokes the active highlight so the
    /// next frame re-seats it on the first match; going empty also drops the
    /// stale selection that highlighted the old one.
    pub fn refresh_find_matches(&mut self) {
        if !self.find_open {
            return;
        }
        let rows: Vec<Vec<char>> = self
            .buffer
            .lock()
            .map(|buf| {
                buf.display_rows(FIND_MAX_SHOWN)
                    .into_iter()
                    .map(|r| r.iter().map(|c| c.ch).collect())
                    .collect()
            })
            .unwrap_or_default();
        let matches = find_in_rows(&rows, &self.find_query);
        if matches != self.find_matches {
            let now_empty = matches.is_empty();
            self.find_matches = matches;
            self.find_active = usize::MAX;
            if now_empty {
                self.sel_anchor = None;
                self.sel_focus = None;
                self.selection = None;
            }
        }
    }

    /// Move to the next (or previous, wrapping) match. The match is marked
    /// through the regular selection machinery, so the same-cell wash and
    /// Ctrl+C copy work on matches for free, and the row is queued for the
    /// renderer to scroll into view.
    pub fn find_step(&mut self, forward: bool) {
        if self.find_matches.is_empty() {
            return;
        }
        let current = if self.find_active < self.find_matches.len() {
            Some(self.find_matches[self.find_active])
        } else {
            None
        };
        let Some(idx) = next_match_after(&self.find_matches, current, forward) else {
            return;
        };
        self.find_active = idx;
        let (r, c) = self.find_matches[idx];
        let focus = (r, c + self.find_query.chars().count());
        self.sel_anchor = Some((r, c));
        self.sel_focus = Some(focus);
        self.selection = Some(normalize_sel((r, c), focus));
        self.find_scroll_to = Some(r);
    }

    /// Spawn a shell process (platform-specific).
    #[cfg(target_os = "windows")]
    pub fn spawn_shell(&mut self) {
        self.spawn_process("cmd.exe", &[]);
    }

    #[cfg(not(target_os = "windows"))]
    pub fn spawn_shell(&mut self) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        self.spawn_process(&shell, &[]);
    }

    /// Spawn a shell from a named profile.
    pub fn spawn_from_profile(&mut self, profile: &TerminalProfile) {
        let args: Vec<&str> = profile.args.iter().map(|s| s.as_str()).collect();
        self.spawn_process(&profile.program, &args);
    }

    /// Spawn a process with a real PTY (ConPTY on Windows, posix pty on Unix).
    pub fn spawn_process(&mut self, program: &str, args: &[&str]) {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};

        let pty_system = native_pty_system();
        let pty_pair = match pty_system.openpty(PtySize {
            rows: self.buffer.lock().map(|b| b.rows as u16).unwrap_or(24),
            cols: self.buffer.lock().map(|b| b.cols as u16).unwrap_or(80),
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(pair) => pair,
            Err(e) => {
                if let Ok(mut buf) = self.buffer.lock() {
                    let msg = format!("Failed to create PTY: {}\r\n", e);
                    buf.process_output(msg.as_bytes());
                }
                return;
            }
        };

        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }

        let mut child = match pty_pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                if let Ok(mut buf) = self.buffer.lock() {
                    let msg = format!("Failed to spawn {}: {}\r\n", program, e);
                    buf.process_output(msg.as_bytes());
                }
                return;
            }
        };
        // Drop the slave so the master side is the only reference
        drop(pty_pair.slave);

        self.running = true;

        // Wrap the master's writer for input
        if let Ok(writer) = pty_pair.master.take_writer() {
            self.pty_writer = Some(Arc::new(Mutex::new(Box::new(writer))));
        }

        // Spawn reader thread for PTY output
        let buffer = self.buffer.clone();
        if let Ok(mut reader) = pty_pair.master.try_clone_reader() {
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(mut term_buf) = buffer.lock() {
                                term_buf.process_output(&buf[..n]);
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        // Monitor child exit in a background thread
        let buffer2 = self.buffer.clone();
        std::thread::spawn(move || {
            let _ = child.wait();
            if let Ok(mut buf) = buffer2.lock() {
                buf.process_output(b"\r\n[Process exited]\r\n");
            }
        });
    }

    /// Render the terminal panel in egui.
    pub fn show(&mut self, ui: &mut egui::Ui, palette: &crate::editor::theme::IdePalette) {
        let code_font = egui::FontId::monospace(13.0);
        // Monospace: one glyph's advance gives the cell width for hit-testing.
        let char_width = ui.ctx().fonts_mut(|tf| tf.glyph_width(&code_font, 'M'));
        // Selection wash painted behind selected cells.
        const SEL_BG: egui::Color32 = egui::Color32::from_rgb(41, 63, 92);
        const MAX_SB_SHOWN: usize = 1000;

        // Ctrl (or Cmd) turns the grid into a link surface: rows get link
        // underlines, hovering a link shows the pointing hand, and a click
        // fires the link instead of starting a selection.
        let links_active = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        let hover_pt = ui.input(|i| i.pointer.interact_pos());

        // Find bar: sits above the grid. Matches are highlighted through the
        // regular selection wash, so painting and Ctrl+C copy need no extra
        // machinery; Enter next, Shift+Enter previous, Esc closes (only while
        // the field owns focus, so a bare Esc still reaches the shell).
        if self.find_open {
            self.refresh_find_matches();
            if self.find_active == usize::MAX && !self.find_matches.is_empty() {
                self.find_step(true);
            }
            let found = !self.find_matches.is_empty();
            let count_label = if found {
                format!("{} / {}", self.find_active + 1, self.find_matches.len())
            } else {
                "0 / 0".to_string()
            };
            egui::Frame::new()
                .fill(palette.bg_secondary)
                .inner_margin(egui::Margin::symmetric(6, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.find_query)
                                .font(code_font.clone())
                                .hint_text("Find in terminal")
                                .desired_width(200.0),
                        );
                        if self.find_just_opened {
                            resp.request_focus();
                            self.find_just_opened = false;
                        }
                        ui.add_enabled_ui(found, |ui| {
                            if ui.button("\u{25B2}").clicked() {
                                self.find_step(false);
                            }
                            if ui.button("\u{25BC}").clicked() {
                                self.find_step(true);
                            }
                        });
                        ui.label(egui::RichText::new(count_label).size(11.0).color(if found {
                            palette.text
                        } else {
                            palette.text_disabled
                        }));
                        let focused = resp.has_focus();
                        let esc = focused && ui.input(|i| i.key_pressed(egui::Key::Escape));
                        let enter = focused && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let shift = ui.input(|i| i.modifiers.shift);
                        if enter && !shift {
                            self.find_step(true);
                        } else if enter && shift {
                            self.find_step(false);
                        }
                        if esc || ui.button("Close").clicked() {
                            self.close_find();
                        }
                    });
                });
        }

        // Row rects are collected this frame and used to resolve the pointer on
        // the next (one-frame layout latency, imperceptible). Selection state is
        // mutated *after* the paint closure so we never hold `&mut self` while
        // the shared buffer is locked.
        let mut row_rects: Vec<(usize, egui::Rect)> = Vec::new();
        let mut cols = 0usize;
        let mut hovering_link = false;
        // The paint closure only holds an immutable borrow of `self` (through
        // the buffer guard), so it flags the consumed scroll here and `show`
        // clears the one-shot afterwards.
        let mut scrolled_to_match = false;

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if let Ok(buf) = self.buffer.lock() {
                    use egui::text::{LayoutJob, TextFormat};
                    cols = buf.cols;
                    // Paint retained history above the live grid so output that
                    // scrolled off is still reachable. Bounded to keep the frame
                    // cheap (egui lays out every painted row).
                    let sel = self.selection;
                    let is_selected = |ri: usize, ci: usize| -> bool {
                        match sel {
                            Some(((r0, c0), (r1, c1))) => {
                                if ri < r0 || ri > r1 {
                                    return false;
                                }
                                let s = if ri == r0 { c0 } else { 0 };
                                let e = if ri == r1 { c1 } else { usize::MAX };
                                ci >= s && ci < e
                            }
                            None => false,
                        }
                    };
                    for (ri, row) in buf.display_rows(MAX_SB_SHOWN).iter().enumerate() {
                        // Trim trailing blank cells so the line height is stable
                        // but we don't paint a full row of spaces.
                        let last = row
                            .iter()
                            .rposition(|c| {
                                c.ch != ' '
                                    || c.attrs.bg_color.is_some()
                                    || c.attrs.bg_rgb.is_some()
                            })
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        let mut job = LayoutJob::default();
                        // With Ctrl down, linkify this row: spans get an
                        // accent underline (and drive the hover cursor above).
                        let row_links: Vec<DetectedLink> = if links_active && last > 0 {
                            let chars: Vec<char> = row[..last].iter().map(|c| c.ch).collect();
                            detect_links(&chars)
                        } else {
                            Vec::new()
                        };
                        for (ci, cell) in row[..last].iter().enumerate() {
                            let color = cell.attrs.resolved_fg(palette.text);
                            let mut fmt = TextFormat {
                                font_id: code_font.clone(),
                                color,
                                ..Default::default()
                            };
                            let bg = cell.attrs.resolved_bg();
                            if bg != egui::Color32::TRANSPARENT {
                                fmt.background = bg;
                            }
                            if is_selected(ri, ci) {
                                fmt.background = SEL_BG;
                            }
                            if cell.attrs.underline {
                                fmt.underline = egui::Stroke::new(1.0, color);
                            }
                            if cell.attrs.italic {
                                fmt.italics = true;
                            }
                            if row_links.iter().any(|l| ci >= l.start && ci < l.end) {
                                fmt.underline = egui::Stroke::new(1.0, palette.accent);
                            }
                            if cell.link.is_some() {
                                // OSC 8 is authoritative: always underlined.
                                fmt.underline = egui::Stroke::new(1.0, palette.accent);
                            }
                            job.append(&cell.ch.to_string(), 0.0, fmt);
                        }
                        if last == 0 {
                            // Preserve blank-line height.
                            job.append(
                                " ",
                                0.0,
                                TextFormat {
                                    font_id: code_font.clone(),
                                    color: palette.text,
                                    ..Default::default()
                                },
                            );
                        }
                        let resp = ui.label(job);
                        // Pointing hand while the pointer sits on a link span
                        // (a real OSC 8 cell link counts even with no
                        // heuristic match on the row).
                        if links_active {
                            if let Some(pt) = hover_pt {
                                if resp.rect.contains(pt) && char_width > 0.0 {
                                    let cell =
                                        (((pt.x - resp.rect.min.x) / char_width).floor().max(0.0)
                                            as usize)
                                            .min(last.saturating_sub(1));
                                    let on_link =
                                        row_links.iter().any(|l| cell >= l.start && cell < l.end)
                                            || row.get(cell).is_some_and(|c| c.link.is_some());
                                    if on_link {
                                        hovering_link = true;
                                    }
                                }
                            }
                        }
                        row_rects.push((ri, resp.rect));
                        if self.find_scroll_to == Some(ri) {
                            resp.scroll_to_me(Some(egui::Align::Center));
                            scrolled_to_match = true;
                        }
                    }
                }
            });
        if scrolled_to_match {
            self.find_scroll_to = None;
        }

        // Resolve the pointer against this frame's row rects and drive the
        // selection state machine (press anchors, drag extends, release keeps).
        let (interact_pos, pressed, down) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.button_down(egui::PointerButton::Primary),
            )
        });
        if pressed {
            let g = interact_pos.and_then(|pt| {
                row_rects
                    .iter()
                    .find(|(_, r)| pt.y >= r.min.y && pt.y <= r.max.y)
                    .map(|&(ri, r)| (ri, col_from_x(pt.x - r.min.x, char_width, cols)))
            });
            // Ctrl+click fires a detected link instead of anchoring a selection.
            let mut fired_link = false;
            if links_active {
                if let (Some(pt), Some((ri, _))) = (interact_pos, g) {
                    if char_width > 0.0 {
                        let row_x = row_rects
                            .iter()
                            .find(|r| r.0 == ri)
                            .map(|(_, r)| r.min.x)
                            .unwrap_or(pt.x);
                        let cell = ((pt.x - row_x) / char_width).floor().max(0.0) as usize;
                        let (row_chars, cell_target): (Vec<char>, Option<Arc<str>>) = self
                            .buffer
                            .lock()
                            .map(|buf| {
                                buf.display_rows(MAX_SB_SHOWN).get(ri).map_or_else(
                                    || (Vec::new(), None),
                                    |r| {
                                        (
                                            r.iter().map(|c| c.ch).collect(),
                                            r.get(cell).and_then(|c| c.link.clone()),
                                        )
                                    },
                                )
                            })
                            .unwrap_or_default();
                        // A real OSC 8 target beats any text heuristic.
                        if let Some(uri) = cell_target {
                            self.pending_link = Some(LinkKind::Url(uri.to_string()));
                            fired_link = true;
                        } else if let Some(link) = detect_links(&row_chars)
                            .into_iter()
                            .find(|l| cell >= l.start && cell < l.end)
                        {
                            self.pending_link = Some(link.kind);
                            fired_link = true;
                        }
                    }
                }
            }
            if !fired_link {
                self.sel_anchor = g;
                self.sel_focus = g;
                self.selection = g.map(|a| (a, a));
            }
        } else if down {
            if let (Some(a), Some(pt)) = (self.sel_anchor, interact_pos) {
                let f = row_rects
                    .iter()
                    .find(|(_, r)| pt.y >= r.min.y && pt.y <= r.max.y)
                    .map(|&(ri, r)| (ri, col_from_x(pt.x - r.min.x, char_width, cols)));
                if let Some(f) = f {
                    self.sel_focus = Some(f);
                    self.selection = Some(normalize_sel(a, f));
                }
            }
        }
        if hovering_link {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // Ctrl/Cmd+C copies the current (non-degenerate) selection to the
        // system clipboard, extracting through the tested `selection_text`.
        let copy = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::C));
        if copy {
            if let Some(((r0, c0), (r1, c1))) = self.selection {
                if (r0, c0) != (r1, c1) {
                    let grid: Vec<Vec<char>> = self
                        .buffer
                        .lock()
                        .map(|buf| {
                            buf.display_rows(MAX_SB_SHOWN)
                                .into_iter()
                                .map(|r| r.iter().map(|c| c.ch).collect())
                                .collect()
                        })
                        .unwrap_or_default();
                    let text = selection_text(&grid, (r0, c0), (r1, c1));
                    if !text.is_empty() {
                        ui.ctx().copy_text(text);
                    }
                }
            }
        }

        // Input line
        ui.horizontal(|ui| {
            ui.colored_label(palette.accent, "\u{276F}");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.input_line)
                    .font(code_font)
                    .desired_width(f32::INFINITY)
                    .hint_text("Enter command..."),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let cmd = self.input_line.clone();
                self.send_command(&cmd);
                self.input_line.clear();
                resp.request_focus();
            }
        });
    }
}

/// Map an ANSI colour index to an egui colour. Indices 0..=15 are the standard
/// and bright 16-colour palette; 16..=255 fall through to a reasonable
/// approximation from the xterm 256-colour cube.
pub fn ansi_color(idx: u8) -> egui::Color32 {
    match idx {
        0 => egui::Color32::from_rgb(0, 0, 0),
        1 => egui::Color32::from_rgb(205, 49, 49),
        2 => egui::Color32::from_rgb(13, 188, 121),
        3 => egui::Color32::from_rgb(229, 229, 16),
        4 => egui::Color32::from_rgb(36, 114, 200),
        5 => egui::Color32::from_rgb(188, 63, 188),
        6 => egui::Color32::from_rgb(17, 168, 205),
        7 => egui::Color32::from_rgb(229, 229, 229),
        8 => egui::Color32::from_rgb(102, 102, 102),
        9 => egui::Color32::from_rgb(241, 76, 76),
        10 => egui::Color32::from_rgb(35, 209, 139),
        11 => egui::Color32::from_rgb(245, 245, 67),
        12 => egui::Color32::from_rgb(59, 142, 234),
        13 => egui::Color32::from_rgb(214, 112, 214),
        14 => egui::Color32::from_rgb(41, 184, 219),
        15 => egui::Color32::from_rgb(255, 255, 255),
        16..=231 => {
            // 6x6x6 colour cube.
            let n = idx - 16;
            let r = n / 36;
            let g = (n % 36) / 6;
            let b = n % 6;
            let comp = |v: u8| if v == 0 { 0u8 } else { 55 + v * 40 };
            egui::Color32::from_rgb(comp(r), comp(g), comp(b))
        }
        232..=255 => {
            // Grayscale ramp.
            let level = 8 + (idx - 232) * 10;
            egui::Color32::from_rgb(level, level, level)
        }
    }
}

/// What a link detected in a terminal row points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    /// An `http(s)` URL; Ctrl+clicking opens it in the default browser.
    Url(String),
    /// A source-file path, optionally with a 1-based `:line` suffix as
    /// emitted by compilers and linters (`src\main.rs:12:34`, `/a/b.py:7`).
    File { path: String, line: Option<usize> },
}

/// A clickable span inside one terminal row: `[start, end)` cell columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedLink {
    pub start: usize,
    pub end: usize,
    pub kind: LinkKind,
}

/// Extensions treated as openable file tokens. Broad enough to catch
/// compiler/linter/git output, narrow enough that ordinary words never link.
const LINKABLE_EXTS: &[&str] = &[
    "rs", "go", "py", "js", "jsx", "mjs", "ts", "tsx", "c", "h", "cpp", "cc", "cxx", "hpp", "java",
    "kt", "kts", "rb", "php", "cs", "swift", "scala", "sh", "bash", "ps1", "bat", "lua", "toml",
    "json", "yaml", "yml", "md", "txt", "html", "css", "sql",
];

/// Trailing punctuation stripped off a URL token (sentences and shells wrap
/// links in these; a real trailing `)` on a wiki URL is an accepted loss).
const URL_TRAILING_PUNCT: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"'];

/// Scan one terminal row for clickable links (URLs and `path[:line][:col]`
/// file references). Whitespace-delimited token scan, so a wrapped compiler
/// diagnostic links exactly the `file.rs:12:34` token, not the prose around
/// it. Spans are cell column indices (`end` exclusive) into `row`.
pub fn detect_links(row: &[char]) -> Vec<DetectedLink> {
    let mut links = Vec::new();
    let mut i = 0usize;
    while i < row.len() {
        if row[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < row.len() && !row[i].is_whitespace() {
            i += 1;
        }
        let token: String = row[start..i].iter().collect();
        let trimmed_end = token.trim_end_matches(URL_TRAILING_PUNCT).len();
        if trimmed_end == 0 {
            continue;
        }
        let token = token[..trimmed_end].to_string();
        let kind = if token.starts_with("http://") || token.starts_with("https://") {
            Some(LinkKind::Url(token.clone()))
        } else {
            split_file_token(&token).map(|(path, line)| LinkKind::File { path, line })
        };
        if let Some(kind) = kind {
            links.push(DetectedLink {
                start,
                end: start + token.chars().count(),
                kind,
            });
        }
    }
    links
}

/// Split a whitespace token into `(path, optional 1-based line)` when it
/// names a linkable file, with an optional `:line` or `:line:col` suffix.
/// Colon segments are consumed from the *right*, so Windows drive letters
/// (`C:\\work\\app.py:7:1`) survive intact in the path.
fn split_file_token(token: &str) -> Option<(String, Option<usize>)> {
    let chars: Vec<char> = token.chars().collect();
    let mut segs: Vec<usize> = Vec::new();
    let mut pos = chars.len();
    while segs.len() < 2 {
        let mut q = pos;
        while q > 0 && chars[q - 1].is_ascii_digit() {
            q -= 1;
        }
        // Require digits *and* a preceding colon that isn't the whole token.
        if q == pos || q == 1 || chars[q - 1] != ':' {
            break;
        }
        let n: usize = chars[q..pos].iter().collect::<String>().parse().ok()?;
        segs.push(n);
        pos = q - 1; // step over the colon
    }
    let path: String = chars[..pos].iter().collect();
    if !has_linkable_ext(&path) {
        return None;
    }
    let line = match segs.len() {
        0 => None,
        1 => Some(segs[0]),
        _ => Some(segs[1]), // segs[0] was the column, closer to the right edge
    };
    Some((path, line))
}

/// Whether the path ends in a known source/config extension (case-insensitive).
fn has_linkable_ext(path: &str) -> bool {
    let after_dot = match path.rsplit(['.', '/']).next() {
        Some(s) => s,
        None => return false,
    };
    // rsplit('.') on "dir" yields the whole name; only trust it if it differs.
    if after_dot == path || after_dot.is_empty() {
        return false;
    }
    LINKABLE_EXTS.contains(&after_dot.to_lowercase().as_str())
}

/// Best-effort: open an `http(s)` URL in the OS default browser.
pub fn open_url_in_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

/// A terminal grid coordinate: `(row, column)`, both 0-based cell indices.
pub type GridPos = (usize, usize);

/// Order two grid positions so the returned first precedes (or equals) the
/// second by row, then column. Selection anchors are recorded pressâ†’release,
/// but highlighting and copy always work from this normalized pair so a
/// backwards (right-to-left / bottom-to-top) drag behaves identically.
pub fn normalize_sel(a: GridPos, b: GridPos) -> (GridPos, GridPos) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Map an x-offset in pixels (relative to a row's left edge) to a character
/// column boundary, rounding to the nearest cell and clamping to `cols`. Using
/// a *boundary* index (0..=cols) lets the selection end sit just past the last
/// chosen character, which the exclusive-`end` [`selection_text`] expects.
pub fn col_from_x(rel_x: f32, char_width: f32, cols: usize) -> usize {
    if char_width <= 0.0 {
        return 0;
    }
    let col = (rel_x / char_width).round();
    if col <= 0.0 {
        0
    } else if col >= cols as f32 {
        cols
    } else {
        col as usize
    }
}

/// Extract the text of a grid selection spanning `[start, end)` (row/col
/// boundary indices, with `end` exclusive) from a character grid. Interior rows
/// run to their full width; the first row starts at `start.1` and the last row
/// ends at `end.1`. Trailing spaces on each row are trimmed (terminal copy
/// semantics) and rows are joined with newlines. A degenerate (empty) selection
/// yields `""`. Positions outside `rows` are clamped, so a selection made against
/// a slightly older layout never panics.
pub fn selection_text(rows: &[Vec<char>], start: GridPos, end: GridPos) -> String {
    let ((r0, c0), (r1, c1)) = normalize_sel(start, end);
    if (r0, c0) == (r1, c1) {
        return String::new();
    }
    let last_row = rows.len().saturating_sub(1);
    let mut lines: Vec<String> = Vec::new();
    for r in r0..=r1.min(last_row) {
        let row = &rows[r];
        let s = if r == r0 { c0 } else { 0 }.min(row.len());
        let e = if r == r1 { c1 } else { row.len() }.clamp(s, row.len());
        let line: String = row[s..e].iter().collect();
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

/// Cap on rows the find scans per frame (matches the renderer's shown cap).
const FIND_MAX_SHOWN: usize = 1000;

/// Search `rows` for `query`, case-insensitively, returning the `(row, col)`
/// of every match in row-major order. Matches within a row never overlap
/// (the scan resumes just past each hit), and an empty query finds nothing.
/// Lowercasing is per-char so column indices always line up with the grid.
pub fn find_in_rows(rows: &[Vec<char>], query: &str) -> Vec<GridPos> {
    let q: Vec<char> = query
        .chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        let lowered: Vec<char> = row
            .iter()
            .map(|c| c.to_lowercase().next().unwrap_or(*c))
            .collect();
        let mut i = 0;
        while i + q.len() <= lowered.len() {
            if lowered[i..i + q.len()] == q[..] {
                hits.push((ri, i));
                i += q.len();
            } else {
                i += 1;
            }
        }
    }
    hits
}

/// Pick the match index after (or before, when `forward` is false) position
/// `from`, wrapping around the ends. No current position means "first match"
/// going forward and "last match" going backward; an empty list yields `None`.
pub fn next_match_after(
    matches: &[GridPos],
    from: Option<GridPos>,
    forward: bool,
) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(if forward {
        match from {
            Some(f) => matches.iter().position(|&m| m > f).unwrap_or(0),
            None => 0,
        }
    } else {
        match from {
            Some(f) => matches
                .iter()
                .rposition(|&m| m < f)
                .unwrap_or(matches.len() - 1),
            None => matches.len() - 1,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_buffer_basic() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.process_output(b"Hello, World!\n");
        let lines = buf.render_lines();
        assert_eq!(lines[0], "Hello, World!");
    }

    #[test]
    fn terminal_cursor_movement() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.process_output(b"ABC\x1b[1;1H");
        assert_eq!(buf.cursor_row, 0);
        assert_eq!(buf.cursor_col, 0);
    }

    #[test]
    fn terminal_clear() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.process_output(b"text\x1b[2J");
        assert_eq!(buf.cursor_row, 0);
    }

    fn char_grid(lines: &[&str]) -> Vec<Vec<char>> {
        lines.iter().map(|l| l.chars().collect()).collect()
    }

    #[test]
    fn find_in_rows_is_case_insensitive_and_row_major() {
        let g = char_grid(&["alpha BETA", "nothing", "beta at start"]);
        assert_eq!(find_in_rows(&g, "beta"), vec![(0, 6), (2, 0)]);
    }

    #[test]
    fn find_in_rows_matches_do_not_overlap() {
        let g = char_grid(&["aaaa"]);
        assert_eq!(find_in_rows(&g, "aa"), vec![(0, 0), (0, 2)]);
    }

    #[test]
    fn find_in_rows_empty_query_finds_nothing() {
        let g = char_grid(&["anything"]);
        assert!(find_in_rows(&g, "").is_empty());
    }

    #[test]
    fn next_match_after_wraps_in_both_directions() {
        let m = vec![(0, 0), (1, 2), (3, 5)];
        assert_eq!(next_match_after(&m, Some((1, 2)), true), Some(2));
        assert_eq!(next_match_after(&m, Some((3, 5)), true), Some(0));
        assert_eq!(next_match_after(&m, Some((0, 0)), false), Some(2));
        assert_eq!(next_match_after(&m, Some((1, 2)), false), Some(0));
        assert_eq!(next_match_after(&m, None, true), Some(0));
        assert_eq!(next_match_after(&m, None, false), Some(2));
    }

    #[test]
    fn next_match_after_empty_list_is_none() {
        assert_eq!(next_match_after(&[], None, true), None);
        assert_eq!(next_match_after(&[], Some((0, 0)), false), None);
    }

    #[test]
    fn find_step_highlights_its_match_through_the_selection() {
        // End-to-end over a real buffer: refresh scans the visible grid, and
        // stepping marks the match exactly like a mouse selection would
        // (wash + Ctrl+C copy ride on that pair).
        let mut st = TerminalState::new(20, 3);
        st.buffer.lock().unwrap().process_output(b"error: boom\n");
        st.find_query = "BOOM".into();
        st.open_find();
        st.refresh_find_matches();
        assert_eq!(st.find_matches, vec![(0, 7)]);
        st.find_step(true);
        assert_eq!(st.find_active, 0);
        assert_eq!(st.selection, Some(((0, 7), (0, 11))));
        assert_eq!(st.find_scroll_to, Some(0));
        // Going past the only match wraps back onto it, not off the list.
        st.find_step(true);
        assert_eq!(st.find_active, 0);
    }

    #[test]
    fn scrollback() {
        let mut buf = TerminalBuffer::new(10, 3);
        for i in 0..10 {
            buf.process_output(format!("line{}\n", i).as_bytes());
        }
        assert!(!buf.scrollback.is_empty());
    }

    #[test]
    fn display_rows_prepends_history_and_respects_cap() {
        // 3-row grid; 10 lines pushed, so output scrolls into history.
        let mut buf = TerminalBuffer::new(10, 3);
        for i in 0..10 {
            buf.process_output(format!("line{}\n", i).as_bytes());
        }
        let row_text = |r: &Vec<Cell>| -> String {
            r.iter()
                .map(|c| c.ch)
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        let sb_len = buf.scrollback.len();
        assert!(sb_len >= 2, "lines should have scrolled into history");
        // No cap: full history (oldest first) followed by the 3-row grid.
        let all = buf.display_rows(usize::MAX);
        assert_eq!(all.len(), sb_len + 3);
        assert_eq!(row_text(all[0]), "line0", "oldest history surfaces first");
        assert_eq!(
            row_text(all[sb_len]),
            row_text(&buf.cells[0]),
            "then the live grid"
        );
        // A cap of 2 keeps only the two most-recent history rows above the grid.
        let capped = buf.display_rows(2);
        assert_eq!(capped.len(), 2 + 3);
        assert_eq!(
            row_text(capped[1]),
            row_text(buf.scrollback.back().unwrap()),
            "cap keeps history tail"
        );
    }

    #[test]
    fn command_history() {
        let mut term = TerminalState::default();
        term.history.push("ls".to_string());
        term.history.push("pwd".to_string());
        term.history_idx = 2;
        term.history_up();
        assert_eq!(term.input_line, "pwd");
        term.history_up();
        assert_eq!(term.input_line, "ls");
    }

    #[test]
    fn sgr_sets_and_resets_colors() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.process_output(b"\x1b[31mred\x1b[0mplain");
        // "red" cells carry foreground index 1.
        assert_eq!(buf.cells[0][0].attrs.fg_color, Some(1));
        assert_eq!(buf.cells[0][2].attrs.fg_color, Some(1));
        // After the reset, subsequent cells have no colour.
        assert_eq!(buf.cells[0][3].attrs.fg_color, None);
        // The reset also cleared the running attribute state.
        assert_eq!(buf.cur_attrs.fg_color, None);
    }

    #[test]
    fn sgr_bright_and_background() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.process_output(b"\x1b[1;92;44mX");
        let a = buf.cells[0][0].attrs;
        assert!(a.bold);
        assert_eq!(a.fg_color, Some(10)); // bright green (92 -> 2 + 8)
        assert_eq!(a.bg_color, Some(4)); // blue background
    }

    #[test]
    fn sgr_256_color() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.process_output(b"\x1b[38;5;200mZ");
        assert_eq!(buf.cells[0][0].attrs.fg_color, Some(200));
    }

    #[test]
    fn sgr_truecolor_fg_and_bg() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.process_output(b"\x1b[38;2;10;20;30;48;2;1;2;3mX");
        let a = buf.cells[0][0].attrs;
        assert_eq!(a.fg_rgb, Some([10, 20, 30]));
        assert_eq!(a.bg_rgb, Some([1, 2, 3]));
        // Truecolor clears the indexed channels so precedence is unambiguous.
        assert_eq!(a.fg_color, None);
        assert_eq!(a.bg_color, None);
    }

    #[test]
    fn sgr_truecolor_reset_and_default_clear_rgb() {
        let mut buf = TerminalBuffer::new(80, 24);
        // After a full reset the truecolor foreground must be gone.
        buf.process_output(b"\x1b[38;2;9;9;9m\x1b[0mX");
        assert_eq!(buf.cells[0][0].attrs.fg_rgb, None);
        // And `39` (default fg) clears an already-set truecolor too.
        let mut buf2 = TerminalBuffer::new(80, 24);
        buf2.process_output(b"\x1b[38;2;9;9;9;39mY");
        assert_eq!(buf2.cells[0][0].attrs.fg_rgb, None);
    }

    #[test]
    fn resolved_fg_prefers_truecolor_over_indexed() {
        let mut attrs = CellAttrs::default();
        attrs.fg_color = Some(1); // indexed red
        attrs.fg_rgb = Some([5, 6, 7]);
        assert_eq!(
            attrs.resolved_fg(egui::Color32::WHITE),
            egui::Color32::from_rgb(5, 6, 7)
        );
        // No colour at all â†’ the supplied default.
        assert_eq!(
            CellAttrs::default().resolved_bg(),
            egui::Color32::TRANSPARENT
        );
    }

    #[test]
    fn erase_display_from_cursor() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.process_output(b"ABCDEFGH");
        // Move the cursor to column 3 (0-based) and clear cursorâ†’end of screen.
        buf.process_output(b"\x1b[1;4H\x1b[0J");
        let line = buf.render_lines()[0].clone();
        assert_eq!(line, "ABC", "columns 0..cursor survive, rest erased");
    }

    #[test]
    fn erase_line_before_cursor() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.process_output(b"abcdef");
        // Cursor at column 2, erase startâ†’cursor (CSI 1K).
        buf.process_output(b"\x1b[1;3H\x1b[1K");
        assert_eq!(buf.cells[0][0].ch, ' ');
        assert_eq!(buf.cells[0][2].ch, ' ');
        assert_eq!(
            buf.cells[0][3].ch, 'd',
            "text after the cursor is untouched"
        );
    }

    #[test]
    fn terminal_profile_default_has_name() {
        let profile = TerminalProfile::default_platform();
        assert!(!profile.name.is_empty());
        assert!(!profile.program.is_empty());
    }

    #[test]
    fn terminal_profile_builtin_not_empty() {
        let profiles = TerminalProfile::builtin_profiles();
        assert!(!profiles.is_empty());
        for p in &profiles {
            assert!(!p.name.is_empty());
            assert!(!p.program.is_empty());
        }
    }

    #[test]
    fn terminal_profile_roundtrip_json() {
        let profile = TerminalProfile {
            name: "Test".to_string(),
            program: "/bin/bash".to_string(),
            args: vec!["-l".to_string()],
            cwd: Some("/tmp".to_string()),
            env: std::collections::HashMap::new(),
        };
        let json = serde_json::to_string(&profile).unwrap();
        let back: TerminalProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Test");
        assert_eq!(back.program, "/bin/bash");
        assert_eq!(back.args, vec!["-l"]);
        assert_eq!(back.cwd, Some("/tmp".to_string()));
    }

    #[test]
    fn normalize_orders_backwards_drag() {
        // Right-to-left / bottom-to-top drags normalize to the same pair.
        assert_eq!(normalize_sel((2, 5), (0, 3)), ((0, 3), (2, 5)));
        assert_eq!(normalize_sel((0, 3), (2, 5)), ((0, 3), (2, 5)));
        assert_eq!(normalize_sel((1, 9), (1, 2)), ((1, 2), (1, 9)));
    }

    #[test]
    fn col_from_x_rounds_and_clamps() {
        // cell width 8px: boundary rounds to nearest cell edge.
        assert_eq!(col_from_x(0.0, 8.0, 80), 0);
        assert_eq!(col_from_x(3.9, 8.0, 80), 0);
        assert_eq!(col_from_x(4.0, 8.0, 80), 1);
        assert_eq!(col_from_x(19.0, 8.0, 80), 2);
        // Clamped into [0, cols].
        assert_eq!(col_from_x(-5.0, 8.0, 80), 0);
        assert_eq!(col_from_x(1000.0, 8.0, 80), 80);
        // Degenerate char width never divides.
        assert_eq!(col_from_x(10.0, 0.0, 80), 0);
    }

    #[test]
    fn selection_text_single_row() {
        let grid: Vec<Vec<char>> = vec!["hello world".chars().collect()];
        // "hello" = columns [0,5)
        assert_eq!(selection_text(&grid, (0, 0), (0, 5)), "hello");
        // "world" = columns [6,11)
        assert_eq!(selection_text(&grid, (0, 6), (0, 11)), "world");
        // whole line
        assert_eq!(selection_text(&grid, (0, 0), (0, 11)), "hello world");
        // degenerate empty selection
        assert_eq!(selection_text(&grid, (0, 4), (0, 4)), "");
    }

    #[test]
    fn selection_text_multiline_trims_and_wraps() {
        let grid: Vec<Vec<char>> = vec![
            "abc   ".chars().collect(), // trailing spaces trimmed
            "defgh".chars().collect(),
            "ij    ".chars().collect(),
        ];
        // Select from row0 col1 through row2 col2: interior row1 is full width.
        let text = selection_text(&grid, (0, 1), (2, 2));
        assert_eq!(text, "bc\ndefgh\nij");
        // Backwards drag yields identical text (normalized internally).
        assert_eq!(selection_text(&grid, (2, 2), (0, 1)), text);
    }

    #[test]
    fn selection_text_clamps_out_of_range() {
        let grid: Vec<Vec<char>> = vec!["ab".chars().collect()];
        // end row/col beyond the grid clamps rather than panicking.
        assert_eq!(selection_text(&grid, (0, 0), (9, 9)), "ab");
    }

    /// Helper: run `detect_links` over a str's char row.
    fn links(line: &str) -> Vec<DetectedLink> {
        detect_links(&line.chars().collect::<Vec<char>>())
    }

    #[test]
    fn detect_url_with_trailing_punctuation() {
        let line = "see https://rust-lang.org/tools, ok";
        let ls = links(line);
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].kind,
            LinkKind::Url("https://rust-lang.org/tools".to_string())
        );
        // The span covers exactly the URL, not the comma or the prose.
        let chars: Vec<char> = line.chars().collect();
        let span: String = chars[ls[0].start..ls[0].end].iter().collect();
        assert_eq!(span, "https://rust-lang.org/tools");
    }

    #[test]
    fn detect_windows_compiler_file_line_col() {
        let ls = links("error[E0308]: src\\editor\\terminal.rs:12:34 mismatched types");
        assert_eq!(ls.len(), 1);
        assert_eq!(
            ls[0].kind,
            LinkKind::File {
                path: "src\\editor\\terminal.rs".to_string(),
                line: Some(12),
            }
        );
    }

    #[test]
    fn detect_unix_file_line_and_drive_letter() {
        assert_eq!(
            links("  /home/u/proj/main.go:42 | bug here")[0].kind,
            LinkKind::File {
                path: "/home/u/proj/main.go".to_string(),
                line: Some(42),
            }
        );
        // The drive-letter colon must not be mistaken for a line suffix.
        assert_eq!(
            links("C:\\work\\app.py:7:1")[0].kind,
            LinkKind::File {
                path: "C:\\work\\app.py".to_string(),
                line: Some(7),
            }
        );
    }

    #[test]
    fn detect_bare_file_and_reject_non_links() {
        assert_eq!(
            links("modified  README.md (unstaged)")[0].kind,
            LinkKind::File {
                path: "README.md".to_string(),
                line: None,
            }
        );
        // Unknown extension, no extension, and bare `word:123` never link.
        assert!(links("dump.bin and plain words here").is_empty());
        assert!(links("connecting to host elapsed:4000ms").is_empty());
        assert!(links("nothing dotless here").is_empty());
    }

    #[test]
    fn detect_multiple_links_spans_ordered() {
        let line = "report at https://ex.dev/r and src/app.tsx:5";
        let ls = links(line);
        assert_eq!(ls.len(), 2);
        assert!(matches!(ls[0].kind, LinkKind::Url(_)));
        assert!(matches!(ls[1].kind, LinkKind::File { line: Some(5), .. }));
        assert!(ls[0].end <= ls[1].start);
        // Spans are within [0, row length] so hit-testing can't go out of bounds.
        assert!(ls.iter().all(|l| l.end <= line.chars().count()));
    }

    // â”€â”€ OSC 8 hyperlinks â”€â”€

    #[test]
    fn osc8_stamps_cells_and_empty_uri_closes() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.process_output(b"\x1b]8;;https://example.dev/docs\x07docs\x1b]8;;\x07 tail");
        let row = &buf.cells[0];
        // "docs" carries the URI; the escape payload itself never prints.
        let printed: String = row[..9].iter().map(|c| c.ch).collect();
        assert_eq!(printed, "docs tail");
        assert_eq!(row[0].link.as_deref(), Some("https://example.dev/docs"));
        assert_eq!(row[3].link.as_deref(), Some("https://example.dev/docs"));
        // After the empty-URI close, later cells are link-free.
        assert_eq!(row[4].link, None);
        assert_eq!(row[5].ch, 't');
        assert_eq!(row[5].link, None);
    }

    #[test]
    fn osc8_accepts_st_terminator_and_ignores_params_field() {
        let mut buf = TerminalBuffer::new(80, 24);
        // `id=2` is an optional params field; ST (ESC \) is the other terminator.
        buf.process_output(b"\x1b]8;id=2;https://a.b/c\x1b\\link\x1b]8;id=2;\x1b\\x");
        let row = &buf.cells[0];
        assert_eq!(row[0].ch, 'l');
        assert_eq!(row[0].link.as_deref(), Some("https://a.b/c"));
        assert_eq!(row[3].ch, 'k');
        assert_eq!(row[3].link.as_deref(), Some("https://a.b/c"));
        assert_eq!(row[4].ch, 'x');
        assert_eq!(row[4].link, None);
    }

    #[test]
    fn osc8_uri_may_contain_semicolons_and_survives_chunk_splits() {
        let mut buf = TerminalBuffer::new(80, 24);
        // Query strings legitimately contain ';': only the first two split.
        buf.process_output(b"\x1b]8;;https://x.dev/q?a=1;b=2\x07");
        // The link is still in effect for text arriving in a later chunk.
        buf.process_output(b"href");
        let row = &buf.cells[0];
        assert_eq!(row[0].link.as_deref(), Some("https://x.dev/q?a=1;b=2"));
        assert_eq!(row[3].ch, 'f');
        assert_eq!(row[3].link.as_deref(), Some("https://x.dev/q?a=1;b=2"));
    }

    #[test]
    fn other_osc_codes_are_consumed_not_printed() {
        let mut buf = TerminalBuffer::new(80, 24);
        // OSC 0 sets the window title: swallow it, print what follows.
        buf.process_output(b"\x1b]0;my window title\x07after");
        let row = &buf.cells[0];
        let printed: String = row[..5].iter().map(|c| c.ch).collect();
        assert_eq!(printed, "after");
        assert_eq!(row[0].link, None);
    }
}
