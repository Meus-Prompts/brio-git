//! Color themes and the container styles built from them.
//!
//! The active theme is app-wide, so it lives in a global read by every view
//! and canvas through [`p`]. [`Palette::iced_theme`] derives the iced theme
//! used by the stock widgets (buttons, inputs) from the same colors.

use std::sync::atomic::{AtomicUsize, Ordering};

use iced::widget::container;
use iced::{Border, Color, Theme, color};

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub name: &'static str,
    pub bg: Color,
    pub panel: Color,
    pub panel_alt: Color,
    pub title: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub selected: Color,
    pub added: Color,
    pub removed: Color,
    pub hunk: Color,
    pub warn: Color,
    pub purple: Color,
    pub orange: Color,
    pub term_bg: Color,
}

impl Palette {
    pub fn iced_theme(&self) -> Theme {
        Theme::custom(
            self.name,
            iced::theme::Palette {
                background: self.panel,
                text: self.text,
                primary: self.accent,
                success: self.added,
                warning: self.warn,
                danger: self.removed,
            },
        )
    }

    /// Lane colors of the commit graph.
    pub fn lane(&self, i: usize) -> Color {
        let lanes = [self.accent, self.warn, self.purple, self.added, self.removed, self.hunk, self.orange, self.muted];
        lanes[i % lanes.len()]
    }
}

macro_rules! palette {
    ($name:literal, bg $bg:literal, panel $panel:literal, alt $alt:literal, title $title:literal,
     border $border:literal, text $text:literal, muted $muted:literal, accent $accent:literal, selected $sel:literal,
     added $added:literal, removed $removed:literal, hunk $hunk:literal, warn $warn:literal, purple $purple:literal,
     orange $orange:literal, term $term:literal) => {
        Palette {
            name: $name,
            bg: color!($bg),
            panel: color!($panel),
            panel_alt: color!($alt),
            title: color!($title),
            border: color!($border),
            text: color!($text),
            muted: color!($muted),
            accent: color!($accent),
            selected: color!($sel),
            added: color!($added),
            removed: color!($removed),
            hunk: color!($hunk),
            warn: color!($warn),
            purple: color!($purple),
            orange: color!($orange),
            term_bg: color!($term),
        }
    };
}

pub static THEMES: [Palette; 13] = [
    palette!("Dark", bg 0x1b1e24, panel 0x21252c, alt 0x262b33, title 0x15171c, border 0x333944,
        text 0xd7dae0, muted 0x8a93a3, accent 0x4fb3ff, selected 0x2c3a4f, added 0x98c379, removed 0xe06c75,
        hunk 0x56b6c2, warn 0xe5c07b, purple 0xc678dd, orange 0xd19a66, term 0x17191e),
    palette!("Tokyo Night", bg 0x1a1b26, panel 0x1f2335, alt 0x24283b, title 0x16161e, border 0x292e42,
        text 0xc0caf5, muted 0x7982a9, accent 0x7aa2f7, selected 0x2e3c64, added 0x9ece6a, removed 0xf7768e,
        hunk 0x7dcfff, warn 0xe0af68, purple 0xbb9af7, orange 0xff9e64, term 0x16161e),
    palette!("Dracula", bg 0x282a36, panel 0x2b2d3a, alt 0x343746, title 0x21222c, border 0x44475a,
        text 0xf8f8f2, muted 0x8a8fb0, accent 0xbd93f9, selected 0x44475a, added 0x50fa7b, removed 0xff5555,
        hunk 0x8be9fd, warn 0xf1fa8c, purple 0xff79c6, orange 0xffb86c, term 0x21222c),
    palette!("One Dark", bg 0x282c34, panel 0x2c313a, alt 0x323842, title 0x21252b, border 0x3e4451,
        text 0xabb2bf, muted 0x7f848e, accent 0x61afef, selected 0x3e4451, added 0x98c379, removed 0xe06c75,
        hunk 0x56b6c2, warn 0xe5c07b, purple 0xc678dd, orange 0xd19a66, term 0x21252b),
    palette!("Nord", bg 0x2e3440, panel 0x3b4252, alt 0x434c5e, title 0x292e39, border 0x4c566a,
        text 0xeceff4, muted 0x9aa5b8, accent 0x88c0d0, selected 0x4c566a, added 0xa3be8c, removed 0xbf616a,
        hunk 0x81a1c1, warn 0xebcb8b, purple 0xb48ead, orange 0xd08770, term 0x2e3440),
    palette!("Gruvbox Dark", bg 0x282828, panel 0x32302f, alt 0x3c3836, title 0x1d2021, border 0x504945,
        text 0xebdbb2, muted 0xa89984, accent 0x83a598, selected 0x504945, added 0xb8bb26, removed 0xfb4934,
        hunk 0x8ec07c, warn 0xfabd2f, purple 0xd3869b, orange 0xfe8019, term 0x1d2021),
    palette!("Solarized Dark", bg 0x002b36, panel 0x073642, alt 0x0a3f4c, title 0x00212b, border 0x1c4f5c,
        text 0x93a1a1, muted 0x657b83, accent 0x268bd2, selected 0x0f4a5a, added 0x859900, removed 0xdc322f,
        hunk 0x2aa198, warn 0xb58900, purple 0x6c71c4, orange 0xcb4b16, term 0x00212b),
    palette!("Catppuccin Mocha", bg 0x1e1e2e, panel 0x181825, alt 0x313244, title 0x11111b, border 0x45475a,
        text 0xcdd6f4, muted 0x9399b2, accent 0x89b4fa, selected 0x3b3d55, added 0xa6e3a1, removed 0xf38ba8,
        hunk 0x94e2d5, warn 0xf9e2af, purple 0xcba6f7, orange 0xfab387, term 0x11111b),
    palette!("Rosé Pine", bg 0x191724, panel 0x1f1d2e, alt 0x26233a, title 0x16141f, border 0x403d52,
        text 0xe0def4, muted 0x908caa, accent 0xc4a7e7, selected 0x393552, added 0x9ccfd8, removed 0xeb6f92,
        hunk 0xebbcba, warn 0xf6c177, purple 0xc4a7e7, orange 0xebbcba, term 0x16141f),
    palette!("Light", bg 0xf5f6f8, panel 0xffffff, alt 0xeef0f3, title 0xe9ebef, border 0xd5d9e0,
        text 0x24292f, muted 0x6e7781, accent 0x0969da, selected 0xdbeafe, added 0x1a7f37, removed 0xcf222e,
        hunk 0x0e7490, warn 0x9a6700, purple 0x8250df, orange 0xbc4c00, term 0xffffff),
    palette!("Catppuccin Latte", bg 0xeff1f5, panel 0xe6e9ef, alt 0xdce0e8, title 0xdce0e8, border 0xccd0da,
        text 0x4c4f69, muted 0x7c7f93, accent 0x1e66f5, selected 0xccd0da, added 0x40a02b, removed 0xd20f39,
        hunk 0x179299, warn 0xdf8e1d, purple 0x8839ef, orange 0xfe640b, term 0xeff1f5),
    palette!("Rosé Pine Dawn", bg 0xfaf4ed, panel 0xfffaf3, alt 0xf2e9e1, title 0xf2e9e1, border 0xdfdad9,
        text 0x575279, muted 0x797593, accent 0x907aa9, selected 0xe4dcd5, added 0x56949f, removed 0xb4637a,
        hunk 0xd7827e, warn 0xea9d34, purple 0x907aa9, orange 0xd7827e, term 0xfffaf3),
    palette!("Solarized Light", bg 0xfdf6e3, panel 0xeee8d5, alt 0xe9e2cb, title 0xeee8d5, border 0xd6cfb8,
        text 0x586e75, muted 0x93a1a1, accent 0x268bd2, selected 0xe3dcc4, added 0x859900, removed 0xdc322f,
        hunk 0x2aa198, warn 0xb58900, purple 0x6c71c4, orange 0xcb4b16, term 0xfdf6e3),
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// The active palette.
pub fn p() -> &'static Palette {
    &THEMES[CURRENT.load(Ordering::Relaxed)]
}

/// Activates the theme called `name`; unknown names are ignored.
pub fn set_theme(name: &str) -> bool {
    match THEMES.iter().position(|t| t.name == name) {
        Some(i) => {
            CURRENT.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

pub fn panel(_: &Theme) -> container::Style {
    let p = p();
    container::Style {
        background: Some(p.panel.into()),
        border: Border { color: p.border, width: 1.0, radius: 0.0.into() },
        text_color: Some(p.text),
        ..Default::default()
    }
}

pub fn bar(_: &Theme) -> container::Style {
    let p = p();
    container::Style {
        background: Some(p.panel_alt.into()),
        border: Border { color: p.border, width: 1.0, radius: 0.0.into() },
        text_color: Some(p.text),
        ..Default::default()
    }
}

pub fn root(_: &Theme) -> container::Style {
    container::Style { background: Some(p().bg.into()), text_color: Some(p().text), ..Default::default() }
}

pub fn error(_: &Theme) -> container::Style {
    let p = p();
    container::Style {
        background: Some(p.removed.scale_alpha(0.18).into()),
        border: Border { color: p.removed, width: 1.0, radius: 4.0.into() },
        text_color: Some(p.text),
        ..Default::default()
    }
}

pub fn title_bar(_: &Theme) -> container::Style {
    container::Style { background: Some(p().title.into()), text_color: Some(p().text), ..Default::default() }
}

pub fn tab(_: &Theme) -> container::Style {
    container::Style { border: Border { radius: 6.0.into(), ..Default::default() }, ..Default::default() }
}

pub fn tab_active(_: &Theme) -> container::Style {
    let p = p();
    container::Style {
        background: Some(p.panel_alt.into()),
        border: Border { color: p.accent, width: 1.0, radius: 6.0.into() },
        ..Default::default()
    }
}

pub fn pane_title(_: &Theme) -> container::Style {
    let p = p();
    container::Style {
        background: Some(p.panel_alt.into()),
        border: Border { color: p.border, width: 1.0, radius: 0.0.into() },
        text_color: Some(p.muted),
        ..Default::default()
    }
}

/// The dimmed backdrop behind a modal.
pub fn backdrop(_: &Theme) -> container::Style {
    container::Style { background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()), ..Default::default() }
}

pub fn modal(_: &Theme) -> container::Style {
    let p = p();
    container::Style {
        background: Some(p.bg.into()),
        border: Border { color: p.border, width: 1.0, radius: 10.0.into() },
        text_color: Some(p.text),
        ..Default::default()
    }
}

/// A flat block of one color (used for theme previews).
pub fn fill(color: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(color.into()),
        border: Border { radius: radius.into(), ..Default::default() },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_names_are_unique_and_selectable() {
        let mut names: Vec<_> = THEMES.iter().map(|t| t.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), THEMES.len());
        assert!(set_theme("Nord"));
        assert_eq!(p().name, "Nord");
        assert!(!set_theme("não existe"));
        assert_eq!(p().name, "Nord");
        set_theme("Dark");
    }
}
