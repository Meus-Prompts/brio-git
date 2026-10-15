//! Line diff of two texts, shaped for display.
//!
//! [`FileDiff::compute`] produces both a unified list (removed lines before
//! added ones) and side-by-side rows where replaced lines are paired up. Paired
//! lines also carry the byte ranges of the words that changed, so the viewer
//! can highlight `isTaxId,` inside an otherwise identical line.

use std::ops::Range;

use similar::{ChangeTag, DiffTag, TextDiff};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Context,
    Added,
    Removed,
}

/// One line on one side. `line` indexes into [`FileDiff::old`] for context and
/// removed lines, and into [`FileDiff::new`] for added lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub kind: Kind,
    pub line: usize,
    /// Byte ranges of changed words inside the line.
    pub emph: Vec<Range<usize>>,
}

/// A line of the unified view. Context lines know both line numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedLine {
    pub cell: Cell,
    pub old_no: Option<usize>,
    pub new_no: Option<usize>,
}

/// A row of the split view; context rows have both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitRow {
    pub left: Option<Cell>,
    pub right: Option<Cell>,
}

impl SplitRow {
    fn is_change(&self) -> bool {
        self.left.as_ref().is_some_and(|c| c.kind != Kind::Context) || self.right.as_ref().is_some_and(|c| c.kind != Kind::Context)
    }
}

/// Either a row to show or a run of hidden unchanged rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row<'a, T> {
    Line(&'a T),
    Gap(usize),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub old: Vec<String>,
    pub new: Vec<String>,
    pub unified: Vec<UnifiedLine>,
    pub split: Vec<SplitRow>,
    pub added: usize,
    pub removed: usize,
}

/// Paired lines more different than this get no word highlighting.
const MIN_SIMILARITY: f32 = 0.35;

impl FileDiff {
    pub fn compute(old_text: &str, new_text: &str) -> Self {
        let diff = TextDiff::from_lines(old_text, new_text);
        let mut out = FileDiff { old: split_lines(old_text), new: split_lines(new_text), ..Default::default() };

        for op in diff.ops() {
            let (tag, old, new) = op.as_tag_tuple();
            match tag {
                DiffTag::Equal => {
                    for (o, n) in old.zip(new) {
                        out.push_context(o, n);
                    }
                }
                DiffTag::Delete | DiffTag::Insert | DiffTag::Replace => out.push_change(old, new),
            }
        }
        out
    }

    fn push_context(&mut self, o: usize, n: usize) {
        let cell = |line| Cell { kind: Kind::Context, line, emph: Vec::new() };
        self.unified.push(UnifiedLine { cell: cell(o), old_no: Some(o + 1), new_no: Some(n + 1) });
        self.split.push(SplitRow { left: Some(cell(o)), right: Some(cell(n)) });
    }

    fn push_change(&mut self, old: Range<usize>, new: Range<usize>) {
        let mut left: Vec<Cell> = old.clone().map(|line| Cell { kind: Kind::Removed, line, emph: Vec::new() }).collect();
        let mut right: Vec<Cell> = new.clone().map(|line| Cell { kind: Kind::Added, line, emph: Vec::new() }).collect();

        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (a, b) = word_changes(&self.old[l.line], &self.new[r.line]);
            l.emph = a;
            r.emph = b;
        }

        self.removed += left.len();
        self.added += right.len();
        for c in &left {
            self.unified.push(UnifiedLine { cell: c.clone(), old_no: Some(c.line + 1), new_no: None });
        }
        for c in &right {
            self.unified.push(UnifiedLine { cell: c.clone(), old_no: None, new_no: Some(c.line + 1) });
        }
        let rows = left.len().max(right.len());
        let mut left = left.into_iter();
        let mut right = right.into_iter();
        for _ in 0..rows {
            self.split.push(SplitRow { left: left.next(), right: right.next() });
        }
    }

    /// Unified rows; with `context`, only changes and that many lines around them.
    pub fn unified_rows(&self, context: Option<usize>) -> Vec<Row<'_, UnifiedLine>> {
        compact(&self.unified, |l| l.cell.kind != Kind::Context, context)
    }

    pub fn split_rows(&self, context: Option<usize>) -> Vec<Row<'_, SplitRow>> {
        compact(&self.split, SplitRow::is_change, context)
    }

    pub fn is_unchanged(&self) -> bool {
        self.added == 0 && self.removed == 0
    }
}

fn split_lines(text: &str) -> Vec<String> {
    text.lines().map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned()).collect()
}

/// Keeps changed items plus `context` neighbours, folding the rest into gaps.
fn compact<T>(items: &[T], is_change: impl Fn(&T) -> bool, context: Option<usize>) -> Vec<Row<'_, T>> {
    let Some(ctx) = context else {
        return items.iter().map(Row::Line).collect();
    };
    let changes: Vec<usize> = items.iter().enumerate().filter(|(_, t)| is_change(t)).map(|(i, _)| i).collect();
    if changes.is_empty() {
        return items.iter().map(Row::Line).collect();
    }
    let mut keep = vec![false; items.len()];
    for &c in &changes {
        for k in keep.iter_mut().take((c + ctx + 1).min(items.len())).skip(c.saturating_sub(ctx)) {
            *k = true;
        }
    }
    let mut rows = Vec::new();
    let mut hidden = 0;
    for (item, keep) in items.iter().zip(keep) {
        if keep {
            if hidden > 0 {
                rows.push(Row::Gap(hidden));
                hidden = 0;
            }
            rows.push(Row::Line(item));
        } else {
            hidden += 1;
        }
    }
    if hidden > 0 {
        rows.push(Row::Gap(hidden));
    }
    rows
}

/// Byte ranges of the words removed from `old` and added to `new`.
fn word_changes(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let diff = TextDiff::from_words(old, new);
    if diff.ratio() < MIN_SIMILARITY {
        return (Vec::new(), Vec::new());
    }
    let (mut o, mut n) = (0, 0);
    let (mut removed, mut added): (Vec<Range<usize>>, Vec<Range<usize>>) = (Vec::new(), Vec::new());
    for change in diff.iter_all_changes() {
        let len = change.value().len();
        match change.tag() {
            ChangeTag::Equal => {
                o += len;
                n += len;
            }
            ChangeTag::Delete => {
                extend(&mut removed, o..o + len);
                o += len;
            }
            ChangeTag::Insert => {
                extend(&mut added, n..n + len);
                n += len;
            }
        }
    }
    (removed, added)
}

fn extend(ranges: &mut Vec<Range<usize>>, r: Range<usize>) {
    match ranges.last_mut() {
        Some(last) if last.end == r.start => last.end = r.end,
        _ => ranges.push(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(rows: &[SplitRow]) -> Vec<(Option<Kind>, Option<Kind>)> {
        rows.iter().map(|r| (r.left.as_ref().map(|c| c.kind), r.right.as_ref().map(|c| c.kind))).collect()
    }

    #[test]
    fn pairs_replaced_lines_side_by_side() {
        let d = FileDiff::compute("a\nold line\nc\n", "a\nnew line\nextra\nc\n");
        use Kind::*;
        assert_eq!(
            kinds(&d.split),
            vec![
                (Some(Context), Some(Context)),
                (Some(Removed), Some(Added)),
                (None, Some(Added)),
                (Some(Context), Some(Context)),
            ]
        );
        assert_eq!((d.added, d.removed), (2, 1));
        let unified: Vec<_> = d.unified.iter().map(|l| (l.cell.kind, l.old_no, l.new_no)).collect();
        assert_eq!(
            unified,
            vec![
                (Context, Some(1), Some(1)),
                (Removed, Some(2), None),
                (Added, None, Some(2)),
                (Added, None, Some(3)),
                (Context, Some(3), Some(4)),
            ]
        );
    }

    #[test]
    fn highlights_changed_words() {
        let old = "import { camelToSnake, snakeToCamel } from 'x'";
        let new = "import { camelToSnake, isTaxId, snakeToCamel } from 'x'";
        let d = FileDiff::compute(&format!("{old}\n"), &format!("{new}\n"));
        let right = d.split[0].right.as_ref().unwrap();
        let emphasized: String = right.emph.iter().map(|r| &new[r.clone()]).collect();
        assert_eq!(emphasized.trim(), "isTaxId,");
        assert!(d.split[0].left.as_ref().unwrap().emph.is_empty());
    }

    #[test]
    fn unrelated_lines_get_no_word_highlight() {
        let d = FileDiff::compute("alpha beta gamma\n", "completely different text here\n");
        assert!(d.split[0].left.as_ref().unwrap().emph.is_empty());
    }

    #[test]
    fn changes_only_folds_unchanged_runs() {
        let old: String = (1..=20).map(|i| format!("{i}\n")).collect();
        let new = old.replace("10\n", "ten\n");
        let d = FileDiff::compute(&old, &new);
        let rows = d.split_rows(Some(2));
        assert_eq!(rows.first(), Some(&Row::Gap(7)));
        assert_eq!(rows.last(), Some(&Row::Gap(8)));
        assert_eq!(rows.len(), 2 + 5);
        assert_eq!(d.split_rows(None).len(), 20);
    }

    #[test]
    fn new_and_deleted_files() {
        let added = FileDiff::compute("", "x\ny\n");
        assert_eq!(added.added, 2);
        assert!(added.split.iter().all(|r| r.left.is_none()));
        let deleted = FileDiff::compute("x\n", "");
        assert_eq!(deleted.removed, 1);
        assert!(FileDiff::compute("same\n", "same\n").is_unchanged());
    }

    #[test]
    fn crlf_is_stripped_for_display() {
        let d = FileDiff::compute("a\r\nb\r\n", "a\r\nc\r\n");
        assert_eq!(d.old, vec!["a", "b"]);
        assert_eq!(d.new, vec!["a", "c"]);
    }
}
