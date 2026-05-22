# wave-2-general — Rotear hooks pelo repository único

### Parent: [[2026-05-22-db-access-repository-and-live-refresh]]
### Stage: Done
### Outcome: Completed
### Flags:
### Lang: pt

## Resumo

Fazer o `mustard-rt` construir um único `Repository` por invocação de processo e
passá-lo aos módulos de hook, em vez de cada módulo reabrir o banco. Substituir os
usos de `replay()` como lookup por consultas de existência. Depende da Wave 1.

## Causa raiz

No hot path, `tracker.rs` abre o banco 2-3 vezes por invocação
(`open_economy_conn` ~74-77 abre o store completo e descarta + abre conexão crua;
`emit_event` ~176 reabre tudo). `knowledge.rs` (`spec_has_retry_events` ~348) faz
`replay()` completo só para checar 1 evento. `session_start.rs` (~480-514) roda
duas queries em duas aberturas separadas.

## Arquivos

- `apps/rt/src/dispatch.rs` — construir um `SqliteEventStore` (ou `store::DbCache`) por invocação e injetá-lo nos módulos
- `apps/rt/src/hooks/tracker.rs` — `open_economy_conn` e `emit_event` usam o handle compartilhado (sem reabrir)
- `apps/rt/src/hooks/knowledge.rs` — `spec_has_retry_events` → `SELECT 1 FROM events WHERE event='retry.attempt' AND spec=?1 LIMIT 1`
- `apps/rt/src/hooks/session_start.rs` — `load_knowledge_sql` + `load_memory_sql` em uma conexão; apoiar no índice composto da Wave 1
- `apps/rt/src/hooks/economy/store.rs` — parar o double-open (`open_for`)

## Tarefas

### General Agent (Wave 2)

- [ ] `dispatch.rs`: instanciar o store (Wave 1) uma vez no início e passar por referência aos módulos do evento.
- [ ] `tracker.rs`: `open_economy_conn` e `emit_event` recebem/usam o handle compartilhado; remover as reaberturas redundantes.
- [ ] `knowledge.rs`: trocar `replay()` por consulta de existência em `spec_has_retry_events`.
- [ ] `session_start.rs`: agrupar as duas leituras numa conexão; remover a segunda abertura.
- [ ] `economy/store.rs`: eliminar o segundo open de `open_for`.
- [ ] Rodar `cargo build -p mustard-rt` e `cargo test -p mustard-rt`.

## Critérios de Aceitação

- [ ] AC-1: `cargo build -p mustard-rt` passa — Command: `cargo build -p mustard-rt`
- [ ] AC-2: `cargo test -p mustard-rt` passa — Command: `cargo test -p mustard-rt`
- [ ] AC-3: `spec_has_retry_events` não usa mais `replay()` — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('apps/rt/src/hooks/knowledge.rs','utf8');const i=s.indexOf('fn spec_has_retry_events');const b=s.slice(i,i+800);process.exit(/replay\(/.test(b)?1:0)"`

## Limites

- `apps/rt/src/dispatch.rs`, `apps/rt/src/hooks/{tracker,knowledge,session_start}.rs`, `apps/rt/src/hooks/economy/store.rs`
- NÃO alterar a API do store (definida na Wave 1) — apenas consumir
- NÃO tocar no dashboard (Wave 3)
