# rtk obrigatório em todos os lugares — Mustard self-consistency

### Parent: [[2026-05-20-mustard-wave-network-standard]] (sister)
### Status: completed
### Phase: CLOSE
### Scope: full
### Checkpoint: 2026-05-20T22:00:00Z
### Lang: pt

## PRD

## Contexto

A Golden Rule global do operador diz **"ALWAYS prefix Bash commands with rtk"**. Mustard implementa via `bash_guard.rs::rtk_rewrite` (blanket-prefix em runtime), mas tem três buracos visíveis que contradizem a regra:

1. **`apps/cli/templates/settings.json` wires hooks como `"command": "mustard-rt on PreToolUse"`** — sem `rtk`. O próprio mecanismo de enforcement não segue a regra que prega. PreToolUse não intercepta suas próprias chamadas (chicken-and-egg) — precisa ser prefixado explicitamente no template.

2. **`bash_guard` rewrites silenciosamente em `Verdict::Rewrite`** — agente não percebe, não aprende; UI do Claude Code mostra o comando original (pre-rewrite). Resultado: parece que o enforcement está desligado mesmo quando funciona, e o agente repete o erro toda sessão.

3. **Subprocess internos do `mustard-rt`** (git diff em `run/diff_context.rs`, builds em `hooks/bash_guard.rs::run_build`, etc.) também rodam sem `rtk`, contradizendo o princípio.

A decisão do operador (2026-05-20): rtk vira **dependência obrigatória** do Mustard — sem rtk, Mustard não instala. Com rtk, usado em todos os lugares. Isso elimina o bug latente de "blanket-prefix sem checar se rtk existe" e força auto-consistência.

## Métrica de sucesso

- `mustard init` falha early (exit 1) se `rtk --version` falhar, com link de instalação.
- Hook PreToolUse retorna `Deny` (default strict) para comandos sem `rtk`, com mensagem clara — agente reenvia com `rtk <cmd>` e UI mostra ambos. Visibilidade direta.
- Templates `apps/cli/templates/settings.json` geram hooks `"command": "rtk mustard-rt on <Event>"`.
- Helper `rtk_command(program, args)` em `mustard-core` e ≥2 call sites usando.

## Não-Objetivos

- Não internalizar a lógica do `rtk` no `mustard-rt` (rtk continua dependência externa).
- Não refatorar todos os subprocess do mustard-rt nesta spec — apenas o helper + ≥2 call sites como demonstração; o restante pode vir em sub-spec futura.
- Não tocar `apps/dashboard` (sem relação).

## Acceptance Criteria

- [x] AC-1: Todos os hooks em `apps/cli/templates/settings.json` prefixam `rtk mustard-rt` — Command: `bash -c "out=$(grep -E '\"command\": \"mustard-rt on' apps/cli/templates/settings.json); test -z \"$out\""`
- [x] AC-2: `bash_guard` tem fn `rtk_gate_mode()` lendo `MUSTARD_RTK_GATE_MODE` — Command: `bash -c "grep -q 'fn rtk_gate_mode' apps/rt/src/hooks/bash_guard.rs"`
- [x] AC-3: `mustard init` invoca rtk probe — Command: `bash -c "grep -E 'rtk.*--version|check_rtk|probe_rtk' apps/cli/src/commands/init.rs > /dev/null"`
- [x] AC-4: Helper `rtk_command` existe em mustard-core — Command: `bash -c "grep -r 'pub fn rtk_command' packages/core/src/ > /dev/null"`
- [x] AC-5: ≥2 call sites usam `rtk_command` em `apps/rt/src/run/` ou `apps/rt/src/hooks/` — Command: `node -e "const out=require('child_process').execSync('grep -r rtk_command apps/rt/src/').toString();const lines=out.split('\n').filter(l=>l.includes('rtk_command(')).length;if(lines<2)throw new Error('only '+lines+' call sites')"`
- [x] AC-6: Cargo check passa nos 3 crates — Command: `cargo check -p mustard-rt -p mustard-cli -p mustard-core`
- [x] AC-7: Testes do `bash_guard` passam (incluindo novo strict mode) — Command: `cargo test -p mustard-rt bash_guard`

## Arquivos (~7)

```
apps/cli/templates/settings.json                       (modify — prefix rtk em todos hooks)
apps/cli/src/commands/init.rs                          (modify — probe rtk antes de copiar templates)
apps/rt/src/hooks/bash_guard.rs                        (modify — rtk_gate_mode + Deny strict path)
packages/core/src/process/mod.rs                       (new ou modify — barrel)
packages/core/src/process/rtk_command.rs               (new — helper)
packages/core/src/lib.rs                               (modify — re-export `process::rtk_command`)
apps/rt/src/run/diff_context.rs                        (modify — usar rtk_command no git diff)
apps/rt/src/hooks/bash_guard.rs::run_build             (modify — usar rtk_command na shell-out de build) [já listado acima]
```

## Tarefas

### General Agent (rt + cli + core)

**Helper rtk_command em mustard-core:**

- [ ] Em `packages/core/src/process/rtk_command.rs` (new):
  - `pub fn rtk_command(program: &str, args: &[&str]) -> std::process::Command`
  - Se `program == "rtk"` → `Command::new("rtk").args(args)` (no double-prefix)
  - Senão → `Command::new("rtk").arg(program).args(args)`
  - Doc-comment explicando Golden Rule
  - 3 testes unitários: (a) program=rtk não duplica, (b) program=git prefixa, (c) args preservados
- [ ] Em `packages/core/src/process/mod.rs` (new): `pub mod rtk_command; pub use rtk_command::rtk_command;`
- [ ] Em `packages/core/src/lib.rs`: adicionar `pub mod process;`

**bash_guard strict mode:**

- [ ] Em `apps/rt/src/hooks/bash_guard.rs`:
  - Adicionar `fn rtk_gate_mode() -> Mode` lendo `MUSTARD_RTK_GATE_MODE`. Default `Mode::Strict`. Parse via `Mode::parse`. Unknown → `Strict`.
  - Em `rtk_rewrite_with`, antes do specific path: ler `rtk_gate_mode()`.
    - `Mode::Off` → return None (mantém Allow).
    - `Mode::Warn` → comportamento atual (Rewrite blanket/specific).
    - `Mode::Strict` → quando o command original sem `rtk` é eligible (passa `should_blanket_prefix`), retornar `Verdict::Deny` com reason:
      ```
      [bash_guard rtk] Comando sem prefixo `rtk` — Mustard exige rtk em todo Bash.
      Reenvie como: rtk <comando>
      Comando original: <cmd truncated 120>
      ```
  - Adicionar 3 testes: `rtk_strict_denies_unprefixed`, `rtk_warn_rewrites_unprefixed`, `rtk_off_allows_unprefixed`. Cada um seta env var via `std::env::set_var` em escopo de teste (note: o crate é `#![forbid(unsafe_code)]` — usar `temp_env::with_var` se disponível, ou redesign: passar Mode como parâmetro pra função interna testável). **Preferência: refatorar `rtk_rewrite_with` pra receber `mode: Mode` parâmetro injetável.**

**Refator call sites (≥2):**

- [ ] Em `apps/rt/src/run/diff_context.rs`: substituir `Command::new("git")` por `rtk_command("git", ...)`. Importar de `mustard_core::process::rtk_command`.
- [ ] Em `apps/rt/src/hooks/bash_guard.rs::run_build`: idem para o shell-out (`cmd /C build_cmd` ou `sh -c build_cmd` — manter shell wrapper, mas envolver via rtk_command quando aplicável). **Nota cuidado:** `run_build` usa `cmd /C <cmd>` que executa string complexa; melhor deixar como está e ESCOLHER outro call site mais simples. **Alternativa:** em `apps/rt/src/run/diff_context.rs` use rtk_command em 2 lugares diferentes (git diff, git log), satisfazendo AC-5.

**Init probe rtk:**

- [ ] Em `apps/cli/src/commands/init.rs`: no entry point (antes de copiar templates), adicionar probe:
  ```rust
  fn probe_rtk() -> Result<(), String> {
      let status = std::process::Command::new("rtk").arg("--version").output();
      match status {
          Ok(out) if out.status.success() => Ok(()),
          _ => Err("RTK is a required dependency. Install: https://github.com/your/rtk (or check $PATH for `rtk`).".into()),
      }
  }
  ```
  - Chamar `probe_rtk()` no início do `init` (após argparse, antes de qualquer Write). Se Err, eprintln e `std::process::exit(1)`.

**settings.json template:**

- [ ] Em `apps/cli/templates/settings.json`: substituir TODOS os 8 hooks (PreToolUse, PostToolUse, SessionStart, PreCompact, SessionEnd, SubagentStart, SubagentStop, UserPromptSubmit) — trocar `"command": "mustard-rt on <Event>"` por `"command": "rtk mustard-rt on <Event>"`. Manter `statusLine.command` como `mustard-rt run statusline` (statusline não é Bash hook, é statusline; rtk não filtra).
  - Decisão consciente: statusLine fica sem rtk pra não adicionar latência ao prompt visual. Justificar inline no spec não, mas em comentário se houver.

**Validação:**

- [ ] `cargo check -p mustard-rt -p mustard-cli -p mustard-core`
- [ ] `cargo test -p mustard-rt bash_guard`
- [ ] Após build, **NÃO reinstalar nesta task** — o orquestrador faz isso na fase pós-EXECUTE.

## Dependências

Nenhuma — independente da Wave 3 de `wave-network-standard`. Files completamente diferentes.

## Network

- Parent (sister): [[2026-05-20-mustard-wave-network-standard]]
- Padrão precedente (sub-spec tática linkada): [[2026-05-20-tactical-fix-via-sub-spec]]
- Resolve memory: [[bash_guard_blanket_rtk]] (precedente: 2026-05-20 01:53)

## Limites

ESCOPO:
  apps/cli/templates/settings.json
  apps/cli/src/commands/init.rs
  apps/rt/src/hooks/bash_guard.rs
  packages/core/src/process/{mod,rtk_command}.rs
  packages/core/src/lib.rs
  apps/rt/src/run/diff_context.rs

OUT-OF-BOUNDS:
  apps/dashboard/**           (sem relação)
  apps/rt/src/hooks/* (exceto bash_guard.rs)
  apps/cli/src/commands/{update,config,add,review,git_flow}.rs
  Internalizar rtk dentro do mustard-rt (próxima spec)
  Refatorar TODOS os subprocess (próxima spec)
