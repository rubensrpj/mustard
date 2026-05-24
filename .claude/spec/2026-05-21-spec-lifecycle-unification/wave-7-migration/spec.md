# Wave 7 — Migration: migrate-spec-headers + execução criteriosa + testes de invariante

### Wave: 7
### Role: rt

## Resumo

Wave **terminal e criteriosa**. Adiciona o subcomando `mustard-rt run migrate-spec-headers` que reescreve cabeçalhos legados (`### Status:` + `### Phase:`) para o formato novo (`### Stage:` + `### Outcome:` + `### Flags:`), aplicado a todos os arquivos em `.claude/spec/**/*.md`. Inclui dry-run obrigatório por default, audit-log, escrita atômica, idempotência e reversibilidade. Executa a migração no repo Mustard ao final. Acrescenta testes de invariante para o conjunto completo de specs.

## Por que esta wave existe (sumário do risco)

Specs são artefatos vivos commitados no git. Reescrever 50+ arquivos `.md` num único commit é arriscado: conflito de merge, perda de informação se o mapeamento errar, divergência se rodar parcialmente. Tudo nesta wave é construído para **eliminar** esses riscos, não mitigá-los.

## Arquivos

```
apps/rt/src/run/migrate_spec_headers.rs        (novo — subcomando)
apps/rt/src/run/mod.rs                          (registrar)
apps/rt/tests/migrate_spec_headers.rs          (novo — cenários)
apps/rt/tests/spec_invariants.rs               (novo — varre .claude/spec/, valida invariantes)
.claude/.harness/migration-2026-05-21.log.json (gerado pela execução; commitado para audit)
.claude/spec/**/spec.md                         (escopo — todos os arquivos com header legado)
```

## Algoritmo da ferramenta

```
mustard-rt run migrate-spec-headers \
    [--dry-run]                  # padrão: true
    [--apply]                    # exige --apply para escrever; --dry-run e --apply são mutuamente exclusivos
    [--root <path>]              # padrão: .claude/spec
    [--log <file>]               # padrão: .claude/.harness/migration-{date}.log.json
    [--filter <glob>]            # opcional: limitar a um subset
```

Para cada `*.md` em `<root>` recursivamente:

1. Lê o arquivo, parseia headers.
2. Se já tem `### Stage:` no formato novo ⇒ skip (idempotente). Loga `skipped: already-migrated`.
3. Se NÃO tem `### Status:` nem `### Phase:` ⇒ skip + warning `no-status-header` (não é spec ou está mal-formada).
4. Mapeia Status/Phase → Stage/Outcome/Flags via tabela documentada em wave-plan.md.
5. Decide o conteúdo novo dos headers:
   ```
   ### Stage: {stage}
   ### Outcome: {outcome}
   ### Flags: {comma-separated or empty}
   ```
   Substitui as linhas `### Status:` e `### Phase:` por essas 3 linhas. Preserva ordem relativa dos outros headers (`### Parent:`, `### Lang:`, `### Checkpoint:`, etc.).
6. Em **modo dry-run**: registra no log o diff (antes/depois) mas não escreve.
7. Em **modo apply**: escreve via tempfile + rename (atômico). Registra no log o diff aplicado.

### Audit log

Arquivo JSON em `.claude/.harness/migration-{date}.log.json`:

```json
{
  "ran_at": "2026-05-21T...",
  "mode": "apply",
  "root": ".claude/spec",
  "total_files": 87,
  "migrated": 73,
  "skipped_already_migrated": 8,
  "skipped_malformed": 1,
  "errors": 0,
  "files": [
    {
      "path": ".claude/spec/2026-05-21-tf-skill-mirror/spec.md",
      "action": "migrated",
      "before": { "status": "approved", "phase": "EXECUTE" },
      "after": { "stage": "Execute", "outcome": "Active", "flags": [] }
    },
    {
      "path": ".claude/spec/legacy-malformed/spec.md",
      "action": "skipped",
      "reason": "no-status-header"
    }
  ]
}
```

### Edge cases tratados explicitamente

| Caso | Comportamento |
|---|---|
| Spec com `Status: completed` mas `Phase: EXECUTE` | Outcome terminal vence: `Stage: Close, Outcome: Completed`. Loga `inferred_stage_override: phase -> close (terminal status)`. |
| Spec com `Status: approved` sem `Phase:` | `Stage: Plan` (default do `approved`), `Outcome: Active`. |
| Spec com `Status: cancelled` e `Phase: PLAN` | `Stage: Close, Outcome: Cancelled`. Loga `inferred_stage_override`. |
| Spec com `Status: closed-followup` | `Stage: Close, Outcome: Active, Flags: followup_open`. |
| Spec com header já no formato novo | Skip. Log `skipped: already-migrated`. |
| Spec sem nenhum header | Skip. Log `skipped: no-status-header`. Não é erro. |
| Header com casing/whitespace inconsistente (`### status:` lowercase, espaço extra) | Parser tolerante (já em W1) aceita. |
| Arquivo `.md` que NÃO é `spec.md` (ex: `wave-plan.md`, `README.md`) | Sempre processado se tem `### Status:` ou `### Phase:` (alguns wave-plans têm). Senão skip. |

### Reversibilidade

- Migração é **atômica por arquivo** (tempfile+rename). Em caso de crash entre arquivos, nenhum fica meio-escrito.
- Reversão é via `git checkout` — o log permite identificar exatamente quais arquivos mudaram para fazer `git checkout -- {path}`.
- `--apply` exige flag explícita; nunca executa por acidente.

## Execução

Após implementar a ferramenta:

1. Rodar `mustard-rt run migrate-spec-headers --dry-run` no repo Mustard. Revisar `.claude/.harness/migration-2026-05-21.log.json` manualmente.
2. Confirmar visualmente os mapeamentos da tabela acima nos casos limite (procurar por `inferred_stage_override` no log).
3. Rodar `mustard-rt run migrate-spec-headers --apply`. Commit em separado: `chore(migration): rewrite legacy spec headers to Stage/Outcome/Flags`.
4. Rodar `cargo test --test spec_invariants` para validar o conjunto.
5. Smoke visual: abrir `/specs` no dashboard, conferir que todas as specs continuam sendo categorizadas corretamente nos grupos do Stage.

## Testes

### tests/migrate_spec_headers.rs

- [ ] Cenário 1 — happy path: arquivo com `Status: approved + Phase: EXECUTE` ⇒ vira `Stage: Execute, Outcome: Active, Flags:`.
- [ ] Cenário 2 — dry-run não escreve: rodar com `--dry-run`, conferir arquivo no disco intacto.
- [ ] Cenário 3 — idempotência: rodar `--apply` duas vezes seguidas; segunda vez deve skip everything.
- [ ] Cenário 4 — atomicidade: simular crash entre arquivos (drop antes do rename) ⇒ verificar que o arquivo original permanece intacto.
- [ ] Cenário 5 — edge case override: `Status: cancelled + Phase: PLAN` ⇒ Stage: Close, Outcome: Cancelled (terminal vence).
- [ ] Cenário 6 — flag mapping: `Status: closed-followup` ⇒ `Flags: followup_open`.
- [ ] Cenário 7 — malformado: arquivo `.md` sem header de status ⇒ skip sem error.

### tests/spec_invariants.rs

- [ ] Varre `.claude/spec/**/*.md` no repo Mustard, parseia o header com `SpecState`. Asserts:
  - Nenhum erro de parsing.
  - Para cada spec, `SpecState::new(stage, outcome, flags)` não retorna erro (invariantes respeitadas).
  - `AC-P-1`: rg `### Status:` retorna vazio no diretório de specs.
  - `AC-P-2`: rg `### Phase:` retorna vazio.
  - `AC-P-3`: rg `### Stage:` retorna ≥1 ocorrência por spec.

## Acceptance Criteria

- [x] AC-W7-1: `cargo build -p mustard-rt` passa. ✅
- [x] AC-W7-2: `cargo test -p mustard-rt` passa — **782 passed**, incl. 12 cenários de `migrate_spec_headers.rs` (7 originais + bullet + body-mentions-stage + combined-pipe + queued+parenthetical). ✅
- [x] AC-W7-3: `cargo test -p mustard-rt --test spec_invariants` passa após `--apply` (un-ignored). Varre 166 specs: 0 erro de parse, 0 header legado, `SpecState::new` legal em todas. ✅
- [x] AC-W7-4: `--dry-run` produziu `migration-2026-05-22.log.json` (`mode: dry-run`), zero modificação em disco. ✅
- [x] AC-W7-5: `--apply` rodado; **166/166 spec.md no formato novo** (auditoria header-scoped). Migração em 2 passadas (cobertura estendida p/ formato bullet + combined-pipe descobertos no dry-run). Diff commitado em separado. ✅
- [x] AC-W7-6/7 (transversal, met-by-intent): **nenhum header legado** `### Status:`/`### Phase:` (provado por `spec_invariants` que parseia o header). O `rg '### Status:'` literal retorna apenas **menções em prosa** (backticks/tabelas) nas próprias specs de migração/skill que documentam o formato legado — corretas de manter. AC-P-1 do parent já antecipava isso ("apenas legado pré-migração ou vazio"). ✅
- [x] AC-W7-8 (transversal): **166/166 spec.md** com `### Stage:` no header (auditoria). ✅
- [~] AC-W7-9 (build-verified; visual manual pendente): dashboard build passa e o parser lê todas as 166 no formato novo; smoke visual `/specs` em `tauri:dev` não lançável nesta sessão — verificação visual manual recomendada.

## Descobertas durante a execução (3 formatos legados, não 1)

O dry-run revelou que o repo tinha **3 formatos de header legado**, não só `### Status:`+`### Phase:` separados:
1. `### Status:` + `### Phase:` (linhas separadas) — previsto.
2. `- **Status**:` + `- **Phase**:` (bullet, specs mustard-2.0/dashboard-1.0 de mai/12) — fix-loop 1.
3. `### Status: X | Phase: Y | Scope: Z` (combined-pipe, ~52 specs de mar-abr) — fix-loop 2.
+ 2 specs com header corrompido (`completed| Phase: CLOSE`) consertadas à mão (guardrail-catalog, mustard-doctor).
+ Bug de idempotência: o check `### Stage:` varria o corpo inteiro → specs que **documentam** o formato (wave-4/7/parent) eram puladas falsamente. Corrigido com header-region scoping (até o primeiro `## ` ou fence).
**Lição:** auditar cobertura por *formato real parseável*, não por presença do token `### Status:`.

## Limites

**IN:**
- `apps/rt/src/run/migrate_spec_headers.rs` (novo).
- `apps/rt/tests/migrate_spec_headers.rs` (novo).
- `apps/rt/tests/spec_invariants.rs` (novo).
- `.claude/spec/**/*.md` (escopo da migração no momento da execução).

**OUT:**
- Remoção de `SpecStatus` do `mustard-core` — adiar para uma wave/spec própria de cleanup; manter `#[deprecated]` por mais um ciclo para back-compat com projetos instalados.
- Remoção de aliases `pipeline.status`/`pipeline.phase` em `emit-pipeline` — adiar; back-compat com Mustard instalado em outros repos.
- Cópias instaladas `.claude/commands/mustard/*` neste repo Mustard — Wave 4 ou tactical-fix.

## Riscos pós-migração (e como detectar)

- **Risco residual**: spec criada por SKILL legado fora deste repo continua emitindo `pipeline.phase`. Detectado pelo `emit-pipeline` em W2 (aceita ambos). Não é problema operacional.
- **Risco residual**: arquivo mal-formado sem header → log de skip surfacea no commit. Decisão manual: arquivar ou consertar.
- **Risco residual**: usuário commitou edições em sessão paralela durante a migração → conflito de merge resolvido manualmente, com `migration-{date}.log.json` como referência de qual era o mapeamento.
