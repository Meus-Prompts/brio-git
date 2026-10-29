# Paridade com o cliente de referência — ledger de recursos

Fonte: análise do cliente git gráfico de referência (versão **12.5.0**, Electron) instalado em `/opt`
(Electron 41.3.0), feita com REA em 2026-10-28.

| Evidência | O que estabelece |
|---|---|
| `ev_ef67e1f8…` (REA `analyze_javascript_application`, app.asar sha256 `f10ad2ee…`) | Identidade do pacote: 41.449 arquivos, 32.426 fontes JS; entrypoint `src/main/static/main.bundle.js` |
| `ev_2bb67e46…` (REA `inspect_electron_fuses`) | `EnableNodeCliInspectArguments` desativado; runtime observado via `--remote-debugging-port` |
| `src/strings/en-us.json` (5.397 strings, 609 namespaces) | Inventário de recursos/menus/ações (observado, estático) |
| `src/keyBindings/shared.json` | Mapa de atalhos (observado, estático) |
| Captura CDP do renderer (dark theme) | Tokens de cor, alturas de toolbar/painéis, colunas do grafo (observado, runtime) |

> Este projeto é uma **implementação original** inspirada no comportamento e
> layout do cliente de referência. Nenhum código, ícone, fonte proprietária, logotipo ou
> asset do pacote é copiado. Os dados acima foram usados apenas como
> especificação funcional.

## Arquitetura observada no original

- UI: React + Redux/redux-saga + jotai; editor Monaco; terminal xterm.js + node-pty
- Git: `@axosoft/nodegit` (libgit2) com fallback para o executável `git`; `nodegit-flow` para Gitflow
- Watcher: `nsfw`; integrações: Octokit, GitBeaker, bitbucket-v2, azure-devops-node-api
- Processos: main, renderer, "Request Handler" (janela oculta) e workers

## Tokens de UI medidos (tema escuro)

- Fundo app `#1C1E23`; toolbar `rgb(42,45,52)`; painel `rgb(39,42,49)` / `rgb(50,54,63)` / `rgb(61,66,77)`
- Texto normal `rgba(255,255,255,.75)`, secundário `.6`, desabilitado `.4`; link `#40c5ec`; acento primário `#4D88FF`
- Linha hover `rgba(77,136,255,.1)`, selecionada `rgba(77,136,255,.2)`
- Paleta do grafo (lanes 0–9): `#15a0bf #0669f7 #8e00c2 #c517b6 #d90171 #cd0101 #f25d2e #f2ca33 #7bd938 #2ece9d`
- Diff: linha adicionada `rgba(92,184,92,.2)`, removida `rgba(217,65,61,.2)`; stats `#347d39` / `#c93c37`
- Conflito: esquerda `#15a0bf`, direita `#f2ca33`, saída `#c517b6`
- Toolbar 48px; cabeçalho do painel esquerdo 60px; linha do grafo ~28px
- Colunas do grafo: `BRANCH / TAG | GRAPH | COMMIT MESSAGE | AUTHOR | COMMIT DATE / TIME | SHA`

## Atalhos (keyBindings/shared.json — `command` = Ctrl no Linux)

Ctrl+B criar branch · Ctrl+/ atalhos · Ctrl+D diff externo · Ctrl+F buscar commits ·
Ctrl+Shift+O abrir repo · Ctrl+Alt+F filtrar painel esquerdo · Ctrl+Shift+P paleta ·
Ctrl+Z/Ctrl+Y undo/redo · ↑/↓ e j/k navegar · Shift+↑/↓ navegação topológica ·
Ctrl+Shift+S stage all · Ctrl+Shift+U unstage all · s/u stage/unstage ·
Ctrl+Shift+M foco na mensagem · Ctrl+L fetch · Ctrl+Shift+T reabrir aba ·
Ctrl+J painel esquerdo · Ctrl+K painel de detalhes · Ctrl+Enter commit ·
Ctrl+Shift+Enter stage all + commit · Ctrl+S salvar resolução de merge ·
Rebase interativo: p/r/d (pick/reword/drop)

## Checklist por fase

Legenda: `[x]` feito · `[~]` parcial · `[ ]` pendente

### Fase 1 — núcleo git
- [x] Abas de repositório, tela New Tab (Open / Clone com shallow / Init + recentes), reabrir aba fechada
- [x] Toolbar: Undo, Redo, Pull (ff / ff-only / rebase + Fetch All), Push (com prompt de upstream), Branch, Stash, Pop, Terminal, Search
- [x] Painel esquerdo: LOCAL (pastas `feat/`), REMOTE, TAGS, STASHES, SUBMODULES, WORKTREES; filtro; ahead/behind; ocultar/solo
- [x] Grafo: lanes com a paleta de 10 cores, labels local+remoto agrupadas, linha WIP tracejada até o HEAD, avatares de iniciais, nós de merge e stash, virtualização
- [x] Colunas configuráveis (branch/tag redimensionável, graph, message, author, date, sha) com descarte automático em janelas estreitas
- [x] Painel de detalhes: metadados, pais navegáveis, stats, lista em caminho ou árvore; comparação de 2 commits
- [x] Painel WIP: unstaged/staged, stage/unstage por arquivo, stage all, discard, ignore, summary + descrição, amend, push after commit, skip hooks
- [x] Diff: Hunk / Inline / Split; stage/unstage/discard por hunk **e por linha**
- [x] Menus de contexto: checkout, criar branch/tag (anotada), merge, rebase, fast-forward, cherry-pick (múltiplos), revert, reset soft/mixed/hard, delete local/remoto, rename, set upstream, copiar sha/nome, push/delete de tag
- [x] Stash: criar (com mensagem), apply, pop, delete; stashes no grafo
- [x] Conflitos: banner de operação em andamento, take current/incoming, ferramenta A/B/Output editável, abort/continue/skip
- [x] Rebase interativo (pick/reword/squash/fixup/drop, reordenar, atalhos p/r/s/f/d e Alt+↑/↓)
- [x] Busca de commits (Ctrl+F) e paleta de comandos (Ctrl+Shift+P)
- [x] Histórico de arquivo (`--follow`) e blame
- [x] Terminal integrado (PTY + emulação VT, 256 cores/truecolor, Nerd Font)
- [x] Watcher de arquivos + autofetch
- [x] Undo/redo (commit, checkout, reset, merge/rebase/pull, criar/apagar branch)
- [x] Preferências: perfil git global, limite de commits, autofetch, formato de data, pasta de clone, zoom
- [x] Prompt de credenciais na UI (usuário/senha HTTPS e passphrase SSH via `GIT_ASKPASS`/`SSH_ASKPASS`)
- [x] Drag-and-drop de branch sobre branch (merge, fast-forward, rebase, rebase interativo, push para remoto)

### Fase 2 — fluxo avançado
- [x] Gitflow (init, start/finish de feature/release/hotfix; compatível com a config do git-flow AVH)
- [x] Worktrees (adicionar, abrir em nova aba, lock/unlock, remover)
- [x] Submodules (adicionar, update recursivo, abrir em nova aba)
- [~] Git LFS (track/untrack, pull, install; seção só aparece com `git-lfs` instalado — não testado nesta máquina, que não tem git-lfs)
- [x] Assinatura de commits: selo verificada/não verificada/inválida (GPG, SSH, X.509) e preferências `commit.gpgsign`/`gpg.format`/`user.signingkey`
- [x] Solo/hide de branches
- [x] Perfis múltiplos (identidade por perfil sem alterar o gitconfig)
- [x] Tema claro e escuro
- [ ] Pin à esquerda, smart branch visibility
- [ ] Gerenciamento de chaves SSH (gerar e enviar para os provedores) e editor de git hooks
- [ ] Sparse checkout

### Fase 3 — integrações
- [x] GitHub (github.com e Enterprise) e GitLab (gitlab.com e self-hosted, namespaces aninhados) detectados pelo remote; hosts custom nas preferências
- [x] Autenticação: token no keyring do sistema, `GH_TOKEN`/`GITLAB_TOKEN`, ou reaproveitando `gh`/`glab` já logados; prompt de token na UI
- [x] Seções PULL REQUESTS / MERGE REQUESTS e GITHUB / GITLAB ISSUES no painel esquerdo
- [x] View de PR/MR: estado, origem → destino, descrição, comentários, comentar, merge (merge/squash/rebase; GitLab: merge/squash)
- [x] Criar PR/MR a partir de branch (com push automático), draft; "Push X and start a pull request"
- [x] Checkout de PR/MR (mesmo repo com upstream + fast-forward; forks via `pull/N/head` / `merge-requests/N/head`)
- [x] Issues: listar, ver, comentar, criar, criar branch a partir da issue
- [x] Fork no provedor (adiciona o remoto do fork), abrir/copiar link de commit e branch no remoto
- [ ] Bitbucket, Azure DevOps
- [ ] Jira, Trello
- [x] Status de CI: checks + statuses (GitHub), pipelines de MR/SHA e jobs (GitLab); bolinha na lista, resumo no PR, aba Checks, aviso ao fazer merge com CI falhando
- [x] Revisão de código: aba Arquivos com diff do PR/MR, comentários de revisão ancorados nas linhas e comentar numa linha
- [ ] Workspaces, Launchpad/Focus View

### Extras pedidos durante o desenvolvimento
- [x] Barra de título própria: minimizar (oculto em compositores tiling sem esse conceito), maximizar/restaurar, fechar, arrastar, duplo clique, redimensionar pelas bordas, F11
- [x] Terminal com várias abas no rodapé (botão na barra de status, Ctrl+` / Ctrl+Shift+`)
- [x] Pill de refs expande ao passar o mouse mostrando todos os branches/tags do commit
- [x] 15 temas (brio-git Dark/Light, Rosé Pine, Rosé Pine Moon/Dawn, Tokyo Night/Storm, Dracula, Catppuccin Mocha/Latte, Nord, Gruvbox Dark, One Dark, Solarized Dark, GitHub Dark) com lanes e ANSI por tema
- [x] Foto dos autores no grafo (GitHub noreply, GitLab conectado, Gravatar), com cache em disco

### Fase 4 — IA
- [ ] Geração de mensagem de commit, explicar commit/branch, resolução de conflito assistida, recompor commits
