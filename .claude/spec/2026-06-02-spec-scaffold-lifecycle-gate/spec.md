# Higiene do scaffold de spec, finalização do lifecycle e gate de aprovação Full scope do mustard

<!-- drafter:tone=didactic -->

<!-- PRD -->

## Contexto

Uma spec gerada pelo mustard num projeto cliente (`sialia/.claude/spec/contas-pagar-reorganizar-formulario-insercao`) expôs sete defeitos do **tool** (não da spec): a spec foi concluída mas ficou presa em `Plan/Active`; checkboxes que nenhum código marca; `meta.json` morto repetido em `qa/`/`review/`; `qa/`/`review/` com `spec.md` template vazio e nenhum resultado materializado; a pasta `.events/` subindo ao git; o `memory/_index.md` nascendo com `<missing-key>`; e — o mais grave — o `/feature` Full scope emendando implementação após uma pergunta, sem passar pelo gate de aprovação `/spec`.

Esta spec corrige a **causa-raiz no código do mustard**. Nada aqui altera specs já geradas; o conserto mora em `apps/rt`, `packages/core`, `apps/cli` e `apps/dashboard`.

Âncoras (do scan + exploração):

- `apps/rt/src/commands/spec/spec_scaffold.rs` — `write_spec_md` emite `## Tarefas` (~73-79) e `## Checklist` (~82-88); `write_meta_json` (~107).
- `apps/rt/src/commands/spec/spec_draft.rs` — `plan_section_default:346` (`"tasks" => "- [ ] T1 — ..."`), `build_checklist:289`, `write_memory_stub:428` (chama `translate("memory.index.intro"/"empty")`), chamada incondicional em `~192`.
- `apps/rt/src/commands/wave/wave_scaffold.rs` — `run` cria `meta.json` em raiz/wave/review/qa (`write_scaffold_meta`, ~365-449); `render_review:195` / `render_qa:215`.
- `apps/rt/src/commands/spec/complete_spec.rs` — `emit_ndjson:151`, `mark_followup:175`, `emit_completed_status:225` emitem eventos via `writer_ndjson` direto, sem sincronizar o `meta.json`.
- `apps/rt/src/commands/event/emit_pipeline.rs` — `patch_meta_complete:622`, `sync_status`, `patch_meta_for_transition`, `meta_path_for:550` (só resolve raiz e `wave-N`, nunca `qa/`/`review/`).
- `apps/rt/src/commands/review/qa_run.rs` — computa pass/fail dos AC, emite `qa.result` (NDJSON + stdout); nada vai para `qa/report.md`.
- `apps/rt/src/commands/review/review_result.rs` — emite `review.result`; nada vai para `review/verdict.md`.
- `packages/core/src/platform/i18n.rs` — `translate` (fail-back para `<missing-key>` em `:485`); chaves `memory.index.intro`/`empty` ausentes (vizinhas em ~295-300).
- `packages/core/src/io/claude_paths.rs` — `events_dir:587` (`spec_dir/.events`), `metrics_dir`.
- `apps/cli/src/commands/init.rs` — copia `apps/cli/templates/` → `.claude/` (~117-128); não há `.gitignore` no template (só `.obsidian/.gitignore`).
- `apps/cli/templates/commands/mustard/feature/SKILL.md` — único gate é a pergunta "Approve and implement?"; §4 EXECUTE inline não está restrito a Light.
- `apps/rt/src/hooks/write/` (`close_gate.rs:417` `find_unmarked_checklist`, `pre_edit_intent_gate.rs`, `mod.rs`) e `apps/rt/src/hooks/task/mod.rs` — infraestrutura de `Check`/`Verdict::Deny`.
- `apps/rt/src/shared/context.rs:110` — `current_spec` (spec ativa); `apps/rt/src/commands/pipeline/resume_bootstrap/post_execute_gate.rs` — aceita Execute sem checar aprovação.
- Consumidores que acompanham: `apps/rt/src/commands/agent/agent_prompt_render.rs:331,352`; `packages/core/src/domain/spec/contract.rs` (`ChecklistEmpty`); `apps/rt/src/commands/migrate/migrate_to_meta.rs`; `apps/dashboard/src-tauri/src/lib.rs:686` (`dashboard_spec_markdown`); `apps/dashboard/src/features/specs/SpecMarkdownViewer/index.tsx:69`.

Por que agora: cada nova spec gerada herda esses defeitos; o gate furado é um risco ativo (implementa código sem aprovação).

## Usuários/Stakeholders

Operadores do pipeline SDD do mustard (quem roda `/feature`, `/spec`, `/qa`, `/close`) e quem lê o resultado no dashboard. Eles precisam de specs que: reflitam o estado real no `meta.json`, não poluam o git, materializem o resultado de QA/review onde o esperam, e — acima de tudo — nunca implementem código sem aprovação explícita.

## Métrica de sucesso

- Toda spec concluída reflete `Close/Completed` no `meta.json` da raiz (zero specs "concluídas mas presas em Plan/Active").
- Zero `.events/` versionados em projetos inicializados pelo mustard.
- Full scope nunca chega a EXECUTE sem um evento de aprovação emitido pelo `/spec`.
- Resultado de QA e review materializado deterministicamente (`qa/report.md`, `review/verdict.md`) e visível no dashboard.
- `meta.json` morto eliminado de `qa/`/`review/`; `memory/_index.md` nunca exibe `<missing-key>`.

## Não-Objetivos

- NÃO alterar specs já geradas em projetos clientes (a correção é só no tool).
- NÃO migrar o layout flat de `.claude/spec/` nem remover o `meta.json` por-wave (esse permanece e passa a transicionar).
- NÃO reescrever o dashboard além do mínimo para exibir `qa/report.md` e `review/verdict.md`.
- NÃO remover a captura de memória da spec (`spec-memory`) — apenas gerar o `_index.md` sob demanda em vez de stub vazio.
- NÃO trocar o mecanismo de eventos NDJSON; apenas religar o caminho de fechamento para sincronizar o sidecar.

## Critérios de Aceitação

- **AC-1** — Workspace compila, testa e linta verde.
  Command: `cargo test && cargo clippy --all-targets`
- **AC-2** — Após `complete-spec`, o `meta.json` da raiz fica `stage=Close, outcome=Completed`.
  Command: `cargo test -p mustard-rt status_sync_integration`
- **AC-3** — O scaffold de ondas não cria `meta.json` em `qa/` nem `review/`.
  Command: `cargo test -p mustard-rt -- wave_scaffold && cargo test -p mustard-rt spec_invariants`
- **AC-4** — `qa-run` grava `qa/report.md` e `review-result` grava `review/verdict.md`; o dashboard os lê.
  Command: `cargo test -p mustard-rt -- qa_run review_result`
- **AC-5** — A spec-pai com ondas não emite `## Tarefas`/`## Checklist`; a spec sem ondas mantém ambos (Tarefas sem `[ ]`, Checklist com `[ ]` + `→ caminho`).
  Command: `cargo test -p mustard-rt -- spec_scaffold && cargo test -p mustard-core -- contract`
- **AC-6** — As chaves `memory.index.intro`/`memory.index.empty` existem (PT-BR + EN-US); `_index.md` só nasce com captura.
  Command: `rg -n "memory.index.intro|memory.index.empty" packages/core/src/platform/i18n.rs`
- **AC-7** — Editar arquivo de produção com spec Full em `Plan` sem aprovação é negado pelo hook; `init` provisiona `.gitignore` cobrindo `.events/`.
  Command: `cargo test -p mustard-rt -- scope_guard && rg -n "\.events/" apps/cli/templates/.gitignore`

<!-- PLAN -->

## Arquivos

**Frente 1 — Spec-pai com ondas vira coordenação (sem `## Tarefas`/`## Checklist`); seção Tarefas sem checkbox:**

- `apps/rt/src/commands/spec/spec_scaffold.rs` — em `write_spec_md`, suprimir o bloco de `plan_sections` "tasks" e o bloco `## Checklist` quando `is_wave_plan` (derivável de `input.total_waves > 0`). Spec sem ondas mantém ambos.
- `apps/rt/src/commands/spec/spec_draft.rs` — `plan_section_default` (`:346`): `"tasks"` passa a `"- T1 — ..."` (lista, sem `[ ]`).
- `packages/core/src/domain/spec/contract.rs` — a regra `ChecklistEmpty` não se aplica à spec-pai wave-plan (sem checklist é estado válido nela).
- `apps/rt/src/hooks/write/close_gate.rs` — `find_unmarked_checklist` (`:417`) passa a olhar os checklists das ondas (não a pai-coordenação) quando a spec é wave-plan.
- `apps/rt/src/commands/agent/agent_prompt_render.rs` — `cut_tasks_section`/`extract_tasks_section` (`:331,352`) leem a seção Tarefas da onda, não da pai.

**Frente 2 — `meta.json` só na raiz + por onda; remover de `qa/` e `review/`:**

- `apps/rt/src/commands/wave/wave_scaffold.rs` — remover as duas chamadas `write_scaffold_meta` para `review/` e `qa/` (~404-435). Mantêm-se raiz e `wave-N`.
- `apps/rt/src/commands/migrate/migrate_to_meta.rs` — não criar `meta.json` ao lado de `qa/spec.md`/`review/spec.md`.

**Frente 3 — `complete_spec` sincroniza o `meta.json` da raiz no fechamento:**

- `apps/rt/src/commands/event/emit_pipeline.rs` — tornar `patch_meta_complete` (e o necessário de `sync_status`) acessível (`pub(crate)`).
- `apps/rt/src/commands/spec/complete_spec.rs` — `emit_completed_status` passa a sincronizar o `meta.json` da raiz (chamar `patch_meta_complete`) após emitir o evento terminal.

**Frente 4 — `qa/` e `review/` materializam resultado por código:**

- `apps/rt/src/commands/review/qa_run.rs` — após computar/emitir `qa.result`, gravar `.claude/spec/{spec}/qa/report.md` (consolida cada AC + pass/fail) de forma atômica.
- `apps/rt/src/commands/review/review_result.rs` — após `review.result`, gravar `.claude/spec/{spec}/review/verdict.md`.
- `apps/rt/src/commands/wave/wave_scaffold.rs` — `render_qa`/`render_review` deixam de gerar `spec.md` template (ou viram só índice apontando para `report.md`/`verdict.md`).
- `apps/dashboard/src-tauri/src/lib.rs` — `dashboard_spec_markdown` (~686) passa a ler `qa/report.md` e `review/verdict.md`.
- `apps/dashboard/src/features/specs/SpecMarkdownViewer/index.tsx` — apontar os candidatos para os novos arquivos.

**Frente 5 — `init` provisiona `.gitignore`:**

- `apps/cli/templates/.gitignore` (NOVO) — cobrir `.events/`, `.metrics/` (e demais efêmeros).
- `apps/cli/src/commands/init.rs` — garantir que o `copy_dir` leva o `.gitignore` para `.claude/.gitignore` (ou gerá-lo se ausente).

**Frente 6 — i18n + `_index.md` sob demanda:**

- `packages/core/src/platform/i18n.rs` — adicionar `memory.index.intro` e `memory.index.empty` (PT-BR + EN-US).
- `apps/rt/src/commands/spec/spec_draft.rs` — `write_memory_stub` deixa de ser chamado incondicionalmente no draft; a geração/atualização do `_index.md` migra para o `spec-memory create` (primeira captura).

**Frente 7 — Gate de aprovação Full scope (prosa + hard-gate no Rust):**

- `apps/cli/templates/commands/mustard/feature/SKILL.md` — cláusula inviolável: Full scope **para** no PLAN e exige `/spec`; §4 EXECUTE inline é **só** Light/Extended-Light.
- `apps/rt/src/commands/event/...` — definir/emitir o sinal de aprovação canônico que **só** o `/spec` emite (ex.: `pipeline.status: approved`).
- `apps/rt/src/hooks/write/scope_guard.rs` (NOVO) — `Check` que nega Edit/Write de arquivo de produção quando a spec ativa é `scope=full`, `stage=Plan` e sem evento de aprovação; libera edição da própria `spec.md`.
- `apps/rt/src/hooks/write/mod.rs` + `apps/rt/src/hooks/task/mod.rs` — registrar o guard (incl. dispatch de Task).
- `apps/rt/src/commands/pipeline/resume_bootstrap/post_execute_gate.rs` — não aceitar início de Execute (Full) sem o evento de aprovação.

## Tarefas

- T1 — Frente 3 (lifecycle): religar `complete_spec` ao `patch_meta_complete`; spec concluída reflete `Close/Completed`.
- T2 — Frente 2: remover `meta.json` de `qa/`/`review/` no scaffold e na migration; ajustar testes de invariante.
- T3 — Frente 4: materializar `qa/report.md` e `review/verdict.md`; dashboard exibir; aposentar o `spec.md` template das fases.
- T4 — Frente 1: spec-pai wave-plan sem `## Tarefas`/`## Checklist`; Tarefas sem `[ ]`; ajustar `contract`, `close_gate`, `agent_prompt_render`.
- T5 — Frente 6: chaves i18n `memory.index.*`; `_index.md` sob demanda no `spec-memory create`.
- T6 — Frente 5: `.gitignore` no template + `init` provisiona em `.claude/`.
- T7 — Frente 7: cláusula no `SKILL.md` + evento de aprovação + hook `scope_guard` + `resume_bootstrap` bloqueando Execute sem aprovação.

## Decisões de design

- **D1** — Seções acionáveis vivem no nível que executa: nas ondas quando há decomposição; na própria spec quando não há. A spec-pai com ondas é documento de coordenação.
- **D2** — `## Tarefas` é roteiro do agente → lista simples sem checkbox. Só `## Checklist` mantém `[ ]` e auto-mark por `→ caminho`.
- **D3** — `meta.json` é fonte única do lifecycle, mas só onde há lifecycle: raiz e `wave-N`. `qa/`/`review/` são fases, não specs → sem sidecar.
- **D4** — O resultado de QA/review é materializado pelo tool (`qa/report.md`, `review/verdict.md`), não dependente de o agente lembrar de escrever.
- **D5** — O hard-gate confia num **evento de aprovação explícito** (emitido só pelo `/spec`), não em `stage=Execute` — senão o `/feature` auto-emitindo Execute burla o gate (a falha observada).
- **D6** — `_index.md` é criado/atualizado na primeira captura de conhecimento, não como stub em toda spec.
- **D7** — Efêmeros (`.events/`, `.metrics/`) são gitignored pelo `init`, alinhado ao "not versioned" já documentado no código.

## Limites

IN:
- Código do tool: `apps/rt`, `packages/core`, `apps/cli` (init + templates), `apps/dashboard` (só leitura de `report.md`/`verdict.md`).
- Testes e snapshots que assertam a estrutura alterada.

OUT:
- Specs já geradas em projetos clientes.
- Layout flat de `.claude/spec/`; `meta.json` por-wave (permanece).
- Qualquer redesenho do dashboard além de exibir os novos artefatos.

## Checklist

- [x] T1 lifecycle → `apps/rt/src/commands/spec/complete_spec.rs`
- [x] T1 lifecycle → `apps/rt/src/commands/event/emit_pipeline.rs`
- [x] T2 meta qa/review → `apps/rt/src/commands/wave/wave_scaffold.rs`
- [x] T2 migration → `apps/rt/src/commands/migrate/migrate_to_meta.rs`
- [x] T3 qa report → `apps/rt/src/commands/review/qa_run.rs`
- [x] T3 review verdict → `apps/rt/src/commands/review/review_result.rs`
- [x] T3 dashboard → `apps/dashboard/src-tauri/src/lib.rs`
- [x] T4 coordenação → `apps/rt/src/commands/spec/spec_scaffold.rs`
- [x] T4 placeholder → `apps/rt/src/commands/spec/spec_draft.rs`
- [x] T4 close-gate → `apps/rt/src/hooks/write/close_gate.rs`
- [x] T5 i18n → `packages/core/src/platform/i18n.rs`
- [x] T6 gitignore → `apps/cli/templates/.gitignore`
- [x] T7 hard-gate → `apps/rt/src/hooks/write/scope_guard.rs`
- [x] T7 skill → `apps/cli/templates/commands/mustard/feature/SKILL.md`

## Concerns

- **close-gate (alto):** ao remover `## Checklist` da pai-coordenação, o `find_unmarked_checklist` deve passar a consolidar os checklists das ondas — senão o CLOSE passa sem checar nada (gate orfão). Cobrir com teste.
- **hard-gate falsos positivos (alto):** o `scope_guard` precisa liberar a edição da própria `spec.md`/artefatos `.claude/` durante o PLAN e só bloquear arquivos de produção; e definir bem o "evento de aprovação" para não travar fluxos legítimos (Light inline, resume pós-approve).
- **testes/snapshots:** `apps/rt/tests/spec_invariants.rs:125,130` (exige `meta.json` em todo dir com `*.md`), `apps/rt/tests/migrate_spec_headers.rs:463,489`, `packages/core/src/domain/spec/contract.rs` (testes de `ChecklistEmpty`), `apps/rt/src/commands/spec/spec_draft.rs:614` precisam acompanhar.
- **dashboard:** `dashboard_spec_markdown` hoje só lê `spec.md`/`wave-plan.md` dentro do subdir; sem o ajuste, `report.md`/`verdict.md` não aparecem (degrada silenciosamente).
- **binário:** as correções só valem após `cargo build` + reinstalação do binário; o tooling em execução ainda é o antigo.
- **spec ativa pré-existente:** `2026-06-02-analyze-feature-quando-anchors-scan-cruzam` (tactical-fix em ANALYZE) segue aberta — decidir se retoma ou abandona.
- `analyze-validation` (WARN, não-bloqueante): apontou "missing-file" para `scope_guard.rs`, os dois `.gitignore`, `qa/report.md`, `review/verdict.md` (todos **arquivos novos**, marcados `(NOVO)`), além de `qa.result`/`review.result` (nomes de **eventos**) e `memory.index.intro`/`empty` (**chaves i18n**). São **falsos positivos** — a validação casa por nome-base e os interpretou como caminhos. `qa/spec.md` e `review/spec.md` aparecem porque estão sendo **aposentados**, não referenciados como entrega.