//! Terminal integrado: PTY + emulação VT do `alacritty_terminal`, renderizado no egui.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, Key, Rect, Sense, Ui, pos2, vec2};

use crate::theme;

/// Largura da barra de rolagem à direita do terminal.
const SCROLLBAR_W: f32 = 10.0;

#[derive(Clone)]
struct Listener {
    ctx: egui::Context,
    sender: Arc<Mutex<Option<EventLoopSender>>>,
    exited: Arc<AtomicBool>,
    title: Arc<Mutex<String>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup | Event::Bell | Event::CursorBlinkingChange => self.ctx.request_repaint(),
            Event::Title(t) => *self.title.lock().unwrap() = t,
            Event::ResetTitle => self.title.lock().unwrap().clear(),
            Event::PtyWrite(s) => {
                if let Some(tx) = self.sender.lock().unwrap().as_ref() {
                    Notifier(tx.clone()).notify(s.into_bytes());
                }
            }
            Event::ClipboardStore(_, text) => self.ctx.copy_text(text),
            Event::ChildExit(_) | Event::Exit => {
                self.exited.store(true, Ordering::Relaxed);
                self.ctx.request_repaint();
            }
            _ => {}
        }
    }
}

struct Size {
    cols: usize,
    lines: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    notifier: Notifier,
    listener: Listener,
    size: (usize, usize),
    id: egui::Id,
    /// Número sequencial da aba ("Terminal 2").
    pub seq: u32,
}

impl Terminal {
    pub fn spawn(cwd: &Path, ctx: &egui::Context, seq: u32) -> anyhow::Result<Self> {
        let listener = Listener {
            ctx: ctx.clone(),
            sender: Arc::new(Mutex::new(None)),
            exited: Arc::new(AtomicBool::new(false)),
            title: Arc::new(Mutex::new(String::new())),
        };
        let (cols, lines) = (100usize, 12usize);
        let term = Term::new(Config::default(), &Size { cols, lines }, listener.clone());
        let term = Arc::new(FairMutex::new(term));
        let env: HashMap<String, String> =
            [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")].into_iter().map(|(k, v)| (k.into(), v.into())).collect();
        // Repositório do WSL aberto no Windows: o terminal é um shell da distro.
        let (shell, working_directory) = match crate::wsl::parse(cwd).filter(|_| cfg!(windows)) {
            Some(w) => (Some(tty::Shell::new("wsl.exe".into(), vec!["-d".into(), w.distro, "--cd".into(), w.linux])), None),
            None => (None, Some(cwd.to_path_buf())),
        };
        // No Windows `Options` tem também `escape_args`; o update cobre esse campo.
        #[allow(clippy::needless_update)]
        let opts = tty::Options { shell, working_directory, drain_on_exit: true, env, ..Default::default() };
        let ws = WindowSize { num_lines: lines as u16, num_cols: cols as u16, cell_width: 8, cell_height: 16 };
        let pty = tty::new(&opts, ws, 0)?;
        let event_loop = EventLoop::new(term.clone(), listener.clone(), pty, true, false)?;
        let sender = event_loop.channel();
        *listener.sender.lock().unwrap() = Some(sender.clone());
        event_loop.spawn();
        Ok(Self { term, notifier: Notifier(sender), listener, size: (cols, lines), id: egui::Id::unique(("terminal", cwd, seq)), seq })
    }

    pub fn exited(&self) -> bool {
        self.listener.exited.load(Ordering::Relaxed)
    }

    pub fn title(&self) -> String {
        self.listener.title.lock().unwrap().clone()
    }

    /// Título da aba: o que o shell definiu (OSC 0/2) ou "Terminal N".
    pub fn tab_label(&self) -> String {
        let t = self.title();
        if t.trim().is_empty() { format!("Terminal {}", self.seq) } else { t }
    }

    pub fn focus(&self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(self.id));
    }

    /// Texto visível na tela (usado pelos testes e2e).
    #[cfg(test)]
    pub fn screen_text(&self) -> String {
        let term = self.term.lock();
        let mut out = String::new();
        let mut last_line = None;
        for cell in term.renderable_content().display_iter {
            if last_line.is_some_and(|l| l != cell.point.line) {
                out.push('\n');
            }
            last_line = Some(cell.point.line);
            out.push(cell.c);
        }
        out
    }

    fn write(&self, bytes: impl Into<Vec<u8>>) {
        self.notifier.notify(bytes.into());
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let font = theme::mono(13.0);
        let (cw, ch) = ui.ctx().fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        let (rect, bar) = full.split_left_right_at_x(full.right() - SCROLLBAR_W);
        let resp = ui.interact(rect, self.id, Sense::click_and_drag());
        if resp.clicked() {
            resp.request_focus();
        }

        let cols = ((rect.width() - 8.0) / cw).floor().max(2.0) as usize;
        let lines = ((rect.height() - 4.0) / ch).floor().max(1.0) as usize;
        if (cols, lines) != self.size {
            self.size = (cols, lines);
            self.term.lock().resize(Size { cols, lines });
            self.notifier.on_resize(WindowSize { num_lines: lines as u16, num_cols: cols as u16, cell_width: cw as u16, cell_height: ch as u16 });
        }

        let focused = resp.has_focus();
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(self.id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
            });
            self.handle_input(ui);
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.5 {
                self.term.lock().scroll_display(Scroll::Delta((scroll / ch).round() as i32));
            }
        }

        self.paint(ui, rect.shrink2(vec2(4.0, 2.0)), cw, ch, focused);
        self.scrollbar(ui, bar);
    }

    /// Barra vertical do histórico (scrollback): clicar ou arrastar posiciona a tela.
    fn scrollbar(&self, ui: &Ui, bar: Rect) {
        ui.painter().rect_filled(bar, 0.0, theme::p().app_bg);
        let mut term = self.term.lock();
        let history = term.grid().history_size();
        if history == 0 {
            return;
        }
        let offset = term.grid().display_offset();
        let track = bar.shrink(2.0);
        let total = (history + self.size.1) as f32;
        let thumb_h = (track.height() * self.size.1 as f32 / total).clamp(20.0_f32.min(track.height()), track.height());
        let free = (track.height() - thumb_h).max(1.0);

        let resp = ui.interact(bar, self.id.with("scrollbar"), Sense::click_and_drag());
        if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.clicked() || resp.dragged()) {
            let frac = ((p.y - track.top() - thumb_h / 2.0) / free).clamp(0.0, 1.0);
            let target = ((1.0 - frac) * history as f32).round() as i32;
            term.scroll_display(Scroll::Delta(target - offset as i32));
        }

        let offset = term.grid().display_offset();
        let top = track.top() + free * (1.0 - offset as f32 / history as f32);
        let thumb = Rect::from_min_size(pos2(track.left(), top), vec2(track.width(), thumb_h));
        let color = if resp.dragged() || resp.hovered() { theme::p().text_secondary } else { theme::p().text_disabled };
        ui.painter().rect_filled(thumb, track.width() / 2.0, color);
    }

    fn handle_input(&self, ui: &Ui) {
        let app_cursor = self.term.lock().mode().contains(TermMode::APP_CURSOR);
        let events = ui.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Text(t) => self.write(t.into_bytes()),
                egui::Event::Paste(t) => self.write(t.replace("\r\n", "\r").replace('\n', "\r").into_bytes()),
                egui::Event::Copy => self.write([0x03]),
                egui::Event::Cut => self.write([0x18]),
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(bytes) = key_bytes(key, modifiers, app_cursor) {
                        self.term.lock().scroll_display(Scroll::Bottom);
                        self.write(bytes);
                    }
                }
                _ => {}
            }
        }
    }

    fn paint(&self, ui: &Ui, rect: Rect, cw: f32, ch: f32, focused: bool) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect.expand2(vec2(4.0, 2.0)), 0.0, theme::p().app_bg);
        let term = self.term.lock();
        let content = term.renderable_content();
        let colors = content.colors;
        let offset = content.display_offset as i32;
        let cursor = content.cursor.point;

        let mut rows: Vec<Vec<(char, Color32, Color32, Flags)>> = vec![Vec::new(); self.size.1];
        for cell in content.display_iter {
            let line = cell.point.line.0 + offset;
            if line < 0 || line as usize >= rows.len() {
                continue;
            }
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let mut fg = resolve(cell.fg, colors, true);
            let mut bg = resolve(cell.bg, colors, false);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(Flags::DIM) {
                fg = fg.gamma_multiply(0.6);
            }
            rows[line as usize].push((cell.c, fg, bg, cell.flags));
        }

        let font = theme::mono(13.0);
        for (li, row) in rows.iter().enumerate() {
            let y = rect.top() + li as f32 * ch;
            // Fundos em blocos contíguos da mesma cor.
            let mut col = 0usize;
            let mut i = 0;
            while i < row.len() {
                let bg = row[i].2;
                let start = col;
                while i < row.len() && row[i].2 == bg {
                    col += if row[i].3.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
                    i += 1;
                }
                if bg != theme::p().app_bg {
                    painter.rect_filled(Rect::from_min_size(pos2(rect.left() + start as f32 * cw, y), vec2((col - start) as f32 * cw, ch)), 0.0, bg);
                }
            }
            let mut job = LayoutJob::default();
            let mut run = String::new();
            let mut run_fg = None;
            for (c, fg, _, flags) in row {
                if run_fg != Some(*fg) && !run.is_empty() {
                    job.append(&run, 0.0, TextFormat::simple(font.clone(), run_fg.unwrap()));
                    run.clear();
                }
                run_fg = Some(*fg);
                let _ = flags;
                run.push(if *c == '\0' { ' ' } else { *c });
            }
            if let Some(fg) = run_fg {
                job.append(&run, 0.0, TextFormat::simple(font.clone(), fg));
            }
            let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
            painter.galley(pos2(rect.left(), y), galley, theme::p().text);
        }

        let cl = cursor.line.0 + offset;
        if cl >= 0 && (cl as usize) < self.size.1 {
            let r = Rect::from_min_size(pos2(rect.left() + cursor.column.0 as f32 * cw, rect.top() + cl as f32 * ch), vec2(cw, ch));
            if focused {
                painter.rect_filled(r, 0.0, theme::p().text.gamma_multiply(0.7));
            } else {
                painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, theme::p().text_secondary), egui::StrokeKind::Inside);
            }
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.notifier.0.send(alacritty_terminal::event_loop::Msg::Shutdown);
    }
}

fn key_bytes(key: Key, m: egui::Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let arrow = |c: u8| if app_cursor { vec![0x1b, b'O', c] } else { vec![0x1b, b'[', c] };
    let s = |x: &str| Some(x.as_bytes().to_vec());
    if m.ctrl && !m.alt {
        let name = key.name();
        if name.len() == 1 {
            let c = name.as_bytes()[0].to_ascii_lowercase();
            if c.is_ascii_lowercase() {
                return Some(vec![c - b'a' + 1]);
            }
        }
        match key {
            Key::OpenBracket => return Some(vec![0x1b]),
            Key::Backslash => return Some(vec![0x1c]),
            Key::CloseBracket => return Some(vec![0x1d]),
            _ => {}
        }
    }
    match key {
        Key::Enter => s("\r"),
        Key::Backspace => Some(if m.alt { vec![0x1b, 0x7f] } else { vec![0x7f] }),
        Key::Tab => Some(if m.shift { b"\x1b[Z".to_vec() } else { vec![b'\t'] }),
        Key::Escape => Some(vec![0x1b]),
        Key::ArrowUp => Some(arrow(b'A')),
        Key::ArrowDown => Some(arrow(b'B')),
        Key::ArrowRight => Some(if m.ctrl { b"\x1b[1;5C".to_vec() } else { arrow(b'C') }),
        Key::ArrowLeft => Some(if m.ctrl { b"\x1b[1;5D".to_vec() } else { arrow(b'D') }),
        Key::Home => s("\x1b[H"),
        Key::End => s("\x1b[F"),
        Key::PageUp => s("\x1b[5~"),
        Key::PageDown => s("\x1b[6~"),
        Key::Delete => s("\x1b[3~"),
        Key::Insert => s("\x1b[2~"),
        Key::F1 => s("\x1bOP"),
        Key::F2 => s("\x1bOQ"),
        Key::F3 => s("\x1bOR"),
        Key::F4 => s("\x1bOS"),
        _ => None,
    }
}

fn indexed(i: u8) -> Color32 {
    match i {
        0..=15 => crate::theme::p().ansi[i as usize],
        16..=231 => {
            let i = i - 16;
            let v = |x: u8| if x == 0 { 0 } else { 55 + x * 40 };
            Color32::from_rgb(v(i / 36), v((i / 6) % 6), v(i % 6))
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
    }
}

fn resolve(c: Color, colors: &alacritty_terminal::term::color::Colors, fg: bool) -> Color32 {
    let rgb = |r: Rgb| Color32::from_rgb(r.r, r.g, r.b);
    match c {
        Color::Spec(r) => rgb(r),
        Color::Indexed(i) => colors[i as usize].map(rgb).unwrap_or_else(|| indexed(i)),
        Color::Named(n) => {
            if let Some(r) = colors[n] {
                return rgb(r);
            }
            match n {
                NamedColor::Foreground | NamedColor::BrightForeground => theme::p().text_bright.gamma_multiply(0.9),
                NamedColor::Background => theme::p().app_bg,
                NamedColor::Cursor => theme::p().text,
                NamedColor::DimForeground => theme::p().text_secondary,
                n => {
                    let idx = n as usize;
                    if idx < 16 { theme::p().ansi[idx] } else if fg { theme::p().text } else { theme::p().app_bg }
                }
            }
        }
    }
}
