//! Syntax highlighting with syntect and bat's grammar/theme collection
//! (two-face), which covers TypeScript, Vue, TOML and friends.

use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use two_face::theme::{EmbeddedLazyThemeSet, EmbeddedThemeName};

pub type Rgb = [u8; 3];
/// One highlighted line: colored pieces whose text concatenates to the line.
pub type Line = Vec<(Rgb, String)>;

/// Files bigger than this are shown without colors.
const MAX_BYTES: usize = 1_500_000;

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn themes() -> &'static EmbeddedLazyThemeSet {
    static SET: OnceLock<EmbeddedLazyThemeSet> = OnceLock::new();
    SET.get_or_init(two_face::theme::extra)
}

/// Code theme matching an app theme.
pub fn code_theme(app_theme: &str) -> EmbeddedThemeName {
    match app_theme {
        "Dracula" => EmbeddedThemeName::Dracula,
        "One Dark" | "Dark" => EmbeddedThemeName::TwoDark,
        "Nord" => EmbeddedThemeName::Nord,
        "Gruvbox Dark" => EmbeddedThemeName::GruvboxDark,
        "Solarized Dark" => EmbeddedThemeName::SolarizedDark,
        "Catppuccin Mocha" | "Tokyo Night" => EmbeddedThemeName::CatppuccinMocha,
        "Rosé Pine" => EmbeddedThemeName::CatppuccinMacchiato,
        "Catppuccin Latte" => EmbeddedThemeName::CatppuccinLatte,
        "Rosé Pine Dawn" => EmbeddedThemeName::OneHalfLight,
        "Solarized Light" => EmbeddedThemeName::SolarizedLight,
        "Light" => EmbeddedThemeName::Github,
        _ => EmbeddedThemeName::TwoDark,
    }
}

/// Highlights `text` as the language of `path`, one entry per line
/// (same line split as `str::lines`).
pub fn highlight(text: &str, path: &str, theme: EmbeddedThemeName) -> Vec<Line> {
    let theme = themes().get(theme);
    let fallback: Rgb = theme.settings.foreground.map_or([0xd4, 0xd4, 0xd4], |c| [c.r, c.g, c.b]);
    let plain = || text.lines().map(|l| vec![(fallback, l.to_owned())]).collect();
    if text.len() > MAX_BYTES {
        return plain();
    }

    let set = syntaxes();
    let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default();
    let name = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let syntax = set
        .find_syntax_by_extension(ext)
        .or_else(|| set.find_syntax_by_extension(name))
        .or_else(|| set.find_syntax_by_first_line(text.lines().next().unwrap_or_default()))
        .unwrap_or_else(|| set.find_syntax_plain_text());

    let mut h = HighlightLines::new(syntax, theme);
    let mut out = Vec::new();
    for line in LinesWithEndings::from(text) {
        let Ok(regions) = h.highlight_line(line, set) else { return plain() };
        let pieces: Line = regions
            .into_iter()
            .map(|(style, piece)| {
                let c = style.foreground;
                ([c.r, c.g, c.b], piece.trim_end_matches(['\n', '\r']).to_owned())
            })
            .filter(|(_, p)| !p.is_empty())
            .collect();
        out.push(pieces);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_rust_and_keeps_text() {
        let src = "fn main() {\n    let x = 1;\n}\n";
        let lines = highlight(src, "main.rs", EmbeddedThemeName::TwoDark);
        assert_eq!(lines.len(), 3);
        let first: String = lines[0].iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(first, "fn main() {");
        let colors: std::collections::HashSet<_> = lines[0].iter().map(|(c, _)| *c).collect();
        assert!(colors.len() > 1, "keyword and name get different colors");
    }

    #[test]
    fn knows_vue_and_typescript() {
        for path in ["a.vue", "a.ts", "Cargo.toml"] {
            let lines = highlight("const a = 1\n", path, EmbeddedThemeName::Nord);
            assert_eq!(lines.len(), 1, "{path}");
        }
        assert!(syntaxes().find_syntax_by_extension("vue").is_some());
        assert!(syntaxes().find_syntax_by_extension("ts").is_some());
    }

    #[test]
    fn every_app_theme_has_a_code_theme() {
        for t in &crate::style::THEMES {
            let _ = themes().get(code_theme(t.name));
        }
    }
}
