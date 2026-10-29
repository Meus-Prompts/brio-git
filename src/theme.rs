//! Temas: paletas por tema (superfícies, texto, semânticas, lanes do grafo e
//! ANSI do terminal). Tokens derivados (hover, seleção, fundos de label, diff)
//! são calculados a partir das cores-base para manter as mesmas proporções em
//! todos os temas.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, Visuals};

pub const TOOLBAR_HEIGHT: f32 = 48.0;
pub const TABBAR_HEIGHT: f32 = 34.0;
pub const STATUSBAR_HEIGHT: f32 = 26.0;
pub const ROW_HEIGHT: f32 = 28.0;
pub const LANE_WIDTH: f32 = 22.0;

pub struct Palette {
    pub id: &'static str,
    pub name: &'static str,
    pub light: bool,
    pub app_bg: Color32,
    pub toolbar_bg: Color32,
    pub toolbar_bg1: Color32,
    pub panel_bg0: Color32,
    pub panel_bg1: Color32,
    pub panel_bg2: Color32,
    pub input_bg: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_disabled: Color32,
    pub text_bright: Color32,
    pub text_accent: Color32,
    pub link: Color32,
    pub primary: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub hover_row: Color32,
    pub selected_row: Color32,
    pub added_line: Color32,
    pub deleted_line: Color32,
    pub lanes: [Color32; 10],
    pub lane_bg: [Color32; 10],
    pub lane_row: [Color32; 10],
    /// 16 cores ANSI do terminal (0-7 normais, 8-15 brilhantes).
    pub ansi: [Color32; 16],
}

/// Cores-base de um tema; o resto é derivado em [`build`].
struct Spec {
    id: &'static str,
    name: &'static str,
    light: bool,
    bg: u32,
    toolbar: u32,
    panel0: u32,
    panel1: u32,
    panel2: u32,
    text: u32,
    subtle: u32,
    bright: u32,
    accent: u32,
    link: u32,
    green: u32,
    yellow: u32,
    red: u32,
    lanes: [u32; 10],
    ansi: [u32; 16],
}

const fn hex(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn build(s: &Spec) -> Palette {
    let bg = hex(s.bg);
    let text = hex(s.text);
    let accent = hex(s.accent);
    let lanes = s.lanes.map(hex);
    Palette {
        id: s.id,
        name: s.name,
        light: s.light,
        app_bg: bg,
        toolbar_bg: hex(s.toolbar),
        toolbar_bg1: mix(hex(s.toolbar), text, 0.07),
        panel_bg0: hex(s.panel0),
        panel_bg1: hex(s.panel1),
        panel_bg2: hex(s.panel2),
        input_bg: if s.light { mix(bg, Color32::WHITE, 0.6) } else { mix(bg, Color32::BLACK, 0.25) },
        text,
        text_secondary: hex(s.subtle),
        text_disabled: mix(text, bg, 0.5),
        text_bright: hex(s.bright),
        text_accent: mix(accent, text, 0.35),
        link: hex(s.link),
        primary: accent,
        success: hex(s.green),
        warning: hex(s.yellow),
        danger: hex(s.red),
        hover_row: accent.linear_multiply(0.10),
        selected_row: accent.linear_multiply(0.22),
        added_line: hex(s.green).linear_multiply(0.18),
        deleted_line: hex(s.red).linear_multiply(0.18),
        lane_bg: lanes.map(|l| mix(l, bg, if s.light { 0.62 } else { 0.55 })),
        lane_row: lanes.map(|l| mix(l, bg, 0.85)),
        lanes,
        ansi: s.ansi.map(hex),
    }
}

const DEFAULT_LANES: [u32; 10] = [0x15a0bf, 0x0669f7, 0x8e00c2, 0xc517b6, 0xd90171, 0xcd0101, 0xf25d2e, 0xf2ca33, 0x7bd938, 0x2ece9d];
const DEFAULT_ANSI: [u32; 16] = [
    0x1c1e23, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf, 0x5c6370, 0xf07c85, 0xa8d389, 0xf5d08b, 0x71bfff, 0xd688ed,
    0x66c6d2, 0xffffff,
];

static SPECS: &[Spec] = &[
    // Escuro padrão: valores medidos no runtime do cliente de referência (docs/PARITY.md).
    Spec { id: "dark", name: "brio-git Dark", light: false, bg: 0x1c1e23, toolbar: 0x2a2d34, panel0: 0x272a31, panel1: 0x32363f, panel2: 0x3d424d,
        text: 0xbfbfbf, subtle: 0x999999, bright: 0xffffff, accent: 0x4d88ff, link: 0x40c5ec, green: 0x5cb85c, yellow: 0xde9b43, red: 0xd9413d,
        lanes: DEFAULT_LANES, ansi: DEFAULT_ANSI },
    Spec { id: "light", name: "brio-git Light", light: true, bg: 0xfafafb, toolbar: 0xe9ebef, panel0: 0xf1f2f5, panel1: 0xe4e6eb, panel2: 0xd3d7de,
        text: 0x383838, subtle: 0x696969, bright: 0x000000, accent: 0x4d88ff, link: 0x0b7bb5, green: 0x2e8b3e, yellow: 0xb7791f, red: 0xc62828,
        lanes: DEFAULT_LANES, ansi: [0x383a42, 0xe45649, 0x50a14f, 0xc18401, 0x0184bc, 0xa626a4, 0x0997b3, 0xfafafa, 0x4f525e, 0xe06c75, 0x98c379, 0xd19a66, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff] },
    Spec { id: "rose-pine", name: "Rosé Pine", light: false, bg: 0x191724, toolbar: 0x1f1d2e, panel0: 0x1f1d2e, panel1: 0x26233a, panel2: 0x403d52,
        text: 0xe0def4, subtle: 0x908caa, bright: 0xffffff, accent: 0xc4a7e7, link: 0x9ccfd8, green: 0x31748f, yellow: 0xf6c177, red: 0xeb6f92,
        lanes: [0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xf6c177, 0xeb6f92, 0x31748f, 0xe0def4, 0x908caa, 0xea9a97, 0x3e8fb0],
        ansi: [0x26233a, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4, 0x6e6a86, 0xeb6f92, 0x31748f, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xebbcba, 0xe0def4] },
    Spec { id: "rose-pine-moon", name: "Rosé Pine Moon", light: false, bg: 0x232136, toolbar: 0x2a273f, panel0: 0x2a273f, panel1: 0x393552, panel2: 0x44415a,
        text: 0xe0def4, subtle: 0x908caa, bright: 0xffffff, accent: 0xc4a7e7, link: 0x9ccfd8, green: 0x3e8fb0, yellow: 0xf6c177, red: 0xeb6f92,
        lanes: [0x9ccfd8, 0xc4a7e7, 0xea9a97, 0xf6c177, 0xeb6f92, 0x3e8fb0, 0xe0def4, 0x908caa, 0xebbcba, 0x31748f],
        ansi: [0x393552, 0xeb6f92, 0x3e8fb0, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xea9a97, 0xe0def4, 0x6e6a86, 0xeb6f92, 0x3e8fb0, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xea9a97, 0xe0def4] },
    Spec { id: "rose-pine-dawn", name: "Rosé Pine Dawn", light: true, bg: 0xfaf4ed, toolbar: 0xf2e9e1, panel0: 0xfffaf3, panel1: 0xf2e9e1, panel2: 0xdfdad9,
        text: 0x575279, subtle: 0x797593, bright: 0x26233a, accent: 0x907aa9, link: 0x286983, green: 0x286983, yellow: 0xea9d34, red: 0xb4637a,
        lanes: [0x56949f, 0x907aa9, 0xd7827e, 0xea9d34, 0xb4637a, 0x286983, 0x575279, 0x797593, 0xd7827e, 0x56949f],
        ansi: [0xf2e9e1, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279, 0x9893a5, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279] },
    Spec { id: "tokyo-night", name: "Tokyo Night", light: false, bg: 0x1a1b26, toolbar: 0x16161e, panel0: 0x16161e, panel1: 0x1f2335, panel2: 0x292e42,
        text: 0xc0caf5, subtle: 0x787c99, bright: 0xffffff, accent: 0x7aa2f7, link: 0x7dcfff, green: 0x9ece6a, yellow: 0xe0af68, red: 0xf7768e,
        lanes: [0x7dcfff, 0x7aa2f7, 0xbb9af7, 0xff007c, 0xf7768e, 0xff9e64, 0xe0af68, 0x9ece6a, 0x73daca, 0x2ac3de],
        ansi: [0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6, 0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5] },
    Spec { id: "tokyo-night-storm", name: "Tokyo Night Storm", light: false, bg: 0x24283b, toolbar: 0x1f2335, panel0: 0x1f2335, panel1: 0x292e42, panel2: 0x3b4261,
        text: 0xc0caf5, subtle: 0x737aa2, bright: 0xffffff, accent: 0x7aa2f7, link: 0x7dcfff, green: 0x9ece6a, yellow: 0xe0af68, red: 0xf7768e,
        lanes: [0x7dcfff, 0x7aa2f7, 0xbb9af7, 0xff007c, 0xf7768e, 0xff9e64, 0xe0af68, 0x9ece6a, 0x73daca, 0x2ac3de],
        ansi: [0x1d202f, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6, 0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5] },
    Spec { id: "dracula", name: "Dracula", light: false, bg: 0x282a36, toolbar: 0x21222c, panel0: 0x21222c, panel1: 0x343746, panel2: 0x44475a,
        text: 0xf8f8f2, subtle: 0x9aa5ce, bright: 0xffffff, accent: 0xbd93f9, link: 0x8be9fd, green: 0x50fa7b, yellow: 0xf1fa8c, red: 0xff5555,
        lanes: [0x8be9fd, 0xbd93f9, 0xff79c6, 0x50fa7b, 0xffb86c, 0xff5555, 0xf1fa8c, 0x6272a4, 0xa4ffff, 0xd6acff],
        ansi: [0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2, 0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff] },
    Spec { id: "catppuccin-mocha", name: "Catppuccin Mocha", light: false, bg: 0x1e1e2e, toolbar: 0x181825, panel0: 0x181825, panel1: 0x313244, panel2: 0x45475a,
        text: 0xcdd6f4, subtle: 0xa6adc8, bright: 0xffffff, accent: 0xcba6f7, link: 0x89dceb, green: 0xa6e3a1, yellow: 0xf9e2af, red: 0xf38ba8,
        lanes: [0x89dceb, 0x89b4fa, 0xcba6f7, 0xf5c2e7, 0xf38ba8, 0xfab387, 0xf9e2af, 0xa6e3a1, 0x94e2d5, 0xb4befe],
        ansi: [0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de, 0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8] },
    Spec { id: "catppuccin-latte", name: "Catppuccin Latte", light: true, bg: 0xeff1f5, toolbar: 0xe6e9ef, panel0: 0xe6e9ef, panel1: 0xdce0e8, panel2: 0xccd0da,
        text: 0x4c4f69, subtle: 0x6c6f85, bright: 0x11111b, accent: 0x8839ef, link: 0x1e66f5, green: 0x40a02b, yellow: 0xdf8e1d, red: 0xd20f39,
        lanes: [0x209fb5, 0x1e66f5, 0x8839ef, 0xea76cb, 0xd20f39, 0xfe640b, 0xdf8e1d, 0x40a02b, 0x179299, 0x7287fd],
        ansi: [0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be, 0x6c6f85, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xbcc0cc] },
    Spec { id: "nord", name: "Nord", light: false, bg: 0x2e3440, toolbar: 0x3b4252, panel0: 0x2e3440, panel1: 0x3b4252, panel2: 0x434c5e,
        text: 0xd8dee9, subtle: 0x9aa3b5, bright: 0xeceff4, accent: 0x88c0d0, link: 0x88c0d0, green: 0xa3be8c, yellow: 0xebcb8b, red: 0xbf616a,
        lanes: [0x88c0d0, 0x81a1c1, 0xb48ead, 0x5e81ac, 0xbf616a, 0xd08770, 0xebcb8b, 0xa3be8c, 0x8fbcbb, 0xd8dee9],
        ansi: [0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0, 0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4] },
    Spec { id: "gruvbox-dark", name: "Gruvbox Dark", light: false, bg: 0x282828, toolbar: 0x1d2021, panel0: 0x1d2021, panel1: 0x3c3836, panel2: 0x504945,
        text: 0xebdbb2, subtle: 0xa89984, bright: 0xfbf1c7, accent: 0xfabd2f, link: 0x83a598, green: 0xb8bb26, yellow: 0xfabd2f, red: 0xfb4934,
        lanes: [0x83a598, 0x458588, 0xd3869b, 0xb16286, 0xfb4934, 0xfe8019, 0xfabd2f, 0xb8bb26, 0x8ec07c, 0x689d6a],
        ansi: [0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, 0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2] },
    Spec { id: "one-dark", name: "One Dark", light: false, bg: 0x282c34, toolbar: 0x21252b, panel0: 0x21252b, panel1: 0x2c313a, panel2: 0x3b4048,
        text: 0xabb2bf, subtle: 0x7f848e, bright: 0xffffff, accent: 0x61afef, link: 0x56b6c2, green: 0x98c379, yellow: 0xe5c07b, red: 0xe06c75,
        lanes: [0x56b6c2, 0x61afef, 0xc678dd, 0xe06c75, 0xbe5046, 0xd19a66, 0xe5c07b, 0x98c379, 0x2bbac5, 0x9da5b4],
        ansi: DEFAULT_ANSI },
    Spec { id: "solarized-dark", name: "Solarized Dark", light: false, bg: 0x002b36, toolbar: 0x073642, panel0: 0x00252f, panel1: 0x073642, panel2: 0x0f4552,
        text: 0x93a1a1, subtle: 0x839496, bright: 0xfdf6e3, accent: 0x268bd2, link: 0x2aa198, green: 0x859900, yellow: 0xb58900, red: 0xdc322f,
        lanes: [0x2aa198, 0x268bd2, 0x6c71c4, 0xd33682, 0xdc322f, 0xcb4b16, 0xb58900, 0x859900, 0x93a1a1, 0x657b83],
        ansi: [0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, 0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3] },
    Spec { id: "github-dark", name: "GitHub Dark", light: false, bg: 0x0d1117, toolbar: 0x010409, panel0: 0x0d1117, panel1: 0x161b22, panel2: 0x30363d,
        text: 0xe6edf3, subtle: 0x7d8590, bright: 0xffffff, accent: 0x2f81f7, link: 0x58a6ff, green: 0x3fb950, yellow: 0xd29922, red: 0xf85149,
        lanes: [0x39c5cf, 0x2f81f7, 0xa371f7, 0xdb61a2, 0xf85149, 0xdb6d28, 0xd29922, 0x3fb950, 0x56d4dd, 0x79c0ff],
        ansi: [0x484f58, 0xff7b72, 0x3fb950, 0xd29922, 0x58a6ff, 0xbc8cff, 0x39c5cf, 0xb1bac4, 0x6e7681, 0xffa198, 0x56d364, 0xe3b341, 0x79c0ff, 0xd2a8ff, 0x56d4dd, 0xffffff] },
];

static PALETTES: LazyLock<Vec<Palette>> = LazyLock::new(|| SPECS.iter().map(build).collect());
static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// Temas disponíveis: (id, nome).
pub fn themes() -> Vec<(&'static str, &'static str)> {
    PALETTES.iter().map(|p| (p.id, p.name)).collect()
}

/// Se o tema `id` é claro (desconhecido = escuro).
pub fn is_light_theme(id: &str) -> bool {
    PALETTES.iter().find(|p| p.id == id).is_some_and(|p| p.light)
}

/// Paleta ativa.
pub fn p() -> &'static Palette {
    &PALETTES[CURRENT.load(Ordering::Relaxed).min(PALETTES.len() - 1)]
}

/// Troca a paleta ativa sem reaplicar o estilo (usado para desenhar amostras).
pub fn set_current(id: &str) {
    if let Some(i) = PALETTES.iter().position(|p| p.id == id) {
        CURRENT.store(i, Ordering::Relaxed);
    }
}

#[cfg(test)]
pub fn is_light() -> bool {
    p().light
}

pub fn lane(i: u8) -> Color32 {
    p().lanes[i as usize % 10]
}

pub fn lane_bg(i: u8) -> Color32 {
    p().lane_bg[i as usize % 10]
}

pub fn lane_row(i: u8) -> Color32 {
    p().lane_row[i as usize % 10]
}

/// Cores da ferramenta de merge (A / B / Output) seguem as lanes do tema.
pub fn conflict_ours() -> Color32 {
    lane(0)
}
pub fn conflict_theirs() -> Color32 {
    lane(7)
}
pub fn conflict_output() -> Color32 {
    lane(3)
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn prop(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Ativa o tema `id` (desconhecido = escuro padrão) e aplica ao egui.
pub fn apply(ctx: &egui::Context, id: &str) {
    CURRENT.store(PALETTES.iter().position(|p| p.id == id).unwrap_or(0), Ordering::Relaxed);
    let c = p();
    let light = c.light;
    let mut v = if light { Visuals::light() } else { Visuals::dark() };
    // Tons de interação derivados do painel: mais claros no escuro, mais escuros no claro.
    let step = |base: Color32, k: f32| {
        let f = |x: u8| if light { (x as f32 * (1.0 - k)) as u8 } else { (x as f32 + (255.0 - x as f32) * k) as u8 };
        Color32::from_rgb(f(base.r()), f(base.g()), f(base.b()))
    };
    v.panel_fill = c.panel_bg0;
    v.window_fill = c.panel_bg1;
    v.extreme_bg_color = c.input_bg;
    v.faint_bg_color = c.panel_bg1;
    v.window_stroke = Stroke::new(1.0, c.panel_bg2);
    v.window_corner_radius = CornerRadius::same(4);
    v.menu_corner_radius = CornerRadius::same(4);
    v.hyperlink_color = c.link;
    v.selection.bg_fill = mix(c.primary, c.app_bg, if light { 0.65 } else { 0.55 });
    v.selection.stroke = Stroke::new(1.0, c.text_bright);
    v.override_text_color = Some(c.text);
    let r = CornerRadius::same(3);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, step(c.panel_bg1, 0.08));
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
    v.widgets.inactive.bg_fill = c.panel_bg2;
    v.widgets.inactive.weak_bg_fill = c.panel_bg1;
    v.widgets.inactive.corner_radius = r;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, c.text);
    v.widgets.hovered.bg_fill = step(c.panel_bg2, 0.1);
    v.widgets.hovered.weak_bg_fill = step(c.panel_bg2, 0.06);
    v.widgets.hovered.corner_radius = r;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, c.primary);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, c.text_bright);
    v.widgets.active.bg_fill = step(c.panel_bg2, 0.16);
    v.widgets.active.weak_bg_fill = step(c.panel_bg2, 0.16);
    v.widgets.active.corner_radius = r;
    v.widgets.open.corner_radius = r;
    v.widgets.open.weak_bg_fill = c.panel_bg2;
    let t = if light { egui::Theme::Light } else { egui::Theme::Dark };
    ctx.set_theme(t);
    ctx.set_visuals_of(t, v);

    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.interact_size.y = 22.0;
        s.text_styles.insert(egui::TextStyle::Body, prop(13.0));
        s.text_styles.insert(egui::TextStyle::Button, prop(13.0));
        s.text_styles.insert(egui::TextStyle::Small, prop(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, prop(18.0));
        s.text_styles.insert(egui::TextStyle::Monospace, mono(12.5));
    });
}

/// Fonte de símbolos embutida: garante ▾ ✔ ✏ ⚠ ↑ ↓ em qualquer sistema
/// (DejaVu Sans, licença em assets/fonts/DejaVu-LICENSE.txt).
const EMBEDDED_SYMBOLS: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");

/// Diretórios de fontes do sistema e do usuário, conforme o SO.
fn font_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        let windir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        dirs.push(windir.join("Fonts"));
        if let Some(local) = dirs::data_local_dir() {
            dirs.push(local.join("Microsoft").join("Windows").join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(std::path::PathBuf::from));
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Library/Fonts"));
        }
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(std::path::PathBuf::from));
        if let Some(data) = dirs::data_dir() {
            dirs.push(data.join("fonts"));
        }
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".fonts"));
        }
    }
    dirs
}

/// Índice nome-de-arquivo (minúsculo) → caminho de todas as fontes instaladas.
fn font_index() -> std::collections::HashMap<String, std::path::PathBuf> {
    let mut index = std::collections::HashMap::new();
    let mut stack = font_dirs();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                let lower = name.to_lowercase();
                if lower.ends_with(".ttf") || lower.ends_with(".otf") {
                    index.entry(lower).or_insert(p);
                }
            }
        }
    }
    index
}

/// Fontes do sistema por nome de arquivo (Linux, Windows e macOS), mais a
/// fonte de símbolos embutida como último recurso.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let index = font_index();
    let find = |names: &[&str]| names.iter().find_map(|n| index.get(&n.to_lowercase()).and_then(|p| std::fs::read(p).ok()));
    let add = |fonts: &mut egui::FontDefinitions, name: &str, bytes: Vec<u8>, families: &[FontFamily], first: bool| {
        fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for family in families {
            let list = fonts.families.entry(family.clone()).or_default();
            if first { list.insert(0, name.into()) } else { list.push(name.into()) }
        }
    };
    let ui = ["NotoSans-Regular.ttf", "OpenSans-Regular.ttf", "segoeui.ttf", "Ubuntu-R.ttf", "Cantarell-Regular.otf", "DejaVuSans.ttf"];
    let mono = ["JetBrainsMono-Regular.ttf", "FiraCode-Regular.ttf", "CascadiaMono.ttf", "NotoSansMono-Regular.ttf", "consola.ttf", "UbuntuMono-R.ttf", "DejaVuSansMono.ttf"];
    if let Some(b) = find(&ui) {
        add(&mut fonts, "ui", b, &[FontFamily::Proportional], true);
    }
    if let Some(b) = find(&mono) {
        add(&mut fonts, "mono", b, &[FontFamily::Monospace], true);
    }

    // O egui 0.37 não embute fonte de símbolos: setas, ✔, ✏, ⚠ e ▾ vêm destes
    // fallbacks, consultados só quando a fonte principal não tem o glifo.
    let both = [FontFamily::Proportional, FontFamily::Monospace];
    for (name, files) in [("sym-system", &["seguisym.ttf"][..]), ("sym-noto2", &["NotoSansSymbols2-Regular.ttf"][..]), ("sym-noto", &["NotoSansSymbols-Regular.ttf"][..])] {
        if let Some(b) = find(files) {
            add(&mut fonts, name, b, &both, false);
        }
    }
    add(&mut fonts, "sym-embedded", EMBEDDED_SYMBOLS.to_vec(), &both, false);

    // Prompts de shell (starship, p10k, oh-my-posh) usam ícones Nerd Font na
    // área de uso privado; qualquer Nerd Font Mono instalada serve de fallback.
    let nerd = index.iter().filter(|(n, _)| n.contains("nerdfontmono-regular")).map(|(_, p)| p).min();
    if let Some(bytes) = nerd.and_then(|p| std::fs::read(p).ok()) {
        add(&mut fonts, "nerd", bytes, &[FontFamily::Monospace], false);
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f32 {
        let f = |x: u8| {
            let v = x as f32 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_theme_has_readable_text() {
        for pal in PALETTES.iter() {
            for (what, fg) in [("text", pal.text), ("text_bright", pal.text_bright)] {
                for (surface, bg) in [("app_bg", pal.app_bg), ("panel_bg0", pal.panel_bg0)] {
                    let c = contrast(fg, bg);
                    assert!(c >= 4.5, "{}: {what} sobre {surface} com contraste {c:.2} (< 4.5)", pal.id);
                }
            }
            assert!(contrast(pal.text_secondary, pal.app_bg) >= 3.0, "{}: text_secondary ilegível", pal.id);
        }
    }

    #[test]
    fn theme_ids_are_unique_and_switchable() {
        let ids: Vec<_> = themes().into_iter().map(|(id, _)| id).collect();
        let mut dedup = ids.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(ids.len(), dedup.len());
        assert!(ids.len() >= 12);
        set_current("dracula");
        assert_eq!(p().name, "Dracula");
        set_current("dark");
    }
}
