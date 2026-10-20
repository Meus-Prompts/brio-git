//! Lucide icons (ISC, see `assets/LICENSE-lucide`), drawn as glyphs of the
//! bundled icon font so they take the text color of whatever shows them.

use iced::Font;
use iced::widget::{Text, text};

pub const FONT_BYTES: &[u8] = include_bytes!("../assets/lucide.ttf");
pub const FONT: Font = Font::with_name("lucide");

pub const GIT_BRANCH: char = '\u{e0e5}';
pub const GIT_GRAPH: char = '\u{e559}';
pub const FILE_DIFF: char = '\u{e31a}';
pub const SPARKLES: char = '\u{e417}';
pub const HISTORY: char = '\u{e1f5}';
pub const REFRESH: char = '\u{e149}';
pub const FETCH: char = '\u{e14d}';
pub const PULL: char = '\u{e45a}';
pub const PUSH: char = '\u{e45f}';
pub const STASH: char = '\u{e045}';
pub const POP: char = '\u{e2cd}';
pub const PENCIL: char = '\u{e1f9}';
pub const UPSTREAM: char = '\u{e24a}';
pub const LOADER: char = '\u{e10d}';
pub const CLOSE: char = '\u{e1b2}';
pub const MAXIMIZE: char = '\u{e117}';
pub const RESTORE: char = '\u{e11f}';
pub const LAYOUT: char = '\u{e1c1}';
pub const PLAY: char = '\u{e140}';
pub const STOP: char = '\u{e16b}';
pub const CHEVRON_DOWN: char = '\u{e071}';
pub const CHECK: char = '\u{e070}';
pub const SEARCH: char = '\u{e155}';
pub const BRANCH_PLUS: char = '\u{e1f4}';
pub const CLOUD: char = '\u{e08c}';
pub const GRIP: char = '\u{e0ed}';
pub const PIN: char = '\u{e259}';
pub const SETTINGS: char = '\u{e158}';
pub const PALETTE: char = '\u{e1dd}';
pub const RESET: char = '\u{e14c}';
pub const SUCCESS: char = '\u{e226}';
pub const ALERT: char = '\u{e07b}';
pub const UNDO: char = '\u{e2a1}';
pub const REDO: char = '\u{e2a0}';

/// An icon glyph at `size` pixels.
pub fn icon<'a>(glyph: char, size: f32) -> Text<'a> {
    text(glyph.to_string()).font(FONT).size(size).line_height(1.0)
}
