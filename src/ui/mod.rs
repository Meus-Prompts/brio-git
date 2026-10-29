pub mod detail_panel;
pub mod dialogs;
pub mod file_view;
pub mod graph_view;
pub mod home;
pub mod hosting_view;
pub mod left_panel;
pub mod merge_view;
pub mod palette;
pub mod rebase_view;
pub mod terminal;
pub mod toolbar;

use egui::{Color32, CornerRadius, Response, RichText, Sense, Ui, vec2};

use crate::git::model::ChangeKind;
use crate::theme;

/// Abre uma pasta ou URL no app padrão do sistema (sem passar por shell).
pub fn open_path(target: &std::ffi::OsStr) {
    #[cfg(target_os = "windows")]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    let _ = std::process::Command::new(program)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub fn format_time(secs: i64, fmt: &str) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.with_timezone(&chrono::Local).format(fmt).to_string())
        .unwrap_or_default()
}

pub fn relative_time(secs: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let d = (now - secs).max(0);
    match d {
        0..60 => "agora".into(),
        60..3600 => format!("{} min atrás", d / 60),
        3600..86400 => format!("{} h atrás", d / 3600),
        86400..2_592_000 => format!("{} dias atrás", d / 86400),
        2_592_000..31_536_000 => format!("{} meses atrás", d / 2_592_000),
        _ => format!("{} anos atrás", d / 31_536_000),
    }
}

pub fn change_color(kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => theme::p().success,
        ChangeKind::Deleted => theme::p().danger,
        ChangeKind::Renamed => theme::p().link,
        ChangeKind::Conflicted => theme::p().warning,
        _ => theme::p().warning,
    }
}

/// Botão de destaque (verde/azul) usado em "Commit", "Stage all" etc.
pub fn accent_button(ui: &mut Ui, text: &str, color: Color32, enabled: bool) -> Response {
    let fill = if enabled { color.gamma_multiply(0.85) } else { theme::p().panel_bg2 };
    let text_color = if enabled { theme::p().text_bright } else { theme::p().text_disabled };
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(text_color).strong()).fill(fill).corner_radius(CornerRadius::same(3)),
    )
}

pub fn section_header(ui: &mut Ui, title: &str, open: bool, count: Option<usize>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme::p().hover_row);
    }
    let icon = if open { crate::icons::Icon::ChevronDown } else { crate::icons::Icon::ChevronRight };
    crate::icons::paint(ui.painter(), egui::Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(12.0, 12.0)), icon, theme::p().text);
    let title = elide(ui, title, &theme::prop(12.5), rect.width() - 28.0 - 34.0);
    ui.painter().text(rect.left_center() + vec2(28.0, 0.0), egui::Align2::LEFT_CENTER, title, theme::prop(12.5), theme::p().text);
    if let Some(n) = count {
        ui.painter().text(rect.right_center() - vec2(12.0, 0.0), egui::Align2::RIGHT_CENTER, n.to_string(), theme::prop(12.0), theme::p().text_accent);
    }
    resp
}

/// Texto truncado com reticências para caber em `width`.
pub fn elide(ui: &Ui, text: &str, font: &egui::FontId, width: f32) -> String {
    let full = ui.ctx().fonts_mut(|f| f.layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE).size().x);
    if full <= width {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        let w = ui.ctx().fonts_mut(|f| f.layout_no_wrap(candidate.clone(), font.clone(), Color32::WHITE).size().x);
        if w <= width {
            return candidate;
        }
    }
    "…".into()
}

/// Avatar circular: foto do autor (se houver) com anel na cor da lane, ou
/// iniciais enquanto a foto não chega / quando não existe.
pub fn avatar(ui: &Ui, center: egui::Pos2, radius: f32, initials: &str, email: &str, color: Color32) {
    let p = ui.painter();
    if let Some(tex) = crate::avatars::get(email) {
        p.circle_filled(center, radius, color);
        let inner = egui::Rect::from_center_size(center, vec2((radius - 1.5) * 2.0, (radius - 1.5) * 2.0));
        egui::Image::from_texture(&tex).corner_radius(radius).paint_at(ui, inner);
        return;
    }
    p.circle_filled(center, radius, color);
    p.circle_filled(center, radius - 2.0, theme::p().panel_bg0);
    p.circle_filled(center, radius - 3.0, color.gamma_multiply(0.55));
    p.text(center, egui::Align2::CENTER_CENTER, initials, theme::prop((radius * 0.8).max(8.0)), theme::p().text_bright);
}

pub fn author_color(name: &str) -> Color32 {
    let h = name.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    theme::p().lanes[(h % theme::p().lanes.len() as u32) as usize]
}
