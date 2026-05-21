# Tactical-fix — rt modules bucket residuals

### Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
### Status: completed
### Phase: CLOSE
### Lang: pt
### Checkpoint: 2026-05-21

## Resumo

Wave-2 do parent so tocou 6 modulos de rt (complete_spec/spec_extract/qa_run/wave_tree/wikilink/emit_pipeline + session_cleanup). 10 outros modulos ainda tem referencias a `spec/active|completed|superseded` — alguns sao paths reais (operacoes de fs), outros sao comentarios ou logs. Esta fix audita os 32 hits e flattena os que ainda fazem trabalho real (path resolution / fs operations); os textuais reescrevem-se em uma frase.

## Arquivos

- `apps/rt/src/hooks/knowledge.rs` — 1 docstring, 1 test fixture
- `apps/rt/src/hooks/post_edit.rs` — 1 docstring em `find_active_spec`
- `apps/rt/src/run/doctor.rs` — 3 comentarios + 1 code path + 1 test comment
- `apps/rt/src/hooks/path_guard.rs` — 1 docstring + 1 code path (`.join("active")`)
- `apps/rt/src/hooks/session_start.rs` — 2 docstrings + 1 code path (hygiene vira no-op) + tests
- `apps/rt/src/hooks/size_gate.rs` — `is_spec_path` usa bucket prefixes + AC audit string
- `apps/rt/src/run/event_projections.rs` — 1 docstring
- `apps/rt/src/run/mark_checklist_item.rs` — 1 docstring + 1 code path (`.join("active")`)
- `apps/rt/src/run/metrics_wave_status.rs` — 1 docstring + 1 help string
- `apps/rt/src/run/mod.rs` — 2 clap doc strings

## Tarefas

- [x] Auditar cada hit e classificar (code/comment/log).
- [x] Para hits code: substituir bucket paths por flat `spec/{name}/`.
- [x] Para hits comment/log: reescrever para refletir o flat layout.
- [x] `run_spec_hygiene` em session_start: tornar no-op (buckets nao existem mais).
- [x] `collect_active_spec_names` em doctor: ler de `spec/` diretamente.
- [x] `is_spec_path` em size_gate: reconhecer flat `.claude/spec/{name}/*.md`.
- [x] `resolve_spec_file` em path_guard: drop segmento `active`.
- [x] `resolve_spec_path` em mark_checklist_item: drop segmento `active`.

## Acceptance Criteria

- [x] AC-TF-C-1: `rg -n 'spec/(active|completed|superseded)' apps/rt/src` retorna vazio.
- [x] AC-TF-C-2: `cargo build -p mustard-rt` passa.
- [x] AC-TF-C-3: `cargo test -p mustard-rt --bin mustard-rt` passa (mantem ~637+ verdes).

## Limites

- `apps/rt/src/hooks/{knowledge,post_edit,path_guard,session_start,size_gate}.rs`
- `apps/rt/src/run/{doctor,event_projections,mark_checklist_item,metrics_wave_status,mod}.rs`

OUT: modulos ja tocados em wave-2; main.rs; dispatch.rs; testes integrados nao listados aqui.
