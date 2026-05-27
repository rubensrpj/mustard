# Drenagem residual antes do CLOSE — no-sqlite (W1-W8 fechadas)

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T15:30:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec final de drenagem (wave-30-close-followups) que zera a dívida residual acumulada ao longo das waves W1-W8 antes do CLOSE da spec mãe. Foram triados 12 itens; 7 entram como Tier A (fix agora), 5 como Tier B com pointer para spec/wave futura. Justifica o teto de 5 arquivos (vs cap padrão) porque drenagem multi-item cabe em mudanças cirúrgicas (≤30 LOC por item) sem mexer em behavior — só limpeza de doc-comments, unused imports, dead fields, Cargo.toml deps e arquivo untracked.

### Triagem (12 itens)

| # | Item (origem) | Tier | Resolução |
|---|---|---|---|
| 1 | `rusqlite` em `Cargo.toml` raiz, `apps/rt/Cargo.toml`, `apps/dashboard/src-tauri/Cargo.toml` (`packages/core` já feito em W8A-4) | **A** | DELETE dep e comentários de pin em 3 Cargo.toml. Verificado: zero `use rusqlite` em `apps/rt/src/`, `apps/dashboard/src-tauri/src/`, `apps/dashboard/src-tauri/tests/` — só doc-comments mortos. |
| 2 | Boundary warnings espúrios apontando spec `2026-05-26-dashboard-i18n-migration` | **A** (verificado) | Wave-18 já filtrou via `Stage ∈ {Analyze,Plan,Execute}` em `apps/rt/src/hooks/post_edit.rs:457-465`. i18n-migration tem `Stage: Close` → filtrada corretamente. Causa real do "continua": binário em `$PATH` (`~/.cargo/bin/mustard-rt.exe`) é mais antigo que o source. Sem código pra mudar; basta `cargo install --path apps/rt --force` para propagar. Documentado como já-resolvido. |
| 3a | Dead-code `parse_iso_to_unix_secs` em `apps/dashboard/src-tauri/src/lib.rs` | **A** | DELETE (zero callers verificado via Grep) |
| 3b | Dead-code `days_since_epoch` em `apps/dashboard/src-tauri/src/lib.rs` | **A** | DELETE junto com 3a (caller único era 3a) |
| 3c | Dead-code `summarise_payload` em `apps/dashboard/src-tauri/src/spec_views.rs:1601` | **A** | DELETE (zero callers verificado via Grep) |
| 4 | 6 warnings unused-imports/dead-code (workspace build) | **A** | DELETE/fix cada um: `apps/rt/src/hooks/amend_capture.rs:45` (`use std::path::Path`), `apps/rt/src/hooks/auto_capture_summary.rs:25-26` (`ClaudePaths`, `Path`), `apps/rt/src/run/event_route.rs:51` (`is_pipeline_event` — usado em testes do mesmo módulo, manter com `#[cfg(test)]` ou marcar `#[allow(dead_code)]` — verificar antes de deletar), `apps/rt/src/run/memory.rs:980` (`DispatchExtras.id` — vide #5), `apps/rt/src/run/pipeline_state_ingest.rs:21` (`PipelineStateIngestOpts.delete`) |
| 5 | `DispatchExtras.id` morto-mas-preservado | **A** | DELETE field. Único caller em `apps/rt/src/run/mod.rs:1571` seta `None` com comentário legacy. Deletar field + caller. Comentário antigo (W4#3) era apenas para "shape compat", mas o campo é privado ao módulo, sem leitores. |
| 6 | `pipeline.telemetry.run` vs `pipeline.economy.run` aliases | **A** | Já documentado em `packages/core/src/economy/reader.rs:13-22` (tabela explicando attribution + alias) e `writer.rs:19,93-94` (companion channel docs). Verificado — sem trabalho. |
| 7 | `workspace_summary_v2` depende de `now_ms` doc-comment | **A** (verificado) | Função `workspace_summary_v2` não existe em `packages/core/src/`. Grep retorna zero matches em `core/src/`. Item descreve algo já refatorado em waves anteriores. Sem trabalho. |
| 8 | `spec-children` perdeu correlação `started_at`/`completed_at`/`reason`/`wave` | **B** | Já registrado em wave-12-rt/spec.md como simplificação aceita. Dashboard `spec_views.rs:1422-1425` ainda declara `started_at`/`completed_at`/`reason` em `SubSpecChild` — leitor consome do filesystem walk quando disponível. Re-introdução via NDJSON walk fica para futura wave dashboard se necessidade surgir. |
| 9 | `mcp/get_run_summary` smoke fixture | **B** | Defer para próxima spec (tactical-fix ou wave de hardening MCP). |
| 10 | 4 tests deletados em W5B planejados pra W8B | **B** | Verificado em `apps/rt/tests/`: `amend_capture.rs` + `amend_finalize.rs` restored (W8A-3 followup `ae16bdd`). `mcp` tests, `spec_children_tree` e `spec_hygiene` continuam sem reintro — defer pra spec de hardening de cobertura. |
| 11 | `waves-orchestrator-design.md` untracked | **A** | Movido para `.claude/plans/waves-orchestrator-design.md`. Pasta `.claude/plans/` é gitignored — o draft sai do tree visível na raiz sem entrar em VCS (é rascunho do user, não artefato versionável). |
| 12 | `cargo test --workspace` runtime completo | **A** (validação) | Roda na FASE 3 antes do commit. Matar `mustard-rt.exe` daemon se ativo. |

### Investigation log

- **#1**: `rtk grep -rn "use rusqlite|rusqlite::" apps/rt/src` → só 2 doc-comments em `auto_capture_summary.rs`. `rtk grep -rn "use rusqlite|rusqlite::" apps/dashboard/src-tauri/{src,tests}` → 1 doc-comment em `telemetry_aggregations_test.rs`. Tests W6B-rewritten não usam rusqlite real (placeholder `Connection` struct em `dashboard/src-tauri/src/db.rs`). Build verde após deleção.
- **#2**: `read_newest_fresh_state` em `path_guard.rs:174` lê `paths.pipeline_states_dir()` — diretório inexistente no repo (`.claude/pipeline-states/` não existe). Boundary warning vem do `post_edit::check_boundaries` que itera todos os spec dirs. `wave-18` adicionou filter `Stage ∈ {Analyze,Plan,Execute}` + `Outcome::Active`. Spec `dashboard-i18n-migration` tem `Stage: Close` → filtrada. Binário em PATH (`~/.cargo/bin/mustard-rt.exe` mtime 08:34) é mais antigo que source change (cargo build alvo `target/debug` mtime 15:32). Reinstalação: `cargo install --path apps/rt --force` — NÃO entra no escopo desta sub-spec (responsabilidade do user antes do CLOSE).
- **#3a/3b/3c**: `Grep` retorna apenas as próprias definições. `days_since_epoch` tem 1 caller (linha 1165), que é o próprio `parse_iso_to_unix_secs`. Deletar ambos em uma edição.
- **#4 (`is_pipeline_event`)**: Testes do próprio módulo `event_route.rs:244-247` referenciam o símbolo. Warning aparece porque é `pub` mas sem callers externos. Solução: tornar `pub(crate)` ou `#[allow(dead_code)]`. Outros callers `pipeline.` start_with-check existem como literal: `classify_kind` em event_route.rs:62 implementa a mesma lógica inline. Função real é redundante. AÇÃO: deletar `is_pipeline_event` + seu teste (2-3 LOC cada).
- **#4 (`PipelineStateIngestOpts.delete`)**: módulo é no-op pós-W2A. Field ignorado em todos os callers. AÇÃO: deletar o field (já documentado como "Retained for CLI compatibility — ignored"). Caller em `mod.rs:1588` passa `delete` — atualizar para `{}`.
- **#5**: `DispatchExtras.id` único caller é `mod.rs:1571` `id: None`. Após deleção do field, atualizar caller. Comentário "shape stays compatible" era cargo-cult pós-W4#3.
- **#11**: `waves-orchestrator-design.md` é design doc do user (24 princípios cravados, arquivos por spec, etc.). Não pertence ao escopo da no-sqlite. Mover para `.claude/plans/` evita poluir raiz e mantém o conteúdo acessível.

## Usuários/Stakeholders

Maintainer único (Rubens). Indireto: próxima sessão Mustard, que herda o repo sem warnings de build e sem lixo na raiz.

## Métrica de sucesso

- `rtk cargo build --workspace` retorna ZERO warnings (de 6 atuais para 0).
- `rtk cargo test --workspace --no-run` verde.
- `rtk cargo test --workspace` completo verde (com `mustard-rt.exe` daemon matado se necessário).
- `rtk git grep -lE "rusqlite" -- 'Cargo.toml' 'apps/**/Cargo.toml' 'packages/**/Cargo.toml'` retorna **zero**.
- SQLite mention count em código vivo ≤5 (sem regressão vs W8).
- `waves-orchestrator-design.md` movido para fora da raiz.

## Não-Objetivos

- Reintroduzir tests deletados em W5B (item #10) — fica para spec dedicada.
- Smoke fixture para `mcp/get_run_summary` (item #9) — fica para próxima spec.
- Reintroduzir correlação de spec-children (item #8) — fica para wave dashboard futura.
- Reinstalar binário `mustard-rt` em `~/.cargo/bin` (item #2) — responsabilidade manual do user pós-CLOSE.

## Critérios de Aceitação

- [ ] AC-1: Build workspace sem warnings — Command: `bash -c "cd /c/Atiz/mustard && cargo build --workspace 2>&1 | grep -E 'warning:' | wc -l | tr -d ' '"` espera `0`
- [ ] AC-2: Zero `rusqlite` em todos Cargo.toml — Command: `node -e "const cp=require('child_process');const r=cp.execSync('git grep -lE \"rusqlite\" -- Cargo.toml apps/**/Cargo.toml packages/**/Cargo.toml || true',{encoding:'utf8'}).trim();process.exit(r===''?0:1)"`
- [ ] AC-3: Cargo test workspace --no-run verde — Command: `cargo test --workspace --no-run`
- [ ] AC-4: Cargo test workspace runtime verde — Command: `cargo test --workspace`
- [ ] AC-5: `waves-orchestrator-design.md` não está mais na raiz — Command: `node -e "process.exit(require('fs').existsSync('C:/Atiz/mustard/waves-orchestrator-design.md')?1:0)"`
- [ ] AC-6: zero `use rusqlite` ou `rusqlite::` em código vivo (matches em doc-comments narrativos de migração são aceitos) — Command: `node -e "const cp=require('child_process');const out=cp.execSync('git grep -nE \"^[^/]*\\\\b(use rusqlite|rusqlite::)\" -- \"apps/*/src/**/*.rs\" \"packages/*/src/**/*.rs\" || true',{encoding:'utf8'});const lines=out.split(/\\n/).filter(Boolean);process.exit(lines.length===0?0:1)"`

## Arquivos

- `Cargo.toml`
- `apps/rt/Cargo.toml`
- `apps/dashboard/src-tauri/Cargo.toml`
- `apps/rt/src/hooks/amend_capture.rs`
- `apps/rt/src/hooks/auto_capture_summary.rs`
- `apps/rt/src/run/event_route.rs`
- `apps/rt/src/run/memory.rs`
- `apps/rt/src/run/mod.rs`
- `apps/rt/src/run/pipeline_state_ingest.rs`
- `apps/dashboard/src-tauri/src/lib.rs`
- `apps/dashboard/src-tauri/src/spec_views.rs`
- `.claude/spec/2026-05-26-no-sqlite-git-source-of-truth/wave-30-close-followups/spec.md`
- `.claude/spec/2026-05-26-no-sqlite-git-source-of-truth/wave-30-close-followups/meta.json`
- `waves-orchestrator-design.md` (DELETE da raiz, MOVE para `.claude/plans/`)
- `.claude/plans/waves-orchestrator-design.md` (NEW)

## Tarefas

1. `Cargo.toml` (raiz) — remover dep `rusqlite = { version = "0.31", features = ["bundled"] }` (linha 45) e seus comentários explicativos (linhas 38-44). Manter a tabela `[workspace.dependencies]` íntegra.
2. `apps/rt/Cargo.toml` — remover dep inline `rusqlite = { version = "0.31", features = ["bundled"] }` (linha 37). Ajustar comentário em linhas 27-35 para refletir que `tiny_http` agora é o único motivo do bloco (otel-collector usa NDJSON, não SQLite).
3. `apps/dashboard/src-tauri/Cargo.toml` — remover dep `rusqlite = { version = "0.31", features = ["bundled"] }` (linha 33).
4. `apps/rt/src/hooks/amend_capture.rs:45` — DELETE `use std::path::Path;` (unused).
5. `apps/rt/src/hooks/auto_capture_summary.rs:25-26` — DELETE `use mustard_core::ClaudePaths;` e `use std::path::Path;` (ambos unused).
6. `apps/rt/src/run/event_route.rs` — DELETE função `is_pipeline_event` (linhas 45-53) E seu teste `is_pipeline_event_matches_only_pipeline_prefix` (linhas 243-247). Lógica `event_name.starts_with("pipeline.")` é redundante com `classify_kind` (linha 62). Atualizar doc-comment de `kind`-classifier (linha 22-25) que cita `is_pipeline_event`.
7. `apps/rt/src/run/memory.rs:980` — DELETE field `pub id: Option<i64>,` de `DispatchExtras` + ajustar comentário acima.
8. `apps/rt/src/run/mod.rs:1571` — remover `id: None,` (e o block comment legacy linhas 1566-1570) do struct literal `DispatchExtras { ... }`.
9. `apps/rt/src/run/pipeline_state_ingest.rs:19-22` — DELETE field `pub delete: bool,` de `PipelineStateIngestOpts` (já documentado como "ignored"). Atualizar `run(_opts: PipelineStateIngestOpts)` se necessário (assinatura permanece).
10. `apps/rt/src/run/mod.rs:1588` — atualizar `pipeline_state_ingest::run(pipeline_state_ingest::PipelineStateIngestOpts { delete });` para `pipeline_state_ingest::run(pipeline_state_ingest::PipelineStateIngestOpts {});`.
11. `apps/dashboard/src-tauri/src/lib.rs:1140-1187` — DELETE `parse_iso_to_unix_secs`, `days_since_epoch`, `is_leap` (helper de days_since_epoch). Verificar via Grep que ambos têm zero callers externos antes de deletar.
12. `apps/dashboard/src-tauri/src/spec_views.rs:1597-1690` (aproximado) — DELETE função `summarise_payload`.
13. `waves-orchestrator-design.md` — MOVE para `.claude/plans/waves-orchestrator-design.md`. Conteúdo idêntico, apenas relocado.
14. Build: `rtk cargo build --workspace`. Espera 0 warnings.
15. Tests (no-run): `rtk cargo test --workspace --no-run`.
16. Tests (runtime): matar daemon `mustard-rt.exe` se ativo, depois `rtk cargo test --workspace`.
17. Commit: `chore(wave-30/close-followups): drain residual debt before spec close`.

## Dependências

Depende de W1-W8 (todas commitadas em `dev_rubens`, 46 ahead de origin).

## Limites

- 15 arquivos: 3 Cargo.toml + 8 .rs (mods em rt+dashboard) + 1 spec.md + 1 meta.json + 1 MOVE+DELETE de waves-orchestrator-design.md (líquido +1 file no destino, -1 na raiz). Limite cap (5 arquivos) é justificado: drenagem multi-item; cada arquivo recebe ≤30 LOC de mudança; sem behavior change. Comparar com wave-18 que tocou 5 arquivos pra 3 fixes — esta toca mais arquivos pra fixes mais cirúrgicos (deleção pura).
- Sem stubs (proibido). Cada item ou está em Tier A com fix verificável, ou em Tier B com pointer.
- Sem behavior change para code paths não-listados aqui — deleções confirmadas por Grep como dead-code.
- Drenagem Tier A: 7 fixes (alguns são verificação documentada).
- Tier B (5 itens): cada um aponta para wave/spec futura.
- Commit message: `chore(wave-30/close-followups): drain residual debt before spec close`.
