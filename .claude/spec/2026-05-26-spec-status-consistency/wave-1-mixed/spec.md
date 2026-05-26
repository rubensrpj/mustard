# Wave 1 — sync único `spec.md` + `meta.json`

### Stage: Plan
### Outcome: Active
### Flags: 

## Contexto

Hoje o `emit_pipeline.rs` mantém dois caminhos paralelos pra atualizar status: `sync_spec_status_header` reescreve o cabeçalho do `spec.md`, e `sync_spec_meta_sidecar` reescreve o `meta.json`. Os dois são chamados juntos só quando `should_sync_parent_header` (linha 340) retorna true — e ela retorna **false** se o payload tem `wave`. Toda transição wave-level dessincroniza. Outro problema: o `tactical_fix_create.rs` tem scaffold próprio (não reusa `write_spec_md`/`write_meta_json` do `spec_draft.rs`), abrindo caminho pra criar spec sem cabeçalhos.

Esta wave consolida tudo em **um helper único atômico** e remove o gate wave.

## Tarefas

- [ ] **T1.1** — Extrair `write_spec_md` (linha ~318) e `write_meta_json` (linha ~181) de `spec_draft.rs` para um módulo público novo `apps/rt/src/run/spec_scaffold.rs`. Atualizar `spec_draft.rs` pra chamar via `use spec_scaffold::{write_spec_md, write_meta_json}`.
- [ ] **T1.2** — Refatorar `tactical_fix_create.rs` pra usar `spec_scaffold::write_spec_md` e `write_meta_json`. Remover o scaffold inline atual. Garantir que o tactical-fix sempre escreve os três cabeçalhos canônicos.
- [ ] **T1.3** — Criar função `sync_status(stage: Stage, outcome: Outcome, spec_path: &Path) -> Result<()>` em `spec_scaffold.rs` que reescreve **atomicamente** o cabeçalho do `spec.md` E o `meta.json` ao lado (mesma transação semântica). Substituir as duas funções separadas em `emit_pipeline.rs`.
- [ ] **T1.4** — Remover o gate `should_sync_parent_header` (linha ~340 de `emit_pipeline.rs`). Toda transição que afeta status — incluindo as wave-level — chama `sync_status` pro arquivo correspondente (parent OU wave). Eventos `pipeline.wave.complete` chamam `sync_status` pra wave + `bump_parent_progress` pro parent.

## Critérios de Aceitação

- **AC-W1.1** — `apps/rt/src/run/spec_scaffold.rs` existe e exporta `write_spec_md`, `write_meta_json`, `sync_status`. Command: `rtk node -e "const fs=require('fs');if(!fs.existsSync('apps/rt/src/run/spec_scaffold.rs'))process.exit(1)"`
- **AC-W1.2** — Grep não encontra mais `sync_spec_status_header` nem `sync_spec_meta_sidecar` em `apps/rt/src/run/emit_pipeline.rs` (substituídas por `sync_status`). Command: `rtk grep -F "sync_spec_status_header\|sync_spec_meta_sidecar" apps/rt/src/run/emit_pipeline.rs && exit 1 || exit 0`
- **AC-W1.3** — Teste unitário: simular `pipeline.wave.complete` para wave 2 e verificar que `wave-2-*/spec.md` E `wave-2-*/meta.json` ficam com `stage=Close, outcome=Completed`. Command: `rtk cargo test -p mustard-rt sync_status_wave_complete`
- **AC-W1.4** — Build + clippy. Command: `rtk cargo build -p mustard-rt && rtk cargo clippy -p mustard-rt -- -D warnings`

## Limites

- **IN**: `apps/rt/src/run/spec_draft.rs`, `apps/rt/src/run/tactical_fix_create.rs`, `apps/rt/src/run/emit_pipeline.rs`, `apps/rt/src/run/spec_scaffold.rs` (novo), `apps/rt/src/run/mod.rs`.
- **OUT**: nenhuma mudança em `packages/core/`, nenhum novo kind de evento, nada em `apps/dashboard/`.
