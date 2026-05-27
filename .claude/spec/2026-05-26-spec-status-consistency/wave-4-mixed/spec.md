# Wave 4 — backfill one-shot `spec-status-backfill`

### Stage: Close
### Outcome: Completed
### Flags: 

## Contexto

As doze specs atuais foram criadas em sessões anteriores, sob código antigo que dessincroniza `spec.md` ↔ `meta.json`. Mesmo depois das waves W1-W3, elas continuam descasadas — porque a sincronização só roda em **novas** transições. Esta wave é o **backfill one-shot**: rodado uma vez agora, alinha as 12 specs, e depois pode ser removido (ou ficar disponível como ferramenta de recuperação).

Resultado esperado: as 4 specs problemáticas (rtk-quiet, dashboard-i18n, template-agnostic, w2-residuals) ficam todas consistentes; doctor passa zero warnings.

## Tarefas

- [x] **T4.1** — Criar `apps/rt/src/run/spec_status_backfill.rs` com função `run(claude_paths: &ClaudePaths, source: BackfillSource) -> Result<BackfillReport>`. `BackfillSource` é enum `Spec | Meta` indicando qual lado é a fonte de verdade.
- [x] **T4.2** — Algoritmo: itera `.claude/spec/*/`. Para cada spec:
  - Se `source=Spec`: lê `spec.md`, extrai (stage, outcome), reescreve `meta.json` com esses valores (preserva outros campos).
  - Se `source=Meta`: lê `meta.json`, reescreve `spec.md` header com (stage, outcome) preservando o resto do corpo.
  - Recursa em `wave-N-*/spec.md` + `wave-N-*/meta.json`.
  - Casos `closed-followup` (Close+Active) ficam preservados (não "normaliza" pra Close+Completed).
- [x] **T4.3** — CLI: `mustard-rt run spec-status-backfill [--source spec|meta] [--dry-run] [--spec <name>]`. `--dry-run` imprime as mudanças sem aplicar. `--spec` restringe a uma spec só.
- [x] **T4.4** — Output JSON com `{ specs_scanned: N, specs_changed: M, files_written: [...], conflicts: [...] }`. Conflitos = casos em que `spec.md` E `meta.json` discordam (precisa do `--source` explícito para resolver).
- [x] **T4.5** — Registrar subcomando em `apps/rt/src/run/mod.rs` + dispatch em `apps/rt/src/main.rs`.
- [x] **T4.6** — Rodar `spec-status-backfill --source spec` uma vez sobre as 12 specs atuais. Verificar que o `doctor --check status-consistency` passa depois.

## Critérios de Aceitação

- **AC-W4.1** — `mustard-rt run spec-status-backfill --help` mostra as 3 flags (`--source`, `--dry-run`, `--spec`). Command: `rtk mustard-rt run spec-status-backfill --help`
- **AC-W4.2** — Dry-run não escreve nada. Command: `rtk node -e "const{execSync}=require('child_process');const fs=require('fs');const before=fs.statSync('.claude/spec/2026-05-26-rtk-quiet-hook-warning/spec.md').mtimeMs;execSync('mustard-rt run spec-status-backfill --dry-run --spec 2026-05-26-rtk-quiet-hook-warning');const after=fs.statSync('.claude/spec/2026-05-26-rtk-quiet-hook-warning/spec.md').mtimeMs;if(before!==after)process.exit(1)"`
- **AC-W4.3** — Após `spec-status-backfill --source spec` rodar sobre `rtk-quiet-hook-warning`, o `meta.json` continua com `stage:"Analyze"` e o `spec.md` ganha os cabeçalhos `### Stage: Analyze`, `### Outcome: Active` se ausentes.
- **AC-W4.4** — Após backfill nas 12 specs, `mustard-rt run doctor --check status-consistency` retorna exit 0. Command: `rtk mustard-rt run spec-status-backfill --source spec && rtk mustard-rt run doctor --check status-consistency`

## Limites

- **IN**: `apps/rt/src/run/spec_status_backfill.rs` (novo), `apps/rt/src/run/mod.rs`, `apps/rt/src/main.rs`.
- **OUT**: não cria evento `pipeline.*`; não toca em conteúdo do `spec.md` além do header; não move arquivos.
