# brio-git

Cliente git gráfico nativo em Rust com [egui](https://github.com/emilk/egui):
grafo de commits, staging por hunk e linha, merge de conflitos, rebase
interativo, gitflow, terminal em abas, GitHub e GitLab (PR/MR, issues, CI e
revisão), 15 temas e atualização automática.

> A partir da v0.6.0 o brio-git usa egui. A versão anterior, em iced, está no
> histórico deste branch (até a v0.5.1). Quem tinha a versão em iced recebe
> esta pela atualização automática, mantendo abas, tema e a preferência de
> atualização (`~/.config/brio-git/state.json` é importado na primeira execução).

## Instalar

Baixe da página de [Releases](https://github.com/Meus-Prompts/brio-git/releases):

- **Linux**: `brio-git-<versão>-x86_64.AppImage` (dê permissão de execução) ou o `.tar.gz` com o binário.
- **macOS** (Apple Silicon e Intel): descompacte o `.zip` e mova `brio-git.app` para Aplicativos. O app não é assinado: na primeira vez, clique com o botão direito > Abrir.
- **WSL**: dentro do WSL, use o `.tar.gz` — a janela abre via WSLg.
- **Windows**: descompacte o `.zip` e rode `brio-git.exe`. Repositórios do WSL
  (`\\wsl$\<distro>\...` ou `\\wsl.localhost\...`) funcionam: o git e o
  terminal rodam dentro da distro, com as credenciais e chaves SSH dela.

Requer `git` no `PATH`.

### Atualização automática

Ao abrir e a cada 6 h, o app procura a release `vX.Y.Z` mais nova, baixa o
arquivo do seu sistema (AppImage, binário Linux, `brio-git.app` ou `.exe`), confere o SHA-256 e
instala em segundo plano. O botão **Reiniciar para vX.Y.Z** aparece no rodapé.
Desligue em Preferências > Atualizações. Builds locais (`dev`) não se atualizam.

Cada push no `main` publica uma release (versão pelos Conventional Commits:
`feat:` → minor, `fix:` → patch, `!`/`BREAKING CHANGE` → major), com AppImage,
`.tar.gz`, `.app` do macOS e `.zip` do Windows — ver `.github/workflows/release.yml`.

## Rodar

```bash
cargo run --release                     # restaura as abas da última sessão
cargo run --release -- ~/caminho/repo   # abre um repositório direto
```

As mutações usam o executável `git`, herdando hooks, assinatura GPG/SSH e
credential helpers.

### Pacotes locais

```bash
packaging/appimage/build.sh --container   # AppImage (Debian bookworm, glibc 2.36)
# .exe do Windows a partir do Linux (contêiner com mingw-w64):
podman run --rm -v "$PWD":/src:Z -w /src -e CARGO_TARGET_DIR=/src/target/win docker.io/library/rust:1-bookworm \
  bash -c 'apt-get update -qq && apt-get install -y -qq gcc-mingw-w64-x86-64 && rustup target add x86_64-pc-windows-gnu && cargo build --release --target x86_64-pc-windows-gnu'
```

## Arquitetura

| Camada | Arquivo | Papel |
|---|---|---|
| Leitura | `src/git/loader.rs` | Snapshot completo via libgit2 (`git2`) numa thread de background |
| Grafo | `src/git/graph.rs` | Layout em lanes; cada linha guarda as arestas do intervalo acima dela |
| Diff | `src/git/diff.rs` | Diffs, patches parciais (stage por linha), blame, histórico |
| Escrita | `src/git/ops.rs`, `cli.rs` | Mutações e rede via `git` CLI |
| Estado | `src/repo_tab.rs` | Seleção, ações diferidas, jobs, undo/redo, watcher |
| UI | `src/ui/*`, `src/app.rs` | egui/eframe (GPU), painéis, grafo virtualizado, terminal |

Fluxo: widgets empilham `Action`s → `RepoTab::handle` dispara jobs em threads →
ao terminar, o snapshot é recarregado e a UI troca o `Arc<Snapshot>` sem piscar.

## Testes

```bash
cargo test                 # unitários + ponta-a-ponta (egui_kittest)
```

Os testes e2e sobem o app inteiro contra repositórios git reais, clicam na UI
e conferem o estado do git (stage de hunk, commit, undo/redo, merge com
conflito, rebase interativo, terminal). Os snapshots ficam em `target/e2e/`.

Benchmark de carga:

```bash
BRIO_GIT_BENCH_REPO=/caminho/repo cargo test --release bench_load -- --ignored --nocapture
```

## Atalhos principais

`Ctrl+Shift+P` paleta · `Ctrl+Shift+O` abrir repo · `Ctrl+T`/`Ctrl+W` abas ·
`Ctrl+F` buscar commits · `Ctrl+L` fetch · `Ctrl+B` branch ·
`Ctrl+Shift+S`/`U` stage/unstage all · `Ctrl+Enter` commit ·
`Ctrl+J`/`Ctrl+K` painéis · ``Ctrl+` `` terminal · `Ctrl+Z`/`Ctrl+Y` undo/redo ·
`↑↓`/`j k` navegar no grafo · `Esc` voltar ao grafo
