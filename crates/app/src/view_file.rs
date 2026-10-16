//! The file viewer: split or unified diff, blame and history of one file.

use std::ops::Range;

use iced::widget::{button, center, column, container, rich_text, row, scrollable, space, span, text};
use iced::{Alignment, Color, Element, Fill, Font, Length};
use brio_core::textdiff::{Cell, Kind, Row, SplitRow, UnifiedLine};
use brio_ops::ChangeKind;

use crate::fileview::{DiffDoc, FileTarget, FileView, Mode};
use crate::highlight::Line;
use crate::style;
use crate::util;
use crate::workspace::{Workspace, WsMessage};

/// Rows rendered at most; the rest is summarized.
const MAX_ROWS: usize = 3000;
const CONTEXT: usize = 3;
const CODE_SIZE: f32 = 12.5;
const NO_W: f32 = 44.0;

pub fn file_viewer<'a>(ws: &'a Workspace, fv: &'a FileView) -> Element<'a, WsMessage> {
    let tab = |label: &'static str, mode: Mode| {
        button(text(label).size(12))
            .on_press(WsMessage::SetFileMode(mode))
            .style(if fv.mode == mode { button::primary } else { button::secondary })
            .padding([4, 10])
    };
    let mut bar = row![
        tab("Diff", Mode::Diff),
        tab("History", Mode::History),
        tab("Blame", Mode::Blame),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if fv.mode == Mode::Diff {
        bar = bar
            .push(space().width(8))
            .push(toggle(if ws.changes_only { "Changes only" } else { "Arquivo inteiro" }, ws.changes_only, WsMessage::ToggleChangesOnly))
            .push(toggle(if ws.split { "Split" } else { "Unified" }, ws.split, WsMessage::ToggleSplit));
    }
    bar = bar.push(space::horizontal()).push(
        button(text("← Back").size(12)).on_press(WsMessage::CloseFile).style(button::secondary).padding([4, 10]),
    );
    let header = column![text(&fv.target.path).size(14), bar].spacing(6);

    let body: Element<'a, WsMessage> = match fv.mode {
        Mode::Diff => match &fv.diff {
            None => loading(),
            Some(Err(e)) => message(e),
            Some(Ok(doc)) => diff_body(ws, doc),
        },
        Mode::Blame => match &fv.blame {
            None => loading(),
            Some(Err(e)) => message(e),
            Some(Ok(doc)) => blame_body(&doc.lines, &doc.hl),
        },
        Mode::History => match &fv.history {
            None => loading(),
            Some(Err(e)) => message(e),
            Some(Ok(entries)) => history_body(entries),
        },
    };

    column![container(header).padding([6, 10]).style(style::bar), body].height(Fill).into()
}

fn toggle<'a>(label: &'static str, on: bool, msg: WsMessage) -> button::Button<'a, WsMessage> {
    button(text(label).size(12))
        .on_press(msg)
        .style(if on { button::primary } else { button::secondary })
        .padding([4, 10])
}

fn loading<'a>() -> Element<'a, WsMessage> {
    center(text("Carregando…").color(style::p().muted)).into()
}

fn message(msg: &str) -> Element<'_, WsMessage> {
    center(text(msg).color(style::p().muted)).into()
}

fn diff_body<'a>(ws: &'a Workspace, doc: &'a DiffDoc) -> Element<'a, WsMessage> {
    if let Some(reason) = doc.unavailable {
        return message(reason);
    }
    if doc.diff.is_unchanged() {
        return message("Sem alterações de conteúdo.");
    }
    let context = ws.changes_only.then_some(CONTEXT);
    let mut lines = column![];
    let mut count = 0;
    if ws.split {
        let rows = doc.diff.split_rows(context);
        count = rows.len();
        for r in rows.into_iter().take(MAX_ROWS) {
            lines = lines.push(match r {
                Row::Line(r) => split_row(doc, r),
                Row::Gap(n) => gap(n),
            });
        }
    } else {
        let rows = doc.diff.unified_rows(context);
        count = count.max(rows.len());
        for r in rows.into_iter().take(MAX_ROWS) {
            lines = lines.push(match r {
                Row::Line(l) => unified_row(doc, l),
                Row::Gap(n) => gap(n),
            });
        }
    }
    if count > MAX_ROWS {
        lines = lines.push(gap_text(format!("… mais {} linhas não exibidas", count - MAX_ROWS)));
    }
    let summary = text(format!("+{}  −{}", doc.diff.added, doc.diff.removed)).size(12).color(style::p().muted);
    column![container(summary).padding([2, 12]), scrollable(lines).height(Fill)].into()
}

/// Text and highlighting of a cell; `line` indexes the new file on the new side.
fn cell_source<'a>(doc: &'a DiffDoc, cell: &Cell, new_side: bool) -> (&'a str, Option<&'a Line>) {
    if new_side {
        (&doc.diff.new[cell.line], doc.new_hl.get(cell.line))
    } else {
        (&doc.diff.old[cell.line], doc.old_hl.get(cell.line))
    }
}

fn line_bg(kind: Kind) -> Option<Color> {
    let p = style::p();
    match kind {
        Kind::Added => Some(p.added.scale_alpha(0.13)),
        Kind::Removed => Some(p.removed.scale_alpha(0.13)),
        Kind::Context => None,
    }
}

fn emph_bg(kind: Kind) -> Color {
    let p = style::p();
    if kind == Kind::Added { p.added.scale_alpha(0.38) } else { p.removed.scale_alpha(0.38) }
}

fn split_row<'a>(doc: &'a DiffDoc, r: &'a SplitRow) -> Element<'a, WsMessage> {
    let side = |cell: &'a Option<Cell>, new_side: bool| -> Element<'a, WsMessage> {
        match cell {
            Some(c) => {
                let (raw, hl) = cell_source(doc, c, new_side);
                container(row![number(Some(c.line + 1)), code(raw, hl, &c.emph, emph_bg(c.kind))].spacing(8))
                    .width(Length::FillPortion(1))
                    .style(fill(line_bg(c.kind)))
                    .into()
            }
            None => container(space()).width(Length::FillPortion(1)).height(Fill).style(fill(Some(style::p().panel_alt))).into(),
        }
    };
    row![side(&r.left, false), side(&r.right, true)].spacing(2).into()
}

fn unified_row<'a>(doc: &'a DiffDoc, l: &'a UnifiedLine) -> Element<'a, WsMessage> {
    let (raw, hl) = cell_source(doc, &l.cell, l.cell.kind == Kind::Added);
    let marker = match l.cell.kind {
        Kind::Added => "+",
        Kind::Removed => "−",
        Kind::Context => " ",
    };
    container(
        row![
            number(l.old_no),
            number(l.new_no),
            text(marker).font(Font::MONOSPACE).size(CODE_SIZE).color(style::p().muted),
            code(raw, hl, &l.cell.emph, emph_bg(l.cell.kind)),
        ]
        .spacing(8),
    )
    .width(Fill)
    .style(fill(line_bg(l.cell.kind)))
    .into()
}

fn number<'a>(n: Option<usize>) -> Element<'a, WsMessage> {
    text(n.map(|n| n.to_string()).unwrap_or_default())
        .font(Font::MONOSPACE)
        .size(CODE_SIZE)
        .color(style::p().muted)
        .width(NO_W)
        .align_x(Alignment::End)
        .into()
}

/// A line of code with syntax colors; `ranges` (changed words) get `emph` behind them.
fn code<'a>(raw: &'a str, hl: Option<&'a Line>, ranges: &[Range<usize>], emph: Color) -> Element<'a, WsMessage> {
    let p = style::p();
    // Fall back to plain text when highlighting is missing or out of sync.
    let pieces: Vec<(Color, &str)> = match hl {
        Some(line) if line.iter().map(|(_, t)| t.len()).sum::<usize>() == raw.len() => {
            line.iter().map(|([r, g, b], t)| (Color::from_rgb8(*r, *g, *b), t.as_str())).collect()
        }
        _ => vec![(p.text, raw)],
    };

    let mut spans = Vec::new();
    let mut pos = 0;
    for (color, piece) in pieces {
        let end = pos + piece.len();
        let mut cuts = vec![pos, end];
        for r in ranges {
            for b in [r.start, r.end] {
                if b > pos && b < end {
                    cuts.push(b);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let piece_text = raw[a..b].replace('\t', "    ");
            let emphasized = ranges.iter().any(|r| r.start <= a && b <= r.end);
            let s: text::Span<'a, (), Font> = span(piece_text).color(color);
            spans.push(if emphasized { s.background(emph) } else { s });
        }
        pos = end;
    }
    rich_text(spans).font(Font::MONOSPACE).size(CODE_SIZE).width(Fill).into()
}

fn gap<'a>(n: usize) -> Element<'a, WsMessage> {
    gap_text(format!("⋯ {n} linhas sem alteração"))
}

fn gap_text<'a>(label: String) -> Element<'a, WsMessage> {
    container(text(label).size(12).color(style::p().muted))
        .width(Fill)
        .padding([3, 12])
        .style(fill(Some(style::p().panel_alt)))
        .into()
}

fn fill(color: Option<Color>) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style { background: color.map(Into::into), ..Default::default() }
}

fn blame_body<'a>(lines: &'a [brio_ops::files::BlameLine], hl: &'a [Line]) -> Element<'a, WsMessage> {
    let p = style::p();
    let now = util::now_secs();
    let mut col = column![];
    let mut run = 0usize;
    for (i, l) in lines.iter().enumerate().take(MAX_ROWS) {
        let first = i == 0 || lines[i - 1].id != l.id;
        if first && i > 0 {
            run += 1;
        }
        let info: Element<'a, WsMessage> = if !first {
            space().width(300).into()
        } else if l.is_uncommitted() {
            text("não commitado").size(12).color(p.warn).width(300).into()
        } else {
            row![
                button(text(&l.id[..7]).font(Font::MONOSPACE).size(12).color(p.accent))
                    .on_press(WsMessage::JumpToCommit(l.id.clone()))
                    .style(button::text)
                    .padding(0),
                text(truncate(&l.author, 16)).size(12).width(120),
                text(util::relative_time(l.time, now)).size(12).color(p.muted),
            ]
            .spacing(8)
            .width(300)
            .into()
        };
        let shade = (run % 2 == 1).then(|| p.panel_alt.scale_alpha(0.6));
        col = col.push(
            container(row![info, number(Some(i + 1)), code(&l.text, hl.get(i), &[], p.accent)].spacing(8))
                .padding([0, 8])
                .style(fill(shade)),
        );
    }
    scrollable(col).height(Fill).into()
}

fn history_body(entries: &[brio_ops::files::HistoryEntry]) -> Element<'_, WsMessage> {
    let p = style::p();
    let now = util::now_secs();
    let mut col = column![].spacing(2);
    for e in entries {
        let target = FileTarget {
            path: e.path.clone(),
            old_path: None,
            old_rev: Some(format!("{}^", e.id)),
            new_rev: Some(e.id.clone()),
            kind: ChangeKind::Modified,
        };
        col = col.push(
            button(
                row![
                    text(&e.id[..7]).font(Font::MONOSPACE).size(12).color(p.accent),
                    text(&e.summary).size(13).width(Fill),
                    text(truncate(&e.author, 20)).size(12).color(p.muted).width(150),
                    text(util::relative_time(e.time, now)).size(12).color(p.muted).width(90),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .on_press(WsMessage::OpenFile(target))
            .style(button::text)
            .padding([6, 12])
            .width(Fill),
        );
    }
    if entries.is_empty() {
        return message("Nenhum commit mexeu neste arquivo ainda.");
    }
    scrollable(col).height(Fill).into()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}
