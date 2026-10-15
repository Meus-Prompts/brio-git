# lazy-git

Cliente Git desktop (Rust + [iced](https://iced.rs)) focado em acompanhar uma IA trabalhando no seu repositório.

- **Grafo de commits** com avatar do autor em cada nó (Gravatar, ou iniciais quando não houver foto; passe o mouse para ver nome e e-mail), branches, remotos, tags e o hash (clique para copiar). A branch atual fica em destaque.
- **Detalhes do commit**: mensagem completa, autor, data e arquivos alterados em lista ou árvore de pastas.
- **Visualizador de arquivo**: diff lado a lado (split) ou unificado, com destaque de sintaxe e das palavras alteradas; "Changes only" mostra só os trechos modificados. Também tem **Blame** e **History** do arquivo.
- **Alterações ao vivo**: arquivos modificados e diff atualizam sozinhos (file watcher).
- **Sessão de IA**: terminal embutido que roda a IA instalada na máquina (`claude`, `codex`, `gemini`, `aider`, …) ou o seu shell, no diretório e na branch atuais.
- **Abas**: vários repositórios abertos ao mesmo tempo, cada um com sua própria sessão de IA. `+` abre o seletor de pasta.
- **Layout livre**: arraste as divisórias para redimensionar painéis; `⤢` maximiza e `✕` fecha um painel (reabra pela barra "Painéis"). Seções de branches/remotos/tags recolhem com um clique.
- **Temas**: 13 temas (Dark, Tokyo Night, Dracula, One Dark, Nord, Gruvbox, Solarized, Catppuccin, Rosé Pine…) em `◐ Tema`.
- **Checkpoints**: snapshots automáticos do working tree durante a sessão, em `refs/lazygit/checkpoints/<sessão>/<n>`, sem tocar na branch, no stage nem no stash. Cada um pode ser comparado e restaurado; restaurar salva antes o estado atual.

## Rodar

Requer Rust 1.88+ e `git` no `PATH`.

```sh
cargo run --release -- /repo/um /repo/dois   # uma aba por repositório
cargo run --release                          # sem argumentos: reabre as abas da última execução
```

As fotos do Gravatar ficam em cache em `~/.cache/lazy-git/avatars` (o app envia ao gravatar.com apenas o hash SHA-256 do e-mail).

Abas, layout e tema ficam salvos em `~/.config/lazy-git/state.json`. A janela usa barra de título própria (minimizar, maximizar, fechar, arrastar e redimensionar pelas bordas).

No terminal embutido: clique para focar, `Ctrl+Shift+V` (`Cmd+V` no macOS) cola, e a roda do mouse rola o histórico.

## Estrutura

| crate | papel |
|---|---|
| `crates/core` | leitura do repositório via gitoxide (refs, log) e layout do grafo |
| `crates/ops` | escrita via executável `git`: status, diff, checkpoints |
| `crates/session` | PTY + emulação VT (alacritty_terminal), watcher, agendador de checkpoints |
| `crates/app` | interface iced |

Para apagar checkpoints antigos: `git for-each-ref --format='delete %(refname)' refs/lazygit/ | git update-ref --stdin`.
