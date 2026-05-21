# rtk_rewrite dual-mode coverage — testes cobrem Warn (rewrite) e Strict (deny)

### Status: completed
### Phase: CLOSE
### Scope: light
### Checkpoint: 2026-05-21T01:35:00Z
### Lang: pt
### Parent: 2026-05-20-rtk-mandatory-everywhere

## Contexto

Tactical fix derivado de [[2026-05-20-rtk-mandatory-everywhere]] (e relacionado a [[2026-05-20-restore-rtk-rewrite]], dona dos testes afetados). Descoberto durante a validação final da spec [[2026-05-20-tactical-fix-via-sub-spec]]: os testes `rtk_rewrite_e2e_rewrites_unprefixed_command` e `rtk_rewrite_emission` (em `apps/rt/tests/rtk_rewrite_emission.rs`, criados pela `restore-rtk-rewrite`) falham porque exercitam `Mode::Warn` → `Verdict::Rewrite` com `updatedInput.command`, mas a `rtk-mandatory-everywhere` mudou o default do gate para `Mode::Strict` → `Verdict::Deny`. Os testes não setam env explícita, então herdam Strict e batem contra o shape antigo.

Os dois modos são código de produção:

- **`Mode::Strict`** (default atual): comandos sem rtk recebem deny + mensagem "Reenvie como: rtk …". UX assertiva, alinha Golden Rule global do user.
- **`Mode::Warn`** (opt-out via `MUSTARD_BASH_REDIRECT_MODE=warn`): comandos sem rtk recebem rewrite transparente via `Verdict::Rewrite`. Path histórico preservado para usuários que querem rewriting silencioso.

A fix é dual coverage: helper `run_hook_with_mode(mode, cmd)` parametrizado, cada AC vira par (warn + strict), cada um valida o shape do seu modo. Honra a existência dos dois paths e fecha o buraco que `rtk-mandatory-everywhere` deixou ao mudar default sem atualizar a suite irmã.

## Critérios de Aceitação

- [x] AC-1: Suite `mustard-rt` passa — Command: `cargo test -p mustard-rt --test rtk_rewrite_emission`
- [x] AC-2: Existe assert em modo Warn que checa `updatedInput.command` começa com `rtk ` — Command: `node -e "const c=require('fs').readFileSync('apps/rt/tests/rtk_rewrite_emission.rs','utf8');process.exit((c.includes('MUSTARD_BASH_REDIRECT_MODE')&&c.includes('warn')&&c.includes('updatedInput'))?0:1)"`
- [x] AC-3: Existe assert em modo Strict que checa deny + "Reenvie como: rtk" — Command: `node -e "const c=require('fs').readFileSync('apps/rt/tests/rtk_rewrite_emission.rs','utf8');process.exit((c.includes('strict')&&c.includes('Reenvie como'))?0:1)"`
- [x] AC-4: Suite workspace inteira passa (modulo dashboard) — Command: `cargo test --workspace --exclude mustard-dashboard`

## Arquivos

```
apps/rt/tests/rtk_rewrite_emission.rs    — helper run_hook_with_mode + paridade warn/strict por AC
```

## Tarefas

- [x] Adicionar helper `run_hook_with_mode(tmp, command, mode: &str) -> (db, stdout)`: igual aos atuais `run_hook`/`run_hook_capture`, mas com `.env("MUSTARD_BASH_REDIRECT_MODE", mode)` antes do `.spawn()`. Refatorar `run_hook` e `run_hook_capture` para serem wrappers que chamam `run_hook_with_mode(..., "warn")` — preserva o sentido histórico (rewrite shape) sob nome antigo.
- [x] Em cada teste existente que checa rewrite shape (`rtk_rewrite_e2e_rewrites_unprefixed_command`, `rtk_rewrite_emission`, `rtk_rewrite_e2e_passes_through_rtk_prefixed_command` se relevante): garantir que usa o helper warn.
- [x] Adicionar 2-3 testes novos em paridade explicitamente no modo Strict:
  - `rtk_rewrite_strict_denies_unprefixed_command`: payload `git status`, espera deny no `permissionDecisionReason` contendo "Reenvie como: rtk".
  - `rtk_rewrite_strict_passes_through_rtk_prefixed`: payload `rtk git status`, espera silent allow (stdout empty).
  - `rtk_rewrite_strict_emits_no_rewrite_event`: payload sem rtk em strict, verifica que NÃO aparece evento `rtk-rewrite` na DB (gate denyou antes do rewrite, não há rewrite a emitir).
- [x] `cargo test -p mustard-rt --test rtk_rewrite_emission` verde.

## Limites

- `apps/rt/tests/rtk_rewrite_emission.rs` apenas.

**Fora dos limites:**
- Mexer em `apps/rt/src/hooks/bash_guard.rs`.
- Mudar o default do gate (continua Strict).
- Alterar o env var name `MUSTARD_BASH_REDIRECT_MODE`.

## Checklist

- [x] AC-1 a AC-4 passam
- [x] Helper compartilhado evita duplicação warn/strict
