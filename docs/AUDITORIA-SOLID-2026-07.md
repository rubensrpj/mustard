# Auditoria de saúde estrutural — 2026-07-18

> Análise read-only da janela ~2026-07-01 → HEAD (foco merges #53–#59), sob os princípios do projeto: SOLID/SRP, agnosticismo, fail-open disciplinado, sem facades, ProjectConfig dono único, write_atomic, zero unwrap/expect fora de teste, saída determinística. Cada achado ancorado em file:line lido de verdade. **Nenhum item deste relatório foi executado — decisão item a item.**

## Sumário executivo

Saúde geral: **boa, com três focos de drift reais**. A família `scan_patterns/` (sweep/list/apply/origin/decline) é exemplar em SRP, fail-open, `write_atomic` e determinismo — a suspeita sobre ela NÃO se confirma. Os três achados mais graves: (1) prosa pt-BR hardcoded em artefatos gerados/injetados; (2) `close_gate.rs` é um motor de política morando na camada de hook, com o único ciclo commands→hooks do crate; (3) `agent_prompt_render.rs` virou god-file (~1.720 linhas de lógica, 7 sub-motores separáveis), o que mais cresceu na janela (+365).

## Achados priorizados

### P1

1. **pt-BR hardcoded em saída gerada/injetada** — `apps/rt/src/commands/scan_claude.rs:133-137,278` + `apps/rt/src/commands/orient.rs:164-165,180` + `apps/rt/src/commands/work_unit_open.rs:122,225-231,264-271,285,330-334`. "Tipo: … arquivos", "[Terreno] subprojetos mapeados…", mensagens de erro do hook — sem consultar `mustard.json#lang` nem o `platform/i18n.rs` existente; um projeto EN recebe bytes em português versionados, enquanto `scan_patterns/origin.rs:19-20` documenta a política oposta ("English by policy"). *Remédio:* rotear pelas duas políticas já existentes (EN canônico para artefato de máquina; `ProjectConfig::i18n()` para texto de usuário). Esforço **M**.

2. **`close_gate.rs` (2.019 linhas; lógica até :1426) — SRP + acoplamento invertido.** Os 5 sub-gates são separáveis (debt :186-438, checklist :440-626, QA :628-773, build-runner :775-871, orquestração :909-1425); o motor mora em `hooks/` mas é consumido por commands: `emit_phase.rs:91` (`gate_close_for_spec`) e `emit_pipeline.rs:1146` (`qa_gate_active`) — único ciclo commands→hooks; o próprio módulo (:970-977) diz que o consumidor real em regime é `emit-phase --to CLOSE`. *Remédio:* extrair o motor para `commands/pipeline/close_gates.rs` (ou `shared/`), hook vira adapter fino de `Check`. Esforço **M**.

3. **`agent_prompt_render.rs` (2.581 linhas; testes de :1722) — god-file em formação.** Sub-motores com costuras limpas: prompt-ref/cache (:132-270), contratos de role (:573-780), cortador de seções (:802-1066), composição de retry (:1111-1209), motor BM25 de capabilities (:1210-1440), prateleira de skills (:1441-1484), reference-files/tree-sitter (:1485-1600). `render_prompt_at` (:271-493) já é um orquestrador limpo — extração mecânica para `commands/agent/render/`. Esforço **M**.

### P2

4. **`hooks/bash/safety.rs:180-188` — agnosticismo de branch.** `is_branch_delete_main` protege só os literais `main|master`, ignorando `mustard.json#git.flow` — num projeto `develop/master`, deletar `develop` passa; `work_branch_gate::is_protected` (:873-877) já deriva do flow corretamente. *Remédio:* `integration_bases()` com main/master como fallback documentado. Esforço **S**.

5. **Três runners de shell com timeout divergente** — `close_gate.rs:791-871` (não mata o filho no timeout — vaza processo), `review_gate.rs:162-169+` (mata), `qa_run/runner.rs:191-256` (poll+kill). *Remédio:* runner único em `shared/proc.rs` preservando a semântica env-error/real-failure de cada chamador. Esforço **M**.

6. **`Cargo.toml:146` — `expect_used` nunca foi negado** (o Guard promete zero unwrap/expect). 2 `.expect(` em produção: `event_projections/mod.rs:475`, `install_grammars.rs:98`. *Remédio:* `expect_used = "deny"` no workspace + degradar os 2 pontos. Esforço **S**.

7. **`emit_pipeline.rs:168-447` — `run()` com ~280 linhas** e seis blocos de efeito por kind embutidos (:297,:328,:366,:383,:398,:412). *Remédio:* tabela kind→efeitos; `run()` reduz a validar-emitir-aplicar. Esforço **S/M**.

8. **Parser de frontmatter duplicado com drift** — `scan_patterns/origin.rs:39-65` (tolera BOM) vs o canônico `core/domain/skill/frontmatter.rs:291-363` (não tolera; CRLF diferente); `mold_gate.rs:165` usa o canônico, o irmão de família usa o ad-hoc. *Remédio:* BOM + accessor `source` no core; origin consome. Esforço **S/M**.

9. **`file_path_of` ×7 cópias** — `boundary_gate.rs:407` (pub(crate)), `size_gate.rs:137`, `scope_guard.rs:78` (variante), `post_edit.rs:66`, `close_gate.rs:157`, `rewave_observer.rs:56`, `wikilink_footer_observer.rs:38`. *Remédio:* método em `HookInput` (core) e deletar as cópias. Esforço **S**.

### P3

10. **`resolve_mode` ×3** (`size_gate.rs:59`, `close_gate.rs:77`, `close_gate.rs:105-117`) → uma função com `default: GateMode`. **S**.
11. **Fallback de spec-dir ×7 sites** (dispatch_plan :99-104, resume_bootstrap :171, digest_adherence :128, emit_pipeline :462, tactical_fix_create :74,123, tactical_fix_detect :136,160) → `ClaudePaths::spec_dir_or_unchecked`. **S**.
12. **Listas de prune ×4** (sweep :29, docs_stale_check :48, doctor_i1 :55, review_gate :143) → `PRUNE_DIRS` compartilhada. **S**.
13. **Corte de seção `## X` ×4 implementações** (close_gate :538-611, render :925-977 e :525-549, scan_claude :369-396) → helper `section_body` no core; migração oportunista. **M**.
14. **`close_gate.rs:1262`** relê e re-ordena todo o NDJSON logo após :1103 já tê-lo feito → propagar `qa_ts`. **S**.
15. **`shared/context.rs` (709 linhas)** — observação: ambiente + store de markers coabitam; se ganhar mais um marker, extrair `shared/markers.rs`. **S** (quando chegar a hora).

## Veredito de agnosticismo

**OK (fallback/detecção documentados):** `config.rs:97-115` (main/master é A lei); `command_detect.rs`, `source_lang.rs:58`, `vocabulary/stacks.toml` (casa sancionada de nomes de stack); listas de prune (higiene de walk); `size_gate.rs:377,401` (detector de geração de script — detecção é a função); `doctor.rs:442,459` (sondas por stack); `render:412` + `origin.rs:19-20` (política EN in loco); `artifact_update.rs:629,680` (`ref: "main"` do próprio repo mustard).

**Borderline:** `qa_run/runner.rs:19-56` e `verify_pipeline.rs:55-69,137` — teto de timeout por NOME de stack (`cargo` 600s, resto 120s). Consequência real: build lento não-Rust estoura 120s → AC skip → `deny-qa-skip` estrito. Preferir teto por config com os literais rebaixados a heurística de fallback.

**Violação:** `safety.rs:186` (achado 4) e o pt-BR do achado 1. Branch literais restantes: todos em `#[cfg(test)]` — limpos.

## O que NÃO mexer (falsos-positivos que um refactor apressado quebraria)

- A família `scan_patterns/` como está — SRP por arquivo é a virtude; `fold_collisions` (união-nunca-soma) e o worklist sem cap são invariantes medidos em campo.
- `matches_affix` `"folder" => true` — é o conserto do defeito que descartava convenção folder-borne; residência É a convenção.
- `close_gate` não reusar `run_build` do bash_guard — divergência contratual (paridade env-error com o JS); a consolidação (achado 5) precisa preservar a semântica por chamador.
- `#[allow(too_many_lines)]` em `run_close_gates` — o ganho é mover o módulo (achado 2), não picar a função.
- Hooks→commands documentados (worktree_create→work_unit_open, active_spec_limit→count_active, subagent_inject→EPISTEMIC_FLOOR) — costuras deliberadas; o anômalo é só o sentido inverso.
- `wave_scaffold.rs` e `dispatch_plan.rs` — coesos; tamanho vem de teste.
- `parse_payload_tolerant` (emit_pipeline :144-156) — absorve classe real de erro de quoting; remover quebra campo.

## Resolução — execução (2026-07-16 → 07-18)

Ordem de execução: **H** (P1 estruturais) → **I** (DRY P2/P3) → **J** (médios). Suíte verde a cada passo (HEAD: 3782 passed, 6 ignored); clippy exit-0.

**H — P1 (achados 2, 3).** `close_gate` motor extraído para `commands/pipeline/close_gates.rs` (`run_close_gates` + sub-gates debt/checklist/QA/build-runner); o hook `hooks/write/close_gate.rs` virou adapter fino — **ciclo commands→hooks eliminado**. `agent_prompt_render.rs` fatiado em `commands/agent/render/{mod,prompt_ref,role,sections,retry,capabilities,skills,reference}.rs` com façade no caminho antigo (zero churn de consumidor).

**I — P2/P3 (achados 8, 9, 10, 11, 12).** `file_path_of` ×7 → `HookInput::file_path()` no core. `resolve_mode` ×3 → `shared/gate_mode.rs`. Prune ×4 → `PRUNE_DIRS` no core. Spec-dir fallback ×7 → `ClaudePaths::spec_dir_or_unchecked`. Frontmatter: `origin.rs` consome o parser canônico do core (BOM + accessor `source` adicionados). Os 2 `.expect(` de produção (achado 6) foram degradados aqui (index no lugar de `.last_mut().expect`; fail-open + `Default` no `install_grammars`).

**J — médios (achados 4, 5, 7, 13).**
- **4 (safety branch):** `is_branch_delete_protected(cmd, bases)` deriva de `GitConfig::integration_bases()` (main/master vira fallback documentado, não literal). `bash_safety` split em `bash_safety_with_bases` (puro/testável) + `guard_integration_bases` (fail-open).
- **5 (runners):** o **leak de processo** (o bug real) foi corrigido nos TRÊS runners — `close_gates::run_command`, `review_gate::run_build`, `rtk_rewrite` — via poll+kill (`try_wait` com o `Child` na thread chamadora; `kill`+`wait` no timeout). A **unificação num runner único** foi **declinada por projeto**: a semântica env-error/real-failure diverge por chamador (a própria linha "O que NÃO mexer" acima já avisava) e forçá-la num só ponto obscurece mais que ajuda. *Follow-up:* os três leem stdout/stderr só após o exit — deadlock latente se a saída passar de ~64KB de pipe; é **pré-existente e idêntico** à versão anterior (não é regressão), fica para um passo dedicado de drain concorrente.
- **7 (emit_pipeline SRP):** `run()` de ~280 → ~60 linhas (validar → emitir → aplicar-efeitos via `match kind`); 12 helpers extraídos, um por efeito.
- **13 (corte de seção):** `section_end(lines, heading_idx)` extraído em `commands/spec/spec_sections.rs`, irmão de `is_heading` (todos os consumidores são da face rt — daí ali, não no core). **4 sites migrados byte-exato:** a própria `section_blocks`, `render::cut_section_at`, `reference::files_section_paths`, o swap de `## Guards` do `scan_claude`. **2 outliers mantidos com NB documentada:** `checklist_unmarked_in` (fronteira inclui bare `##`) e `read_guards_block` (tolera `## ` indentado) — folding mudaria comportamento, não removeria duplicação.

**Achado 1 (pt-BR em saída) — FEITO (2026-07-18), pelas duas políticas do próprio remédio.** Decisão do user: artefatos de orientação seguem `mustard.json#lang`.
- **Artefatos DISPLAY → `mustard.json#lang`:** `scan_claude::render_map` (scan-map.md) e `orient::render_terrain` (banner de terreno) agora recebem o locale e rendem pelo catálogo i18n (chaves `scan.map.type_line`/`scan.map.pointer`/`orient.terrain.header`/`orient.census.files_suffix`). Locale resolvido uma vez via `project_config_cached(root).i18n().lang`; default `pt-BR` preserva installs existentes byte-a-byte (testes pt-BR verdes + 2 testes EN novos provando a rota).
- **Diagnósticos → inglês canônico (carve-out logs/diagnóstico):** as 9 strings de `work_unit_open.rs` (`WorktreeCreate: …`, hint de CLI) viraram inglês — são erros técnicos de git/worktree, não prosa de usuário; roteá-los pelo catálogo de banners seria misturar diagnóstico com texto de usuário, e o resto do crate já erra em inglês. (Se o user preferir que esses diagnósticos TAMBÉM sigam `lang`, é um passo de i18n adicional — alavanca registrada.)
- **Legado deixado de propósito:** `scan_claude.rs` PLACEHOLDERS `<!-- seed DO/DON'T aqui -->` (padrão de DETECÇÃO retro-compatível de stub, não emitido) e a fixture `render_migrates_legacy_inline_block_out` (bytes históricos) — pt-BR ali representa o que binários antigos escreveram; mudá-los quebraria migração/fixture.

**Adiado (decisão pendente ou baixo valor):**
- **6 (`expect_used = deny`):** **revertido** — desproporcional a um P2: ~32 arquivos de teste usam `expect` em helpers FORA de `#[test]` (o `allow-expect-in-tests` do clippy não os cobre), e o guard quebrava o clippy nesses. Os 2 pontos de produção já foram degradados (em I). Fica como follow-up se a política endurecer.
- **14 (qa_ts) e 15 (`context.rs` split):** P3 oportunistas, não tocados.
