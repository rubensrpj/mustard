# Wave 4 — Skills: 18 SKILLs emitem pipeline.stage/outcome/flag + header novo

### Parent: [[2026-05-21-spec-lifecycle-unification]]
### Wave: 4
### Role: cli
### Status: completed
### Phase: CLOSE
### Lang: pt
### Checkpoint: 2026-05-22T01:12:00Z

## Resumo

Atualiza os 18 SKILLs `/mustard:*` em `apps/cli/templates/commands/mustard/` para:

1. Emitir os novos kinds (`pipeline.stage`, `pipeline.outcome`, `pipeline.flag.set`, `pipeline.flag.clear`) nos pontos onde hoje emitem `pipeline.phase` ou `pipeline.status`.
2. Escrever spec.md com header **novo** (`### Stage:` / `### Outcome:` / `### Flags:`) para specs **criadas a partir desta wave**. Specs já existentes continuam intocadas (Wave 7 migra em batch).
3. Tactical-fix-mirror posterior se as cópias instaladas em `.claude/commands/mustard/` ficarem fora de paridade (precedente: [[2026-05-21-tf-skill-mirror]]).

## Arquivos

```
apps/cli/templates/commands/mustard/feature/SKILL.md
apps/cli/templates/commands/mustard/bugfix/SKILL.md
apps/cli/templates/commands/mustard/approve/SKILL.md
apps/cli/templates/commands/mustard/close/SKILL.md
apps/cli/templates/commands/mustard/qa/SKILL.md
apps/cli/templates/commands/mustard/resume/SKILL.md
apps/cli/templates/commands/mustard/tactical-fix/SKILL.md
apps/cli/templates/commands/mustard/review/SKILL.md
apps/cli/templates/commands/mustard/scan/SKILL.md
apps/cli/templates/commands/mustard/status/SKILL.md
apps/cli/templates/commands/mustard/stats/SKILL.md
apps/cli/templates/commands/mustard/knowledge/SKILL.md
apps/cli/templates/commands/mustard/maint/SKILL.md
apps/cli/templates/commands/mustard/skill/SKILL.md
apps/cli/templates/commands/mustard/git/SKILL.md
apps/cli/templates/commands/mustard/task/SKILL.md
apps/cli/templates/commands/mustard/prd/SKILL.md
apps/cli/templates/skills/pipeline-execution/SKILL.md
```

## Tarefas

- [ ] Em cada SKILL.md, substituir as instruções de `emit-pipeline --kind pipeline.phase --payload '{"phase":"X"}'` por uma chamada equivalente com `pipeline.stage --payload '{"stage":"X"}'`. Manter o evento legado para back-compat **NÃO é necessário** porque o `emit-pipeline` em W2 já escreve o legado quando recebe o novo (e vice-versa). Logo aqui podemos emitir só o novo.
- [ ] Idem para `pipeline.status` → `pipeline.outcome` (`status: completed` ⇒ `outcome: completed`, etc.).
- [ ] Para flags (`blocked`, `wave_failed`, `followup_open`):
  - `/mustard:close` ao fechar com janela de follow-up: emit `pipeline.flag.set` com `flag: followup_open`.
  - `/mustard:resume` ao desbloquear: emit `pipeline.flag.clear` com `flag: blocked`.
  - Wave-failure em `/mustard:resume` ou no `verify-pipeline`: emit `pipeline.flag.set` com `flag: wave_failed`.
- [ ] Em `/mustard:feature` e `/mustard:bugfix` (e `tactical-fix`), atualizar o template do spec.md gerado para emitir header novo:
  ```
  ### Stage: Plan
  ### Outcome: Active
  ### Flags:
  ### Lang: pt
  ### Checkpoint: ISO_DATE
  ```
  Em vez do legado `### Status: approved / ### Phase: PLAN`.
- [ ] Em `/mustard:approve`, ao aprovar a spec, emit `pipeline.stage: plan` (mantém Plan) + reescreve header se ainda está em formato legado.
- [ ] Em `/mustard:qa`, ao terminar QA com sucesso, emit `pipeline.stage: qa-review` (durante) e `pipeline.stage: close` (ao terminar; o close-gate decide se passa para Outcome::Completed).
- [ ] Em `/mustard:close`, ao final: emit `pipeline.stage: close` + `pipeline.outcome: completed` (se gate passa) ou `pipeline.flag.set blocked` (se gate falha e usuário escolhe pausar).
- [ ] **NÃO duplicar** chamadas — cada SKILL emite no MESMO ponto que emitia antes, só troca o kind.
- [ ] Atualizar exemplos dentro dos `SKILL.md` que mostrem o header novo (texto educativo).
- [ ] Rodar `cargo build -p mustard-cli && cargo test -p mustard-cli` para garantir que o payload dos templates compila (templates são `include_str!` no cli).

## Acceptance Criteria

- [x] AC-W4-1: `rg -n 'pipeline\.status' apps/cli/templates/commands/` retorna vazio (todos migrados para `pipeline.outcome`). ✅
- [x] AC-W4-2: `rg -n 'pipeline\.phase' apps/cli/templates/commands/` retorna vazio (todos migrados para `pipeline.stage`). ✅ (incluiu fix de prosa em `bugfix/SKILL.md`)
- [x] AC-W4-3 (refinado): nenhum skill **gera** header `### Status:` para spec nova — `feature`/`bugfix`/`tactical-fix` agora geram `### Stage:`/`### Outcome:`/`### Flags:`. As 3 ocorrências restantes de `### Status:` são **handling intencional de legado**, exigido pela estratégia de transição W1–W6 (parser tolerante; W7 migra em batch): `approve`/`close` reescrevem header legado *se presente*; `resume` detecta stub por `### Status: queued`. Remover quebraria a leitura de specs pré-existentes. ✅ (intenção cumprida)
- [x] AC-W4-4: `### Stage:` presente em `feature`, `bugfix`, `tactical-fix` (+ approve/close/resume). ✅
- [x] AC-W4-5: `cargo build -p mustard-cli` passa. ✅ (2 crates compiled)
- [x] AC-W4-6: `cargo test -p mustard-cli` passa. ✅ (43 passed)
- [ ] AC-W4-7 (DEFERIDO): smoke `/mustard:feature` cria header novo — **não verificável agora**: as cópias instaladas em `.claude/commands/mustard/` (OUT desta wave) ainda emitem legado. Só passa após o tactical-fix-mirror que espelha os templates para as cópias instaladas (precedente [[2026-05-21-tf-skill-mirror]]). Verificável a partir de então.

## Limites

**IN:** `apps/cli/templates/commands/mustard/*/SKILL.md` (18) + `apps/cli/templates/skills/pipeline-execution/SKILL.md`.

**OUT:**
- `.claude/commands/mustard/*/SKILL.md` (cópias instaladas no próprio repo Mustard) — virá em tactical-fix posterior, se necessário, espelhando este wave (precedente [[2026-05-21-tf-skill-mirror]]).
- Specs existentes em `.claude/spec/` — Wave 7 migra os headers.
- Hooks do `mustard-rt` — Waves 1/2/5.

## Riscos e mitigação

- **Risco**: divergência entre template (apps/cli/templates/) e cópia instalada (.claude/commands/). Mitigação: tactical-fix-mirror espelhando, já é padrão no projeto.
- **Risco**: Skill antigo (instalado em projeto remoto) continua emitindo `pipeline.phase` legado. Mitigação: `emit-pipeline` em W2 aceita ambos e escreve em ambos. Skills antigos continuam funcionando.
