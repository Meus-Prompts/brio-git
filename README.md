# brio-git

Cliente Git desktop (Rust + [iced](https://iced.rs)) focado em acompanhar uma IA trabalhando no seu repositório.

- **Grafo de commits** com avatar do autor em cada nó (Gravatar, ou iniciais quando não houver foto; passe o mouse para ver nome e e-mail), branches, remotos, tags e o hash (clique para copiar). A branch atual fica em destaque.
- **Operações Git** por menu de contexto (botão direito no commit, na branch, na tag ou no stash): checkout (duplo clique no grafo também faz checkout), criar/renomear/apagar branch (local e no remoto), criar/apagar/enviar tag, cherry-pick, revert, reset soft/mixed/hard, merge, rebase, comparar revisões, abrir merge request (GitHub, GitLab, Bitbucket, Azure DevOps) e fixar uma branch à esquerda do grafo. Operações destrutivas pedem confirmação; checkout com alterações pendentes oferece guardar no stash.
- **Trocar ou criar branch** clicando no nome da branch atual na barra: busca por nome, ↑↓ e Enter; se o nome não existir, cria a branch a partir de HEAD.
- **Fetch / Pull / Push** na barra (passe o mouse para ver o que cada botão faz), com commits à frente/atrás do upstream; merge/rebase com conflito mostram uma faixa com Continuar e Abortar.
- **Commit**: arquivos fora do stage e em stage (stage, unstage, descartar), mensagem com resumo e descrição, **amend** e **mensagem gerada pela IA** escolhida (`claude`, `codex`, `gemini`…) a partir do diff em stage.
- **Detalhes do commit**: mensagem completa, autor, data e arquivos alterados em lista ou árvore de pastas.
- **Visualizador de arquivo**: diff lado a lado (split) ou unificado, com destaque de sintaxe e das palavras alteradas; "Changes only" mostra só os trechos modificados. Também tem **Blame** e **History** do arquivo.
- **Alterações ao vivo**: arquivos modificados e diff atualizam sozinhos (file watcher).
- **Sessão de IA**: terminal embutido que roda a IA instalada na máquina (`claude`, `codex`, `gemini`, `aider`, …) ou o seu shell, no diretório e na branch atuais.
- **Abas**: vários repositórios abertos ao mesmo tempo, cada um com sua própria sessão de IA. `+` abre o seletor de pasta.
- **Layout livre**: arraste qualquer painel pela barra de título e solte no centro de outro (troca os dois) ou numa borda (divide aquele espaço), montando o arranjo que quiser: 3 linhas, 2 linhas × 3 colunas… As divisórias redimensionam, e o arranjo fica salvo. Os ícones no rodapé (estilo Zed/VS Code) abrem e fecham cada painel, e o último ícone restaura o layout padrão. Seções de branches/remotos/tags recolhem com um clique.
- **Reflog**: painel com os últimos movimentos do HEAD (commit, checkout, reset, rebase…). Clique mostra o commit, mesmo os que nenhuma branch alcança mais; o botão direito abre o menu de commit (criar branch aqui, checkout, reset…) para recuperar trabalho perdido.
- **Alterações não commitadas** aparecem como uma linha tracejada no topo do grafo, ligada ao HEAD; clique para ver o working tree, botão direito para stage, stash ou descartar.
- **Temas**: 13 temas (Dark, Tokyo Night, Dracula, One Dark, Nord, Gruvbox, Solarized, Catppuccin, Rosé Pine…) em `◐ Tema`.

## Rodar

Requer Rust 1.88+ e `git` no `PATH`.

```sh
cargo run --release -- /repo/um /repo/dois   # uma aba por repositório
cargo run --release                          # sem argumentos: reabre as abas da última execução
```

As fotos do Gravatar ficam em cache em `~/.cache/brio-git/avatars` (o app envia ao gravatar.com apenas o hash SHA-256 do e-mail).

Abas, layout e tema ficam salvos em `~/.config/brio-git/state.json`. Os ícones usam a fonte [Lucide](https://lucide.dev) (ISC), embutida em `crates/app/assets`. A janela usa barra de título própria (minimizar, maximizar, fechar, arrastar e redimensionar pelas bordas).

No terminal embutido: clique para focar, `Ctrl+Shift+V` (`Cmd+V` no macOS) cola, e a roda do mouse rola o histórico.

### Desenvolvimento local

O `cargo run` recompila só o que mudou antes de abrir o app, então depois de editar o código basta fechar a janela e rodar de novo:

```sh
cargo run -- .              # build de debug: compila mais rápido, ideal enquanto você mexe no código
cargo run --release -- .    # build otimizado: mais lento para compilar, roda mais leve
```

Para recompilar e reabrir automaticamente a cada arquivo salvo, use o [`cargo-watch`](https://crates.io/crates/cargo-watch):

```sh
cargo install cargo-watch
cargo watch -x 'run -- .'
```

Antes de commitar:

```sh
cargo clippy --workspace
cargo test --workspace
```

Para usar a sua versão no dia a dia como um comando `brio-git` no `PATH` (`~/.cargo/bin`), instale a partir do código local e repita sempre que quiser atualizar:

```sh
cargo install --path crates/app --locked
brio-git /caminho/do/repo
```

## AppImage (Linux)

```sh
packaging/appimage/build.sh              # usa o toolchain da máquina (roda em distros com glibc igual ou mais nova)
packaging/appimage/build.sh --container  # compila no Debian bookworm via podman/docker (glibc 2.36, roda na maioria das distros)
```

O arquivo sai em `target/appimage/brio-git-<versão>-x86_64.AppImage`. O script baixa o `appimagetool` oficial para `target/appimage-tools` na primeira vez.

## Estrutura

| crate | papel |
|---|---|
| `crates/core` | leitura do repositório via gitoxide (refs, log) e layout do grafo |
| `crates/ops` | escrita via executável `git`: status, diff, ações |
| `crates/session` | PTY + emulação VT (alacritty_terminal), watcher, IA |
| `crates/app` | interface iced |

Para apagar checkpoints de versões anteriores: `git for-each-ref --format='delete %(refname)' refs/brio/ | git update-ref --stdin`.
