//! Testes ponta-a-ponta: o app inteiro roda no `egui_kittest` contra um
//! repositório git real, recebe cliques de ponteiro e tem o estado do git
//! conferido após cada fluxo. Os PNGs vão para `target/e2e/` para inspeção.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::{BrioApp, Tab};
use crate::repo_tab::{CenterView, RepoTab, Selection};

const W: f32 = 1600.0;
const H: f32 = 900.0;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").current_dir(dir).args(args).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Repositório com histórico ramificado, merge, tag e mudanças pendentes.
fn demo_repo(root: &Path) -> PathBuf {
    let dir = root.join("demo");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "Ana Souza"]);
    git(&dir, &["config", "user.email", "ana@example.com"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    let body: String = (1..=40).map(|i| format!("linha {i}\n")).collect();
    std::fs::write(dir.join("app.txt"), &body).unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "feat: estrutura inicial"]);
    for i in 2..=3 {
        std::fs::write(dir.join(format!("m{i}.txt")), "x\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-qm", &format!("feat: passo {i}")]);
    }
    git(&dir, &["checkout", "-qb", "feat/login"]);
    std::fs::write(dir.join("login.txt"), "login\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "feat: tela de login"]);
    git(&dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("README.md"), "readme\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "docs: readme"]);
    git(&dir, &["merge", "-q", "--no-ff", "feat/login", "-m", "Merge branch 'feat/login'"]);
    git(&dir, &["tag", "v1.0"]);

    // Duas regiões distantes alteradas => dois hunks no diff unstaged.
    let edited = body.replace("linha 2\n", "linha 2 editada\n").replace("linha 35\n", "linha 35 editada\n");
    std::fs::write(dir.join("app.txt"), edited).unwrap();
    std::fs::write(dir.join("novo.txt"), "novo\n").unwrap();
    dir
}

/// Os testes mexem em `XDG_CONFIG_HOME`; serializa para não haver corrida.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Ctx {
    _guard: std::sync::MutexGuard<'static, ()>,
    harness: Harness<'static, BrioApp>,
    repo: PathBuf,
    shots: PathBuf,
}

fn setup(name: &str) -> Ctx {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = std::env::temp_dir().join(format!("brio-git-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // Isola settings.json do usuário real.
    // SAFETY: os testes e2e alteram apenas variáveis próprias antes de criar o app.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", tmp.join("config")) };
    // Config git hermética: não depende (nem altera) o ~/.gitconfig da máquina.
    let gitconfig = tmp.join("gitconfig");
    std::fs::write(
        &gitconfig,
        "[user]\n\tname = Ana Souza\n\temail = ana@example.com\n[commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n[protocol \"file\"]\n\tallow = always\n[init]\n\tdefaultBranch = main\n",
    )
    .unwrap();
    // SAFETY: idem; os testes são serializados por SERIAL.
    unsafe {
        std::env::set_var("GIT_CONFIG_GLOBAL", &gitconfig);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        // Cache de avatares isolado; fotos desligadas por padrão (sem rede nos testes).
        std::env::set_var("XDG_CACHE_HOME", tmp.join("cache"));
    }
    let cfg_dir = tmp.join("config/brio-git");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(cfg_dir.join("settings.json"), r#"{"show_avatars": false, "autofetch_minutes": 0}"#).unwrap();
    let repo = demo_repo(&tmp);
    let shots = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/e2e");
    std::fs::create_dir_all(&shots).unwrap();
    let r = repo.clone();
    // Relógio realista (60 fps): duplo clique e animações dependem do intervalo entre frames.
    let harness = Harness::builder().with_size([W, H]).with_step_dt(1.0 / 60.0).build_eframe(move |cc| {
        let mut app = BrioApp::new(cc, vec![r]);
        app.settings.autofetch_minutes = 0;
        app
    });
    let mut c = Ctx { _guard: guard, harness, repo, shots };
    c.settle();
    c
}

impl Ctx {
    fn tab(&mut self) -> &mut RepoTab {
        match &mut self.harness.state_mut().tabs[0] {
            Tab::Repo(r) => r,
            Tab::Home(_) => panic!("aba de repositório esperada"),
        }
    }

    /// Roda frames até o loader e os jobs em background terminarem.
    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.harness.step();
            let tab = self.tab();
            let net = tab.hosting.as_ref().is_some_and(|h| h.loading || h.pending > 0);
            let idle = tab.snap.is_some() && !tab.jobs.busy() && !tab.is_loading() && !net;
            if idle {
                for _ in 0..3 {
                    self.harness.step();
                }
                if !self.tab().is_loading() && !self.tab().jobs.busy() {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "timeout esperando o app ficar ocioso");
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn click(&mut self, p: Pos2) {
        self.harness.event(Event::PointerMoved(p));
        self.harness.step();
        for pressed in [true, false] {
            self.harness.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            self.harness.step();
        }
        self.settle();
    }

    fn shot(&mut self, name: &str) {
        match self.harness.render() {
            Ok(img) => img.save(self.shots.join(format!("{name}.png"))).unwrap(),
            Err(e) => eprintln!("render indisponível ({e}); pulando snapshot {name}"),
        }
    }

    fn row_y(&self, row: usize) -> f32 {
        // abas + toolbar + cabeçalho do grafo + metade da linha
        crate::theme::TABBAR_HEIGHT + crate::theme::TOOLBAR_HEIGHT + 26.0 + row as f32 * crate::theme::ROW_HEIGHT + crate::theme::ROW_HEIGHT / 2.0
    }

    fn message_x(&mut self) -> f32 {
        // Meio da área central, sempre dentro da coluna de mensagem.
        let left = (W * 0.18).clamp(170.0, 260.0);
        let right = W - (W * 0.28).clamp(260.0, 420.0);
        left + (right - left) * 0.72
    }
}

#[test]
fn e2e_graph_selection_staging_commit_and_undo() {
    let mut c = setup("main");
    {
        let snap = c.tab().snap.clone().unwrap();
        assert_eq!(snap.commits.len(), 6, "5 commits + merge");
        assert_eq!(snap.status.unstaged.len(), 2);
        assert!(snap.graph.max_cols >= 2, "merge deve abrir uma segunda lane");
    }
    c.shot("01_graph_wip");

    // Clique na linha do 3º commit (linha 0 é o WIP).
    let x = c.message_x();
    let y = c.row_y(3);
    c.click(pos2(x, y));
    let snap = c.tab().snap.clone().unwrap();
    assert_eq!(c.tab().selection, Selection::Commits(vec![snap.commits[2].id]), "clique deve selecionar o commit da linha");
    c.shot("02_commit_selected");

    // Volta ao WIP clicando na linha 0.
    let y0 = c.row_y(0);
    c.click(pos2(x, y0));
    assert_eq!(c.tab().selection, Selection::Wip);

    // Abre o diff de app.txt e faz stage só do primeiro hunk.
    let repo = c.repo.clone();
    c.tab().open_file(crate::git::diff::DiffSource::Unstaged, "app.txt".into());
    c.settle();
    c.shot("03_diff_hunks");
    let hunks = match &c.tab().center {
        Some(CenterView::File(f)) => f.diff.as_ref().map(|d| d.hunks.len()).unwrap_or(0),
        _ => 0,
    };
    assert_eq!(hunks, 2, "app.txt deve ter dois hunks");
    let patch = match &c.tab().center {
        Some(CenterView::File(f)) => crate::git::diff::partial_patch(f.diff.as_ref().unwrap(), 0, None, crate::git::diff::PatchMode::Stage).unwrap(),
        _ => unreachable!(),
    };
    let settings = c.harness.state().settings.clone();
    c.tab().handle(crate::repo_tab::Action::ApplyPatch { patch, mode: crate::git::diff::PatchMode::Stage }, &settings);
    c.settle();
    let cached = git(&repo, &["diff", "--cached", "--", "app.txt"]);
    assert!(cached.contains("+linha 2 editada"), "hunk 1 staged");
    assert!(!cached.contains("linha 35 editada"), "hunk 2 continua unstaged");
    c.shot("04_after_hunk_stage");

    // Commit pela UI: digita mensagem e dispara a ação de commit.
    c.tab().wip_summary = "feat: edita linha 2".into();
    c.tab().handle(crate::repo_tab::Action::Commit { stage_all: false }, &settings);
    c.settle();
    assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "feat: edita linha 2");
    assert!(git(&repo, &["diff", "--", "app.txt"]).contains("linha 35 editada"), "hunk 2 ainda pendente");
    assert_eq!(c.tab().undo.len(), 1);

    // Undo desfaz o commit (reset --soft) mantendo as mudanças staged.
    c.tab().handle(crate::repo_tab::Action::Undo, &settings);
    c.settle();
    assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "Merge branch 'feat/login'");
    assert!(git(&repo, &["diff", "--cached"]).contains("linha 2 editada"));
    assert_eq!(c.tab().redo.len(), 1);

    // Redo refaz.
    c.tab().handle(crate::repo_tab::Action::Redo, &settings);
    c.settle();
    assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "feat: edita linha 2");
    c.shot("05_after_commit");
}

#[test]
fn e2e_branch_merge_conflict_resolution() {
    let mut c = setup("conflict");
    let repo = c.repo.clone();
    git(&repo, &["stash", "-u", "-q"]);
    // Dois branches mexendo na mesma linha.
    git(&repo, &["checkout", "-qb", "feat/a"]);
    std::fs::write(repo.join("README.md"), "versao A\n").unwrap();
    git(&repo, &["commit", "-qam", "A"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("README.md"), "versao main\n").unwrap();
    git(&repo, &["commit", "-qam", "main muda readme"]);
    c.settle();
    let settings = c.harness.state().settings.clone();
    c.tab().handle(crate::repo_tab::Action::Refresh, &settings);
    c.settle();

    c.tab().handle(crate::repo_tab::Action::Merge("feat/a".into()), &settings);
    c.settle();
    // Merge com conflito: o job falha e o diálogo de erro aparece.
    assert!(c.tab().dialog.is_some(), "conflito deve abrir diálogo");
    c.tab().dialog = None;
    let snap = c.tab().snap.clone().unwrap();
    assert_eq!(snap.state, crate::git::model::OpState::Merge);
    assert_eq!(snap.status.conflicted.len(), 1);
    c.shot("06_merge_conflict_wip");

    // Ferramenta de merge: escolhe os dois lados e salva.
    c.tab().handle(crate::repo_tab::Action::OpenConflict("README.md".into()), &settings);
    if let Some(CenterView::Merge(mv)) = &mut c.tab().center {
        for s in &mut mv.segments {
            if let crate::git::conflict::Segment::Conflict(cf) = s {
                cf.take_ours = true;
                cf.take_theirs = true;
            }
        }
        mv.output = crate::git::conflict::render(&mv.segments);
    } else {
        panic!("merge view esperada");
    }
    c.harness.step();
    c.shot("07_merge_tool");
    c.tab().handle(crate::repo_tab::Action::SaveMerge, &settings);
    c.settle();
    assert_eq!(std::fs::read_to_string(repo.join("README.md")).unwrap(), "versao main\nversao A\n");
    c.tab().handle(crate::repo_tab::Action::Continue, &settings);
    c.settle();
    assert_eq!(c.tab().snap.as_ref().unwrap().state, crate::git::model::OpState::Clean);
    assert_eq!(git(&repo, &["log", "-1", "--format=%p"]).split(' ').count(), 2, "merge commit com 2 pais");
    c.shot("08_merged");
}

#[test]
fn e2e_interactive_rebase_squash_and_reword() {
    let mut c = setup("rebase");
    let repo = c.repo.clone();
    git(&repo, &["stash", "-u", "-q"]);
    git(&repo, &["checkout", "-qb", "topic", "feat/login"]);
    for i in 1..=3 {
        std::fs::write(repo.join(format!("t{i}.txt")), "t\n").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", &format!("wip {i}")]);
    }
    let settings = c.harness.state().settings.clone();
    c.tab().handle(crate::repo_tab::Action::Refresh, &settings);
    c.settle();
    let base = git(&repo, &["rev-parse", "feat/login"]);
    c.tab().handle(crate::repo_tab::Action::StartInteractiveRebase(git2::Oid::from_str(&base).unwrap()), &settings);
    {
        let plan = c.tab().rebase.as_mut().expect("plano de rebase");
        assert_eq!(plan.steps.len(), 3);
        // Exibição: mais novo primeiro. wip 3 e wip 2 viram squash/fixup em wip 1 (reword).
        plan.steps[0].1 = crate::git::ops::RebaseAction::Fixup;
        plan.steps[1].1 = crate::git::ops::RebaseAction::Fixup;
        plan.steps[2].1 = crate::git::ops::RebaseAction::Reword;
        plan.steps[2].2 = "feat: tópico completo".into();
    }
    c.harness.step();
    c.shot("09_interactive_rebase");
    c.tab().handle(crate::repo_tab::Action::RunInteractiveRebase, &settings);
    c.settle();
    assert_eq!(git(&repo, &["rev-list", "--count", "feat/login..topic"]), "1");
    assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "feat: tópico completo");
    assert!(repo.join("t3.txt").exists());
}

#[test]
fn e2e_integrated_terminal_runs_commands_in_repo() {
    let mut c = setup("terminal");
    let settings = c.harness.state().settings.clone();
    c.tab().handle(crate::repo_tab::Action::ToggleTerminal, &settings);
    c.harness.step();
    assert_eq!(c.tab().terminals.len(), 1, "terminal deve abrir");

    // Clica dentro do painel do terminal (rodapé da área central) para focar.
    c.click(pos2(W / 2.0, H - 80.0));
    c.harness.event(Event::Text("echo brio-$((40+2)) && git rev-parse --abbrev-ref HEAD".into()));
    c.harness.key_press(egui::Key::Enter);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        c.harness.step();
        let text = c.tab().terminals[0].screen_text();
        if text.contains("brio-42") && text.lines().any(|l| l.trim() == "main") {
            break;
        }
        assert!(Instant::now() < deadline, "saída do terminal não apareceu:\n{text}");
        std::thread::sleep(Duration::from_millis(50));
    }
    c.shot("10_terminal");
}

impl Ctx {
    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.harness.event(Event::PointerMoved(from));
        self.harness.step();
        self.harness.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        self.harness.step();
        for i in 1..=10 {
            let p = from + (to - from) * (i as f32 / 10.0);
            self.harness.event(Event::PointerMoved(p));
            self.harness.step();
        }
        self.harness.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        // Soltar -> ação no fim do frame -> Area aparece após o passe de medição.
        self.harness.run_steps(3);
    }
}

#[test]
fn e2e_drag_branch_onto_branch_fast_forwards() {
    let mut c = setup("dnd");
    let repo = c.repo.clone();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "chore: pendências"]);
    git(&repo, &["checkout", "-qb", "topic"]);
    std::fs::write(repo.join("topic.txt"), "t\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "feat: topic"]);
    git(&repo, &["checkout", "-q", "main"]);
    c.harness.state_mut().show_left = false;
    let settings = c.harness.state().settings.clone();
    c.tab().handle(crate::repo_tab::Action::Refresh, &settings);
    c.settle();
    let snap = c.tab().snap.clone().unwrap();
    assert_eq!(snap.commits[0].summary, "feat: topic", "linha 0 = topic (sem WIP)");

    // Pills da coluna BRANCH/TAG: painel esquerdo oculto => coluna começa em x=0.
    let (from, to) = (pos2(60.0, c.row_y(0)), pos2(60.0, c.row_y(1)));
    c.drag(from, to);
    c.shot("11_drop_menu");
    let dm = c.tab().drop_menu.as_ref().expect("soltar deve abrir o menu");
    assert_eq!((dm.source.name.as_str(), dm.target.name.as_str()), ("topic", "main"));

    c.harness.get_by_label("Fast-forward main to topic").click();
    c.harness.step();
    c.settle();
    assert!(c.tab().drop_menu.is_none());
    assert_eq!(git(&repo, &["rev-parse", "main"]), git(&repo, &["rev-parse", "topic"]), "main avançou até topic");
    c.shot("12_after_fast_forward");
}

fn clean(c: &mut Ctx) {
    let repo = c.repo.clone();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "chore: pendências"]);
    let settings = c.harness.state().settings.clone();
    c.tab().handle(crate::repo_tab::Action::Refresh, &settings);
    c.settle();
}

#[test]
fn e2e_gitflow_init_feature_release() {
    use crate::git::gitflow::{FlowKind, GitflowConfig};
    use crate::repo_tab::Action;
    let mut c = setup("gitflow");
    clean(&mut c);
    let repo = c.repo.clone();
    git(&repo, &["config", "tag.gpgsign", "false"]);
    let settings = c.harness.state().settings.clone();
    assert!(c.tab().snap.as_ref().unwrap().gitflow.is_none());

    // Sem Gitflow a seção some; "Initialize Gitflow" fica no menu "+" do painel.
    c.harness.get_by_label("Add").click();
    c.harness.run_steps(3);
    c.harness.get_by_label("Initialize Gitflow").click();
    c.harness.run_steps(4);
    assert!(matches!(c.tab().dialog, Some(crate::repo_tab::Dialog::GitflowInit(ref g)) if g.master == "main"));
    c.harness.get_all_by_role_and_label(egui::accesskit::Role::Button, "Initialize Gitflow").last().unwrap().click();
    c.harness.step();
    c.settle();
    let dialog = format!("{:?}", c.tab().dialog);
    let cfg = c.tab().snap.as_ref().unwrap().gitflow.clone().unwrap_or_else(|| panic!("gitflow não inicializado; diálogo: {dialog}"));
    assert_eq!(cfg, GitflowConfig { master: "main".into(), ..Default::default() });

    c.tab().handle(Action::GitflowStart { kind: FlowKind::Feature, name: "pagamento".into() }, &settings);
    c.settle();
    assert_eq!(git(&repo, &["branch", "--show-current"]), "feature/pagamento");
    std::fs::write(repo.join("pag.txt"), "p\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "feat: pagamento"]);
    c.tab().handle(Action::Refresh, &settings);
    c.settle();
    c.shot("13_gitflow_panel");
    c.tab().handle(Action::GitflowFinish { kind: FlowKind::Feature, name: "pagamento".into(), tag_message: String::new(), keep_branch: false }, &settings);
    c.settle();
    assert_eq!(git(&repo, &["branch", "--show-current"]), "develop");
    assert_eq!(git(&repo, &["show", "develop:pag.txt"]), "p");

    c.tab().handle(Action::GitflowStart { kind: FlowKind::Release, name: "2.0".into() }, &settings);
    c.settle();
    c.tab().handle(Action::GitflowFinish { kind: FlowKind::Release, name: "2.0".into(), tag_message: "Release 2.0".into(), keep_branch: false }, &settings);
    c.settle();
    assert!(git(&repo, &["tag", "--list"]).lines().any(|t| t == "2.0"));
    assert_eq!(git(&repo, &["show", "main:pag.txt"]), "p", "release levou a feature ao main");
}

#[test]
fn e2e_worktree_submodule_and_lfs_visibility() {
    use crate::repo_tab::Action;
    let mut c = setup("worktree");
    clean(&mut c);
    let repo = c.repo.clone();
    let settings = c.harness.state().settings.clone();
    let wt = repo.parent().unwrap().join("demo-wt");
    c.tab().handle(Action::WorktreeAdd { path: wt.display().to_string(), branch: "wt-branch".into(), new_branch: true }, &settings);
    c.settle();
    let snap = c.tab().snap.clone().unwrap();
    assert_eq!(snap.worktrees.len(), 1);
    assert_eq!(git(&wt, &["branch", "--show-current"]), "wt-branch");

    // Abrir o worktree cria uma segunda aba apontando para ele.
    let wt_path = std::path::PathBuf::from(&snap.worktrees[0].path);
    c.harness.state_mut().open_repo(wt_path.clone(), &egui::Context::default());
    let tabs: Vec<PathBuf> = c.harness.state().tabs.iter().filter_map(|t| match t { Tab::Repo(r) => Some(r.path.clone()), _ => None }).collect();
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[1].canonicalize().unwrap(), wt.canonicalize().unwrap());
    c.harness.state_mut().active = 0;

    // Submodule a partir de outro repositório local.
    let lib = repo.parent().unwrap().join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    git(&lib, &["init", "-q", "-b", "main"]);
    git(&lib, &["-c", "user.name=L", "-c", "user.email=l@l", "commit", "-q", "--allow-empty", "-m", "lib"]);
    c.tab().handle(Action::SubmoduleAdd { url: lib.display().to_string(), path: "vendor/lib".into() }, &settings);
    c.settle();
    let snap = c.tab().snap.clone().unwrap();
    assert_eq!(snap.submodules.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), vec!["vendor/lib"]);
    assert_eq!(snap.lfs.is_some(), crate::git::ops::lfs_available(), "seção LFS só aparece com git-lfs instalado");
    c.shot("14_worktrees_submodules");
}

#[test]
fn e2e_signed_commit_shows_verified_badge() {
    let mut c = setup("signing");
    let repo = c.repo.clone();
    let key = repo.parent().unwrap().join("id_test");
    let st = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-C", "ana@example.com", "-f"]).arg(&key).status().unwrap();
    assert!(st.success());
    let pubkey = std::fs::read_to_string(key.with_extension("pub")).unwrap();
    let allowed = repo.parent().unwrap().join("allowed_signers");
    std::fs::write(&allowed, format!("ana@example.com {pubkey}")).unwrap();
    git(&repo, &["config", "gpg.format", "ssh"]);
    git(&repo, &["config", "user.signingkey", &key.display().to_string()]);
    git(&repo, &["config", "gpg.ssh.allowedSignersFile", &allowed.display().to_string()]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-S", "-m", "feat: commit assinado"]);
    let settings = c.harness.state().settings.clone();
    c.tab().handle(crate::repo_tab::Action::Refresh, &settings);
    c.settle();
    let snap = c.tab().snap.clone().unwrap();
    let signed = snap.commits[0].id;
    let unsigned = snap.commits[1].id;
    assert_eq!(c.tab().signature(signed).0, crate::git::ops::Signature::Good);
    assert_eq!(c.tab().signature(unsigned).0, crate::git::ops::Signature::None);
    c.tab().handle(crate::repo_tab::Action::SelectCommit { oid: signed, add: false }, &settings);
    c.harness.run_steps(2);
    c.harness.get_by_label("✔ Assinatura verificada");
    c.shot("15_signed_commit");
}

#[test]
fn e2e_light_theme_renders() {
    let mut c = setup("light");
    let ctx = c.harness.ctx.clone();
    c.harness.state_mut().settings.theme = "light".into();
    let x = c.message_x();
    c.click(pos2(x, c.row_y(2)));
    assert!(crate::theme::is_light(), "tema das settings é aplicado no frame seguinte");
    c.shot("16_light_theme");
    c.harness.state_mut().settings.theme = "dark".into();
    c.harness.step();
    assert!(!crate::theme::is_light());
    let _ = ctx;
}

#[test]
fn e2e_profile_identity_used_for_commits() {
    let mut c = setup("profile");
    let repo = c.repo.clone();
    {
        let s = &mut c.harness.state_mut().settings;
        s.profiles.push(crate::settings::Profile { label: "Freela".into(), name: "Bia Freela".into(), email: "bia@freela.dev".into() });
        s.active_profile = Some(0);
    }
    let settings = c.harness.state().settings.clone();
    c.tab().wip_summary = "feat: commit com perfil".into();
    c.tab().handle(crate::repo_tab::Action::Commit { stage_all: true }, &settings);
    c.settle();
    assert_eq!(git(&repo, &["log", "-1", "--format=%an <%ae> / %cn <%ce>"]), "Bia Freela <bia@freela.dev> / Bia Freela <bia@freela.dev>");
    assert_eq!(git(&repo, &["config", "user.name"]), "Ana Souza", "gitconfig intocado");
    c.harness.run_steps(2);
    c.harness.get_by_label("Freela  ▾");
}

#[test]
fn e2e_gitlab_merge_requests_end_to_end() {
    use crate::hosting::mock::Mock;
    use crate::repo_hosting::HostingAction as H;
    use crate::repo_tab::{Action, Dialog};
    let mut c = setup("gitlab");
    clean(&mut c);
    let repo = c.repo.clone();

    // "Servidor" git: repositório bare; o remoto aparenta ser um GitLab self-hosted.
    let server = repo.parent().unwrap().join("server.git");
    git(repo.parent().unwrap(), &["init", "-q", "--bare", "server.git"]);
    let fake = "git@gitlab.example.test:grupo/sub/demo.git";
    git(&repo, &["remote", "add", "origin", fake]);
    git(&repo, &["config", &format!("url.{}.insteadOf", server.display()), fake]);
    git(&repo, &["push", "-q", "origin", "main", "feat/login"]);

    let mock = Mock::start(Box::new(|m, p, _| {
        let base = "/api/v4/projects/grupo%2Fsub%2Fdemo";
        let p = p.strip_prefix(base).map(|r| format!("P{r}")).unwrap_or_else(|| p.to_string());
        match (m, p.as_str()) {
            ("GET", "/api/v4/user") => Some((200, r#"{"username":"ana"}"#.into())),
            ("GET", "P") => Some((200, r#"{"default_branch":"main"}"#.into())),
            ("GET", q) if q.starts_with("P/merge_requests?") => Some((200, r#"[{"iid":5,"title":"Tela de login","description":"Implementa o login","author":{"username":"bia"},
                "state":"opened","draft":false,"source_branch":"feat/login","target_branch":"main","web_url":"https://gitlab.example.test/grupo/sub/demo/-/merge_requests/5",
                "sha":"x","source_project_id":1,"target_project_id":1}]"#.into())),
            ("GET", q) if q.starts_with("P/issues?") => Some((200, r#"[{"iid":9,"title":"Corrigir login","description":"","author":{"username":"c"},"state":"opened","labels":["bug"],"assignees":[],"web_url":"w"}]"#.into())),
            ("GET", q) if q.starts_with("P/merge_requests/5/notes") => Some((200, r#"[{"body":"LGTM, só um ajuste","system":false,"author":{"username":"bia"},"created_at":"2026-10-28T10:00:00Z"}]"#.into())),
            ("POST", "P/merge_requests") => Some((201, r#"{"iid":6,"title":"t","state":"opened","source_branch":"feat/novo","target_branch":"main","author":{"username":"ana"},"source_project_id":1,"target_project_id":1}"#.into())),
            ("POST", "P/merge_requests/5/notes") => Some((201, "{}".into())),
            ("PUT", "P/merge_requests/5/merge") => Some((200, "{}".into())),
            ("GET", q) if q.starts_with("P/merge_requests/5/pipelines") => Some((200, "[]".into())),
            ("GET", q) if q.starts_with("P/pipelines?sha=x") => Some((200, r#"[{"id":77,"status":"failed","web_url":"https://gitlab.example.test/p/77"}]"#.into())),
            ("GET", q) if q.starts_with("P/pipelines/77/jobs") => Some((200, r#"[{"name":"lint","stage":"test","status":"success","web_url":"j1"},{"name":"unit","stage":"test","status":"failed","web_url":"j2"}]"#.into())),
            ("GET", "P/merge_requests/5/changes") => Some((200, r#"{"diff_refs":{"base_sha":"b1","start_sha":"s1","head_sha":"h1"},
                "changes":[{"old_path":"login.txt","new_path":"login.txt","diff":"@@ -1,2 +1,3 @@\n login\n+senha\n fim\n"}]}"#.into())),
            ("GET", q) if q.starts_with("P/merge_requests/5/discussions") => Some((200, r#"[{"notes":[{"body":"usar bcrypt aqui","system":false,
                "author":{"username":"bia"},"position":{"new_path":"login.txt","old_path":"login.txt","new_line":2,"old_line":null}}]}]"#.into())),
            ("POST", "P/merge_requests/5/discussions") => Some((201, "{}".into())),
            _ => None,
        }
    }));
    crate::hosting::auth::set_session("gitlab.example.test", "tok-e2e");
    c.harness.state_mut().settings.hosting_hosts =
        vec![crate::hosting::HostConfig { host: "gitlab.example.test".into(), kind: crate::hosting::Kind::GitLab, api: format!("{}/api/v4", mock.url) }];
    let settings = c.harness.state().settings.clone();
    c.tab().handle(Action::Refresh, &settings);
    c.settle();
    {
        let h = c.tab().hosting.as_ref().expect("GitLab detectado pelo remote");
        assert_eq!((h.repo.path.as_str(), h.user.as_deref(), h.default_branch.as_deref()), ("grupo/sub/demo", Some("ana"), Some("main")));
        assert_eq!(h.prs.as_ref().unwrap().as_ref().unwrap().len(), 1);
        assert_eq!(h.issues.as_ref().unwrap().as_ref().unwrap()[0].number, 9);
    }
    c.shot("17_gitlab_sections");

    // Abre o MR: comentários chegam e aparecem na UI.
    c.tab().handle(Action::Hosting(H::OpenPr(5)), &settings);
    c.settle();
    c.harness.run_steps(2);
    c.harness.get_by_label("LGTM, só um ajuste");
    c.harness.get_by_label("Merge Merge Request");
    c.shot("18_merge_request_view");
    assert_eq!(c.tab().hosting.as_ref().unwrap().ci_by_pr.get(&5).copied().flatten(), Some(crate::hosting::CiState::Failure), "CI do MR na lista");
    c.harness.get_by_label("CI falhou · 1 falha(s)");

    // Aba Arquivos: diff do MR com o comentário de revisão existente sob a linha 2.
    c.harness.get_by_label("Arquivos").click();
    c.harness.step();
    c.settle();
    c.harness.run_steps(2);
    c.harness.get_by_label("usar bcrypt aqui");
    c.shot("26_mr_files_review");
    // Clicar numa linha abre o rascunho; enviar cria a discussão com a posição completa.
    if let Some(crate::repo_tab::CenterView::PullRequest(v)) = &mut c.tab().center {
        let anchor = crate::hosting::LineAnchor { path: "login.txt".into(), old_path: "login.txt".into(), new_line: Some(3), old_line: None };
        v.draft = Some((anchor.clone(), "ok".into()));
        let _ = anchor;
    }
    c.tab().handle(Action::Hosting(H::ReviewComment {
        number: 5,
        anchor: crate::hosting::LineAnchor { path: "login.txt".into(), old_path: "login.txt".into(), new_line: Some(3), old_line: None },
        body: "trocar por argon2".into(),
    }), &settings);
    c.settle();
    let disc = mock.requests().into_iter().find(|r| r.method == "POST" && r.path.ends_with("/discussions")).expect("POST discussions");
    let body: serde_json::Value = serde_json::from_str(&disc.body).unwrap();
    assert_eq!(body["position"]["head_sha"], "h1");
    assert_eq!(body["position"]["base_sha"], "b1");
    assert_eq!(body["position"]["start_sha"], "s1");
    assert_eq!(body["position"]["new_line"], 3);
    assert_eq!(body["body"], "trocar por argon2");

    // Checkout do MR: branch local rastreando origin/feat/login.
    c.tab().handle(Action::Hosting(H::CheckoutPr(5)), &settings);
    c.settle();
    assert_eq!(git(&repo, &["rev-parse", "--abbrev-ref", "feat/login@{upstream}"]), "origin/feat/login");
    assert_eq!(git(&repo, &["branch", "--show-current"]), "feat/login");

    // Comentar e fazer merge (via API).
    c.tab().handle(Action::Hosting(H::CommentPr { number: 5, body: "Ajustado!".into() }), &settings);
    c.tab().handle(Action::Hosting(H::MergePr { number: 5, method: crate::hosting::MergeMethod::Squash }), &settings);
    c.settle();

    // Novo branch -> "Push and start a merge request" pelo diálogo.
    git(&repo, &["checkout", "-qb", "feat/novo"]);
    std::fs::write(repo.join("novo2.txt"), "n\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "feat: recurso novo"]);
    c.tab().handle(Action::Refresh, &settings);
    c.settle();
    c.tab().handle(Action::StartPr("feat/novo".into()), &settings);
    match &c.tab().dialog {
        Some(Dialog::CreatePr { source, target, title, push_first, .. }) => {
            assert_eq!((source.as_str(), target.as_str(), title.as_str(), *push_first), ("feat/novo", "main", "feat: recurso novo", true));
        }
        d => panic!("diálogo CreatePr esperado, veio {d:?}"),
    }
    c.harness.run_steps(3);
    c.shot("19_create_merge_request");
    c.harness.get_all_by_role_and_label(egui::accesskit::Role::Button, "Create Merge Request").last().unwrap().click();
    c.harness.step();
    c.settle();
    assert_eq!(git(&server, &["rev-parse", "feat/novo"]), git(&repo, &["rev-parse", "feat/novo"]), "push antes de criar o MR");

    let reqs = mock.requests();
    let create = reqs.iter().find(|r| r.method == "POST" && r.path.ends_with("/merge_requests")).expect("POST do MR");
    let body: serde_json::Value = serde_json::from_str(&create.body).unwrap();
    assert_eq!((body["source_branch"].as_str(), body["target_branch"].as_str(), body["title"].as_str()), (Some("feat/novo"), Some("main"), Some("feat: recurso novo")));
    assert!(reqs.iter().any(|r| r.method == "POST" && r.path.ends_with("/merge_requests/5/notes") && r.body.contains("Ajustado!")));
    let merge = reqs.iter().find(|r| r.method == "PUT").expect("PUT do merge");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&merge.body).unwrap()["squash"], true);
    assert!(reqs.iter().all(|r| r.header("private-token") == Some("tok-e2e")));

    // Branch a partir de issue.
    c.tab().handle(Action::Hosting(H::BranchFromIssue(9)), &settings);
    assert!(matches!(&c.tab().dialog, Some(Dialog::CreateBranch { name, .. }) if name == "9-corrigir-login"));
}

#[test]
fn e2e_github_connect_and_checkout_fork_pr() {
    use crate::hosting::mock::Mock;
    use crate::repo_hosting::HostingAction as H;
    use crate::repo_tab::Action;
    let mut c = setup("github");
    clean(&mut c);
    let repo = c.repo.clone();
    let server = repo.parent().unwrap().join("gh.git");
    git(repo.parent().unwrap(), &["init", "-q", "--bare", "gh.git"]);
    let fake = "git@github.example.test:ana/demo.git";
    git(&repo, &["remote", "add", "origin", fake]);
    git(&repo, &["config", &format!("url.{}.insteadOf", server.display()), fake]);
    git(&repo, &["push", "-q", "origin", "main"]);
    // PR de fork: o servidor expõe o head em refs/pull/7/head.
    git(&repo, &["checkout", "-qb", "contrib"]);
    std::fs::write(repo.join("contrib.txt"), "c\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "feat: contribuição externa"]);
    let contrib_sha = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["push", "-q", "origin", "contrib:refs/pull/7/head"]);
    git(&repo, &["checkout", "-q", "main"]);
    git(&repo, &["branch", "-D", "contrib"]);

    let mock = Mock::start(Box::new(|m, p, _| match (m, p) {
        ("GET", "/user") => Some((200, r#"{"login":"ana"}"#.into())),
        ("GET", "/repos/ana/demo") => Some((200, r#"{"default_branch":"main"}"#.into())),
        ("GET", q) if q.starts_with("/repos/ana/demo/pulls?") => Some((200, r#"[{"number":7,"title":"Contribuição","body":"","user":{"login":"bia"},"state":"open","draft":false,"merged_at":null,
            "head":{"ref":"contrib","sha":"s","repo":{"full_name":"bia/demo"}},"base":{"ref":"main"},"html_url":"https://github.example.test/ana/demo/pull/7","updated_at":"t"}]"#.into())),
        ("GET", q) if q.starts_with("/repos/ana/demo/issues") => Some((200, "[]".into())),
        _ => None,
    }));
    c.harness.state_mut().settings.hosting_hosts =
        vec![crate::hosting::HostConfig { host: "github.example.test".into(), kind: crate::hosting::Kind::GitHub, api: mock.url.clone() }];
    let settings = c.harness.state().settings.clone();
    c.tab().handle(Action::Refresh, &settings);
    c.settle();
    assert!(c.tab().hosting.as_ref().unwrap().connected.is_none(), "sem token: desconectado");

    // Conectar pela UI: menu "+" do painel -> diálogo -> digitar token -> Conectar
    // (sem PRs/issues, as seções de hosting ficam ocultas).
    c.harness.run_steps(2);
    c.harness.get_by_label("Add").click();
    c.harness.run_steps(3);
    c.harness.get_by_label("Conectar ao GitHub (github.example.test)").click();
    c.harness.run_steps(4);
    c.shot("20_connect_github");
    let field = c.harness.get_by_role(egui::accesskit::Role::PasswordInput);
    field.click();
    c.harness.run_steps(1);
    c.harness.get_by_role(egui::accesskit::Role::PasswordInput).type_text("ghp_teste");
    c.harness.run_steps(1);
    c.harness.get_all_by_role_and_label(egui::accesskit::Role::Button, "Conectar").last().unwrap().click();
    c.harness.step();
    c.settle();
    let h = c.tab().hosting.as_ref().unwrap();
    assert_eq!(h.connected, Some(crate::hosting::auth::Source::Session));
    assert_eq!(h.user.as_deref(), Some("ana"));
    let pr = h.pr(7).expect("PR listado após conectar").clone();
    assert_eq!(pr.fork.as_deref(), Some("bia/demo"));
    assert!(mock.requests().iter().all(|r| r.header("authorization") == Some("Bearer ghp_teste")));

    c.tab().handle(Action::Hosting(H::CheckoutPr(7)), &settings);
    c.settle();
    assert_eq!(git(&repo, &["branch", "--show-current"]), "pr/7");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), contrib_sha, "head do PR de fork via refs/pull/7/head");
    c.shot("21_github_fork_pr_checked_out");
}

#[test]
fn e2e_window_controls_send_viewport_commands() {
    let mut c = setup("window");
    // Botões no canto direito da barra de abas, 42px cada: [—][□][×].
    let y = crate::theme::TABBAR_HEIGHT / 2.0;
    let sent = |c: &mut Ctx, x: f32| {
        c.harness.event(Event::PointerMoved(pos2(x, y)));
        c.harness.step();
        let mut cmds = Vec::new();
        for pressed in [true, false] {
            c.harness.event(Event::PointerButton { pos: pos2(x, y), button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            c.harness.step();
            cmds.extend(c.harness.output().viewport_output.values().flat_map(|v| v.commands.clone()));
        }
        cmds
    };
    use egui::ViewportCommand as V;
    if crate::app::compositor_supports_minimize() {
        assert!(sent(&mut c, W - 21.0 - 84.0).contains(&V::Minimized(true)), "minimizar");
    } else {
        // niri/sway/...: o botão de minimizar fica oculto; ali fica o de Preferences.
        assert!(!sent(&mut c, W - 21.0 - 84.0).contains(&V::Minimized(true)), "sem minimizar em compositor tiling");
    }
    assert!(sent(&mut c, W - 21.0 - 42.0).contains(&V::Maximized(true)), "maximizar");
    assert!(sent(&mut c, W - 21.0).contains(&V::Close), "fechar");

    // Duplo clique na área vazia da barra alterna maximizar; F11 alterna tela cheia.
    let empty = pos2(W / 2.0, y);
    c.harness.event(Event::PointerMoved(empty));
    // Pausa > 0,3 s: senão os cliques nos botões acima contam e vira triplo clique.
    c.harness.run_steps(30);
    // Cada clique (press+release) num frame; com 60 fps o intervalo fica bem abaixo de 0,3 s.
    let mut cmds = Vec::new();
    for _ in 0..2 {
        for pressed in [true, false] {
            c.harness.event(Event::PointerButton { pos: empty, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        c.harness.step();
        cmds.extend(c.harness.output().viewport_output.values().flat_map(|v| v.commands.clone()));
    }
    assert!(cmds.contains(&V::Maximized(true)), "duplo clique maximiza: {cmds:?}");
    // `step()` roda um frame por evento enfileirado e `output()` guarda só o
    // último; por isso enviamos só o "key down" e lemos a saída desse frame.
    c.harness.event(Event::Key { key: egui::Key::F11, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    c.harness.step();
    let cmds: Vec<V> = c.harness.output().viewport_output.values().flat_map(|v| v.commands.clone()).collect();
    assert!(cmds.contains(&V::Fullscreen(true)), "F11: {cmds:?}");
    c.shot("22_window_controls");
}

#[test]
fn second_instance_does_not_clobber_tabs() {
    let c = setup("instances");
    assert!(c.harness.state().primary, "primeira instância é primária");
    // O lock é global do processo; outro teste pode tê-lo pegado noutro XDG_CONFIG_HOME.
    crate::settings::release_primary();
    assert!(crate::settings::acquire_primary(), "adquire no diretório de config deste teste");
    assert!(crate::settings::acquire_primary(), "a mesma instância reconfirma o lock");
    // Outro processo veria o arquivo travado: um segundo handle não consegue o lock.
    let lock = dirs::config_dir().unwrap().join("brio-git/instance.lock");
    let other = std::fs::OpenOptions::new().write(true).open(&lock).unwrap();
    assert!(other.try_lock().is_err(), "lock exclusivo enquanto a primária vive");
    crate::settings::release_primary();
    assert!(other.try_lock().is_ok(), "liberado ao fechar a primária");
}

#[test]
fn e2e_terminal_tabs_are_independent() {
    use crate::repo_tab::Action;
    let mut c = setup("termtabs");
    let settings = c.harness.state().settings.clone();
    c.tab().handle(Action::ToggleTerminal, &settings);
    c.tab().handle(Action::NewTerminal, &settings);
    c.harness.run_steps(3);
    assert_eq!(c.tab().terminals.len(), 2);
    assert_eq!(c.tab().active_terminal, 1, "a aba nova fica ativa");

    let wait_for = |c: &mut Ctx, idx: usize, needle: &str| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            c.harness.step();
            let text = c.tab().terminals[idx].screen_text();
            if text.contains(needle) {
                return text;
            }
            assert!(Instant::now() < deadline, "terminal {idx} sem '{needle}':\n{text}");
            std::thread::sleep(Duration::from_millis(40));
        }
    };
    // Digita na aba ativa (2ª) clicando dentro do painel.
    c.click(pos2(W / 2.0, H - 80.0));
    c.harness.event(Event::Text("echo segunda-$((1+1))".into()));
    c.harness.key_press(egui::Key::Enter);
    wait_for(&mut c, 1, "segunda-2");
    c.shot("23_terminal_tabs");

    // Volta para a 1ª aba e roda outro comando; a 2ª não recebe nada.
    c.tab().handle(Action::SelectTerminal(0), &settings);
    c.harness.run_steps(2);
    c.harness.event(Event::Text("echo primeira-$((0+1))".into()));
    c.harness.key_press(egui::Key::Enter);
    wait_for(&mut c, 0, "primeira-1");
    assert!(!c.tab().terminals[1].screen_text().contains("primeira-1"), "abas isoladas");

    // `exit` na 1ª fecha só ela.
    c.harness.event(Event::Text("exit".into()));
    c.harness.key_press(egui::Key::Enter);
    let deadline = Instant::now() + Duration::from_secs(10);
    while c.tab().terminals.len() != 1 {
        c.harness.step();
        assert!(Instant::now() < deadline, "aba com exit deveria fechar");
        std::thread::sleep(Duration::from_millis(40));
    }
    assert!(c.tab().terminals[0].screen_text().contains("segunda-2"), "sobrou a aba certa");
}

#[test]
fn e2e_hover_expands_all_refs_on_commit() {
    use crate::repo_tab::Action;
    let mut c = setup("refs");
    clean(&mut c);
    let repo = c.repo.clone();
    git(&repo, &["branch", "hotfix/a"]);
    git(&repo, &["branch", "release/b"]);
    git(&repo, &["tag", "v2.0"]);
    c.harness.state_mut().show_left = false;
    let settings = c.harness.state().settings.clone();
    c.tab().handle(Action::Refresh, &settings);
    c.settle();
    let head = c.tab().snap.as_ref().unwrap().commits[0].id;
    assert_eq!(c.tab().snap.as_ref().unwrap().refs_by_commit[&head].len(), 4, "main + 2 branches + tag");

    // Hover no pill da linha 0 (sem WIP: repo limpo).
    let y = c.row_y(0);
    c.harness.event(Event::PointerMoved(pos2(60.0, y)));
    c.harness.run_steps(4);
    let id = egui::Id::unique(("ref_expand", head));
    assert!(c.harness.ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false), "lista expandida ao passar o mouse");
    c.shot("24_refs_expanded");

    // Ordem: HEAD (main) primeiro, depois locais, depois tag. Duplo clique no 2º item.
    let second = pos2(60.0, y + 24.0);
    c.harness.event(Event::PointerMoved(second));
    c.harness.run_steps(2);
    for _ in 0..2 {
        for pressed in [true, false] {
            c.harness.event(Event::PointerButton { pos: second, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        c.harness.step();
    }
    c.settle();
    assert_eq!(git(&repo, &["branch", "--show-current"]), "hotfix/a", "checkout do branch escolhido na lista");

    // Ponteiro sai: a lista fecha.
    c.harness.event(Event::PointerMoved(pos2(W / 2.0, H - 50.0)));
    c.harness.run_steps(3);
    assert!(!c.harness.ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false), "lista fecha ao sair");
}

#[test]
fn e2e_render_every_theme() {
    let mut c = setup("themes");
    let ctx = c.harness.ctx.clone();
    let x = c.message_x();
    c.click(pos2(x, c.row_y(2)));
    for (id, _) in crate::theme::themes() {
        c.harness.state_mut().settings.theme = id.into();
        c.harness.run_steps(2);
        c.shot(&format!("theme_{id}"));
    }
    crate::theme::apply(&ctx, "dark");
    c.harness.state_mut().settings.theme = "dark".into();
    c.harness.step();
}

#[test]
fn e2e_author_photo_replaces_initials() {
    let mut c = setup("avatar");
    // Foto "baixada" antes: o pipeline lê do cache em disco, decodifica e vira textura.
    let dir = dirs::cache_dir().unwrap().join("brio-git/avatars");
    std::fs::create_dir_all(&dir).unwrap();
    let img = image::RgbaImage::from_fn(64, 64, |x, y| if (x / 8 + y / 8) % 2 == 0 { image::Rgba([240, 90, 40, 255]) } else { image::Rgba([30, 160, 220, 255]) });
    let hash = crate::avatars::md5_hex("ana@example.com");
    // `save` escolhe o formato pela extensão; o cache usa `.img` (formato-agnóstico).
    img.save(dir.join(format!("{hash}.png"))).unwrap();
    std::fs::rename(dir.join(format!("{hash}.png")), dir.join(format!("{hash}.img"))).unwrap();
    crate::avatars::set_enabled(true);
    assert!(crate::avatars::get("Ana@Example.com").is_none(), "primeira chamada agenda o carregamento");
    let deadline = Instant::now() + Duration::from_secs(5);
    while crate::avatars::get("ana@example.com").is_none() {
        c.harness.step();
        assert!(Instant::now() < deadline, "avatar do cache não carregou");
        std::thread::sleep(Duration::from_millis(20));
    }
    c.harness.run_steps(2);
    c.shot("25_author_photos");
    crate::avatars::set_enabled(false);
}

#[test]
fn e2e_reset_local_to_remote() {
    use crate::git::ops::ResetMode;
    use crate::repo_tab::Action;
    let mut c = setup("reset_remote");
    clean(&mut c);
    let repo = c.repo.clone();
    git(repo.parent().unwrap(), &["init", "-q", "--bare", "origin.git"]);
    git(&repo, &["remote", "add", "origin", repo.parent().unwrap().join("origin.git").to_str().unwrap()]);
    git(&repo, &["push", "-q", "-u", "origin", "main", "feat/login"]);
    git(&repo, &["branch", "-q", "-u", "origin/feat/login", "feat/login"]);
    let remote_main = git(&repo, &["rev-parse", "origin/main"]);
    // Diverge: commit local no main e outro no feat/login.
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "local: só aqui"]);
    git(&repo, &["checkout", "-q", "feat/login"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "local: login"]);
    git(&repo, &["checkout", "-q", "main"]);
    let settings = c.harness.state().settings.clone();
    c.tab().handle(Action::Refresh, &settings);
    c.settle();

    let snap = c.tab().snap.clone().unwrap();
    let (up, target) = crate::ui::graph_view::upstream_of(&snap, "main").expect("main tem upstream");
    assert_eq!((up.as_str(), target.to_string()), ("origin/main", remote_main.clone()));

    // Branch que não é o atual: só move o ponteiro.
    let (_, login_target) = crate::ui::graph_view::upstream_of(&snap, "feat/login").unwrap();
    c.tab().handle(Action::ResetBranchTo { branch: "feat/login".into(), to: login_target }, &settings);
    c.settle();
    assert_eq!(git(&repo, &["rev-parse", "feat/login"]), git(&repo, &["rev-parse", "origin/feat/login"]));

    // Branch atual: reset hard para o remoto.
    c.tab().handle(Action::Reset { to: target, mode: ResetMode::Hard }, &settings);
    c.settle();
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), remote_main);
    assert!(crate::ui::graph_view::upstream_of(c.tab().snap.as_ref().unwrap(), "main").is_some_and(|(_, t)| t.to_string() == remote_main));
}

#[test]
fn e2e_preferences_tabs_and_system_theme() {
    let mut c = setup("prefs_tabs");
    c.harness.state_mut().open_prefs();
    c.harness.run_steps(3);
    // Abas à esquerda: a página "Temas" troca o conteúdo da direita.
    c.harness.get_by_label("Temas").click();
    c.harness.run_steps(3);
    assert!(c.harness.state().prefs.as_ref().is_some_and(|p| p.section == super::PrefsSection::Themes));
    c.harness.get_by_label("Seguir o tema do sistema (claro e escuro)").click();
    c.harness.run_steps(2);
    assert!(c.harness.state().settings.follow_system_theme);
    c.shot("18_prefs_themes");

    c.harness.state_mut().settings.theme_light = "catppuccin-latte".into();
    c.harness.input_mut().system_theme = Some(egui::Theme::Light);
    c.harness.run_steps(3);
    assert_eq!(crate::theme::p().id, "catppuccin-latte");
    c.harness.input_mut().system_theme = Some(egui::Theme::Dark);
    c.harness.run_steps(3);
    assert_eq!(crate::theme::p().id, "dark");

    // Janela centralizada.
    let screen = c.harness.ctx.content_rect();
    let win = c.harness.ctx.memory(|m| m.area_rect(egui::Id::unique("prefs_window"))).expect("janela de preferências");
    assert!((win.center().x - screen.center().x).abs() < 2.0 && (win.center().y - screen.center().y).abs() < 2.0, "{win:?} não está no centro de {screen:?}");
}

#[test]
fn e2e_panel_sizes_are_remembered() {
    let mut c = setup("layout");
    let layout = c.harness.state().settings.layout.clone();
    assert!(layout.left.is_some() && layout.right.is_some(), "tamanhos dos painéis registrados: {layout:?}");
    // Um tamanho salvo vira o padrão do painel na próxima abertura.
    c.harness.state_mut().settings.layout.left = Some(321.0);
    c.harness.ctx.memory_mut(|m| *m = Default::default());
    c.harness.run_steps(3);
    assert_eq!(c.harness.state().settings.layout.left, Some(321.0));
}
