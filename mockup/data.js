// Dados fictícios e temas. Os temas são cópia de crates/app/src/style.rs.

const THEMES = [
  ["Dark", "1b1e24 21252c 262b33 15171c 333944 d7dae0 8a93a3 4fb3ff 2c3a4f 98c379 e06c75 56b6c2 e5c07b c678dd d19a66 17191e"],
  ["Tokyo Night", "1a1b26 1f2335 24283b 16161e 292e42 c0caf5 7982a9 7aa2f7 2e3c64 9ece6a f7768e 7dcfff e0af68 bb9af7 ff9e64 16161e"],
  ["Dracula", "282a36 2b2d3a 343746 21222c 44475a f8f8f2 8a8fb0 bd93f9 44475a 50fa7b ff5555 8be9fd f1fa8c ff79c6 ffb86c 21222c"],
  ["One Dark", "282c34 2c313a 323842 21252b 3e4451 abb2bf 7f848e 61afef 3e4451 98c379 e06c75 56b6c2 e5c07b c678dd d19a66 21252b"],
  ["Nord", "2e3440 3b4252 434c5e 292e39 4c566a eceff4 9aa5b8 88c0d0 4c566a a3be8c bf616a 81a1c1 ebcb8b b48ead d08770 2e3440"],
  ["Gruvbox Dark", "282828 32302f 3c3836 1d2021 504945 ebdbb2 a89984 83a598 504945 b8bb26 fb4934 8ec07c fabd2f d3869b fe8019 1d2021"],
  ["Solarized Dark", "002b36 073642 0a3f4c 00212b 1c4f5c 93a1a1 657b83 268bd2 0f4a5a 859900 dc322f 2aa198 b58900 6c71c4 cb4b16 00212b"],
  ["Catppuccin Mocha", "1e1e2e 181825 313244 11111b 45475a cdd6f4 9399b2 89b4fa 3b3d55 a6e3a1 f38ba8 94e2d5 f9e2af cba6f7 fab387 11111b"],
  ["Rosé Pine", "191724 1f1d2e 26233a 16141f 403d52 e0def4 908caa c4a7e7 393552 9ccfd8 eb6f92 ebbcba f6c177 c4a7e7 ebbcba 16141f"],
  ["Light", "f5f6f8 ffffff eef0f3 e9ebef d5d9e0 24292f 6e7781 0969da dbeafe 1a7f37 cf222e 0e7490 9a6700 8250df bc4c00 ffffff"],
  ["Catppuccin Latte", "eff1f5 e6e9ef dce0e8 dce0e8 ccd0da 4c4f69 7c7f93 1e66f5 ccd0da 40a02b d20f39 179299 df8e1d 8839ef fe640b eff1f5"],
  ["Rosé Pine Dawn", "faf4ed fffaf3 f2e9e1 f2e9e1 dfdad9 575279 797593 907aa9 e4dcd5 56949f b4637a d7827e ea9d34 907aa9 d7827e fffaf3"],
  ["Solarized Light", "fdf6e3 eee8d5 e9e2cb eee8d5 d6cfb8 586e75 93a1a1 268bd2 e3dcc4 859900 dc322f 2aa198 b58900 6c71c4 cb4b16 fdf6e3"],
].map(([name, hex]) => {
  const keys = ["bg", "panel", "panel-alt", "title", "border", "text", "muted", "accent", "selected", "added", "removed", "hunk", "warn", "purple", "orange", "term-bg"];
  const values = hex.split(" ");
  return { name, colors: Object.fromEntries(keys.map((k, i) => [k, "#" + values[i]])) };
});

// Códigos da fonte lucide (crates/app/src/icons.rs).
const ICONS = {
  GIT_BRANCH: "", GIT_GRAPH: "", FILE_DIFF: "", SPARKLES: "", HISTORY: "",
  REFRESH: "", FETCH: "", PULL: "", PUSH: "", STASH: "", POP: "",
  PENCIL: "", UPSTREAM: "", LOADER: "", CLOSE: "", MAXIMIZE: "",
  RESTORE: "", LAYOUT: "", PLAY: "", STOP: "", CHEVRON_DOWN: "",
  CHECK: "", SEARCH: "", BRANCH_PLUS: "", CLOUD: "", GRIP: "",
  SETTINGS: "", PALETTE: "", RESET: "", SUCCESS: "", ALERT: "",
  UNDO: "", REDO: "",
};

const PANES = {
  sidebar: { title: "Branches", icon: "GIT_BRANCH" },
  reflog: { title: "Reflog", icon: "HISTORY" },
  graph: { title: "Grafo", icon: "GIT_GRAPH" },
  details: { title: "Alterações", icon: "FILE_DIFF" },
  session: { title: "Sessão de IA", icon: "SPARKLES" },
};

const REPO_TABS = [
  { name: "brio-git", running: true, working: true },
  { name: "trevo-web", running: true, working: false },
  { name: "dotfiles", running: false },
];

const AUTHORS = {
  gilson: { name: "Gilson Terra", email: "gilson@example.com" },
  ana: { name: "Ana Souza", email: "ana@example.com" },
  bot: { name: "github-actions", email: "bot@example.com" },
};

// Commits do mais novo ao mais antigo. `lane` é a coluna no grafo; `parents` são ids.
// refs: { name, local, remote, current, tag }
const COMMITS = [
  { id: "7a3b9e6c41", lane: 0, parents: ["dbdfee1a02"], author: "gilson", ago: "agora", refs: [{ name: "main", local: true, current: true }],
    summary: "feat(session): selecionar e copiar em sessões encerradas" },
  { id: "e19c0d4b77", lane: 1, parents: ["5b2f8e1d90"], author: "ana", ago: "1 h", refs: [{ name: "feat/ui-nova", local: true, remote: true }],
    summary: "wip: novo cabeçalho do grafo" },
  { id: "dbdfee1a02", lane: 0, parents: ["4248b24f13"], author: "gilson", ago: "2 h", refs: [{ name: "main", remote: true }, { name: "v0.4.0", tag: true }],
    summary: "feat(session): selecionar e copiar texto do terminal" },
  { id: "5b2f8e1d90", lane: 1, parents: ["4248b24f13"], author: "ana", ago: "3 h",
    summary: "style(app): testa espaçamento maior nos painéis" },
  { id: "4248b24f13", lane: 0, parents: ["a1c3e5f700"], author: "gilson", ago: "ontem", refs: [{ name: "v0.3.0", tag: true }],
    summary: "feat(app): atualização automática a partir das releases do GitHub" },
  { id: "a1c3e5f700", lane: 0, parents: ["6308c2f9aa", "debde5f311"], author: "gilson", ago: "ontem",
    summary: "Merge branch 'feat/abas-sessao'" },
  { id: "6308c2f9aa", lane: 0, parents: ["a36f19d4e2"], author: "gilson", ago: "2 dias",
    summary: "ci: fixa actions de terceiros por SHA e atualiza as oficiais" },
  { id: "debde5f311", lane: 1, parents: ["c0ffee1234"], author: "gilson", ago: "2 dias", refs: [{ name: "feat/abas-sessao", remote: true }],
    summary: "feat(session): várias sessões de IA em abas, com estado e total no rodapé" },
  { id: "c0ffee1234", lane: 1, parents: ["a36f19d4e2"], author: "gilson", ago: "3 dias",
    summary: "refactor(session): extrai Session de workspace.rs" },
  { id: "a36f19d4e2", lane: 0, parents: ["9c2aa1f5b8"], author: "bot", ago: "3 dias", refs: [{ name: "v0.2.0", tag: true }],
    summary: "ci: build e release automáticos para Linux, macOS e Windows" },
  { id: "9c2aa1f5b8", lane: 0, parents: ["8d1e2f3a4b"], author: "gilson", ago: "4 dias",
    summary: "feat(app): desfazer/refazer, toasts, configurações e versão no rodapé" },
  { id: "8d1e2f3a4b", lane: 0, parents: ["7e6d5c4b3a", "f1e2d3c4b5"], author: "ana", ago: "5 dias",
    summary: "Merge branch 'fix/pty-resize'" },
  { id: "7e6d5c4b3a", lane: 0, parents: ["6a5b4c3d2e"], author: "gilson", ago: "5 dias",
    summary: "feat(graph): badges de refs com ícone local e remoto" },
  { id: "f1e2d3c4b5", lane: 2, parents: ["6a5b4c3d2e"], author: "ana", ago: "6 dias",
    summary: "fix(pty): redimensiona o terminal junto com o painel" },
  { id: "6a5b4c3d2e", lane: 0, parents: ["5f4e3d2c1b"], author: "gilson", ago: "1 sem", refs: [{ name: "v0.1.1", tag: true }],
    summary: "fix(ops): push sem upstream configura o remoto" },
  { id: "5f4e3d2c1b", lane: 0, parents: ["4e3d2c1b0a"], author: "gilson", ago: "1 sem",
    summary: "feat(details): visualização em árvore dos arquivos" },
  { id: "4e3d2c1b0a", lane: 0, parents: ["3d2c1b0a99"], author: "gilson", ago: "2 sem",
    summary: "feat(app): layout de painéis arrastáveis e redimensionáveis" },
  { id: "3d2c1b0a99", lane: 0, parents: [], author: "gilson", ago: "2 sem", refs: [{ name: "v0.1.0", tag: true }],
    summary: "chore: projeto inicial" },
];

const REFS = {
  BRANCHES: ["main", "feat/ui-nova", "feat/abas-sessao-local"],
  REMOTOS: ["origin/main", "origin/feat/ui-nova", "origin/feat/abas-sessao"],
  TAGS: ["v0.4.0", "v0.3.0", "v0.2.0", "v0.1.1", "v0.1.0"],
};
const HEAD = "main";

const STASHES = [
  { message: "WIP on main: experimento de cores", name: "stash@{0}", ago: "há 3 h" },
  { message: "WIP on feat/ui-nova: header", name: "stash@{1}", ago: "há 2 dias" },
];

const REFLOG = [
  { selector: "HEAD@{0}", id: "7a3b9e6", action: "commit", message: "feat(session): selecionar e copiar em sessões encerradas", ago: "agora" },
  { selector: "HEAD@{1}", id: "dbdfee1", action: "commit", message: "feat(session): selecionar e copiar texto do terminal", ago: "2 h" },
  { selector: "HEAD@{2}", id: "4248b24", action: "checkout", message: "moving from feat/ui-nova to main", ago: "3 h" },
  { selector: "HEAD@{3}", id: "5b2f8e1", action: "commit", message: "style(app): testa espaçamento maior nos painéis", ago: "3 h" },
  { selector: "HEAD@{4}", id: "4248b24", action: "checkout", message: "moving from main to feat/ui-nova", ago: "4 h" },
  { selector: "HEAD@{5}", id: "4248b24", action: "pull", message: "pull: Fast-forward", ago: "ontem" },
  { selector: "HEAD@{6}", id: "a1c3e5f", action: "merge", message: "merge feat/abas-sessao: Merge made by 'ort'", ago: "ontem" },
  { selector: "HEAD@{7}", id: "6308c2f", action: "reset", message: "reset: moving to HEAD~1", ago: "2 dias" },
  { selector: "HEAD@{8}", id: "a36f19d", action: "rebase", message: "rebase (finish): returning to refs/heads/main", ago: "3 dias" },
];

// Working tree: kind = M (modificado) A (adicionado) D (removido) R (renomeado) U (não rastreado) C (conflito)
const CHANGES = [
  { path: "crates/app/src/view.rs", kind: "M", staged: true },
  { path: "crates/app/src/style.rs", kind: "M", staged: true },
  { path: "mockup/index.html", kind: "U", staged: false },
  { path: "mockup/styles.css", kind: "U", staged: false },
  { path: "crates/app/src/widgets/graph.rs", kind: "M", staged: false },
  { path: "docs/old-notes.md", kind: "D", staged: false },
];

// Arquivos alterados de um commit qualquer (o mock repete a mesma lista com variações).
const COMMIT_FILES = [
  { path: "crates/session/src/pty.rs", kind: "M" },
  { path: "crates/app/src/widgets/terminal.rs", kind: "M" },
  { path: "crates/app/src/workspace.rs", kind: "M" },
  { path: "crates/app/src/keys.rs", kind: "M" },
  { path: "crates/session/src/lib.rs", kind: "M" },
  { path: "docs/selecao.md", kind: "A" },
];

const SESSIONS = [
  { name: "claude", tool: "claude", state: "waiting" },
  { name: "claude 2", tool: "claude", state: "working" },
  { name: "terminal", tool: "terminal", state: "exited", code: 0 },
];

// Saída fictícia do terminal (classes: t-dim, t-acc, t-ok, t-warn, t-err, t-bold).
const TERMINAL_LINES = [
  `<span class="t-acc">╭───────────────────────────────────────────────╮</span>`,
  `<span class="t-acc">│</span> <span class="t-bold">✻ Welcome to Claude Code!</span>                     <span class="t-acc">│</span>`,
  `<span class="t-acc">│</span>   <span class="t-dim">cwd: ~/Development/brio-git</span>                 <span class="t-acc">│</span>`,
  `<span class="t-acc">╰───────────────────────────────────────────────╯</span>`,
  ``,
  `<span class="t-dim">&gt;</span> permitir seleção em sessões encerradas também`,
  ``,
  `<span class="t-ok">●</span> O caminho mais simples é parar de descartar o Terminal quando a sessão termina.`,
  ``,
  `<span class="t-ok">●</span> <span class="t-bold">Update</span>(crates/app/src/session.rs)`,
  `  <span class="t-dim">⎿</span>  Updated <span class="t-bold">crates/app/src/session.rs</span> with 3 additions and 4 removals`,
  `      <span class="t-err">-        if let Some(t) = self.terminal.take() {</span>`,
  `      <span class="t-ok">+        self.refresh();</span>`,
  ``,
  `<span class="t-ok">●</span> <span class="t-bold">Bash</span>(cargo test --workspace)`,
  `  <span class="t-dim">⎿</span>  <span class="t-ok">cargo test: 105 passed</span>, 1 ignored (7 suites, 3.28s)`,
  ``,
  `<span class="t-ok">●</span> Agora dá para selecionar e copiar o texto de sessões já encerradas.`,
  ``,
  `<span class="t-dim">╭──────────────────────────────────────────────────────────────╮</span>`,
  `<span class="t-dim">│</span> &gt; <span class="cursor"> </span>                                                           <span class="t-dim">│</span>`,
  `<span class="t-dim">╰──────────────────────────────────────────────────────────────╯</span>`,
  `  <span class="t-dim">? for shortcuts</span>`,
];
