# brio-git

Cliente Git desktop (Rust + [iced](https://iced.rs)) focado em acompanhar uma IA trabalhando no seu repositório.

- **Grafo de commits** com avatar do autor em cada nó (Gravatar, ou iniciais quando não houver foto; passe o mouse para ver nome e e-mail), branches, remotos, tags e o hash (clique para copiar). A branch atual fica em destaque.
- **Operações Git** por menu de contexto (botão direito no commit, na branch, na tag ou no stash): checkout (duplo clique no grafo também faz checkout), criar/renomear/apagar branch (local e no remoto), criar/apagar/enviar tag, cherry-pick, revert, reset soft/mixed/hard, merge, rebase, comparar revisões, abrir merge request (GitHub, GitLab, Bitbucket, Azure DevOps) e fixar uma branch à esquerda do grafo. Operações destrutivas pedem confirmação; checkout com alterações pendentes oferece guardar no stash.
- **Fetch / Pull / Push** na barra, com commits à frente/atrás do upstream; merge/rebase com conflito mostram uma faixa com Continuar e Abortar.
- **Commit**: arquivos fora do stage e em stage (stage, unstage, descartar), mensagem com resumo e descrição, **amend** e **mensagem gerada pela IA** escolhida (`claude`, `codex`, `gemini`…) a partir do diff em stage.
- **Detalhes do commit**: mensagem completa, autor, data e arquivos alterados em lista ou árvore de pastas.
- **Visualizador de arquivo**: diff lado a lado (split) ou unificado, com destaque de sintaxe e das palavras alteradas; "Changes only" mostra só os trechos modificados. Também tem **Blame** e **History** do arquivo.
- **Alterações ao vivo**: arquivos modificados e diff atualizam sozinhos (file watcher).
- **Sessão de IA**: terminal embutido que roda a IA instalada na máquina (`claude`, `codex`, `gemini`, `aider`, …) ou o seu shell, no diretório e na branch atuais.
- **Abas**: vários repositórios abertos ao mesmo tempo, cada um com sua própria sessão de IA. `+` abre o seletor de pasta.
- **Layout livre**: arraste as divisórias para redimensionar painéis; `⤢` maximiza e `✕` fecha um painel (reabra pela barra "Painéis"). Seções de branches/remotos/tags recolhem com um clique.
- **Temas**: 13 temas (Dark, Tokyo Night, Dracula, One Dark, Nord, Gruvbox, Solarized, Catppuccin, Rosé Pine…) em `◐ Tema`.

## Rodar

Requer Rust 1.88+ e `git` no `PATH`.

```sh
cargo run --release -- /repo/um /repo/dois   # uma aba por repositório
cargo run --release                          # sem argumentos: reabre as abas da última execução
```

As fotos do Gravatar ficam em cache em `~/.cache/brio-git/avatars` (o app envia ao gravatar.com apenas o hash SHA-256 do e-mail).

Abas, layout e tema ficam salvos em `~/.config/brio-git/state.json`. A janela usa barra de título própria (minimizar, maximizar, fechar, arrastar e redimensionar pelas bordas).

No terminal embutido: clique para focar, `Ctrl+Shift+V` (`Cmd+V` no macOS) cola, e a roda do mouse rola o histórico.

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
