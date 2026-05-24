# Wave 6 — Sub-specs: union spec.link + Parent: header

## Resumo

Hoje `spec_children_v2` em `apps/dashboard/src-tauri/src/spec_views.rs` chama `mustard_core::SqliteSpecReader::children_of(parent)` que consulta apenas a projeção do SQLite (eventos `spec.link`). Sub-specs criadas via `/mustard:tactical-fix` que tenham `### Parent: <slug>` no header MAS não tenham emitido evento `spec.link` (ou cujo SQLite local nunca viu o evento — colaborador que deu pull) ficam invisíveis. Wave 6 faz o `spec_children_v2` retornar a **union** de duas fontes: eventos do SQLite + scan filesystem de `spec/*/spec.md` procurando `### Parent: <slug>`. Dedupe por slug.

## Contexto

Caso de teste real: a spec `2026-05-21-flatten-spec-layout-and-multi-collab` tem três sub-specs criadas no CLOSE: `2026-05-21-tf-skill-mirror`, `2026-05-21-tf-dashboard-residuals`, `2026-05-21-tf-rt-residuals` (visíveis no `git status` como `??`). Esses arquivos têm `### Parent: [[2026-05-21-flatten-spec-layout-and-multi-collab]]` no header mas hoje não aparecem na sub-aba "Sub-specs" do drill-down — o reader só consulta SQLite.

Política: header `### Parent:` é o canon cross-dev (versionado em git, sobrevive `pull`). SQLite é fonte de verdade local (telemetria rica). A union prioriza dados do SQLite quando há intersecção (preserve `started_at`, `completed_at`, `reason` que vêm da projeção), e adiciona linhas filesystem-only com defaults seguros.

## Arquivos

```
apps/rt/src/run/spec_children.rs                             — NOVO subcommand union (Rust)
apps/rt/src/run/mod.rs                                       — registrar spec-children
apps/dashboard/src-tauri/src/spec_views.rs                   — spec_children_v2 chama union helper
apps/dashboard/src-tauri/src/lib.rs                          — sem mudança de assinatura, só refrescada se necessário
apps/dashboard/src/components/specs/SpecChildrenTab.tsx      — pequeno tweak se a render mostrar fonte (badge "via header" vs "via evento")
```

## Tarefas

- [ ] Criar `apps/rt/src/run/spec_children.rs` com:
  ```rust
  pub struct ChildEntry {
    pub spec: String,
    pub status: String,         // kebab-case
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub reason: Option<String>,
    pub source: ChildSource,    // Event | Header | Both
  }
  pub enum ChildSource { Event, Header, Both }

  pub fn list_children(project: &Path, parent: &str) -> Vec<ChildEntry>;
  ```
  Implementação:
  1. Chama `SqliteSpecReader::children_of(parent)` → set A (com status/timestamps).
  2. Escaneia `project/.claude/spec/*/spec.md` (Glob). Pra cada arquivo, lê as primeiras ~30 linhas, procura `### Parent: <slug>` (slug pode estar entre `[[...]]` ou cru). Match contra `parent` → set B (com `spec = nome do dir`, status = lido do header `### Status:` ou `"unknown"`, timestamps = `None`).
  3. Union: itera A; pra cada, marca `source = Event` (ou `Both` se também aparecer em B). Itera B; adiciona quem não estava em A com `source = Header`. Dedupe por `spec`.
  4. Ordena por `spec` (slug alfabético).
- [ ] Adicionar testes em `spec_children.rs`:
  - `union_returns_event_only_when_no_header` — fixture com event `spec.link parent→child` e nenhuma sub-spec em disco → 1 entry com `source=Event`.
  - `union_returns_header_only_when_no_event` — fixture com `spec/{name}/spec.md` contendo `### Parent: <parent>` mas sem evento → 1 entry com `source=Header`.
  - `union_dedupes_when_both_present` — entry com `source=Both`.
  - `union_skips_unrelated_parents` — header com outro slug pai → não aparece.
- [ ] Registrar em `apps/rt/src/run/mod.rs`: `dispatch_run` reconhece `"spec-children"` com `--parent`, devolve JSON `Vec<ChildEntry>`.
- [ ] Em `apps/dashboard/src-tauri/src/spec_views.rs`: refatorar `spec_children_v2` pra chamar o helper de `mustard-rt` (ou portar a função pra `mustard-core` se o reuse fizer mais sentido — preferir `mustard-core` pra evitar spawn de subprocesso na Tauri command).
  - Decisão: portar `list_children` pra `mustard-core::reader` como `union_children_of(reader, project, parent)`. O subcommand do `mustard-rt` chama o helper do core. `spec_views::spec_children_v2` chama o helper do core diretamente (sem subprocesso).
  - O `SpecChild` payload existente ganha campo `source: "event" | "header" | "both"`. Frontend pode usar pra badge.
- [ ] Em `apps/dashboard/src/lib/types/specs.ts`: adicionar `source?: "event" | "header" | "both"` em `SpecChild`.
- [ ] Em `SpecChildrenTab.tsx`: render badge pequeno por entry mostrando `source` (cinza minúsculo, "via header" / "via evento" / "via header + evento"). Não é o foco — apenas surface da fonte.
- [ ] Build + test:
  - `cargo build -p mustard-core -p mustard-rt`
  - `cargo test -p mustard-rt --bin mustard-rt spec_children`
  - `cargo test -p mustard-core --lib reader::union_children` (se o helper viver no core)
  - `pnpm --filter mustard-dashboard build`

## Acceptance Criteria

- [ ] AC-W6-1: Testes de `spec_children` passam — Command: `cargo test -p mustard-rt --bin mustard-rt spec_children`
- [ ] AC-W6-2: `dashboard_spec_children` retorna sub-specs via header — Command: `bash -c 'OUT=$(cargo run -q -p mustard-rt -- run spec-children --parent 2026-05-21-flatten-spec-layout-and-multi-collab); echo "$OUT" | node -e "const j=JSON.parse(require(\"fs\").readFileSync(0,\"utf8\"));process.exit(j.some(c=>c.spec.startsWith(\"2026-05-21-tf-\"))?0:1)"'`
- [ ] AC-W6-3: `SpecChild` ganha campo `source` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/lib/types/specs.ts','utf8');process.exit(/source\??:\s*['\"]?event/.test(s)?0:1)"`
- [ ] AC-W6-4: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`

## Limites

- `apps/rt/src/run/spec_children.rs` (novo)
- `apps/rt/src/run/mod.rs`
- `apps/dashboard/src-tauri/src/spec_views.rs`
- `apps/dashboard/src/lib/types/specs.ts`
- `apps/dashboard/src/components/specs/SpecChildrenTab.tsx`
- `packages/core/src/reader/` (helper `union_children_of` — apenas se decidirmos portar pro core; caso contrário fica em `mustard-rt`)

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs]]
- Depende: [[wave-1-ui]]
