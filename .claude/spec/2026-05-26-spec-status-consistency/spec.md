# spec-status-consistency — padronizar geração e atualização de status das specs

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Lang: pt-BR
### Checkpoint: 2026-05-26T19:30:00Z

<!-- PRD -->

## Contexto

Hoje doze specs convivem em `.claude/spec/` e quatro delas estão em estado descasado: uma sem cabeçalhos no arquivo principal (`rtk-quiet-hook-warning`), uma com `### Stage: Close` + `### Outcome: Active` invisível ao listing (`dashboard-i18n-migration`), uma sem progressão de wave no `meta.json` (`template-agnostic-audit`), e uma divergente entre `spec.md` (Execute) e `meta.json` (Plan) — `w2-residuals-50-unlisted-apps-rt`. AC = critério de aceitação (Acceptance Criteria); wave = onda (uma fatia de trabalho dentro de um spec).

A investigação no código (rt-explorer, 26-05-2026) achou quatro causas concretas:

1. **`tactical_fix_create.rs` não reusa o helper de `spec_draft.rs`.** O subcomando `spec-draft` (apps/rt/src/run/spec_draft.rs:318) sempre escreve os três cabeçalhos canônicos (Stage/Outcome/Flags); já o `tactical_fix_create` tem scaffold próprio e — em pelo menos um caminho — pula esses headers. Resultado: spec criada sem `### Stage:`.

2. **`emit_pipeline.rs` sincroniza `spec.md` e `meta.json` em dois trilhos.** As funções `sync_spec_status_header` (linha 631) e `sync_spec_meta_sidecar` (linha 469) são chamadas juntas só quando `should_sync_parent_header` (linha 340) retorna true — e ela retorna **false** se o payload tem campo `wave`. Toda transição wave-level dessincroniza os dois arquivos.

3. **`bump_parent_progress` deliberadamente não toca `stage`/`outcome`.** Mesmo no novo handler de `pipeline.wave.complete` (recém-mergeado em `2026-05-26-harness-sync-emit-pipeline-must-update`), o progresso só atualiza `currentWave`/`completedWaves`/`phase`. Specs antigas ficam com `meta.json` incompleto sem ninguém pra backfillar.

4. **`active_specs.rs:136-138` esconde specs malformadas em vez de sinalizar.** O picker filtra qualquer spec sem `stage` E `outcome` no `meta.json`. Resultado: o usuário não vê a spec problemática no listing — ela some.

E mais um sintoma sistêmico: **não existe nenhum check no `doctor`** que detecte estes três estados ruins. A lista atual de checks em `doctor.rs:1097-1106` tem `skill-discovery`, `wave-integrity`, `claude-paths`, `workspace-leaks`, `i1` — nenhum cobre consistência de status.

Esta spec ataca os quatro pontos juntos. Sem isso, o problema repete na próxima spec gerada.

## Usuários

- **Rubens** (operador único hoje): quer ver o estado real ao rodar `/mustard:spec`, sem specs invisíveis e sem campos descasados.
- **Quem mantém o harness Rust**: ganha um helper único (`sync_status`) em vez de dois caminhos paralelos que podem divergir.
- **O próprio doctor**: passa a detectar inconsistência como categoria FAIL, fechando o ciclo de feedback.

## Métrica

- Zero specs reportadas por `doctor --check status-consistency` nas doze specs atuais após o backfill rodar uma vez.
- Zero specs sumindo do picker `active-specs` por causa de `meta.json` incompleto — todas listam, malformadas com flag `??`.
- 100% das transições wave-level (qualquer evento `pipeline.*` com campo `wave`) escrevem `spec.md` e `meta.json` no mesmo passo — verificável por teste de integração.
- `tactical-fix new X` sempre produz `spec.md` com os três cabeçalhos canônicos — verificável por teste de regressão.

## Não-Objetivos

- Reescrever conteúdo (Contexto, Tarefas, ACs) das specs históricas — só cabeçalhos e meta.
- Criar novos estados além dos já mapeados em `state_from_status_word` (Plan/Active, Execute/Active, Close/Completed, Close/Cancelled, Close+Active=`closed-followup`).
- Mudar layout de pastas (`.claude/spec/{name}/` flat permanece) ou criar buckets `active/`/`completed/`.
- Migrar specs pra outro idioma (continuam em pt-BR ou en-US conforme `meta.json#lang`).
- Tocar no dashboard Tauri (apps/dashboard) — fora de escopo. Picker visual já lê o que o `active-specs` devolver.

## Critérios de Aceitação

- **AC-1** — `doctor --check status-consistency` falha (exit 1) se houver spec com cabeçalhos ausentes, divergência `spec.md` ↔ `meta.json`, ou combinação Stage+Outcome não mapeada; passa (exit 0) quando todas estão alinhadas.
  Command: `rtk mustard-rt run doctor --check status-consistency`
- **AC-2** — `tactical-fix-create` produz `spec.md` com `### Stage: Analyze`, `### Outcome: Active`, `### Flags:` (regressão do bug descoberto na `rtk-quiet-hook-warning`).
  Command: `rtk node -e "const{execSync}=require('child_process');const out=execSync('mustard-rt run tactical-fix-create --intent test-ac2 --parent 2026-05-25-mustard-deep-refactor',{encoding:'utf8'});const fs=require('fs');const j=JSON.parse(out);const spec=fs.readFileSync(j.files[0],'utf8');if(!/### Stage: Analyze/.test(spec)||!/### Outcome: Active/.test(spec))process.exit(1)"`
- **AC-3** — Rodar pipeline wave-plan completa numa spec teste deixa `spec.md` (parent + cada wave) e `meta.json` (parent + cada wave) alinhados — mesmo `stage`/`outcome`.
  Command: `rtk cargo test -p mustard-rt --test status_sync_integration`
- **AC-4** — `spec-status-backfill` aplicado sobre as 12 specs atuais resulta em `doctor --check status-consistency` passando com zero warnings depois.
  Command: `rtk mustard-rt run spec-status-backfill --source spec && rtk mustard-rt run doctor --check status-consistency`
- **AC-5** — `active-specs` lista a spec `dashboard-i18n-migration` (Close+Active = `closed-followup`) explicitamente, com sigla `CLR→fu`, em vez de ocultar.
  Command: `rtk mustard-rt run active-specs | rtk node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{if(!/dashboard-i18n/.test(s)||!/CLR.*fu/.test(s))process.exit(1)})"`
- **AC-6** — Build verde com clippy estrito.
  Command: `rtk cargo build -p mustard-rt && rtk cargo clippy -p mustard-rt -- -D warnings`

<!-- PLAN -->

## Arquivos

- `apps/rt/src/run/spec_draft.rs` — extrair `write_spec_md`/`write_meta_json` em util pública.
- `apps/rt/src/run/tactical_fix_create.rs` — passa a usar a util compartilhada.
- `apps/rt/src/run/emit_pipeline.rs` — função `sync_status` única, atômica; remover gate `should_sync_parent_header`.
- `apps/rt/src/run/doctor.rs` — novo check `status-consistency`.
- `apps/rt/src/run/active_specs.rs` — não filtra spec malformada; exibe `closed-followup`.
- `apps/rt/src/run/spec_status_backfill.rs` — novo subcomando one-shot.
- `apps/rt/src/run/mod.rs` — registra subcomando + check.
- `apps/rt/tests/status_sync_integration.rs` — teste novo para AC-3.

## Tarefas

- [x] **T1** (W1) — Extrair helper `spec_scaffold` (spec_draft.rs → módulo público).
- [x] **T2** (W1) — Função `sync_status(stage, outcome, spec_path, meta_path)` atômica.
- [x] **T3** (W1) — Remover gate `should_sync_parent_header`; sincronizar parent+wave juntos.
- [x] **T4** (W2) — Doctor check `status-consistency` + agregador default.
- [x] **T5** (W3) — Picker `active-specs` lista malformadas com flag `??`; exibe `closed-followup` como `CLR→fu`.
- [x] **T6** (W4) — Subcomando `spec-status-backfill --source spec|meta`.
- [x] **T7** (W5) — Teste de integração `status_sync_integration` + QA dos 6 ACs.

## Dependências

- W1 deve precede W2 (doctor depende de saber o que é "alinhado").
- W4 deve precede W5 (QA roda backfill sobre as 12 specs atuais).

## Limites

- **IN**: `apps/rt/`, `packages/core/spec/contract.rs` (se preciso ajustar contract).
- **OUT**: `apps/dashboard/`, `apps/cli/`, nenhum hook novo, nenhum kind novo de evento, nenhum bucket de pasta.

<!-- signals: rt,picker,doctor,backfill -->
