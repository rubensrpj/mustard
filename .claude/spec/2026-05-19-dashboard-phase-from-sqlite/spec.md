# Dashboard derivação de fase a partir do SQlite (store único)

> **Roda antes de b6** (não depende dele) e é prerequisite arquitetural da Wave 3 de `2026-05-19-artifact-update-followups` — não faz sentido surface de "artefatos defasados" no dashboard se a base de phase reading ainda é fragmentada.

## PRD

## Contexto

A migração para storage SQLite único (spec `eliminate-bun`, CLOSE 2026-05-19)
consolidou `mustard.db` como source of truth da harness — `emit_phase.rs`,
`post_edit.rs` e demais emissores gravam o evento `pipeline.phase` direto no
`SqliteEventStore`. Mas o leitor do dashboard ficou no meio da migração: o
comando Tauri `dashboard_specs` (`apps/dashboard/src-tauri/src/lib.rs:508`)
ainda walka `.claude/.pipeline-states/{spec}.json` e usa o campo `phaseName`
desses JSONs como fonte da coluna `phase` dos cards; a função `specs_from_db`
existe e até é consultada, mas o merge só puxa `started_at`/`completed_at`/
`affected_files` do DB — a fase do DB é descartada. Consequência observada:
o card `ANALYZE` nunca atualiza (essa fase nunca chega ao JSON — só ao SQLite),
e `PLAN`/`EXECUTE`/`CLOSE` só refletem por janelas curtas dependendo de quando
o JSON foi reescrito. A causa é leitor fragmentado, não escritor faltando.

## Usuários/Stakeholders

Mantenedores e usuários do `mustard-dashboard`, que precisam ver a fase corrente
e a progressão completa (incluindo ANALYZE) das specs ativas. Solicitado por
Rubens ao identificar a inconsistência durante o close de `artifact-update-scheme`.

## Métrica de sucesso

Após `mustard-rt run emit-phase --spec X --to ANALYZE`, a query
`dashboard_specs` para o repo retorna a spec com `phase: "ANALYZE"`. O campo
`phaseName` dos arquivos `.pipeline-states/*.json` deixa de existir nos writes
novos (pipeline corre normalmente sem ele). **Mais:** `mustard-rt run
docs-stale-check` passa a auditar termos obsoletos em CLAUDE.md / pipeline-config.md
contra `.claude/.docs-audit.json` e é invocado por `/mustard:close` — drift
narrativo igual ao que aconteceu pós-eliminate-bun deixa de fechar verde em silêncio.

## Não-Objetivos

- Não reescrever a UI dos cards — só corrigir a fonte de dados.
- Não migrar os JSONs `.pipeline-states/*.json` existentes (legacy `phaseName` pode ficar; o leitor passa a ignorar).
- Não tocar o caminho de escrita do SQLite (`emit_phase.rs`, `post_edit.rs` já corretos).
- Não unificar `.pipeline-states/{spec}.json` inteiro no SQLite — só a fase. Os outros campos (tasks/scope/wave) seguem no JSON por enquanto.
- Não bloquear/depender de b6 nem do esquema artifact-update.

## Critérios de Aceitação

Critérios binários, executáveis. Cada um roda da raiz do projeto; exit 0 = passou.

- [x] AC-1: O dashboard buila — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: O workspace Rust compila — Command: `cargo build -p mustard-core -p mustard-rt`
- [x] AC-3: Testes de rt e dashboard backend passam — Command: `cargo test -p mustard-rt -p mustard-dashboard`
- [x] AC-4: `specs_from_db` deriva a fase a partir dos eventos `pipeline.phase` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/db.rs','utf8');if(!c.includes('pipeline.phase')||!c.toLowerCase().includes('phase'))process.exit(1)"`
- [x] AC-5: `specs_from_fs` não lê mais `phaseName` do JSON — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');const s=c.indexOf('fn specs_from_fs');if(s<0)process.exit(1);const e=c.indexOf('\nfn ',s+1);const body=e>0?c.slice(s,e):c.slice(s);if(body.includes('phaseName'))process.exit(1)"`
- [x] AC-6: Os SKILL.md de pipeline (feature/approve/resume/close) não instruem mais escrever `phaseName` em `.pipeline-states/*.json` — Command: `node -e "const fs=require('fs'),p=require('path'); const dirs=['feature','approve','resume','close'].map(d=>'apps/cli/templates/commands/mustard/'+d+'/SKILL.md'); const bad=dirs.filter(f=>fs.existsSync(f)&&/phaseName/.test(fs.readFileSync(f,'utf8'))); if(bad.length)process.exit(1)"`
- [x] AC-7: `.claude/.docs-audit.json` existe e enumera ≥2 specs (eliminate-bun + esta) com `obsolete_terms` — Command: `node -e "const m=require('./.claude/.docs-audit.json'); if(!m.audits||m.audits.length<2||!m.audits.every(a=>a.from_spec&&Array.isArray(a.obsolete_terms)&&a.obsolete_terms.length))process.exit(1)"`
- [x] AC-8: O subcomando `docs-stale-check` está registrado — Command: `node -e "const fs=require('fs');if(!/DocsStaleCheck/.test(fs.readFileSync('apps/rt/src/run/mod.rs','utf8')))process.exit(1)"`
- [x] AC-9: `docs-stale-check` roda e reporta 0 hits para os termos obsoletos de eliminate-bun (validação dogfood pós-Wave 2) — Command: `cargo run -q -p mustard-rt -- run docs-stale-check --from eliminate-bun | node -e "let d='';process.stdin.on('data',c=>d+=c).on('end',()=>{const r=JSON.parse(d); if(r.hits&&r.hits.length>0)process.exit(1)})"`
- [x] AC-10: `/mustard:close` invoca `docs-stale-check` na Verification Gate — Command: `node -e "const fs=require('fs');if(!/docs-stale-check/.test(fs.readFileSync('apps/cli/templates/commands/mustard/close/SKILL.md','utf8')))process.exit(1)"`

## Plano

## Informações da Entidade

`pipeline.phase` event — entidade já definida em `mustard-core` (`HarnessEvent`
com `event: "pipeline.phase"`, `payload: { from, to }`, `spec: Some(...)`).
Esta spec só consolida o consumo: o dashboard passa a derivar a fase corrente
de uma spec pela query "último `pipeline.phase` com `spec == X`" — o mesmo
padrão de `emit_phase::last_phase_for_spec`. O `.pipeline-states/{spec}.json`
deixa de carregar fase (mantém os demais campos: scope, tasks, wave, etc.).

## Arquivos

- `apps/dashboard/src-tauri/src/db.rs` (edição) — `specs_from_db` retorna `SpecRow` com `phase` derivado do último `pipeline.phase` por spec; nova query SQL agrupada por `spec`.
- `apps/dashboard/src-tauri/src/lib.rs` (edição) — `dashboard_specs::merge` passa a copiar `phase` do DB sobre o FS (DB ganha); `specs_from_fs` para de ler `v["phaseName"]` (continua lendo `status`, `scope`, etc.).
- `apps/cli/templates/commands/mustard/feature/SKILL.md` (edição) — remover instrução de escrever `phaseName` no pipeline-state JSON; manter o `mustard-rt run emit-phase` que já era a fonte canônica.
- `apps/cli/templates/commands/mustard/approve/SKILL.md` (edição) — idem.
- `apps/cli/templates/commands/mustard/resume/SKILL.md` (edição) — idem.
- `apps/cli/templates/commands/mustard/close/SKILL.md` (edição) — idem.
- (Investigar) qualquer módulo em `apps/rt/src/run/` ou `apps/rt/src/hooks/` que escreva `phaseName` em pipeline-state — remover (`complete_spec.rs` é candidato).
- Testes: `apps/dashboard/src-tauri/tests/` — `dashboard_specs` retorna `phase: "ANALYZE"` após `emit_phase` para ANALYZE.
- `.claude/.docs-audit.json` (novo) — registry de termos obsoletos por spec arquitetural; seed: eliminate-bun + esta spec.
- `apps/rt/src/run/docs_stale_check.rs` (novo) — subcomando `docs-stale-check`: lê `.docs-audit.json`, globa CLAUDE.md / pipeline-config.md / `.claude/refs/**`, regex-match dos termos obsoletos, emite JSON `{ scanned, hits: [{file, line, pattern, from_spec, hint}] }`.
- `apps/rt/src/run/mod.rs` (edição) — registrar `mod docs_stale_check;` + variante `RunCmd::DocsStaleCheck { from: Option<String>, strict: bool }`.
- `apps/cli/templates/commands/mustard/close/SKILL.md` (edição) — invocar `docs-stale-check` na Verification Gate (warn por padrão, block sob `MUSTARD_DOCS_AUDIT_MODE=strict`).
- `apps/cli/templates/.artifacts.json` (edição) — registrar `.claude/.docs-audit.json` como artefato `first-party`.

## Tarefas

### general-purpose Agent (Wave 1 — Dashboard lê fase do SQLite)

- [x] Em `apps/dashboard/src-tauri/src/db.rs`, estender `specs_from_db` para que cada `SpecRow` retornado carregue `phase` derivada do `pipeline.phase` mais recente para aquele `spec` na tabela de eventos. Usar o mesmo padrão de query que `mustard_core::emit_phase::last_phase_for_spec` (reverse iterate / `ORDER BY ts DESC LIMIT 1` por spec).
- [x] Em `apps/dashboard/src-tauri/src/lib.rs::dashboard_specs` (linhas 527-552), adicionar `phase` à lista de campos enriquecidos a partir do DB. **DB ganha sobre FS** para `phase` (inverter o atual default — comentário inline na linha 512 fica obsoleto, atualizar).
- [x] Em `apps/dashboard/src-tauri/src/lib.rs::specs_from_fs` (linha 579+), remover a leitura de `v["phaseName"]` (linhas 602 e 1329). Continuar lendo `status`, `scope` e demais campos do JSON.
- [x] Teste de integração em `apps/dashboard/src-tauri/tests/`: cria DB temporário, emite `pipeline.phase` com `to: "ANALYZE"` para spec demo, chama `specs_from_db`, asserta `phase == "ANALYZE"`.
- [x] `pnpm --filter mustard-dashboard build` e `cargo build/test -p mustard-dashboard`.

### general-purpose Agent (Wave 2 — Limpar writes de `phaseName`) — depende da Wave 1

- [x] Remover de `apps/cli/templates/commands/mustard/feature/SKILL.md` qualquer instrução para gravar `phaseName` no `.claude/.pipeline-states/{spec}.json`. Manter as chamadas a `mustard-rt run emit-phase` (canônicas). Os outros campos do JSON (status, scope, tasks, wave) permanecem.
- [x] Mesmo em `approve/SKILL.md`, `resume/SKILL.md`, `close/SKILL.md`.
- [x] Buscar em `apps/rt/src/` por módulos que escrevem/leem o campo — remover. **Escopo expandido sob diretiva do usuário ("decidimos não mais usar JSON e sim SQL, resolva isso"):** migrados todos os leitores rt para SQLite via `pipeline.phase` events — `close_gate.rs`, `path_guard.rs`, `post_edit.rs`, `event_projections.rs`, `statusline.rs`, `epic_fold.rs`. Helper `last_phase_for_spec` agora `pub` em `emit_phase.rs`. `gate_close_for_spec` movido inline para `emit-phase --to CLOSE` (gate ativo, não mais passivo via Write/Edit matcher). Dead `run_pipeline_phase` em `post_edit.rs` deletado. `epic_fold.rs` para de escrever `phaseName` (mantém `phase` como shape; emite `pipeline.phase` event).
- [x] Notar nos arquivos editados, em comentário breve, que `phaseName` foi removido: agora a fase vem de `pipeline.phase` no SQLite (linkar à spec).
- [x] Atualizar `apps/dashboard/CLAUDE.md` (Shared memory section) e `.claude/pipeline-config.md` (Shared Memory Architecture / Persistent projections): refletir que `phaseName` no pipeline-state JSON foi descontinuado; fase vem do SQLite.
- [x] `cargo build -p mustard-rt -p mustard-cli` e re-rodar o dashboard build para garantir que nada quebrou. **498 testes rt verdes + 13 dashboard verdes + builds limpas.**

### general-purpose Agent (Wave 3 — Linter `docs-stale-check` + close-gate audit) — depende da Wave 2

- [x] Autorar `.claude/.docs-audit.json`: schema `{ "version": 1, "audits": [{ "from_spec", "closed_at", "obsolete_terms": [<regex>], "replacement_hint" }] }`. Seed inicial com 2 entradas: (a) `eliminate-bun` com termos `events\.jsonl.*truth source`, `harness-views\.js`, `session-knowledge\.js`, `memory-persist\.js`, `harness-init\.js`, hint apontando pra SQLite + `SqliteEventStore`; (b) esta spec (`2026-05-19-dashboard-phase-from-sqlite`) com termo `phaseName.*pipeline-state`, hint apontando pra `pipeline.phase` no SQLite.
- [x] Implementar `apps/rt/src/run/docs_stale_check.rs`: lê `.docs-audit.json`, globa targets (root `.claude/refs/**`, root `.claude/commands/**`, `.claude/pipeline-config.md`, root `CLAUDE.md`, `apps/*/CLAUDE.md`), exclui nested install copies (`apps/*/.claude/**` por default, opt-in via `--include-nested` ou `MUSTARD_DOCS_AUDIT_INCLUDE_NESTED=1`). Saída JSON `{ scanned, hits, scanned_errors }`. `--from <spec>` filtra audit; `--strict` exit≠0 com hits. Fail-open.
- [x] Registrar em `apps/rt/src/run/mod.rs`: `mod docs_stale_check;` + variante `RunCmd::DocsStaleCheck { from: Option<String>, strict: bool, include_nested: bool }`.
- [x] Editar `apps/cli/templates/commands/mustard/close/SKILL.md` — bloco "Docs Audit (narrative drift)" antes de "Surface Accumulated Concerns": `mustard-rt run docs-stale-check`; warn default; `MUSTARD_DOCS_AUDIT_MODE=strict` bloqueia.
- [x] Editar `apps/cli/templates/.artifacts.json` — registro `hook:docs-audit` (fallback de `config` → `hook` porque `ArtifactCategory` em `packages/core/src/model/provenance.rs` só aceita {Skill,Recipe,Ref,Command,Hook,Tool}).
- [x] Dogfood: `cargo run -p mustard-rt -- run docs-stale-check --from eliminate-bun` → `hits: []` ✓ (após ajuste de glob default + 1-line surgical em `apps/dashboard/CLAUDE.md:59` removendo menção literal a `harness-views.js`).
- [x] `cargo build -p mustard-rt` + `cargo test -p mustard-rt` (507 tests passed, 8 novos em `docs_stale_check`).

## Dependências

- **Spec ascendente:** `eliminate-bun` (CLOSE 2026-05-19) — estabeleceu storage SQLite único. Esta spec termina a migração no lado de leitura do dashboard.
- **Prerequisite de:** `2026-05-19-artifact-update-followups` Wave 3 — surface de "artefatos defasados" no dashboard depende de phase reading consistente.
- **Independente de:** b6 (não toca o registro de projetos nem o fluxo install/update).

## Limites

- `apps/dashboard/src-tauri/src/` (`db.rs`, `lib.rs`)
- `apps/dashboard/src-tauri/tests/` (novo teste)
- `apps/cli/templates/commands/mustard/{feature,approve,resume,close}/SKILL.md`
- `apps/rt/src/` (módulos que escrevem `phaseName` — cirúrgico; + `apps/rt/src/run/docs_stale_check.rs` + `apps/rt/src/run/mod.rs`)
- `apps/dashboard/CLAUDE.md`, `.claude/pipeline-config.md` (notas de descontinuação)
- `.claude/.docs-audit.json` (novo)
- `apps/cli/templates/.artifacts.json` (registro do `.docs-audit.json` como first-party)
- **Fora dos limites:** o `emit_phase.rs` (já correto); a UI dos cards (`LivePipelineCard.tsx` etc.); migração/cleanup dos JSONs `.pipeline-states/*` existentes (legacy `phaseName` é ignorado, não removido); qualquer trabalho do b6 ou do artifact-update.
