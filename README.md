# orkai

Um terminal para orquestrar vários agentes de IA (Claude Code, Codex) e terminais comuns,
em vários projetos ao mesmo tempo, organizados em sessões. Binário único, nativo, sem runtime,
para Windows, Linux e macOS.

![orkai overview](docs/overview.png)

## Instalação

Requer o toolchain Rust (1.85+).

```sh
git clone https://github.com/lucasmarujo/orkai-cli
cd orkai-cli
cargo install --path .
```

O binário vai para `~/.cargo/bin/orkai` (Windows: `%USERPROFILE%\.cargo\bin\orkai.exe`).
Depois é só digitar `orkai` no PowerShell, Bash ou zsh.

Para usar agentes de IA, `claude` e/ou `codex` precisam estar no `PATH`.

## Uso

```
orkai
```

| Área | O que mostra |
| --- | --- |
| Lateral | máquina, sessões com o número de agentes ativos em cada uma, `+ new session` |
| Topo | o homenzinho pixelado e os contadores `agents`, `working`, `paused` e `done` da sessão |
| Lista | agentes agrupados por pasta (com a contagem de cada uma): número, estado, nome, tempo no estado atual, tempo de vida, tipo (`claude`, `codex`, `terminal`) e a frase do que o agente está fazendo |
| Rodapé | `write a task`, `pc`, `who` e `where` para lançar novos agentes |

Estados: `⠋` trabalhando · `‖` pausado esperando sua aprovação · `✔` terminou (ainda não visto) ·
`●` ocioso · `$` terminal · `↻` interrompido (crash/reboot) · `✗` encerrado.

### Teclas

| Tecla | Ação |
| --- | --- |
| digitar + `enter` | inicia um agente com a tarefa digitada |
| `tab` / `shift+tab` | escolhe quem executa: claude, codex ou terminal |
| `ctrl+l` | escolhe a pasta (`tab` completa diretórios); clicar no nome de um projeto também |
| `↑` `↓` / mouse | seleciona; `enter` ou clique abre o agente numa aba |
| `ctrl+]` | sai da aba do agente e volta para a visão geral |
| `shift+pgup` / roda do mouse | histórico do agente |
| `alt+1..9` | pula para uma aba aberta |
| `ctrl+e` / `f2` | renomeia o agente |
| `ctrl+x` | encerra um agente em execução / remove um encerrado |
| `ctrl+r` | recupera os agentes interrompidos da sessão |
| `ctrl+n` · `alt+↑↓` | nova sessão · troca de sessão |
| `/` | comandos: `/cd`, `/who`, `/session`, `/new-session`, `/rename-session`, `/delete-session`, `/rename`, `/kill`, `/restart`, `/recover`, `/open`, `/help`, `/quit` |
| `ctrl+c` / `ctrl+q` | sai do orkai; os agentes continuam rodando |

Dentro da aba de um agente todas as teclas vão para ele (Claude Code, Codex, PowerShell, bash…),
inclusive `esc`, setas, colagem e mouse quando o programa usa mouse.

### Linha de comando

```
orkai ls [-s sessão]             lista agentes com o estado ao vivo
orkai new -k claude -C <pasta> <tarefa...>
orkai send <id> <texto...>       digita no agente e aperta enter
orkai attach <id>                usa o terminal atual direto no agente (ctrl+] solta)
orkai logs <id> [-n linhas]      transcrição gravada do agente
orkai kill <id> · orkai rm <id>
orkai recover [-s sessão]        relança agentes interrompidos retomando a conversa
orkai sessions · orkai home
```

## Persistência e recuperação

Cada agente roda dentro de um processo `holder` próprio e destacado, dono do pseudo-terminal
(ConPTY no Windows, PTY no Unix). A interface só se conecta a eles. Por isso:

- **Fechar, reiniciar ou atualizar o orkai não derruba nenhum agente.** Ao abrir de novo, ele
  reconecta em todos. O holder roda a partir de uma cópia do binário em `bin/`, então dá para
  recompilar e reinstalar o `orkai` com agentes rodando.
- **Se a interface travar**, os agentes seguem intactos.
- **Se um holder morrer ou a máquina reiniciar**, o agente aparece como `↻ interrupted`.
  `ctrl+r` (ou `orkai recover`) relança todos os agentes interrompidos da sessão, cada um na
  sua pasta: o Claude Code volta com `--resume <id da conversa>` (o orkai inicia cada Claude
  com um `--session-id` conhecido), o Codex com `codex resume <id>` e os terminais reexecutam
  o comando original.
- Toda a saída de cada agente é gravada (com rotação) em `agents/<id>/output.log`. Abrir um
  agente encerrado ou interrompido mostra a última tela, e `orkai logs <id>` imprime a transcrição.

Os dados ficam em `orkai home` (Windows: `%LOCALAPPDATA%\orkai`, Linux:
`~/.local/share/orkai`, macOS: `~/Library/Application Support/orkai`). A variável
`ORKAI_HOME` muda esse local.

## Configuração

`config.json` dentro da pasta de dados (criado na primeira execução):

```json
{
  "shell": null,
  "shell_args": [],
  "claude_command": "claude",
  "claude_args": [],
  "codex_command": "codex",
  "codex_args": []
}
```

`shell` nulo usa `pwsh`/`powershell` no Windows e `$SHELL` no Unix.
Exemplo: `"claude_args": ["--model", "opus"]`.

## Segurança

Os holders escutam somente em `127.0.0.1`, numa porta aleatória, e exigem um token de 256
bits por agente, que fica apenas na pasta de dados do usuário (permissões `0700`/`0600` no Unix).
Comandos `.cmd`/`.bat` no Windows nunca recebem texto livre como argumento: a tarefa é digitada
no terminal do agente, não passada pela linha de comando.

## Desenvolvimento

```sh
cargo test        # unitários + e2e com PTY real (holder e a própria TUI)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
