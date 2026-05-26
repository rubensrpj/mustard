# Tactical Fix: rtk quiet hook warning

## Contexto

Tactical fix derivado de [[2026-05-25-mustard-deep-refactor]].

O `rtk` 0.34.1 (Rust Token Killer) emite em stderr, em **toda** invocação, a linha:

```
[rtk] /!\ No hook installed — run `rtk init -g` for automatic token savings
```

O hook nativo do `rtk` só existe para Gemini CLI / Copilot (`rtk hook --help` confirma: subcomandos `gemini` e `copilot`, sem `claude`). Para o Claude Code o caminho oficial seria `rtk init -g`, que **viola** [[feedback_mustard_install_workflow]] (não escrever em `~/.claude/settings.json` global). No Mustard a redireção pro `rtk` já é feita pelo módulo `bash_guard` do `mustard-rt` ([[project_bash_guard_blanket_rtk]]), então o aviso é puro ruído.

Auditei o binário do `rtk` (strings) e as env vars expostas são: `RTK_AUDIT_DIR`, `RTK_DB_PATH`, `RTK_DISABLED`, `RTK_HOOK_AUDIT`, `RTK_NO_TOML`, `RTK_TEE`, `RTK_TEE_DIR`, `RTK_TELEMETRY_DISABLED`, `RTK_TOML_DEBUG`, `RTK_TRUST_PROJECT_FILTERS`, `RTK_VERSION`. **Não existe** `RTK_QUIET` nem equivalente — o warning é hardcoded. Solução: filtrar a linha no consumidor.

Impacto concreto: cada chamada Bash polui (a) o stderr exibido ao agente (~95 chars), (b) o NDJSON per-spec event-log ([[project_db_bloat_per_spec_events]]) — em sessões longas, isso multiplica por N invocações.

## Critérios de Aceitação

- [ ] **AC-1** — Testes verdes. Command: `rtk cargo test -p mustard-rt`
- [ ] **AC-2** — Linha "No hook installed" não aparece no stderr emitido por uma chamada Bash arbitrária via harness. Command: `rtk node -e "const{execSync}=require('child_process');const out=execSync('mustard-rt on PreToolUse',{input:JSON.stringify({tool_name:'Bash',tool_input:{command:'rtk ls'}}),encoding:'utf8',stdio:['pipe','pipe','pipe']});if(/No hook installed/.test(out))process.exit(1)"`

## Arquivos

- `apps/rt/src/hooks/bash_guard.rs` — adicionar filtro de stderr pós-execução do comando reescrito (match literal por prefixo `[rtk] /!\ No hook installed`).
- `apps/rt/src/hooks/bash_guard/tests.rs` (ou módulo de teste inline) — teste unitário: stderr com a linha + outras → filtra só a do `rtk`.
