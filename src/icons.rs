//! Ícones vetoriais desenhados com o painter (sem fontes/assets externos).

use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Undo,
    Redo,
    Pull,
    Push,
    Branch,
    Stash,
    Pop,
    Terminal,
    Search,
    Folder,
    Plus,
    Close,
    ChevronRight,
    ChevronDown,
    Computer,
    Cloud,
    Tag,
    Check,
    Eye,
    EyeOff,
    Gear,
    Warning,
    File,
    List,
    Tree,
    ArrowLeft,
    Minimize,
    Maximize,
    Restore,
}

fn line(p: &Painter, a: Pos2, b: Pos2, s: Stroke) {
    p.line_segment([a, b], s);
}

fn arrow_head(p: &Painter, tip: Pos2, dir: Vec2, size: f32, s: Stroke) {
    let d = dir.normalized();
    let n = vec2(-d.y, d.x);
    line(p, tip, tip - d * size + n * size, s);
    line(p, tip, tip - d * size - n * size, s);
}

pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) / 2.0;
    let s = Stroke::new((r / 6.0).clamp(1.2, 2.0), color);
    match icon {
        Icon::Undo | Icon::Redo => {
            let flip = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.15 + i as f32 / 12.0 * 1.2);
                    pos2(c.x + flip * -a.cos() * r * 0.7, c.y + a.sin() * r * 0.7 + r * 0.15)
                })
                .collect();
            let tip = pts[0];
            p.add(Shape::line(pts, s));
            arrow_head(p, tip, vec2(flip * -0.6, 1.0), r * 0.35, s);
        }
        Icon::Pull => {
            line(p, pos2(c.x, c.y - r * 0.8), pos2(c.x, c.y + r * 0.35), s);
            arrow_head(p, pos2(c.x, c.y + r * 0.35), vec2(0.0, 1.0), r * 0.4, s);
            line(p, pos2(c.x - r * 0.7, c.y + r * 0.8), pos2(c.x + r * 0.7, c.y + r * 0.8), s);
        }
        Icon::Push => {
            line(p, pos2(c.x, c.y + r * 0.45), pos2(c.x, c.y - r * 0.7), s);
            arrow_head(p, pos2(c.x, c.y - r * 0.7), vec2(0.0, -1.0), r * 0.4, s);
            line(p, pos2(c.x - r * 0.7, c.y + r * 0.85), pos2(c.x + r * 0.7, c.y + r * 0.85), s);
        }
        Icon::Branch => {
            let a = pos2(c.x - r * 0.4, c.y - r * 0.6);
            let b = pos2(c.x - r * 0.4, c.y + r * 0.65);
            let d = pos2(c.x + r * 0.45, c.y - r * 0.35);
            p.circle_stroke(a, r * 0.22, s);
            p.circle_stroke(b, r * 0.22, s);
            p.circle_stroke(d, r * 0.22, s);
            line(p, a + vec2(0.0, r * 0.22), b - vec2(0.0, r * 0.22), s);
            let pts = vec![d + vec2(0.0, r * 0.22), pos2(d.x, c.y + r * 0.1), pos2(b.x + r * 0.15, c.y + r * 0.35)];
            p.add(Shape::line(pts, s));
        }
        Icon::Stash | Icon::Pop => {
            let tray = [
                pos2(c.x - r * 0.8, c.y + r * 0.15),
                pos2(c.x - r * 0.8, c.y + r * 0.8),
                pos2(c.x + r * 0.8, c.y + r * 0.8),
                pos2(c.x + r * 0.8, c.y + r * 0.15),
            ];
            p.add(Shape::line(tray.to_vec(), s));
            if icon == Icon::Stash {
                line(p, pos2(c.x, c.y - r * 0.9), pos2(c.x, c.y + r * 0.35), s);
                arrow_head(p, pos2(c.x, c.y + r * 0.35), vec2(0.0, 1.0), r * 0.35, s);
            } else {
                line(p, pos2(c.x, c.y + r * 0.45), pos2(c.x, c.y - r * 0.85), s);
                arrow_head(p, pos2(c.x, c.y - r * 0.85), vec2(0.0, -1.0), r * 0.35, s);
            }
        }
        Icon::Terminal => {
            p.add(Shape::line(vec![pos2(c.x - r * 0.8, c.y - r * 0.5), pos2(c.x - r * 0.2, c.y), pos2(c.x - r * 0.8, c.y + r * 0.5)], s));
            line(p, pos2(c.x, c.y + r * 0.6), pos2(c.x + r * 0.85, c.y + r * 0.6), s);
        }
        Icon::Search => {
            p.circle_stroke(pos2(c.x - r * 0.15, c.y - r * 0.15), r * 0.5, s);
            line(p, pos2(c.x + r * 0.22, c.y + r * 0.22), pos2(c.x + r * 0.8, c.y + r * 0.8), s);
        }
        Icon::Folder => {
            let pts = vec![
                pos2(c.x - r * 0.85, c.y + r * 0.65),
                pos2(c.x - r * 0.85, c.y - r * 0.6),
                pos2(c.x - r * 0.3, c.y - r * 0.6),
                pos2(c.x - r * 0.1, c.y - r * 0.4),
                pos2(c.x + r * 0.85, c.y - r * 0.4),
                pos2(c.x + r * 0.85, c.y + r * 0.65),
                pos2(c.x - r * 0.85, c.y + r * 0.65),
            ];
            p.add(Shape::line(pts, s));
        }
        Icon::Plus => {
            line(p, pos2(c.x - r * 0.65, c.y), pos2(c.x + r * 0.65, c.y), s);
            line(p, pos2(c.x, c.y - r * 0.65), pos2(c.x, c.y + r * 0.65), s);
        }
        Icon::Close => {
            line(p, pos2(c.x - r * 0.5, c.y - r * 0.5), pos2(c.x + r * 0.5, c.y + r * 0.5), s);
            line(p, pos2(c.x + r * 0.5, c.y - r * 0.5), pos2(c.x - r * 0.5, c.y + r * 0.5), s);
        }
        Icon::ChevronRight => {
            p.add(Shape::line(vec![pos2(c.x - r * 0.2, c.y - r * 0.45), pos2(c.x + r * 0.25, c.y), pos2(c.x - r * 0.2, c.y + r * 0.45)], s));
        }
        Icon::ChevronDown => {
            p.add(Shape::line(vec![pos2(c.x - r * 0.45, c.y - r * 0.2), pos2(c.x, c.y + r * 0.25), pos2(c.x + r * 0.45, c.y - r * 0.2)], s));
        }
        Icon::Minimize => {
            let s1 = Stroke::new(1.2, color);
            line(p, pos2(c.x - r * 0.6, c.y), pos2(c.x + r * 0.6, c.y), s1);
        }
        Icon::Maximize => {
            let s1 = Stroke::new(1.2, color);
            p.rect_stroke(Rect::from_center_size(c, vec2(r * 1.2, r * 1.2)), 0.0, s1, egui::StrokeKind::Middle);
        }
        Icon::Restore => {
            // Duas janelas sobrepostas: a de trás só mostra as bordas de cima/direita.
            let s1 = Stroke::new(1.2, color);
            let front = Rect::from_center_size(c + vec2(-r * 0.15, r * 0.15), vec2(r * 0.95, r * 0.95));
            p.rect_stroke(front, 0.0, s1, egui::StrokeKind::Middle);
            let back = front.translate(vec2(r * 0.3, -r * 0.3));
            p.add(Shape::line(vec![pos2(back.left(), front.top()), back.left_top(), back.right_top(), back.right_bottom(), pos2(front.right(), back.bottom())], s1));
        }
        Icon::ArrowLeft => {
            line(p, pos2(c.x + r * 0.7, c.y), pos2(c.x - r * 0.6, c.y), s);
            arrow_head(p, pos2(c.x - r * 0.6, c.y), vec2(-1.0, 0.0), r * 0.4, s);
        }
        Icon::Computer => {
            let screen = Rect::from_center_size(pos2(c.x, c.y - r * 0.15), vec2(r * 1.5, r * 0.95));
            p.rect_stroke(screen, 1.0, s, egui::StrokeKind::Middle);
            line(p, pos2(c.x - r * 0.95, c.y + r * 0.6), pos2(c.x + r * 0.95, c.y + r * 0.6), s);
        }
        Icon::Cloud => {
            p.circle_stroke(pos2(c.x - r * 0.35, c.y + r * 0.1), r * 0.4, s);
            p.circle_stroke(pos2(c.x + r * 0.15, c.y - r * 0.15), r * 0.5, s);
            p.circle_stroke(pos2(c.x + r * 0.55, c.y + r * 0.2), r * 0.32, s);
        }
        Icon::Tag => {
            let pts = vec![
                pos2(c.x - r * 0.85, c.y - r * 0.85),
                pos2(c.x + r * 0.05, c.y - r * 0.85),
                pos2(c.x + r * 0.85, c.y - r * 0.05),
                pos2(c.x - r * 0.05, c.y + r * 0.85),
                pos2(c.x - r * 0.85, c.y + r * 0.05),
                pos2(c.x - r * 0.85, c.y - r * 0.85),
            ];
            p.add(Shape::line(pts, s));
            p.circle_filled(pos2(c.x - r * 0.4, c.y - r * 0.4), r * 0.13, color);
        }
        Icon::Check => {
            p.add(Shape::line(vec![pos2(c.x - r * 0.6, c.y), pos2(c.x - r * 0.15, c.y + r * 0.45), pos2(c.x + r * 0.65, c.y - r * 0.45)], s));
        }
        Icon::Eye | Icon::EyeOff => {
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / 16.0;
                    pos2(c.x + a.cos() * r * 0.85, c.y + a.sin() * r * 0.45)
                })
                .collect();
            p.add(Shape::closed_line(pts, s));
            p.circle_filled(c, r * 0.22, color);
            if icon == Icon::EyeOff {
                line(p, pos2(c.x - r * 0.75, c.y + r * 0.75), pos2(c.x + r * 0.75, c.y - r * 0.75), s);
            }
        }
        Icon::Gear => {
            p.circle_stroke(c, r * 0.35, s);
            for i in 0..8 {
                let a = std::f32::consts::TAU * i as f32 / 8.0;
                let d = vec2(a.cos(), a.sin());
                line(p, c + d * r * 0.55, c + d * r * 0.85, s);
            }
        }
        Icon::Warning => {
            let pts = vec![pos2(c.x, c.y - r * 0.85), pos2(c.x + r * 0.9, c.y + r * 0.75), pos2(c.x - r * 0.9, c.y + r * 0.75)];
            p.add(Shape::closed_line(pts, s));
            line(p, pos2(c.x, c.y - r * 0.3), pos2(c.x, c.y + r * 0.2), s);
            p.circle_filled(pos2(c.x, c.y + r * 0.45), r * 0.08, color);
        }
        Icon::File => {
            let pts = vec![
                pos2(c.x - r * 0.6, c.y - r * 0.85),
                pos2(c.x + r * 0.2, c.y - r * 0.85),
                pos2(c.x + r * 0.6, c.y - r * 0.45),
                pos2(c.x + r * 0.6, c.y + r * 0.85),
                pos2(c.x - r * 0.6, c.y + r * 0.85),
            ];
            p.add(Shape::closed_line(pts, s));
        }
        Icon::List => {
            for dy in [-0.5, 0.0, 0.5] {
                p.circle_filled(pos2(c.x - r * 0.65, c.y + r * dy), r * 0.09, color);
                line(p, pos2(c.x - r * 0.35, c.y + r * dy), pos2(c.x + r * 0.8, c.y + r * dy), s);
            }
        }
        Icon::Tree => {
            line(p, pos2(c.x - r * 0.6, c.y - r * 0.7), pos2(c.x - r * 0.6, c.y + r * 0.5), s);
            line(p, pos2(c.x - r * 0.6, c.y - r * 0.1), pos2(c.x + r * 0.7, c.y - r * 0.1), s);
            line(p, pos2(c.x - r * 0.6, c.y + r * 0.5), pos2(c.x + r * 0.7, c.y + r * 0.5), s);
            line(p, pos2(c.x - r * 0.6, c.y - r * 0.7), pos2(c.x + r * 0.7, c.y - r * 0.7), s);
        }
    }
}

/// Botão quadrado só com ícone.
pub fn button(ui: &mut Ui, icon: Icon, size: f32, tooltip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let color = if resp.hovered() { crate::theme::p().text_bright } else { crate::theme::p().text };
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, crate::theme::p().panel_bg2);
    }
    paint(ui.painter(), rect.shrink(size * 0.22), icon, color);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}
