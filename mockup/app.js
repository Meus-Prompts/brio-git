// Monta a interface do mock a partir de data.js. Sem framework: cada render*
// reescreve o innerHTML do seu pedaço a partir de `state`.

const state = {
  theme: load("theme", "Dark"),
  selection: "wip", // "wip" ou o id de um commit
  openFile: null,
  treeView: false,
  collapsed: new Set(),
  activeTab: 0,
  activeSession: 0,
  hidden: new Set(load("hidden", [])),
  maximized: null,
  termFocused: false,
  settings: null, // seção aberta nas configurações
  // Estilo do layout, independente do tema de cores.
  layout: load("layout", load("minimal", false) ? "minimal" : "default"),
};

const $ = (sel, root = document) => root.querySelector(sel);
const icon = (name, extra = "") => `<span class="ic ${extra}">${ICONS[name]}</span>`;
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

function load(key, fallback) {
  try { return JSON.parse(localStorage.getItem("brio-mock:" + key)) ?? fallback; } catch { return fallback; }
}
function save(key, value) {
  try { localStorage.setItem("brio-mock:" + key, JSON.stringify(value)); } catch {}
}

// ---------- Tema ----------

function applyTheme(name) {
  const theme = THEMES.find((t) => t.name === name) ?? THEMES[0];
  state.theme = theme.name;
  for (const [k, v] of Object.entries(theme.colors)) document.documentElement.style.setProperty("--" + k, v);
  save("theme", theme.name);
}

const LANES = ["accent", "warn", "purple", "added", "removed", "hunk", "orange", "muted"];
const laneColor = (i) => `var(--${LANES[i % LANES.length]})`;

// ---------- Barra de título e ferramentas ----------

function renderTitlebar() {
  $("#repo-tabs").innerHTML =
    REPO_TABS.map((t, i) => `
      <div class="repo-tab ${i === state.activeTab ? "active" : ""}" data-tab="${i}">
        <button class="name">${t.running ? `<span class="dot ${t.working ? "working" : ""}">● </span>` : ""}${esc(t.name)}</button>
        <button class="x">✕</button>
      </div>`).join("") + `<button class="add-tab" title="Abrir repositório">+</button>`;
  $("#repo-tabs").querySelectorAll("[data-tab]").forEach((el) =>
    el.querySelector(".name").addEventListener("click", () => { state.activeTab = +el.dataset.tab; renderTitlebar(); }));
}

function renderToolbar() {
  const tool = (ic, label, count = 0, disabled = false) =>
    `<button class="ghost" ${disabled ? "disabled" : ""}>${icon(ic)}${label}${count ? `<span class="pill">${count}</span>` : ""}</button>`;
  $("#toolbar").innerHTML = `
    <button class="ghost icon-btn" title="Desfazer: commit (Ctrl+Z)">${icon("UNDO")}</button>
    <button class="ghost icon-btn" disabled title="Nada para refazer">${icon("REDO")}</button>
    <span class="divider"></span>
    <button class="chip" title="Trocar de branch ou criar uma nova">${icon("GIT_BRANCH")}${HEAD}${icon("CHEVRON_DOWN")}</button>
    <button class="ghost icon-btn" title="Recarregar">${icon("REFRESH")}</button>
    <span class="divider"></span>
    ${tool("FETCH", "Fetch")}
    ${tool("PULL", "Pull")}
    ${tool("PUSH", "Push", 1)}
    <span class="divider"></span>
    ${tool("STASH", "Stash")}
    ${tool("POP", "Pop")}`;
}

// ---------- Painéis ----------

function renderPanes() {
  document.querySelectorAll(".pane").forEach((pane) => {
    const kind = pane.dataset.kind;
    const { title, icon: ic } = PANES[kind];
    const max = state.maximized === kind;
    pane.innerHTML = `
      <div class="pane-title">
        ${icon("GRIP", "grip")}${icon(ic)}<span>${title}</span>
        <div class="controls">
          <button data-act="max" title="${max ? "Restaurar" : "Maximizar"}">${icon(max ? "RESTORE" : "MAXIMIZE")}</button>
          <button data-act="close" title="Fechar">${icon("CLOSE")}</button>
        </div>
      </div>
      <div class="pane-body"></div>`;
    pane.classList.toggle("maximized", max);
    pane.querySelector('[data-act="max"]').onclick = () => { state.maximized = max ? null : kind; renderPanes(); };
    pane.querySelector('[data-act="close"]').onclick = () => togglePane(kind);
  });
  renderSidebar();
  renderReflog();
  renderGraph();
  renderDetails();
  renderSession();
}

const body = (kind) => $(`.pane[data-kind="${kind}"] .pane-body`);

function togglePane(kind) {
  state.hidden.has(kind) ? state.hidden.delete(kind) : state.hidden.add(kind);
  if (state.maximized === kind) state.maximized = null;
  save("hidden", [...state.hidden]);
  layoutVisibility();
  renderStatusbar();
}

// Um split some quando todos os seus painéis estão fechados; os divisores ficam só entre filhos visíveis.
function layoutVisibility() {
  document.querySelectorAll(".pane").forEach((p) => (p.hidden = state.hidden.has(p.dataset.kind)));
  const visit = (split) => {
    const kids = [...split.children].filter((c) => !c.classList.contains("gutter"));
    kids.filter((k) => k.classList.contains("split")).forEach(visit);
    split.querySelectorAll(":scope > .gutter").forEach((g) => g.remove());
    const shown = kids.filter((k) => (k.classList.contains("split") ? !k.hidden : !k.hidden));
    shown.slice(1).forEach((k) => split.insertBefore(makeGutter(), k));
    split.hidden = shown.length === 0;
  };
  visit($("#root"));
  $("#root").hidden = false;
}

function makeGutter() {
  const g = document.createElement("div");
  g.className = "gutter";
  g.addEventListener("pointerdown", (e) => {
    const split = g.parentElement;
    const horizontal = split.classList.contains("cols");
    const size = (el) => (horizontal ? el.getBoundingClientRect().width : el.getBoundingClientRect().height);
    const kids = [...split.children].filter((c) => !c.classList.contains("gutter") && !c.hidden);
    // Fixa os tamanhos atuais como pesos, para só os dois vizinhos mudarem.
    kids.forEach((k) => (k.style.flex = `${size(k)} 1 0`));
    let prev = g.previousElementSibling, next = g.nextElementSibling;
    while (prev?.hidden) prev = prev.previousElementSibling;
    while (next?.hidden) next = next.nextElementSibling;
    const start = horizontal ? e.clientX : e.clientY;
    const a = size(prev), b = size(next);
    g.classList.add("dragging");
    g.setPointerCapture(e.pointerId);
    const move = (ev) => {
      const d = Math.max(-a + 60, Math.min(b - 60, (horizontal ? ev.clientX : ev.clientY) - start));
      prev.style.flex = `${a + d} 1 0`;
      next.style.flex = `${b - d} 1 0`;
    };
    const up = () => { g.classList.remove("dragging"); g.removeEventListener("pointermove", move); g.removeEventListener("pointerup", up); };
    g.addEventListener("pointermove", move);
    g.addEventListener("pointerup", up);
  });
  return g;
}

// ---------- Branches ----------

function renderSidebar() {
  let html = "";
  for (const [title, refs] of Object.entries(REFS)) {
    const collapsed = state.collapsed.has(title);
    html += `<button class="section-head" data-section="${title}">${collapsed ? "▸" : "▾"} ${title}  (${refs.length})</button>`;
    if (!collapsed) {
      for (const r of refs) {
        const head = title === "BRANCHES" && r === HEAD;
        html += `<button class="ref ${head ? "head" : ""}">${head ? "✔ " : ""}${esc(r)}</button>`;
      }
      html += `<div class="spacer8"></div>`;
    }
  }
  const collapsed = state.collapsed.has("STASHES");
  html += `<button class="section-head" data-section="STASHES">${collapsed ? "▸" : "▾"} STASHES  (${STASHES.length})</button>`;
  if (!collapsed) {
    html += STASHES.map((s) => `<button class="stash">${esc(s.message.replace(/^On /, ""))}<small>${s.name} · ${s.ago}</small></button>`).join("");
  }
  const el = body("sidebar");
  el.innerHTML = `<div class="refs">${html}</div>`;
  el.querySelectorAll("[data-section]").forEach((b) =>
    (b.onclick = () => {
      const s = b.dataset.section;
      state.collapsed.has(s) ? state.collapsed.delete(s) : state.collapsed.add(s);
      renderSidebar();
    }));
}

// ---------- Reflog ----------

function renderReflog() {
  const color = (action) =>
    ({ commit: "added", merge: "added", "cherry-pick": "added", checkout: "accent", switch: "accent", reset: "removed", rebase: "purple", pull: "purple" })[action] ?? "muted";
  const el = body("reflog");
  el.innerHTML = `<div class="reflog">${REFLOG.map((e) => `
    <button class="reflog-item ${state.selection.startsWith(e.id) ? "sel" : ""}" data-id="${e.id}">
      <div class="reflog-head">
        <span class="mono muted">${e.selector}</span>
        <span class="mono" style="color: var(--accent)">${e.id}</span>
        <span class="pill" style="--c: var(--${color(e.action)})">${e.action}</span>
        <span class="when">${e.ago}</span>
      </div>
      <div class="reflog-msg">${esc(e.message)}</div>
    </button>`).join("")}</div>`;
  el.querySelectorAll("[data-id]").forEach((b) =>
    (b.onclick = () => {
      const c = COMMITS.find((c) => c.id.startsWith(b.dataset.id));
      if (c) select(c.id);
    }));
}

// ---------- Grafo ----------

// Medidas do modo normal; o minimalista as troca em setGraphMetrics.
let ROW_H = 30, LANE_W = 26, LANE_X0 = 16, AVATAR_R = 10, STROKE = 2;
// Largura da coluna de hash, à esquerda do grafo.
const HASH_W = 76;
const laneX = (l) => LANE_X0 + l * LANE_W;
const midY = (row) => row * ROW_H + ROW_H / 2;

function authorColor(key) {
  const palette = ["#5b8def", "#d9730d", "#8b5cf6", "#0f9d58", "#d6336c", "#0891b2"];
  return palette[[...key].reduce((a, c) => a + c.charCodeAt(0), 0) % palette.length];
}
const initials = (name) => name.split(/\s+/).map((w) => w[0]).slice(0, 2).join("").toUpperCase();

function setGraphMetrics() {
  [ROW_H, LANE_W, LANE_X0, AVATAR_R, STROKE] = state.layout === "minimal" ? [26, 16, 12, 4, 1.5] : [30, 26, 16, 10, 2];
}

function renderGraph() {
  setGraphMetrics();
  const rowOf = new Map(COMMITS.map((c, i) => [c.id, i + 1])); // linha 0 = alterações não commitadas
  const lanes = Math.max(...COMMITS.map((c) => c.lane)) + 1;
  const graphW = LANE_X0 + (lanes - 1) * LANE_W + 22;
  const height = (COMMITS.length + 1) * ROW_H;

  let paths = "";
  const line = (d, lane, dashed = false) =>
    (paths += `<path d="${d}" style="stroke:${laneColor(lane)}" stroke-width="${STROKE}" fill="none" ${dashed ? 'stroke-dasharray="3 3"' : ""}/>`);

  COMMITS.forEach((c, i) => {
    const row = i + 1;
    c.parents.forEach((pid, n) => {
      const p = COMMITS[rowOf.get(pid) - 1];
      const prow = rowOf.get(pid);
      const [x1, y1, x2, y2] = [laneX(c.lane), midY(row), laneX(p.lane), midY(prow)];
      if (c.lane === p.lane) return line(`M${x1},${y1} L${x2},${y2}`, c.lane);
      if (n === 0) {
        // A branch desce na própria coluna e curva para o pai na última linha.
        const yc = midY(prow - 1);
        return line(`M${x1},${y1} L${x1},${yc} C${x1},${(yc + y2) / 2} ${x2},${(yc + y2) / 2} ${x2},${y2}`, c.lane);
      }
      // Merge: sai do commit curvando para a coluna do segundo pai e desce.
      const yc = midY(row + 1);
      line(`M${x1},${y1} C${x1},${(y1 + yc) / 2} ${x2},${(y1 + yc) / 2} ${x2},${yc} L${x2},${y2}`, p.lane);
    });
  });

  // Alterações não commitadas: nó tracejado na coluna do HEAD, ligado a ele.
  const head = COMMITS.findIndex((c) => c.refs?.some((r) => r.current));
  const hl = COMMITS[head].lane;
  line(`M${laneX(hl)},${midY(0) + AVATAR_R + 1} L${laneX(hl)},${midY(head + 1) - AVATAR_R - 1}`, hl, true);
  paths += `<circle cx="${laneX(hl)}" cy="${midY(0)}" r="${AVATAR_R}" fill="none" style="stroke:${laneColor(hl)}" stroke-width="${STROKE}" stroke-dasharray="${state.layout === "minimal" ? "2 2" : "3 3"}"/>`;

  COMMITS.forEach((c, i) => {
    const [x, y] = [laneX(c.lane), midY(i + 1)];
    const a = AUTHORS[c.author];
    const isHead = i === head;
    if (state.layout === "minimal") {
      // Só um ponto na cor da branch; o HEAD ganha um anel.
      paths += `<circle cx="${x}" cy="${y}" r="${AVATAR_R}" style="fill:${laneColor(c.lane)}"/>`;
      if (isHead) paths += `<circle cx="${x}" cy="${y}" r="${AVATAR_R + 3}" fill="none" style="stroke:${laneColor(c.lane)}" stroke-width="1.5"/>`;
      return;
    }
    paths += `<circle cx="${x}" cy="${y}" r="${AVATAR_R + 1}" style="fill:var(--panel);stroke:${laneColor(c.lane)}" stroke-width="${isHead ? 3 : 2}"/>`;
    paths += `<circle cx="${x}" cy="${y}" r="${AVATAR_R - 1}" fill="${authorColor(c.author)}"/>`;
    paths += `<text x="${x}" y="${y + 3.5}" text-anchor="middle" font-size="9" font-weight="600" fill="#fff">${initials(a.name)}</text>`;
  });

  const monitor = `<svg width="11" height="10" viewBox="0 0 11 10" style="flex:none"><rect x="0.5" y="0.5" width="10" height="7" rx="1" fill="none" stroke="currentColor"/><path d="M3 9.5h5" stroke="currentColor"/></svg>`;
  const badge = (r, lane) => {
    const c = r.tag ? "var(--warn)" : r.local ? laneColor(lane) : "var(--muted)";
    const icons = `${r.local ? monitor : ""}${r.remote ? icon("CLOUD") : ""}`;
    return `<span class="badge ${r.current ? "current" : ""}" style="--c:${c}">${r.current ? "✔ " : ""}${esc(r.name)}${icons}</span>`;
  };

  const rows = COMMITS.map((c, i) => {
    const refs = c.refs ?? [];
    const badges = refs.length ? badge(refs[0], c.lane) + (refs.length > 1 ? `<span class="badge more">+${refs.length - 1}</span>` : "") : "";
    return `<div class="g-row ${i === head ? "head" : ""} ${state.selection === c.id ? "sel" : ""}" data-id="${c.id}">
      <span class="hash">${c.id.slice(0, 7)}</span><span class="lanes" style="width:${graphW}px"></span>
      ${badges}<span class="summary">${esc(c.summary)}</span><span class="date">${c.ago}</span><span class="who" title="${esc(AUTHORS[c.author].email)}">${esc(AUTHORS[c.author].name)}</span>
    </div>`;
  }).join("");

  const el = body("graph");
  el.innerHTML = `
    <div class="graph" style="height:${height}px">
      <svg width="${graphW}" height="${height}" style="left:${HASH_W}px">${paths}</svg>
      <div class="g-row wip ${state.selection === "wip" ? "sel" : ""}" data-id="wip">
        <span class="hash"></span><span class="lanes" style="width:${graphW}px"></span>
        <span class="ic" style="margin-right:7px">${ICONS.PENCIL}</span>
        <span class="summary">Alterações não commitadas · ${CHANGES.length} arquivos</span>
      </div>
      ${rows}
    </div>`;
  el.querySelectorAll(".g-row").forEach((r) => (r.onclick = () => select(r.dataset.id)));
}

function select(id) {
  state.selection = id;
  renderGraph();
  renderReflog();
  renderDetails();
}

// ---------- Alterações ----------

const KIND_COLOR = { M: "accent", R: "accent", A: "added", U: "added", D: "removed", C: "warn" };

function fileList(title, files, mode) {
  const bulk = mode === "unstaged" ? "Stage all" : mode === "staged" ? "Unstage all" : null;
  const modes = mode !== "staged"
    ? `<button class="mode ${state.treeView ? "" : "on"}" data-tree="0">Lista</button><button class="mode ${state.treeView ? "on" : ""}" data-tree="1">Árvore</button>`
    : "";
  let html = `<div class="list-head"><span class="title">${title}</span>${bulk && files.length ? `<button class="secondary" style="font-size:11px;padding:2px 8px">${bulk}</button>` : ""}${modes}</div>`;
  if (!files.length) return html + `<div class="muted">Nenhum arquivo alterado.</div>`;

  const entry = (f, label, depth = 0) => `
    <div class="file">
      <button class="text-btn ${state.openFile === f.path ? "sel" : ""}" data-file="${esc(f.path)}" style="padding-left:${6 + depth * 14}px">
        <span class="letter" style="color:var(--${KIND_COLOR[f.kind]})">${f.kind}</span><span class="path">${esc(label)}</span>
      </button>
      ${mode === "unstaged" ? `<button class="act" title="Descartar">⟲</button><button class="act" title="Stage">+</button>` : ""}
      ${mode === "staged" ? `<button class="act" title="Unstage">−</button>` : ""}
    </div>`;

  if (state.treeView && mode !== "staged") {
    // Agrupa por pasta, um nível só: o bastante para ver o visual.
    const dirs = {};
    files.forEach((f) => { const i = f.path.lastIndexOf("/"); (dirs[f.path.slice(0, i + 1)] ??= []).push(f); });
    for (const [dir, fs] of Object.entries(dirs)) {
      if (dir) html += `<button class="text-btn muted" style="width:100%">▾ ${esc(dir)}</button>`;
      html += fs.map((f) => entry(f, f.path.slice(dir.length), dir ? 1 : 0)).join("");
    }
    return html;
  }
  return html + files.map((f) => entry(f, f.path)).join("");
}

function renderDetails() {
  const el = body("details");
  const worktree = state.selection === "wip";
  let content = `<button class="secondary ${worktree ? "on" : ""}" style="font-size:13px;align-self:flex-start" id="show-wt">Working tree (${CHANGES.length})</button>`;

  if (worktree) {
    const unstaged = CHANGES.filter((c) => !c.staged), staged = CHANGES.filter((c) => c.staged);
    content += fileList(`FORA DO STAGE (${unstaged.length})`, unstaged, "unstaged");
    content += fileList(`EM STAGE (${staged.length})`, staged, "staged");
    el.innerHTML = `
      <div class="details-wrap">
        <div class="pane-scroll"><div class="details">${content}</div></div>
        <div class="commit-box bar">
          <span class="label">MENSAGEM DO COMMIT</span>
          <input class="input" placeholder="Resumo (obrigatório)">
          <textarea class="textarea" placeholder="Descrição (opcional)"></textarea>
          <div class="row">
            <label class="check"><input type="checkbox"> Amend</label>
            <span class="grow"></span>
            <button class="secondary">✨ Gerar com claude</button>
          </div>
          <button class="primary" disabled>Commit</button>
        </div>
      </div>`;
    const input = el.querySelector(".input"), btn = el.querySelector(".primary");
    input.oninput = () => (btn.disabled = !input.value.trim());
    btn.onclick = () => toast(`Commit criado: ${input.value.trim()}`);
  } else {
    const c = COMMITS.find((c) => c.id === state.selection);
    const a = AUTHORS[c.author];
    // Varia a lista por commit para não parecer tudo igual.
    const n = 2 + (parseInt(c.id.slice(0, 2), 16) % (COMMIT_FILES.length - 1));
    const files = COMMIT_FILES.slice(0, n);
    content += `
      <div class="commit-header">
        <button class="hash" title="Copiar hash">commit: ${c.id.slice(0, 7)}</button>
        <h2>${esc(c.summary)}</h2>
        <div class="author">
          <span class="avatar" style="background:${authorColor(c.author)}">${initials(a.name)}</span>
          <div>${esc(a.name)}<small>${esc(a.email)}</small><small>authored ${c.ago}</small></div>
        </div>
      </div>
      ${fileList(`${files.length} FILE(S) CHANGED`, files, "plain")}`;
    el.innerHTML = `<div class="details">${content}</div>`;
    el.querySelector(".hash").onclick = () => toast(`Hash ${c.id.slice(0, 10)} copiado.`);
  }

  el.querySelector("#show-wt").onclick = () => select("wip");
  el.querySelectorAll("[data-tree]").forEach((b) => (b.onclick = () => { state.treeView = b.dataset.tree === "1"; renderDetails(); }));
  el.querySelectorAll("[data-file]").forEach((b) => (b.onclick = () => { state.openFile = b.dataset.file; renderDetails(); }));
}

// ---------- Sessão de IA ----------

function renderSession() {
  const s = SESSIONS[state.activeSession];
  const status = {
    working: `${s.tool} trabalhando em ⎇ ${HEAD}`,
    waiting: `${s.tool} aguardando em ⎇ ${HEAD} — clique no terminal para digitar`,
    exited: `${s.tool} encerrado (código ${s.code})`,
  }[s.state];
  const el = body("session");
  el.innerHTML = `
    <div class="session">
      <div class="session-bar">
        <div class="session-tabs">${SESSIONS.map((t, i) => `
          <div class="s-tab ${i === state.activeSession ? "active" : ""}" data-s="${i}">
            <span class="dot ${t.state}"></span><span>${esc(t.name)}</span><button class="x">✕</button>
          </div>`).join("")}
        </div>
        <div class="session-actions">
          <select class="select" title="IA usada nas próximas sessões"><option>claude</option><option>codex</option><option>gemini</option><option>terminal</option></select>
          <button class="soft icon-only" data-tip="Nova sessão: abre outra aba com a IA escolhida, no diretório e na branch atuais">${icon("PLAY")}</button>
          <button class="soft icon-only" style="--c: var(--removed)" ${s.state === "exited" ? "disabled" : ""}
            data-tip="${s.state === "exited" ? "Sessão já encerrada" : `Parar: encerra ${esc(s.name)}; a saída continua visível para copiar`}">${icon("STOP")}</button>
        </div>
      </div>
      <div class="session-info">
        <span class="grow">${status}</span>
      </div>
      <pre class="terminal ${state.termFocused ? "focused" : ""}" tabindex="0">${TERMINAL_LINES.join("\n")}</pre>
    </div>`;
  el.querySelectorAll("[data-s]").forEach((t) => (t.onclick = () => { state.activeSession = +t.dataset.s; renderSession(); }));
  const term = el.querySelector(".terminal");
  term.onfocus = () => term.classList.add("focused");
  term.onblur = () => term.classList.remove("focused");
}

// ---------- Rodapé ----------

function renderStatusbar() {
  const toggles = Object.entries(PANES).map(([kind, p]) => {
    const open = !state.hidden.has(kind);
    return `<button class="toggle ${open ? "open" : ""}" data-pane="${kind}" title="${p.title} (${open ? "fechar" : "abrir"})">${icon(p.icon)}</button>`;
  }).join("");
  const working = SESSIONS.filter((s) => s.state === "working").length;
  $("#statusbar").innerHTML = `
    <div class="toggles">${toggles}<span class="divider"></span>
      <button class="toggle" id="reset-layout" title="Restaurar o layout padrão dos painéis">${icon("LAYOUT")}</button>
    </div>
    <span class="grow"></span>
    <span class="item">${icon("UPSTREAM")} origin/main</span>
    <button class="counter item" title="Sessões de IA abertas em todos os repositórios">${icon("SPARKLES")} ${SESSIONS.length} sessões${working ? ` · ${working} trabalhando` : ""}</button>
    <span class="version" title="brio-git v0.4.0 · commit 7a3b9e6">v0.4.0</span>`;
  $("#statusbar").querySelectorAll("[data-pane]").forEach((b) => (b.onclick = () => togglePane(b.dataset.pane)));
  $("#reset-layout").onclick = () => {
    state.hidden.clear();
    state.maximized = null;
    save("hidden", []);
    location.reload();
  };
}

// ---------- Configurações ----------

function renderSettings() {
  const overlay = $("#overlay");
  if (!state.settings) return (overlay.innerHTML = "");
  const nav = [["appearance", "PALETTE", "Aparência"], ["ai", "SPARKLES", "Commit com IA"], ["updates", "REFRESH", "Atualizações"]]
    .map(([id, ic, label]) => `<button class="${state.settings === id ? "on" : ""}" data-sec="${id}">${icon(ic)} ${label}</button>`).join("");
  const sections = {
    appearance: `<h3>Estilo</h3><div class="layouts">${Object.entries(LAYOUTS).map(([id, label]) =>
      `<button class="secondary ${state.layout === id ? "on" : ""}" data-layout="${id}">${label}</button>`).join("")}</div>
      <h3>Tema</h3><div class="themes">${THEMES.map((t) => {
      const c = t.colors;
      return `<button class="theme-card ${t.name === state.theme ? "on" : ""}" data-theme="${t.name}">
        <div class="theme-preview" style="background:${c.bg};border-color:${c.border}">
          <div class="side" style="background:${c.title}"></div>
          <div class="lines"><i style="width:72px;background:${c.text}"></i><i style="width:52px;background:${c.muted}"></i><i style="width:62px;background:${c.muted}"></i><i style="width:18px;margin-top:4px;background:${c.accent}"></i></div>
        </div>${t.name}</button>`;
    }).join("")}</div>`,
    ai: `<h3>Prompt da mensagem de commit</h3><p class="muted">Instrução enviada à IA escolhida na Sessão de IA ao clicar em gerar mensagem.</p><textarea class="textarea" style="height:200px">Escreva uma mensagem de commit no padrão Conventional Commits…</textarea>`,
    updates: `<h3>Atualizações</h3><p>Versão instalada: v0.4.0</p><label class="check"><input type="checkbox" checked> Atualizar automaticamente</label><p class="muted">Ao abrir e a cada 6 horas, o brio-git procura uma versão nova no GitHub.</p><button class="secondary">${icon("REFRESH")} Procurar agora</button>`,
  };
  overlay.innerHTML = `
    <div class="backdrop">
      <div class="modal">
        <div class="modal-head"><span class="grow">Configurações</span><button class="ghost icon-btn" id="close-settings">${icon("CLOSE")}</button></div>
        <div class="modal-main"><nav class="modal-nav">${nav}</nav><div class="modal-content">${sections[state.settings]}</div></div>
      </div>
    </div>`;
  overlay.querySelector(".backdrop").onclick = (e) => { if (e.target === e.currentTarget) closeSettings(); };
  $("#close-settings").onclick = closeSettings;
  overlay.querySelectorAll("[data-sec]").forEach((b) => (b.onclick = () => { state.settings = b.dataset.sec; renderSettings(); }));
  overlay.querySelectorAll("[data-layout]").forEach((b) => (b.onclick = () => setLayout(b.dataset.layout)));
  overlay.querySelectorAll("[data-theme]").forEach((b) => (b.onclick = () => { applyTheme(b.dataset.theme); renderSettings(); }));
}
function closeSettings() { state.settings = null; renderSettings(); }

// ---------- Toast ----------

let toastTimer;
function toast(msg) {
  document.querySelector(".toast")?.remove();
  const t = document.createElement("div");
  t.className = "toast";
  t.textContent = msg;
  document.body.append(t);
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.remove(), 2500);
}

// ---------- Início ----------

const LAYOUTS = { default: "Padrão", minimal: "Minimalista" };

function setLayout(name) {
  if (!(name in LAYOUTS)) name = "default"; // estilo salvo que não existe mais
  state.layout = name;
  document.body.classList.toggle("minimal", name === "minimal");
  save("layout", name);
  renderGraph();
  if (state.settings) renderSettings();
}

function cycleLayout() {
  const names = Object.keys(LAYOUTS);
  setLayout(names[(names.indexOf(state.layout) + 1) % names.length]);
  toast(`Estilo: ${LAYOUTS[state.layout]}`);
}

applyTheme(state.theme);
renderTitlebar();
renderToolbar();
renderPanes();
renderStatusbar();
layoutVisibility();
setLayout(state.layout);
document.querySelectorAll("[data-icon]").forEach((el) => (el.textContent = ICONS[el.dataset.icon]));
$("#toggle-layout").onclick = cycleLayout;
$("#open-settings").onclick = () => { state.settings = "appearance"; renderSettings(); };
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeSettings();
  // Alt+M alterna entre o estilo padrão e o minimalista.
  if (e.altKey && e.key.toLowerCase() === "m") cycleLayout();
  // Atalho para testar temas rápido: Alt+T passa para o próximo.
  if (e.altKey && e.key.toLowerCase() === "t") {
    const i = THEMES.findIndex((t) => t.name === state.theme);
    applyTheme(THEMES[(i + 1) % THEMES.length].name);
    toast(`Tema: ${state.theme}`);
    if (state.settings) renderSettings();
  }
});
