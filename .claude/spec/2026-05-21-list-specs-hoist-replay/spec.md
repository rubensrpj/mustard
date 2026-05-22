# list_specs hoist replay — eliminar O(N×M) ao popular children_count

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-21T00:40:00Z
### Lang: pt
### Parent: 2026-05-20-tactical-fix-via-sub-spec

## Contexto

Tactical fix derivado de [[2026-05-20-tactical-fix-via-sub-spec]]. O review backend desta spec apontou que `SqliteSpecReader::list_specs` e `InMemorySpecReader::list_specs` agora chamam `self.children_of(name)` dentro do loop por spec. Cada chamada faz um `store.replay()` (ou `self.snapshot()`) completo — `SELECT * FROM events` sem WHERE — depois filtra eventos `spec.link` em Rust. Para N specs e M total de eventos, a operação é O(N×M). Em monorepos reais com >>10k eventos e >>50 specs, isso adiciona segundos a um endpoint que é hot path da Sidebar do dashboard.

Solução: hoistar o scan único antes do loop, construir um `HashMap<String, u32>` (parent → contagem distinta de children) e injetar o count em cada `SpecSummary` por lookup O(1). Mantém o contrato do trait intacto; muda só a implementação interna dos dois readers.

## Critérios de Aceitação

- [x] AC-1: Workspace compila — Command: `cargo build -p mustard-core`
- [x] AC-2: Contract tests continuam passando — Command: `cargo test -p mustard-core`
- [x] AC-3: `list_specs` em `sqlite.rs` NÃO chama `.children_of(` no corpo da fn — Command: `node -e "const c=require('fs').readFileSync('packages/core/src/reader/sqlite.rs','utf8');const s=c.indexOf('fn list_specs(');const e=c.indexOf('\nfn ',s+1);const body=c.slice(s,e>0?e:c.length);process.exit(body.includes('.children_of(')?1:0)"`
- [x] AC-4: `list_specs` em `memory.rs` NÃO chama `.children_of(` no corpo da fn — Command: `node -e "const c=require('fs').readFileSync('packages/core/src/reader/memory.rs','utf8');const s=c.indexOf('fn list_specs(');const e=c.indexOf('\nfn ',s+1);const body=c.slice(s,e>0?e:c.length);process.exit(body.includes('.children_of(')?1:0)"`

## Arquivos

```
packages/core/src/reader/sqlite.rs    — refator list_specs: replay() único + HashMap<parent, u32>
packages/core/src/reader/memory.rs    — análogo: snapshot() único + HashMap<parent, u32>
```

## Tarefas

- [x] Em `SqliteSpecReader::list_specs`: antes do loop, chamar `let events = self.store()?.replay()?;` uma única vez. Construir `let counts: HashMap<String, std::collections::HashSet<String>>` (parent → set distinct children) filtrando `events` por `event == "spec.link"`. Dentro do loop por spec, em vez de chamar `children_of(name)`, fazer `summary.children_count = counts.get(&name).map_or(0, ...)`.
- [x] Em `InMemorySpecReader::list_specs`: análogo com `self.snapshot()`.
- [x] Garantir comportamento equivalente: testes de contrato `children_of_contract_*` e `parent.children_count` permanecem verdes.

## Limites

- `packages/core/src/reader/{sqlite.rs, memory.rs}` — apenas a fn `list_specs`. Não alterar `children_of`, `spec_summary`, `spec_summary_core`, nem o trait.

**Fora dos limites:**
- Adicionar SQL `WHERE event='spec.link'` em SqliteEventStore (poderia ser uma future-spec — esta fica com replay+filter como combinado na original).
- Mudar o contrato do trait.

## Checklist

- [x] AC-1 a AC-4 passam
- [x] Diff ≤30 LOC líquido por arquivo
