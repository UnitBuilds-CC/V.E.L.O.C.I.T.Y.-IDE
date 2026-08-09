#![allow(dead_code)]
//! Interactive terminal emulator with PTY support.
//!
//! Provides a real pseudo-terminal (conpty on Windows, pty on Unix) for
//! interactive shell sessions within the IDE.

use eframe::egui;
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
}

/// A single character cell in the terminal grid.
#[derive(Debug, Clone, Default)]
pub struct Cell {
    pub ch: char,
    pub attrs: CellAttrs,
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
}

impl TerminalBuffer {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cells = vec![
            vec![
                Cell {
                    ch: ' ',
                    attrs: CellAttrs::default()
                };
                cols
            ];
            rows
        ];
        Self {
            cols,
            rows,
            cells,
            cursor_row: 0,
            cursor_col: 0,
            scrollback: VecDeque::new(),
            scrollback_limit: 5000,
            cur_attrs: CellAttrs::default(),
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
        self.cells[self.cursor_row][self.cursor_col] = Cell { ch, attrs };
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
        self.cells.push(vec![
            Cell {
                ch: ' ',
                attrs: CellAttrs::default()
            };
            self.cols
        ]);
    }

    /// Clear the entire screen.
    pub fn clear(&mut self) {
        for row in &mut self.cells {
            for cell in row.iter_mut() {
                cell.ch = ' ';
                cell.attrs = CellAttrs::default();
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
                }
            } else {
                self.put_char(ch, self.cur_attrs);
            }
        }
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
                // Erase display
                let n: usize = params.parse().unwrap_or(0);
                if n == 2 || n == 3 {
                    self.clear();
                }
            }
            'K' => {
                // Erase line
                let n: usize = params.parse().unwrap_or(0);
                if n == 0 {
                    for col in self.cursor_col..self.cols {
                        self.cells[self.cursor_row][col] = Cell::default();
                    }
                } else if n == 2 {
                    for col in 0..self.cols {
                        self.cells[self.cursor_row][col] = Cell::default();
                    }
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
                39 => self.cur_attrs.fg_color = None,
                40..=47 => self.cur_attrs.bg_color = Some((codes[i] - 40) as u8),
                49 => self.cur_attrs.bg_color = None,
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
                            } else {
                                self.cur_attrs.bg_color = Some(idx);
                            }
                        }
                        i += 2;
                    } else if codes.get(i + 1) == Some(&2) {
                        // RGB truecolor: approximate to nearest indexed slot is
                        // lossy; we only keep the 8-bit channel budget, so skip
                        // the three components without setting a colour.
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

    /// Spawn a process with PTY.
    pub fn spawn_process(&mut self, program: &str, args: &[&str]) {
        use std::process::{Command, Stdio};

        let mut child = match Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                if let Ok(mut buf) = self.buffer.lock() {
                    let msg = format!("Failed to spawn {}: {}\r\n", program, e);
                    buf.process_output(msg.as_bytes());
                }
                return;
            }
        };

        self.running = true;

        // Take stdin for writing
        if let Some(stdin) = child.stdin.take() {
            self.pty_writer = Some(Arc::new(Mutex::new(Box::new(stdin))));
        }

        // Spawn reader thread for stdout
        let buffer = self.buffer.clone();
        if let Some(mut stdout) = child.stdout.take() {
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match stdout.read(&mut buf) {
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

        // Spawn reader thread for stderr
        let buffer2 = self.buffer.clone();
        if let Some(mut stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match stderr.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(mut term_buf) = buffer2.lock() {
                                term_buf.process_output(&buf[..n]);
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
    }

    /// Render the terminal panel in egui.
    pub fn show(&mut self, ui: &mut egui::Ui, palette: &crate::editor::theme::IdePalette) {
        let code_font = egui::FontId::monospace(13.0);

        // Terminal output area
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if let Ok(buf) = self.buffer.lock() {
                    use egui::text::{LayoutJob, TextFormat};
                    for row in &buf.cells {
                        // Trim trailing blank cells so the line height is stable
                        // but we don't paint a full row of spaces.
                        let last = row
                            .iter()
                            .rposition(|c| c.ch != ' ' || c.attrs.bg_color.is_some())
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        let mut job = LayoutJob::default();
                        for cell in &row[..last] {
                            let color = cell.attrs.fg_color.map(ansi_color).unwrap_or(palette.text);
                            let mut fmt = TextFormat {
                                font_id: code_font.clone(),
                                color,
                                ..Default::default()
                            };
                            if let Some(bg) = cell.attrs.bg_color {
                                fmt.background = ansi_color(bg);
                            }
                            if cell.attrs.underline {
                                fmt.underline = egui::Stroke::new(1.0, color);
                            }
                            if cell.attrs.italic {
                                fmt.italics = true;
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
                        ui.label(job);
                    }
                }
            });

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

    #[test]
    fn scrollback() {
        let mut buf = TerminalBuffer::new(10, 3);
        for i in 0..10 {
            buf.process_output(format!("line{}\n", i).as_bytes());
        }
        assert!(!buf.scrollback.is_empty());
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
}
