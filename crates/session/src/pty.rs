//! An embedded terminal: a child process on a pseudo terminal whose output is
//! interpreted by `alacritty_terminal` into a grid of cells.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor, Rgb};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::tools::Tool;

pub type Rgb8 = [u8; 3];

pub const DEFAULT_FG: Rgb8 = [0xd4, 0xd4, 0xd4];
pub const DEFAULT_BG: Rgb8 = [0x17, 0x19, 0x1e];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapCell {
    pub ch: char,
    pub fg: Rgb8,
    pub bg: Rgb8,
    pub bold: bool,
}

/// What the UI needs to draw the terminal; detached from the live state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermSnapshot {
    pub cols: usize,
    pub rows: usize,
    /// Row-major, `rows * cols` cells.
    pub cells: Vec<SnapCell>,
    /// Cursor (row, col) when visible.
    pub cursor: Option<(usize, usize)>,
    /// Lines scrolled back into history (0 = following output).
    pub scrolled: usize,
    /// Lines kept in the scrollback history.
    pub history: usize,
}

impl TermSnapshot {
    pub fn cell(&self, row: usize, col: usize) -> &SnapCell {
        &self.cells[row * self.cols + col]
    }

    /// The visible text, one line per row, trailing spaces trimmed.
    pub fn text(&self) -> String {
        (0..self.rows)
            .map(|r| {
                let line: String = (0..self.cols).map(|c| self.cell(r, c).ch).collect();
                line.trim_end().to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    /// New output is available; call [`Terminal::snapshot`].
    Output,
    /// The child exited with this code (if known).
    Exited(Option<u32>),
}

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// Answers terminal queries (cursor position, colors...) back to the child.
#[derive(Clone)]
struct Listener {
    writer: SharedWriter,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event
            && let Ok(mut w) = self.writer.lock()
        {
            let _ = w.write_all(text.as_bytes());
            let _ = w.flush();
        }
    }
}

struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

pub struct Terminal {
    term: Arc<Mutex<Term<Listener>>>,
    writer: SharedWriter,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// Set by the reader thread, cleared by [`snapshot`](Self::snapshot): avoids
    /// flooding the UI with one event per read.
    pending: Arc<AtomicBool>,
    size: (u16, u16),
}

impl Terminal {
    /// Starts `tool` in `cwd` on a `cols` x `rows` terminal.
    pub fn spawn(tool: &Tool, cwd: &Path, cols: u16, rows: u16) -> anyhow::Result<(Self, UnboundedReceiver<TermEvent>)> {
        let (cols, rows) = (cols.max(2), rows.max(2));
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;

        let mut cmd = CommandBuilder::new(&tool.program);
        cmd.args(&tool.args);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let mut child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader()?;
        let writer: SharedWriter = Arc::new(Mutex::new(pair.master.take_writer()?));

        let size = Size { cols: cols as usize, rows: rows as usize };
        let config = Config { scrolling_history: 5000, ..Config::default() };
        let term = Arc::new(Mutex::new(Term::new(config, &size, Listener { writer: writer.clone() })));
        let pending = Arc::new(AtomicBool::new(false));
        let (tx, rx) = unbounded();

        {
            let (term, pending, tx) = (term.clone(), pending.clone(), tx.clone());
            std::thread::Builder::new().name("brio-pty-reader".into()).spawn(move || {
                let mut parser: Processor = Processor::new();
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut t) = term.lock() {
                                parser.advance(&mut *t, &buf[..n]);
                            }
                            notify(&pending, &tx);
                        }
                    }
                }
            })?;
        }
        std::thread::Builder::new().name("brio-pty-wait".into()).spawn(move || {
            let code = child.wait().ok().map(|s| s.exit_code());
            let _ = tx.unbounded_send(TermEvent::Exited(code));
        })?;

        Ok((Self { term, writer, master: pair.master, killer, pending, size: (cols, rows) }, rx))
    }

    /// Sends input (keystrokes, pasted text) to the child.
    pub fn write(&self, bytes: &[u8]) {
        if let Ok(mut t) = self.term.lock() {
            // Typing jumps back to the live output.
            t.scroll_display(Scroll::Bottom);
        }
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.max(2), rows.max(2));
        if (cols, rows) == self.size {
            return;
        }
        self.size = (cols, rows);
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        if let Ok(mut t) = self.term.lock() {
            t.resize(Size { cols: cols as usize, rows: rows as usize });
        }
    }

    /// Scrolls the view through history; positive goes back in time.
    ///
    /// Full-screen programs (alternate screen) have no history, so the wheel
    /// is forwarded to them as mouse wheel reports or arrow keys instead.
    pub fn scroll(&self, lines: i32) {
        if lines == 0 {
            return;
        }
        let Ok(mut t) = self.term.lock() else { return };
        let mode = *t.mode();
        if !mode.contains(TermMode::ALT_SCREEN) {
            t.scroll_display(Scroll::Delta(lines));
            return;
        }
        drop(t);

        let up = lines > 0;
        let seq: &[u8] = if mode.intersects(TermMode::MOUSE_MODE) && mode.contains(TermMode::SGR_MOUSE) {
            if up { b"\x1b[<64;1;1M" } else { b"\x1b[<65;1;1M" }
        } else if mode.contains(TermMode::ALTERNATE_SCROLL) {
            match (up, mode.contains(TermMode::APP_CURSOR)) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            }
        } else {
            return;
        };
        if let Ok(mut w) = self.writer.lock() {
            for _ in 0..lines.unsigned_abs() {
                let _ = w.write_all(seq);
            }
            let _ = w.flush();
        }
    }

    /// Shows the view `offset` lines back into history (0 = live output).
    pub fn scroll_to(&self, offset: usize) {
        if let Ok(mut t) = self.term.lock() {
            let current = t.grid().display_offset() as i32;
            t.scroll_display(Scroll::Delta(offset as i32 - current));
        }
    }

    /// Whether arrow keys must be sent in application mode (`ESC O A`).
    pub fn app_cursor(&self) -> bool {
        self.has_mode(TermMode::APP_CURSOR)
    }

    /// Whether the program wants pasted text wrapped in bracketed paste markers.
    pub fn bracketed_paste(&self) -> bool {
        self.has_mode(TermMode::BRACKETED_PASTE)
    }

    fn has_mode(&self, mode: TermMode) -> bool {
        self.term.lock().is_ok_and(|t| t.mode().contains(mode))
    }

    pub fn kill(&mut self) {
        let _ = self.killer.kill();
    }

    pub fn snapshot(&self) -> TermSnapshot {
        self.pending.store(false, Ordering::SeqCst);
        let Ok(term) = self.term.lock() else {
            return TermSnapshot { cols: 0, rows: 0, cells: Vec::new(), cursor: None, scrolled: 0, history: 0 };
        };
        let content = term.renderable_content();
        let (cols, rows) = (term.columns(), term.screen_lines());
        let offset = content.display_offset;
        let blank = SnapCell { ch: ' ', fg: DEFAULT_FG, bg: DEFAULT_BG, bold: false };
        let mut cells = vec![blank; cols * rows];

        for indexed in content.display_iter {
            let row = indexed.point.line.0 + offset as i32;
            let col = indexed.point.column.0;
            if row < 0 || row as usize >= rows || col >= cols {
                continue;
            }
            let cell = indexed.cell;
            let mut fg = resolve(content.colors, cell.fg);
            let mut bg = resolve(content.colors, cell.bg);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let hidden = cell.flags.intersects(Flags::HIDDEN | Flags::WIDE_CHAR_SPACER);
            cells[row as usize * cols + col] = SnapCell {
                ch: if hidden { ' ' } else { cell.c },
                fg,
                bg,
                bold: cell.flags.contains(Flags::BOLD),
            };
        }

        let cursor_row = content.cursor.point.line.0 + offset as i32;
        let cursor = (content.mode.contains(TermMode::SHOW_CURSOR) && (0..rows as i32).contains(&cursor_row))
            .then(|| (cursor_row as usize, content.cursor.point.column.0.min(cols.saturating_sub(1))));

        let history = term.grid().history_size();
        TermSnapshot { cols, rows, cells, cursor, scrolled: offset, history }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}

fn notify(pending: &AtomicBool, tx: &UnboundedSender<TermEvent>) {
    if !pending.swap(true, Ordering::SeqCst) {
        let _ = tx.unbounded_send(TermEvent::Output);
    }
}

/// Resolves a cell color: colors set by the program (OSC 4/10/11) win over
/// the built-in palette.
fn resolve(colors: &Colors, color: Color) -> Rgb8 {
    let rgb = |c: Rgb| [c.r, c.g, c.b];
    match color {
        Color::Spec(c) => rgb(c),
        Color::Indexed(i) => colors[i as usize].map(rgb).unwrap_or_else(|| palette(i)),
        Color::Named(named) => {
            if let Some(c) = colors[named] {
                return rgb(c);
            }
            match named {
                NamedColor::Foreground | NamedColor::BrightForeground => DEFAULT_FG,
                NamedColor::Background => DEFAULT_BG,
                NamedColor::Cursor => DEFAULT_FG,
                NamedColor::DimForeground => dim(DEFAULT_FG),
                n if (n as usize) < 16 => palette(n as u8),
                // Dim variants follow Foreground/Background/Cursor in the enum.
                n => dim(palette((n as usize).saturating_sub(NamedColor::DimBlack as usize) as u8 % 8)),
            }
        }
    }
}

fn dim([r, g, b]: Rgb8) -> Rgb8 {
    [(r as u16 * 2 / 3) as u8, (g as u16 * 2 / 3) as u8, (b as u16 * 2 / 3) as u8]
}

/// xterm 256-color palette with a softer set of the 16 base colors.
pub fn palette(i: u8) -> Rgb8 {
    const BASE: [Rgb8; 16] = [
        [0x1d, 0x1f, 0x21],
        [0xe0, 0x6c, 0x75],
        [0x98, 0xc3, 0x79],
        [0xe5, 0xc0, 0x7b],
        [0x61, 0xaf, 0xef],
        [0xc6, 0x78, 0xdd],
        [0x56, 0xb6, 0xc2],
        [0xd4, 0xd4, 0xd4],
        [0x5c, 0x63, 0x70],
        [0xff, 0x7b, 0x86],
        [0xb5, 0xe8, 0x90],
        [0xff, 0xd6, 0x8a],
        [0x82, 0xc4, 0xff],
        [0xde, 0x9b, 0xf0],
        [0x7f, 0xd6, 0xe0],
        [0xff, 0xff, 0xff],
    ];
    match i {
        0..=15 => BASE[i as usize],
        16..=231 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            [level(i / 36), level((i / 6) % 6), level(i % 6)]
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            [v, v, v]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_cube_and_grays() {
        assert_eq!(palette(16), [0, 0, 0]);
        assert_eq!(palette(231), [255, 255, 255]);
        assert_eq!(palette(232), [8, 8, 8]);
        assert_eq!(palette(255), [238, 238, 238]);
    }

    #[cfg(unix)]
    #[test]
    fn runs_a_program_and_renders_its_colored_output() {
        let tool = Tool {
            name: "sh".into(),
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "printf 'hello \\033[31mred\\033[0m'; sleep 0.2".into()],
        };
        let dir = tempfile::tempdir().unwrap();
        let (term, mut rx) = Terminal::spawn(&tool, dir.path(), 40, 5).unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut exited = false;
        while !exited {
            assert!(std::time::Instant::now() < deadline, "terminal never exited");
            match rx.try_recv() {
                Ok(TermEvent::Exited(code)) => {
                    assert_eq!(code, Some(0));
                    exited = true;
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        let snap = term.snapshot();
        assert!(snap.text().starts_with("hello red"), "got {:?}", snap.text());
        assert_eq!(snap.cell(0, 6).fg, palette(1));
        assert_eq!(snap.cell(0, 0).fg, DEFAULT_FG);
    }
}
