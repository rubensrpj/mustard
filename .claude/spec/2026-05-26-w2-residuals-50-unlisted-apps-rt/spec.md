# Tactical Fix: W2 residuals: 50 unlisted apps/rt files + integration-test fixture wiring + legacy artifact cleanup

### Stage: Analyze
### Outcome: Active
### Flags: 
### Scope: full
### Lang: pt-BR
### Checkpoint: 2026-05-26T05:30:52.462Z
### Parent: 2026-05-26-claude-paths-single-source

## Contexto

Tactical fix derivado de [[2026-05-26-claude-paths-single-source]] W2. A wave-plan declarou "33 arquivos" mas enumerou só 18 em T2.7 + ~12 em T2.1-T2.6. O agente migrou tudo que foi nomeado por path. O grep global de AC-W2.4 ainda encontra ~50 arquivos com `.join(".X")` literal em `apps/rt/src/` fora do escopo enumerado.

Além disso AC-W2.9 (`cargo test -p mustard-rt` não vaza `apps/rt/.claude/`) ficou aberto porque os testes de integração chamam `env::project_dir()` direto, não usam o helper `test_workspace()` (que já existe em `apps/rt/tests/common/mod.rs`).

ACs W2.6/W2.7 dependem de uma limpeza manual one-shot dos arquivos legados em `.claude/` raiz que os writers novos deixaram de tocar (`.qa-reports/`, `.pipeline-states/`, `.economy-baselines.json`, `.scan-dispatch.json`, `.detect-cache.json`, `.knowledge-seen.json`, `.memory-seen.json`).

## Critérios de Aceitação

- [ ] **AC-TF1.** Zero `.join(".X")` ou `cwd.join(".claude")` em `apps/rt/src/` fora de `ClaudePaths` callers. Command: `rtk node -e "const{execSync}=require('child_process');const out=execSync('rtk grep -rn --include=\"*.rs\" \"\\.join(\\\".\" apps/rt/src',{encoding:'utf8'});const v=out.split('\\n').filter(l=>l&&!/ClaudePaths|test|claude_paths\\.rs/.test(l));if(v.length>0){console.error(v.join('\\n'));process.exit(1)}"`
- [ ] **AC-TF2.** 10× `rtk cargo test -p mustard-rt` consecutivos não criam `apps/rt/.claude/`. Command: `rtk powershell -Command "Remove-Item -Recurse -Force apps/rt/.claude -ErrorAction SilentlyContinue; 1..10 | ForEach-Object { cargo test -p mustard-rt --quiet }; if (Test-Path apps/rt/.claude) { exit 1 }"`
- [ ] **AC-TF3.** Raiz `.claude/` não contém legados volatile movidos. Command: `rtk node -e "const fs=require('fs');for(const p of ['.qa-reports','.pipeline-states','.economy-baselines.json','.scan-dispatch.json','.detect-cache.json','.knowledge-seen.json','.memory-seen.json']){if(fs.existsSync('.claude/'+p))process.exit(1)}"`

## Arquivos

- `apps/rt/src/run/active_specs.rs`, `recipe_match.rs`, `spec_extract.rs`, `security_scan.rs`, `backup_specs.rs`, `spec_clear.rs`, `task_checklist.rs`, `tactical_fix_create.rs`, `pipeline_summary.rs`, `metrics_wave_status.rs`, `wave_scaffold.rs`, `wave_files.rs`, `memory*.rs` (sweep)
- `apps/rt/src/run/scan/cluster_discovery.rs`, `interpret.rs`, `mod.rs`
- `apps/rt/src/hooks/amend_capture.rs`, `tool_result.rs`, `subagent_inject.rs`, `enforce_registry.rs`, `skills_audit.rs`, `size_gate.rs`, `tracker.rs`, `budget.rs`
- `apps/rt/tests/**/*.rs` — migrar para `test_workspace()` (criar via `common::test_workspace().root()` em vez de `env::project_dir()` cru)
- One-shot cleanup script — apagar `.claude/.qa-reports/`, `.claude/.pipeline-states/`, `.claude/.economy-baselines.json`, `.claude/.scan-dispatch.json`, `.claude/.detect-cache.json`, `.claude/.knowledge-seen.json`, `.claude/.memory-seen.json`

## Tarefas

- [ ] **TF.1** — Sweep `apps/rt/src/run/` (15 arquivos listados): para cada `.join(".X")` ou `cwd.join(".claude")`, substituir por método correspondente de `ClaudePaths`. Rodar `rtk cargo check -p mustard-rt` após cada arquivo.
- [ ] **TF.2** — Sweep `apps/rt/src/hooks/` (8 arquivos listados) + `apps/rt/src/run/scan/` (3 arquivos): mesma substituição mecânica.
- [ ] **TF.3** — Migrar testes de integração em `apps/rt/tests/**` para consumir `common::test_workspace()` — o helper já existe; só falta wiring. Cada teste que hoje chama `env::project_dir()` ou usa `current_dir()` recebe `let ws = test_workspace(); let root = ws.root();` no setup.
- [ ] **TF.4** — One-shot script PowerShell para apagar os 7 arquivos legados em `.claude/` raiz (lista nos Arquivos). Rodar uma vez; depois AC-TF3 trava regressão.
- [ ] **TF.5** — Verificar com `rtk mustard-rt run doctor --check claude-paths --format json` que filesystem casa com catálogo após o cleanup.

## Dependências

- **Parent**: [[2026-05-26-claude-paths-single-source]] (Closed). Esta sub-spec não bloqueia ninguém; é cleanup-only.
- **Pode rodar em paralelo a**: [[2026-05-26-template-agnostic-audit]] (escopos não se sobrepõem — esta é apps/rt, aquela é apps/cli/templates + i18n).

## Limites

IN: `apps/rt/src/run/**`, `apps/rt/src/hooks/**`, `apps/rt/tests/**`, script one-shot de cleanup em `.claude/`.
OUT: tudo mais (dashboard, cli, core já fechados na parent).
