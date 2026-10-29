//! Grafo de commits com colunas BRANCH/TAG | GRAPH | MESSAGE | AUTHOR | DATE | SHA.

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use git2::Oid;

use crate::git::model::{CommitInfo, RefKind, RefLabel, Snapshot};
use crate::git::ops::ResetMode;
use crate::icons::{self, Icon};
use crate::repo_tab::{Action, Dialog, DragRef, RepoTab, Selection};
use crate::settings::Settings;
use crate::theme;

const HEADER_H: f32 = 26.0;
const SHA_W: f32 = 72.0;

struct Cols {
    refs: Option<(f32, f32)>,
    graph: Option<(f32, f32)>,
    message: (f32, f32),
    author: Option<(f32, f32)>,
    date: Option<(f32, f32)>,
    sha: Option<(f32, f32)>,
}

fn columns(left: f32, width: f32, max_cols: u16, s: &Settings) -> Cols {
    let c = &s.columns;
    let mut x = left;
    let mut take = |w: f32, on: bool| {
        on.then(|| {
            let r = (x, x + w);
            x += w;
            r
        })
    };
    let refs = take(c.refs_width, c.refs);
    let graph_w = (max_cols as f32 * theme::LANE_WIDTH + 18.0).clamp(48.0, 380.0);
    let graph = take(graph_w, c.graph);
    // Colunas opcionais à direita só entram se a mensagem mantiver ≥ 200px;
    // a ordem de descarte (autor → data → SHA) preserva a mais útil.
    const MIN_MESSAGE: f32 = 200.0;
    let mut budget = (left + width - x - MIN_MESSAGE).max(0.0);
    let mut fits = |w: f32, on: bool| {
        let ok = on && budget >= w;
        if ok {
            budget -= w;
        }
        ok
    };
    let show_sha = fits(SHA_W, c.sha);
    let show_date = fits(c.date_width, c.date);
    let show_author = fits(c.author_width, c.author);

    let mut right = left + width;
    let mut take_right = |w: f32, on: bool| {
        on.then(|| {
            let r = (right - w, right);
            right -= w;
            r
        })
    };
    let sha = take_right(SHA_W, show_sha);
    let date = take_right(c.date_width, show_date);
    let author = take_right(c.author_width, show_author);
    Cols { refs, graph, message: (x, right.max(x + 60.0)), author, date, sha }
}

pub fn show(ui: &mut Ui, tab: &mut RepoTab, settings: &mut Settings, actions: &mut Vec<Action>) {
    let Some(snap) = tab.snap.clone() else {
        ui.centered_and_justified(|ui| match &tab.load_error {
            Some(e) => ui.colored_label(theme::p().danger, e),
            None => ui.spinner(),
        });
        return;
    };

    drop_menu(ui, tab, &snap, actions);
    let menu_open = tab.drop_menu.is_some();
    let hosting_kind = tab.hosting.as_ref().map(|h| h.repo.kind);
    let full = ui.available_rect_before_wrap();
    let cols = columns(full.left(), full.width(), snap.graph.max_cols, settings);
    header(ui, full, &cols, settings);

    let has_wip = !snap.status.is_empty() || snap.state != crate::git::model::OpState::Clean;
    let offset = usize::from(has_wip);
    let total = snap.commits.len() + offset;
    let h = theme::ROW_HEIGHT;

    let mut scroll = egui::ScrollArea::vertical().id_salt(("graph", &tab.path)).auto_shrink([false, false]);
    if let Some(row) = tab.scroll_to_row.take() {
        let viewport = ui.available_height();
        scroll = scroll.vertical_scroll_offset(((row + offset) as f32 * h - viewport / 2.0).max(0.0));
    }

    let head_row = snap.head.oid.and_then(|o| snap.index_of.get(&o).copied());
    let search: Vec<usize> = tab.search.as_ref().map(|s| s.matches.clone()).unwrap_or_default();
    let selected: Vec<Oid> = match &tab.selection {
        Selection::Commits(v) => v.clone(),
        Selection::Wip => Vec::new(),
    };

    scroll.show_rows(ui, h, total, |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let base_y = ui.cursor().top() - range.start as f32 * h;
        let y_of = |r: usize| base_y + r as f32 * h + h / 2.0;
        let lane_x = |c: u16| cols.graph.map(|g| g.0 + 14.0 + c as f32 * theme::LANE_WIDTH).unwrap_or(-100.0);

        let mut node_rows = Vec::new();
        for r in range.clone() {
            let (rect, resp) = ui.allocate_exact_size(vec2(full.width(), h), Sense::click());
            if has_wip && r == 0 {
                wip_row(ui, rect, &resp, &snap, tab, &cols, actions);
                continue;
            }
            let i = r - offset;
            let commit = &snap.commits[i];
            let is_sel = selected.contains(&commit.id);
            let p = ui.painter();
            if is_sel {
                p.rect_filled(rect, 0.0, theme::p().selected_row);
            } else if search.binary_search(&i).is_ok() {
                p.rect_filled(rect, 0.0, Color32::from_rgba_premultiplied(40, 36, 10, 60));
            } else if resp.hovered() {
                p.rect_filled(rect, 0.0, theme::p().hover_row);
            }
            let row = &snap.graph.rows[i];
            let color = theme::lane(row.color);

            if let Some((x0, x1)) = cols.refs {
                refs_cell(ui, Rect::from_x_y_ranges(x0..=x1, rect.y_range()), &snap, commit, row.color, lane_x(row.col), menu_open, hosting_kind, actions);
            }
            message_cell(ui, Rect::from_x_y_ranges(cols.message.0..=cols.message.1, rect.y_range()), commit, color, is_sel, settings);
            if let Some((x0, x1)) = cols.author {
                let r = Rect::from_x_y_ranges(x0..=x1, rect.y_range());
                crate::ui::avatar(ui, pos2(r.left() + 14.0, r.center().y), 9.0, &commit.initials(), &commit.email, crate::ui::author_color(&commit.author));
                crate::ui::avatar_tooltip(ui, pos2(r.left() + 14.0, r.center().y), 9.0, &commit.author, &commit.email);
                let name = crate::ui::elide(ui, &commit.author, &theme::prop(12.0), r.width() - 32.0);
                ui.painter().text(pos2(r.left() + 28.0, r.center().y), Align2::LEFT_CENTER, name, theme::prop(12.0), theme::p().text);
            }
            if let Some((x0, _)) = cols.date {
                let txt = crate::ui::format_time(commit.time, &settings.date_format);
                ui.painter().text(pos2(x0 + 6.0, rect.center().y), Align2::LEFT_CENTER, txt, theme::prop(12.0), theme::p().text_secondary);
            }
            if let Some((x0, _)) = cols.sha {
                ui.painter().text(pos2(x0 + 6.0, rect.center().y), Align2::LEFT_CENTER, &commit.short, theme::mono(12.0), theme::p().text_secondary);
            }

            if resp.clicked() {
                let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                actions.push(Action::SelectCommit { oid: commit.id, add });
            }
            if resp.double_clicked() && commit.stash_index.is_none() {
                actions.push(Action::OpenDialog(Dialog::Confirm {
                    title: "Checkout".into(),
                    body: format!("Fazer checkout do commit {} (HEAD destacado)?", commit.short),
                    confirm: "Checkout".into(),
                    danger: false,
                    action: Box::new(Action::CheckoutCommit(commit.id)),
                }));
            }
            resp.context_menu(|ui| commit_menu(ui, tab, &snap, commit, &selected, actions));
            node_rows.push((r, i));
        }

        // Arestas e nós por cima das células, recortados na coluna do grafo.
        let Some((gx0, gx1)) = cols.graph else { return };
        let clip = Rect::from_x_y_ranges(gx0..=gx1, ui.clip_rect().y_range());
        let painter = ui.painter().with_clip_rect(clip);
        let edge_rows = range.start.max(offset)..(range.end + 1).min(total);
        for r in edge_rows {
            let i = r - offset;
            if i == 0 {
                continue;
            }
            for e in &snap.graph.rows[i].edges_up {
                draw_edge(&painter, pos2(lane_x(e.from), y_of(r - 1)), pos2(lane_x(e.to), y_of(r)), theme::lane(e.color));
            }
        }
        if has_wip {
            let col = head_row.map(|hr| snap.graph.rows[hr].col).unwrap_or(0);
            let x = lane_x(col);
            let end = head_row.map(|hr| y_of(hr + offset)).unwrap_or(y_of(0));
            dashed(&painter, pos2(x, y_of(0) + 9.0), pos2(x, end - 10.0), theme::p().text_secondary);
            painter.circle_filled(pos2(x, y_of(0)), 9.0, theme::p().app_bg);
            dashed_circle(&painter, pos2(x, y_of(0)), 9.0, theme::p().text);
        }
        for (r, i) in node_rows {
            let commit = &snap.commits[i];
            let row = &snap.graph.rows[i];
            let center = pos2(lane_x(row.col), y_of(r));
            let color = theme::lane(row.color);
            if commit.stash_index.is_some() {
                let rect = Rect::from_center_size(center, vec2(16.0, 16.0));
                painter.rect_filled(rect, 3.0, theme::p().app_bg);
                painter.rect_stroke(rect, 3.0, Stroke::new(1.5, color), egui::StrokeKind::Middle);
                icons::paint(&painter, rect.shrink(3.0), Icon::Stash, color);
            } else if commit.is_merge() {
                painter.circle_filled(center, 5.0, color);
            } else {
                crate::ui::avatar(ui, center, 10.0, &commit.initials(), &commit.email, color);
                crate::ui::avatar_tooltip(ui, center, 10.0, &commit.author, &commit.email);
            }
            if snap.head.oid == Some(commit.id) {
                painter.circle_stroke(center, 12.5, Stroke::new(1.5, theme::p().text_bright));
            }
        }
    });
}

/// Opções ao soltar `source` sobre `target`, no espírito do cliente de referência.
pub fn drop_options(snap: &Snapshot, repo: Option<&git2::Repository>, source: &DragRef, target: &DragRef) -> Vec<(String, Action)> {
    let cur = if snap.head.detached { None } else { snap.head.branch.clone() };
    let (s, t) = (source.name.clone(), target.name.clone());
    let mut out = Vec::new();
    match (source.kind, target.kind) {
        (_, RefKind::Local) => {
            let merge = if cur.as_deref() == Some(t.as_str()) { Action::Merge(s.clone()) } else { Action::MergeInto { source: s.clone(), target: t.clone() } };
            out.push((format!("Merge {s} into {t}"), merge));
            let ff = repo.is_some_and(|r| r.graph_descendant_of(source.oid, target.oid).unwrap_or(false));
            if ff {
                out.push((format!("Fast-forward {t} to {s}"), Action::FastForward { branch: t.clone(), to: source.oid }));
            }
            let rebase_t = if cur.as_deref() == Some(t.as_str()) { Action::Rebase(s.clone()) } else { Action::RebaseBranch { branch: t.clone(), onto: s.clone() } };
            out.push((format!("Rebase {t} onto {s}"), rebase_t));
            if source.kind == RefKind::Local {
                let rebase_s = if cur.as_deref() == Some(s.as_str()) { Action::Rebase(t.clone()) } else { Action::RebaseBranch { branch: s.clone(), onto: t.clone() } };
                out.push((format!("Rebase {s} onto {t}"), rebase_s));
                if cur.as_deref() == Some(s.as_str()) {
                    out.push((format!("Interactive Rebase {s} onto {t}"), Action::StartInteractiveRebase(target.oid)));
                }
            }
        }
        (RefKind::Local, RefKind::Remote) => {
            let (remote, rb) = t.split_once('/').map(|(r, b)| (r.to_string(), b.to_string())).unwrap_or(("origin".into(), t.clone()));
            out.push((format!("Push {s} to {t}"), Action::PushTo { remote, branch: s.clone(), remote_branch: rb, set_upstream: false, force: false }));
            out.push((format!("Merge {t} into {s}"), Action::MergeInto { source: t.clone(), target: s.clone() }));
            let rebase = if cur.as_deref() == Some(s.as_str()) { Action::Rebase(t.clone()) } else { Action::RebaseBranch { branch: s.clone(), onto: t.clone() } };
            out.push((format!("Rebase {s} onto {t}"), rebase));
        }
        _ => {}
    }
    out
}

fn drop_menu(ui: &mut Ui, tab: &mut RepoTab, snap: &Snapshot, actions: &mut Vec<Action>) {
    let Some(dm) = &tab.drop_menu else { return };
    let repo = tab.repo().ok();
    let options = drop_options(snap, repo.as_ref(), &dm.source, &dm.target);
    let mut close = options.is_empty();
    let area = egui::Area::new(egui::Id::unique("drop_menu")).order(egui::Order::Foreground).fixed_pos(dm.pos + vec2(10.0, 6.0)).show(ui.ctx(), |ui| {
        egui::Frame::menu(ui.style()).show(ui, |ui| {
            ui.set_min_width(240.0);
            for (label, action) in options {
                if ui.button(label).clicked() {
                    actions.push(action);
                    close = true;
                }
            }
            ui.separator();
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    let clicked_outside = ui.input(|i| i.pointer.any_pressed()) && !area.response.contains_pointer();
    if close || clicked_outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        tab.drop_menu = None;
    }
}

fn header(ui: &mut Ui, full: Rect, cols: &Cols, settings: &mut Settings) {
    let (rect, resp) = ui.allocate_exact_size(vec2(full.width(), HEADER_H), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, theme::p().panel_bg0);
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, theme::p().panel_bg2));
    let label = |x: f32, text: &str| {
        p.text(pos2(x + 8.0, rect.center().y), Align2::LEFT_CENTER, text, theme::prop(10.5), theme::p().text_secondary);
    };
    let sep = |x: f32| p.line_segment([pos2(x, rect.top() + 5.0), pos2(x, rect.bottom() - 5.0)], Stroke::new(1.0, theme::p().panel_bg2));
    if let Some((x0, x1)) = cols.refs {
        label(x0, "BRANCH / TAG");
        sep(x1);
    }
    if let Some((x0, x1)) = cols.graph {
        label(x0, "GRAPH");
        sep(x1);
    }
    label(cols.message.0, "COMMIT MESSAGE");
    if let Some((x0, _)) = cols.author {
        sep(x0);
        label(x0, "AUTHOR");
    }
    if let Some((x0, _)) = cols.date {
        sep(x0);
        label(x0, "COMMIT DATE / TIME");
    }
    if let Some((x0, _)) = cols.sha {
        sep(x0);
        label(x0, "SHA");
    }

    // Arrastar a borda da coluna BRANCH/TAG redimensiona.
    if let Some((_, x1)) = cols.refs {
        let handle = Rect::from_x_y_ranges(x1 - 3.0..=x1 + 3.0, rect.y_range());
        let r = ui.interact(handle, ui.scope_id().with("refs_resize"), Sense::drag());
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if r.dragged() {
            settings.columns.refs_width = (settings.columns.refs_width + r.drag_delta().x).clamp(80.0, 400.0);
        }
    }

    resp.context_menu(|ui| {
        ui.label(egui::RichText::new("Colunas").strong());
        let c = &mut settings.columns;
        ui.checkbox(&mut c.refs, "Branch / Tag");
        ui.checkbox(&mut c.graph, "Graph");
        ui.checkbox(&mut c.author, "Author");
        ui.checkbox(&mut c.date, "Commit Date / Time");
        ui.checkbox(&mut c.sha, "SHA");
        ui.separator();
        ui.checkbox(&mut settings.show_body_in_graph, "Mostrar descrição do commit");
        ui.checkbox(&mut settings.show_stashes, "Mostrar stashes no grafo");
        if ui.button("Resetar colunas para o layout padrão").clicked() {
            settings.columns = Default::default();
            ui.close();
        }
    });
}

fn draw_edge(p: &egui::Painter, a: Pos2, b: Pos2, color: Color32) {
    let stroke = Stroke::new(2.0, color);
    if (a.x - b.x).abs() < 0.5 {
        p.line_segment([a, b], stroke);
    } else {
        let mid = (a.y + b.y) / 2.0;
        p.add(Shape::CubicBezier(egui::epaint::CubicBezierShape::from_points_stroke(
            [a, pos2(a.x, mid), pos2(b.x, mid), b],
            false,
            Color32::TRANSPARENT,
            stroke,
        )));
    }
}

fn dashed(p: &egui::Painter, a: Pos2, b: Pos2, color: Color32) {
    if b.y <= a.y {
        return;
    }
    p.add(Shape::dashed_line(&[a, b], Stroke::new(1.5, color), 3.0, 3.0));
}

fn dashed_circle(p: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let n = 16;
    for i in (0..n).step_by(2) {
        let a0 = std::f32::consts::TAU * i as f32 / n as f32;
        let a1 = std::f32::consts::TAU * (i + 1) as f32 / n as f32;
        p.line_segment([c + vec2(a0.cos(), a0.sin()) * r, c + vec2(a1.cos(), a1.sin()) * r], Stroke::new(1.5, color));
    }
}

fn wip_row(ui: &mut Ui, rect: Rect, resp: &egui::Response, snap: &Snapshot, tab: &RepoTab, cols: &Cols, actions: &mut Vec<Action>) {
    let p = ui.painter();
    let selected = tab.selection == Selection::Wip;
    if selected {
        p.rect_filled(rect, 0.0, theme::p().selected_row);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, theme::p().hover_row);
    }
    let x = cols.message.0 + 6.0;
    let summary = if tab.wip_summary.trim().is_empty() { "// WIP".to_string() } else { tab.wip_summary.clone() };
    let box_rect = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(150.0f32.min(cols.message.1 - x - 80.0).max(60.0), rect.height() - 8.0));
    p.rect(box_rect, 3.0, theme::p().input_bg, Stroke::new(1.0, theme::p().panel_bg2), egui::StrokeKind::Inside);
    let txt = crate::ui::elide(ui, &summary, &theme::prop(12.5), box_rect.width() - 12.0);
    ui.painter().text(box_rect.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, txt, theme::prop(12.5), theme::p().text_secondary);

    let st = &snap.status;
    let mut cx = box_rect.right() + 12.0;
    let mut stat = |n: usize, label: &str, color: Color32| {
        if n > 0 {
            let g = ui.painter().text(pos2(cx, rect.center().y), Align2::LEFT_CENTER, format!("{label}{n}"), theme::prop(12.5), color);
            cx = g.right() + 10.0;
        }
    };
    let added = st.unstaged.iter().chain(&st.staged).filter(|f| matches!(f.kind, crate::git::model::ChangeKind::Added | crate::git::model::ChangeKind::Untracked)).count();
    let deleted = st.unstaged.iter().chain(&st.staged).filter(|f| f.kind == crate::git::model::ChangeKind::Deleted).count();
    let modified = st.unstaged.len() + st.staged.len() - added - deleted;
    stat(modified, "✏ ", theme::p().warning);
    stat(added, "+ ", theme::p().success);
    stat(deleted, "− ", theme::p().danger);
    stat(st.conflicted.len(), "⚠ ", theme::p().danger);
    if snap.state != crate::git::model::OpState::Clean {
        ui.painter().text(pos2(cx, rect.center().y), Align2::LEFT_CENTER, format!("{} em andamento", snap.state.label()), theme::prop(12.0), theme::p().warning);
    }
    if resp.clicked() {
        actions.push(Action::SelectWip);
    }
    resp.context_menu(|ui| {
        if ui.button("Stash all changes").clicked() {
            actions.push(Action::OpenDialog(Dialog::Stash { message: String::new(), include_untracked: true }));
            ui.close();
        }
        if ui.button("Discard all changes").clicked() {
            actions.push(Action::OpenDialog(Dialog::Confirm {
                title: "Descartar todas as alterações".into(),
                body: "Isto descarta todas as mudanças (staged, unstaged e arquivos não rastreados). Continuar?".into(),
                confirm: "Descartar tudo".into(),
                danger: true,
                action: Box::new(Action::DiscardAll),
            }));
            ui.close();
        }
    });
}

/// Agrupa `main` local + `origin/main` remoto numa só label, como o cliente de referência.
fn grouped_refs(refs: &[RefLabel]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for r in refs.iter().filter(|r| r.kind == RefKind::Local) {
        out.push((r.name.clone(), true, false, r.is_head, RefKind::Local));
    }
    for r in refs.iter().filter(|r| r.kind == RefKind::Remote) {
        let short = r.name.split_once('/').map(|(_, b)| b).unwrap_or(&r.name).to_string();
        if let Some(g) = out.iter_mut().find(|g| g.0 == short && g.4 == RefKind::Local) {
            g.2 = true;
        } else {
            out.push((r.name.clone(), false, true, false, RefKind::Remote));
        }
    }
    for r in refs.iter().filter(|r| r.kind == RefKind::Tag) {
        out.push((r.name.clone(), false, false, false, RefKind::Tag));
    }
    out.sort_by_key(|g| (!g.3, g.4 != RefKind::Local));
    out
}

#[allow(clippy::too_many_arguments)]
fn refs_cell(ui: &mut Ui, cell: Rect, snap: &Snapshot, commit: &CommitInfo, color: u8, node_x: f32, menu_open: bool, hosting: Option<crate::hosting::Kind>, actions: &mut Vec<Action>) {
    let refs = if let Some(si) = commit.stash_index {
        let p = ui.painter();
        p.text(cell.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, format!("stash@{{{si}}}"), theme::prop(11.5), theme::p().text_secondary);
        return;
    } else {
        match snap.refs_by_commit.get(&commit.id) {
            Some(r) if !r.is_empty() => r,
            _ => return,
        }
    };
    let groups = grouped_refs(refs);
    let Some(first) = groups.first() else { return };
    let lane_color = theme::lane(color);
    let pill = Rect::from_min_max(pos2(cell.left() + 6.0, cell.top() + 4.0), pos2(cell.right() - 6.0, cell.bottom() - 4.0));

    // Linha horizontal ligando a label ao nó (cor da lane).
    ui.painter().line_segment([pos2(pill.right(), cell.center().y), pos2(node_x, cell.center().y)], Stroke::new(1.0, lane_color.gamma_multiply(0.7)));

    let resp = ui.interact(pill, ui.scope_id().with(("ref", commit.id)), Sense::click_and_drag());

    // Arrastar um ref sobre outro abre o menu de merge/rebase/fast-forward.
    let me = DragRef { name: first.0.clone(), kind: first.4, oid: commit.id };
    if me.kind != RefKind::Tag {
        resp.dnd_set_drag_payload(me.clone());
    }
    let hovering = resp.dnd_hover_payload::<DragRef>().is_some_and(|src| *src != me);
    if let Some(src) = resp.dnd_release_payload::<DragRef>()
        && *src != me
        && let Some(pos) = ui.ctx().pointer_interact_pos()
    {
        actions.push(Action::ShowDropMenu { source: (*src).clone(), target: me.clone(), pos });
    }
    if resp.dragged() {
        // Fantasma da label seguindo o cursor.
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let ghost = Rect::from_min_size(pos + vec2(10.0, -10.0), pill.size());
            let layer = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag_ghost")));
            layer.rect_filled(ghost, 3.0, theme::lane_bg(color).gamma_multiply(0.9));
            layer.text(ghost.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, &first.0, theme::prop(12.0), theme::p().text_bright);
        }
    }

    let more = groups.len() - 1;
    let style = PillStyle::of(first.3, resp.hovered() || hovering, color);
    let border = if hovering { Stroke::new(2.0, theme::p().lanes[9]) } else { style.border };
    if first.3 {
        // Halo discreto em volta do branch em checkout.
        ui.painter().rect_stroke(pill.expand(2.0), CornerRadius::same(5), Stroke::new(2.0, lane_color.gamma_multiply(0.35)), egui::StrokeKind::Outside);
    }
    ui.painter().rect(pill, CornerRadius::same(3), style.fill, border, egui::StrokeKind::Inside);
    paint_pill_content(ui, pill, first, if more > 0 { Some(more) } else { None }, &style);

    // Vários refs no commit: passar o mouse expande a lista com todos eles.
    let quiet = menu_open || ui.ctx().dragged_id().is_some();
    if groups.len() > 1 && !quiet {
        expanded_refs(ui, pill, &resp, snap, commit, &groups, color, hosting, actions);
    }
    if resp.double_clicked() {
        match first.4 {
            RefKind::Local => actions.push(Action::Checkout(first.0.clone())),
            RefKind::Remote => actions.push(Action::CheckoutRemote(first.0.clone())),
            RefKind::Tag => actions.push(Action::CheckoutCommit(commit.id)),
        }
    }
    if resp.clicked() {
        actions.push(Action::SelectCommit { oid: commit.id, add: false });
    }
    resp.context_menu(|ui| {
        for g in &groups {
            match g.4 {
                RefKind::Local => ref_menu_local(ui, snap, &g.0, commit.id, hosting, actions),
                RefKind::Remote => ref_menu_remote(ui, snap, &g.0, commit.id, actions),
                RefKind::Tag => tag_menu(ui, &g.0, commit.id, actions),
            }
            if groups.len() > 1 {
                ui.separator();
            }
        }
    });
}

type Group = (String, bool, bool, bool, RefKind);

/// Conteúdo de um pill: ✓ (HEAD), ícone de tag, nome, 💻/☁ e o contador "+N".
/// Destaque do pill: o ref em checkout é sólido e em negrito; os demais ficam
/// apagados até o ponteiro passar por cima.
struct PillStyle {
    fill: Color32,
    border: Stroke,
    text: Color32,
    icon: Color32,
    font: egui::FontId,
}

impl PillStyle {
    fn of(checked_out: bool, hovered: bool, color: u8) -> Self {
        let lane = theme::lane(color);
        let pal = theme::p();
        if checked_out {
            // Cor sólida da lane; o texto escolhe preto/branco pelo contraste.
            let lum = 0.299 * lane.r() as f32 + 0.587 * lane.g() as f32 + 0.114 * lane.b() as f32;
            let ink = if lum > 150.0 { Color32::from_rgb(20, 20, 24) } else { Color32::WHITE };
            Self { fill: lane, border: Stroke::new(1.5, ink.gamma_multiply(0.6)), text: ink, icon: ink, font: egui::FontId::new(12.5, egui::FontFamily::Proportional) }
        } else if hovered {
            Self { fill: theme::lane_bg(color), border: Stroke::new(1.0, lane.gamma_multiply(0.8)), text: pal.text_bright, icon: pal.text, font: theme::prop(12.0) }
        } else {
            let dim = lerp_color(theme::lane_bg(color), pal.app_bg, 0.45);
            Self { fill: dim, border: Stroke::NONE, text: pal.text_secondary, icon: pal.text_disabled, font: theme::prop(12.0) }
        }
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn paint_pill_content(ui: &Ui, pill: Rect, g: &Group, more: Option<usize>, style: &PillStyle) {
    let p = ui.painter();
    let mut x = pill.left() + 6.0;
    if g.3 {
        icons::paint(p, Rect::from_center_size(pos2(x + 6.0, pill.center().y), vec2(12.0, 12.0)), Icon::Check, style.icon);
        x += 16.0;
    }
    if g.4 == RefKind::Tag {
        icons::paint(p, Rect::from_center_size(pos2(x + 6.0, pill.center().y), vec2(11.0, 11.0)), Icon::Tag, style.icon);
        x += 16.0;
    }
    let icon_w = 16.0 * (g.1 as u8 + g.2 as u8) as f32;
    let more_w = if more.is_some() { 22.0 } else { 0.0 };
    let avail = pill.right() - x - icon_w - more_w - 6.0;
    let name = crate::ui::elide(ui, &g.0, &style.font, avail);
    let gal = p.text(pos2(x, pill.center().y), Align2::LEFT_CENTER, name, style.font.clone(), style.text);
    if g.3 {
        // Negrito simulado: a fonte da UI tem um só peso.
        p.text(pos2(x + 0.6, pill.center().y), Align2::LEFT_CENTER, crate::ui::elide(ui, &g.0, &style.font, avail), style.font.clone(), style.text);
    }
    let mut ix = gal.right() + 4.0;
    if g.1 {
        icons::paint(p, Rect::from_center_size(pos2(ix + 7.0, pill.center().y), vec2(13.0, 13.0)), Icon::Computer, style.icon);
        ix += 16.0;
    }
    if g.2 {
        icons::paint(p, Rect::from_center_size(pos2(ix + 7.0, pill.center().y), vec2(13.0, 13.0)), Icon::Cloud, style.icon);
    }
    if let Some(n) = more {
        p.text(pos2(pill.right() - 5.0, pill.center().y), Align2::RIGHT_CENTER, format!("+{n}"), theme::prop(11.0), style.icon);
    }
}

fn group_width(ui: &Ui, g: &Group) -> f32 {
    let text = ui.ctx().fonts_mut(|f| f.layout_no_wrap(g.0.clone(), theme::prop(12.0), theme::p().text).size().x);
    let icons = 16.0 * (g.1 as u8 + g.2 as u8 + g.3 as u8 + (g.4 == RefKind::Tag) as u8) as f32;
    text + icons + 20.0
}

/// Lista expandida (sobre o grafo) com todos os refs do commit, cada um
/// interativo: clique seleciona, duplo clique faz checkout, botão direito abre
/// o menu daquele ref e dá para arrastar para outro ref.
#[allow(clippy::too_many_arguments)]
fn expanded_refs(ui: &Ui, pill: Rect, pill_resp: &egui::Response, snap: &Snapshot, commit: &CommitInfo, groups: &[Group], color: u8, hosting: Option<crate::hosting::Kind>, actions: &mut Vec<Action>) {
    let ctx = ui.ctx().clone();
    let id = egui::Id::unique(("ref_expand", commit.id));
    let was_open = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    // Mantém aberto enquanto um menu de contexto de um item estiver aberto.
    let menu_of_item = was_open && egui::Popup::is_any_open(&ctx);
    if !(pill_resp.hovered() || was_open || menu_of_item) {
        return;
    }
    let width = groups.iter().map(|g| group_width(ui, g)).fold(pill.width(), f32::max);
    let row_h = pill.height() + 4.0;
    let area = egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(pill.left_top() - vec2(4.0, 4.0)).show(&ctx, |ui| {
        egui::Frame::new()
            .fill(theme::p().panel_bg1)
            .stroke(Stroke::new(1.0, theme::p().panel_bg2))
            .corner_radius(4)
            .inner_margin(4)
            .shadow(egui::epaint::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(90) })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (i, g) in groups.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, row_h - 4.0), Sense::click_and_drag());
                    let style = PillStyle::of(g.3, resp.hovered(), color);
                    ui.painter().rect(rect, CornerRadius::same(3), style.fill, style.border, egui::StrokeKind::Inside);
                    paint_pill_content(ui, rect, g, None, &style);
                    let me = DragRef { name: g.0.clone(), kind: g.4, oid: commit.id };
                    if me.kind != RefKind::Tag {
                        resp.dnd_set_drag_payload(me);
                    }
                    if resp.double_clicked() {
                        actions.push(match g.4 {
                            RefKind::Local => Action::Checkout(g.0.clone()),
                            RefKind::Remote => Action::CheckoutRemote(g.0.clone()),
                            RefKind::Tag => Action::CheckoutCommit(commit.id),
                        });
                    } else if resp.clicked() {
                        actions.push(Action::SelectCommit { oid: commit.id, add: false });
                    }
                    let kind = match g.4 {
                        RefKind::Local if g.2 => "branch local + remoto",
                        RefKind::Local => "branch local",
                        RefKind::Remote => "branch remoto",
                        RefKind::Tag => "tag",
                    };
                    resp.on_hover_text(format!("{} — {kind}", g.0)).context_menu(|ui| match g.4 {
                        RefKind::Local => ref_menu_local(ui, snap, &g.0, commit.id, hosting, actions),
                        RefKind::Remote => ref_menu_remote(ui, snap, &g.0, commit.id, actions),
                        RefKind::Tag => tag_menu(ui, &g.0, commit.id, actions),
                    });
                    let _ = i;
                }
            });
    });
    // Fecha quando o ponteiro sai do pill e da lista (com uma pequena folga).
    let inside = ctx.pointer_hover_pos().is_some_and(|p| area.response.rect.expand(6.0).contains(p) || pill.contains(p));
    // Calculado fora do `data_mut`: consultar a memória do egui com o lock de
    // escrita aberto causa deadlock.
    let keep = inside || (was_open && egui::Popup::is_any_open(&ctx));
    ctx.data_mut(|d| d.insert_temp(id, keep));
}

fn message_cell(ui: &mut Ui, cell: Rect, commit: &CommitInfo, color: Color32, selected: bool, settings: &Settings) {
    let p = ui.painter();
    p.rect_filled(Rect::from_min_size(pos2(cell.left(), cell.top() + 2.0), vec2(3.0, cell.height() - 4.0)), 0.0, color);
    let x = cell.left() + 10.0;
    let width = cell.width() - 16.0;
    let font = theme::prop(13.0);
    let summary = crate::ui::elide(ui, &commit.summary, &font, width);
    let text_color = if selected { theme::p().text_bright } else { theme::p().text };
    let g = ui.painter().text(pos2(x, cell.center().y), Align2::LEFT_CENTER, &summary, font.clone(), text_color);
    if settings.show_body_in_graph && !commit.body.is_empty() && summary == commit.summary {
        let rest = width - g.width() - 10.0;
        if rest > 40.0 {
            let body = commit.body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            let body = crate::ui::elide(ui, body, &theme::prop(12.5), rest);
            ui.painter().text(pos2(g.right() + 8.0, cell.center().y), Align2::LEFT_CENTER, body, theme::prop(12.5), theme::p().text_disabled);
        }
    }
}

fn menu_item(ui: &mut Ui, label: impl Into<String>, actions: &mut Vec<Action>, action: Action) {
    if ui.button(label.into()).clicked() {
        actions.push(action);
        ui.close();
    }
}

fn confirm(title: &str, body: String, confirm: &str, danger: bool, action: Action) -> Action {
    Action::OpenDialog(Dialog::Confirm { title: title.into(), body, confirm: confirm.into(), danger, action: Box::new(action) })
}

pub fn commit_menu(ui: &mut Ui, tab: &RepoTab, snap: &Snapshot, commit: &CommitInfo, selected: &[Oid], actions: &mut Vec<Action>) {
    if let Some(si) = commit.stash_index {
        menu_item(ui, "Apply Stash", actions, Action::StashApply(si));
        menu_item(ui, "Pop Stash", actions, Action::StashPop(si));
        menu_item(ui, "Delete Stash", actions, confirm("Delete stash", format!("Apagar stash@{{{si}}}?"), "Delete", true, Action::StashDrop(si)));
        return;
    }
    let cur = snap.head.branch.clone().unwrap_or_else(|| "HEAD".into());
    let is_head = snap.head.oid == Some(commit.id);
    if selected.len() > 1 && selected.contains(&commit.id) {
        menu_item(ui, format!("Cherry pick {} commits", selected.len()), actions, Action::CherryPick(selected.to_vec()));
        ui.separator();
    }
    menu_item(ui, "Checkout this commit", actions, Action::CheckoutCommit(commit.id));
    menu_item(ui, "Create branch here", actions, Action::OpenDialog(Dialog::CreateBranch { at: commit.id, name: String::new(), checkout: true }));
    if !is_head {
        menu_item(ui, "Cherry pick commit", actions, Action::CherryPick(vec![commit.id]));
        ui.menu_button(format!("Reset {cur} to this commit"), |ui| {
            menu_item(ui, "Soft - keep all changes", actions, Action::Reset { to: commit.id, mode: ResetMode::Soft });
            menu_item(ui, "Mixed - keep working copy but reset index", actions, Action::Reset { to: commit.id, mode: ResetMode::Mixed });
            menu_item(
                ui,
                "Hard - discard all changes",
                actions,
                confirm("Reset hard", format!("Resetar {cur} para {} descartando todas as mudanças?", commit.short), "Reset", true, Action::Reset { to: commit.id, mode: ResetMode::Hard }),
            );
        });
        menu_item(ui, format!("Rebase {cur} onto this commit"), actions, Action::Rebase(commit.id.to_string()));
        menu_item(ui, format!("Interactive Rebase {cur} onto this commit"), actions, Action::StartInteractiveRebase(commit.id));
    }
    menu_item(ui, "Revert commit", actions, confirm("Revert", format!("Criar um commit revertendo {}?", commit.short), "Revert", false, Action::Revert(commit.id)));
    if is_head {
        let msg = if commit.body.is_empty() { commit.summary.clone() } else { format!("{}\n\n{}", commit.summary, commit.body) };
        menu_item(ui, "Reword commit", actions, Action::OpenDialog(Dialog::Reword { message: msg }));
    }
    ui.separator();
    menu_item(ui, "Create tag here", actions, Action::OpenDialog(Dialog::CreateTag { at: commit.id, name: String::new(), message: String::new(), annotated: false }));
    menu_item(ui, "Create annotated tag here", actions, Action::OpenDialog(Dialog::CreateTag { at: commit.id, name: String::new(), message: String::new(), annotated: true }));
    ui.separator();
    menu_item(ui, "Copy commit sha", actions, Action::CopyText(commit.id.to_string()));
    if let Some(h) = &tab.hosting {
        let url = h.repo.web_commit(&commit.id.to_string());
        menu_item(ui, format!("Copy link to this commit on {}", h.repo.kind.label()), actions, Action::CopyText(url.clone()));
        menu_item(ui, format!("View commit on {}", h.repo.host), actions, Action::Hosting(crate::repo_hosting::HostingAction::OpenUrl(url)));
    }
    if !tab.undo.is_empty() || !tab.redo.is_empty() {
        // Atalhos de undo/redo também aparecem aqui para descoberta.
        ui.separator();
        menu_item(ui, "Undo", actions, Action::Undo);
    }
}

pub fn ref_menu_local(ui: &mut Ui, snap: &Snapshot, name: &str, oid: Oid, hosting: Option<crate::hosting::Kind>, actions: &mut Vec<Action>) {
    let cur = snap.head.branch.clone().unwrap_or_else(|| "HEAD".into());
    let is_cur = snap.head.branch.as_deref() == Some(name) && !snap.head.detached;
    let branch = snap.locals.iter().find(|b| b.name == name);
    ui.label(egui::RichText::new(name).strong().color(theme::p().text_bright));
    if !is_cur {
        menu_item(ui, format!("Checkout {name}"), actions, Action::Checkout(name.into()));
        menu_item(ui, format!("Merge {name} into {cur}"), actions, Action::Merge(name.into()));
        menu_item(ui, format!("Rebase {cur} onto {name}"), actions, Action::Rebase(name.into()));
        menu_item(ui, format!("Interactive Rebase {cur} onto {name}"), actions, Action::StartInteractiveRebase(oid));
    }
    if let Some((up, target)) = upstream_of(snap, name) {
        let b = branch.expect("upstream_of só acha branches locais");
        if b.behind > 0 && b.ahead == 0 {
            menu_item(ui, format!("Fast-forward {name} to {up}"), actions, Action::FastForward { branch: name.into(), to: target });
        }
        if b.oid != target {
            reset_to_upstream_menu(ui, snap, name, &up, target, actions);
        }
    }
    if is_cur {
        menu_item(ui, "Push", actions, Action::Push { force: false });
        menu_item(ui, "Force push (with lease)", actions, confirm("Force push", format!("Forçar push de {name} (--force-with-lease)?"), "Force push", true, Action::Push { force: true }));
    }
    if let Some(r) = snap.remotes.first() {
        let up = format!("{}/{}", r.name, name);
        if branch.is_some_and(|b| b.upstream.as_deref() != Some(up.as_str())) && r.branches.iter().any(|b| b.name == name) {
            menu_item(ui, format!("Set upstream to {up}"), actions, Action::SetUpstream { branch: name.into(), upstream: up });
        }
    }
    if let Some(kind) = hosting {
        let needs_push = branch.is_some_and(|b| b.upstream.is_none() || b.ahead > 0);
        let label = if needs_push { format!("Push {name} and start a {}", kind.pr_noun().to_lowercase()) } else { format!("Start a {} from {name}", kind.pr_noun().to_lowercase()) };
        menu_item(ui, label, actions, Action::StartPr(name.into()));
    }
    menu_item(ui, "Create branch here", actions, Action::OpenDialog(Dialog::CreateBranch { at: oid, name: String::new(), checkout: true }));
    menu_item(ui, format!("Rename {name}"), actions, Action::OpenDialog(Dialog::RenameBranch { old: name.into(), new: name.into() }));
    if !is_cur {
        menu_item(ui, format!("Delete {name}"), actions, confirm("Delete branch", format!("Apagar o branch local {name}?"), "Delete", true, Action::DeleteBranch { name: name.into(), force: true }));
    }
    menu_item(ui, "Copy branch name", actions, Action::CopyText(name.into()));
    ui.separator();
    menu_item(ui, "Hide in the graph", actions, Action::ToggleHidden(name.into()));
    menu_item(ui, "Solo", actions, Action::Solo(name.into()));
}

/// Upstream de um branch local e o commit para onde ele aponta.
pub fn upstream_of(snap: &Snapshot, name: &str) -> Option<(String, Oid)> {
    let up = snap.locals.iter().find(|b| b.name == name)?.upstream.clone()?;
    let oid = snap.remotes.iter().flat_map(|r| r.branches.iter().map(move |b| (format!("{}/{}", r.name, b.name), b.oid))).find(|(n, _)| *n == up)?.1;
    Some((up, oid))
}

/// "Reset <branch> to <remoto>": no branch atual escolhe o modo (soft/mixed/hard);
/// nos outros só move o ponteiro (`git branch -f`), sem tocar na working copy.
pub fn reset_to_upstream_menu(ui: &mut Ui, snap: &Snapshot, name: &str, up: &str, target: Oid, actions: &mut Vec<Action>) {
    let is_cur = snap.head.branch.as_deref() == Some(name) && !snap.head.detached;
    let short = &target.to_string()[..7];
    if !is_cur {
        let body = format!("Mover {name} para {up} ({short})? Os commits locais que não estão em {up} deixam de ser referenciados por {name}.");
        menu_item(ui, format!("Reset {name} to {up}"), actions, confirm("Reset to remote", body, "Reset", true, Action::ResetBranchTo { branch: name.into(), to: target }));
        return;
    }
    ui.menu_button(format!("Reset {name} to {up}"), |ui| {
        menu_item(ui, "Soft - keep all changes", actions, Action::Reset { to: target, mode: ResetMode::Soft });
        menu_item(ui, "Mixed - keep working copy but reset index", actions, Action::Reset { to: target, mode: ResetMode::Mixed });
        let body = format!("Resetar {name} para {up} ({short}) descartando commits locais e todas as mudanças não commitadas?");
        menu_item(ui, "Hard - discard all changes", actions, confirm("Reset hard to remote", body, "Reset", true, Action::Reset { to: target, mode: ResetMode::Hard }));
    });
}

pub fn ref_menu_remote(ui: &mut Ui, snap: &Snapshot, full: &str, oid: Oid, actions: &mut Vec<Action>) {
    let cur = snap.head.branch.clone().unwrap_or_else(|| "HEAD".into());
    let (remote, branch) = full.split_once('/').unwrap_or(("origin", full));
    ui.label(egui::RichText::new(full).strong().color(theme::p().text_bright));
    menu_item(ui, format!("Checkout {full}"), actions, Action::CheckoutRemote(full.into()));
    menu_item(ui, format!("Merge {full} into {cur}"), actions, Action::Merge(full.into()));
    menu_item(ui, format!("Rebase {cur} onto {full}"), actions, Action::Rebase(full.into()));
    menu_item(ui, format!("Fetch {remote}"), actions, Action::FetchRemote(remote.into()));
    menu_item(ui, "Create branch here", actions, Action::OpenDialog(Dialog::CreateBranch { at: oid, name: branch.into(), checkout: true }));
    menu_item(
        ui,
        format!("Delete {branch} from {remote}"),
        actions,
        confirm("Delete remote branch", format!("Apagar {full} no remoto?"), "Delete", true, Action::DeleteRemoteBranch { remote: remote.into(), branch: branch.into() }),
    );
    menu_item(ui, "Copy branch name", actions, Action::CopyText(full.into()));
    ui.separator();
    menu_item(ui, "Hide in the graph", actions, Action::ToggleHidden(full.into()));
    menu_item(ui, "Solo", actions, Action::Solo(full.into()));
}

pub fn tag_menu(ui: &mut Ui, name: &str, oid: Oid, actions: &mut Vec<Action>) {
    ui.label(egui::RichText::new(name).strong().color(theme::p().text_bright));
    menu_item(ui, format!("Checkout {name}"), actions, Action::CheckoutCommit(oid));
    menu_item(ui, "Push tag to remote", actions, Action::PushTag(name.into()));
    menu_item(ui, format!("Delete {name} locally"), actions, confirm("Delete tag", format!("Apagar a tag {name}?"), "Delete", true, Action::DeleteTag(name.into())));
    menu_item(ui, "Delete tag from remote", actions, confirm("Delete remote tag", format!("Apagar a tag {name} do remoto?"), "Delete", true, Action::DeleteRemoteTag(name.into())));
    menu_item(ui, "Copy tag name", actions, Action::CopyText(name.into()));
    menu_item(ui, "Hide in the graph", actions, Action::ToggleHidden(name.into()));
}
