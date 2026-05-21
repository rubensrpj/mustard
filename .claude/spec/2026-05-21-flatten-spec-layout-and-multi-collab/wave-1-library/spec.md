# Wave 1 — Core: fallback header → SQLite

### Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
### Status: completed
### Phase: CLOSE
### Lang: pt
### Checkpoint: 2026-05-21T14:05:00Z

## Resumo

Quando o SQLite local não tem nenhum evento `pipeline.status` para uma spec, o `project_spec_view` em `packages/core/src/projection/card.rs` lê o cabeçalho `### Status:` do `spec.md` correspondente e usa esse valor como ponto de partida. Isso faz o header versionado em git virar a fonte canônica de status entre colaboradores, sem precisar emitir eventos manualmente no SQLite do colega que deu pull.

## Contexto

`SpecReader::spec_view` hoje carrega só os eventos da spec e faz fold. Se vier vazio, devolve `None`, e o painel mostra a spec como "sem eventos". Em colaboração isso é falso: a `spec.md` já tem `### Status: completed` no header (versionado em git). Wave 1 ensina o fold a olhar para o header como evidência adicional quando o stream está vazio.

A integração também aciona o backfill descrito na Wave 5: quando o fold cai no fallback, ele opcionalmente emite um evento sintético no SQLite local pra próxima leitura ser O(1) e não depender de re-parse de markdown.

## Arquivos

```
packages/core/src/projection/card.rs    — adicionar header_fallback no project_spec_view
packages/core/src/reader/sqlite.rs       — passar spec_dir opcional ao fold
packages/core/src/reader/memory.rs       — implementação espelho pra fakes de teste
```

## Tarefas

- [x] Adicionar parâmetro opcional `spec_md_path: Option<&Path>` à assinatura interna do `project_spec_view`. O caller (sqlite reader) passa quando souber resolver; testes que não precisam de fallback passam `None`.
- [x] Quando `events` está vazio E `spec_md_path` aponta para arquivo existente: ler header, parse `### Status:` via `SpecStatus::parse`, parse `### Phase:` via `Phase::parse`, parse `### Scope:` via `Scope::parse`, parse `### Lang:`. Construir `SpecView` com esses valores e marcar `started_at`/`last_event_at` como `None` (não inventar timestamps).
- [x] No `SqliteSpecReader::spec_view`, resolver o `spec_md_path` via `mustard_core::env::project_dir()` (ou via campo do reader) + `.claude/spec/{name}/spec.md`. Sem subbuckets — wave 2/5 garantem que a pasta esteja em `spec/{name}/`.
- [x] Adicionar emissão opcional do evento sintético `pipeline.status` (kind: `pipeline.status`, payload `{from:null,to:<header_status>}`) quando o fallback dispara — só se o caller passar uma `&dyn EventSink` opcional. Default off, fail-open.
- [x] Tests:
  - `project_spec_view_falls_back_to_header_when_events_empty` — sem eventos, header `### Status: completed` → view.status == Completed.
  - `project_spec_view_prefers_events_over_header` — header diz `completed` mas evento `pipeline.status: implementing` → status == Implementing.
  - `project_spec_view_handles_missing_header_file` — events vazios e arquivo não existe → view.status == NoEvents.

## Acceptance Criteria

- [x] AC-W1-1: Testes do módulo `projection::card` passam — Command: `cargo test -p mustard-core --lib projection::card`
- [x] AC-W1-2: Testes do módulo `reader::sqlite` passam — Command: `cargo test -p mustard-core --lib reader::sqlite`
- [x] AC-W1-3: Construção espelho pro fake reader passa — Command: `cargo test -p mustard-core --lib reader::memory`

## Limites

- `packages/core/src/projection/card.rs`
- `packages/core/src/reader/sqlite.rs`
- `packages/core/src/reader/memory.rs`

## Network

- Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]
- Bloqueia: [[wave-2-general]], [[wave-3-general]]
