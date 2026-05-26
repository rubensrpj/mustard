# W3 — run/ skills + spec helpers + scan/ sweep (24 violations / 13 files)

## Contexto

Sweep mecânico em três sub-áreas de `apps/rt/src/run/`:
- **skills resolution** — 5 arquivos, 13 violações. Substituir por `ClaudePaths::skills()` e `ClaudePaths::skill_cache()`.
- **spec helpers** — 4 arquivos, 4 violações. Substituir por `ClaudePaths::spec(slug)` ou `ClaudePaths::spec_root()`.
- **scan/** subdir — 3 arquivos, 8 violações. Substituir por `ClaudePaths::graph()`, `ClaudePaths::skills()`, `ClaudePaths::resolve_cache()`.
- **Tail**: `wikilink.rs` (1) + `migrate_to_meta.rs` (1).

## Arquivos (lista enumerada)

| # | Arquivo | Violações |
|---|---------|-----------|
| 1 | `apps/rt/src/run/skills.rs` | 4 (linhas 92, 103, 677, 1071) |
| 2 | `apps/rt/src/run/skill_resolve.rs` | 4 (linhas 139, 142, 196, 222) |
| 3 | `apps/rt/src/run/skill_cache.rs` | 2 (linhas 34, 52) |
| 4 | `apps/rt/src/run/skill_fetch.rs` | 2 (linhas 168, 192) |
| 5 | `apps/rt/src/run/skill_discovery_lint.rs` | 1 (linha 208) |
| 6 | `apps/rt/src/run/spec_children.rs` | 1 (linha 140) |
| 7 | `apps/rt/src/run/spec_draft.rs` | 1 (linha 133) |
| 8 | `apps/rt/src/run/spec_lang_resolve.rs` | 1 (linha 60) |
| 9 | `apps/rt/src/run/spec_validate.rs` | 1 |
| 10 | `apps/rt/src/run/scan/graph.rs` | 5 (linhas 260, 369, 690, 766, 933) |
| 11 | `apps/rt/src/run/scan/resolve.rs` | 2 (linhas 309, 453) |
| 12 | `apps/rt/src/run/scan/refs_installer.rs` | 1 (linha 222) |
| 13 | `apps/rt/src/run/wikilink.rs` + `apps/rt/src/run/migrate_to_meta.rs` | 2 |

## Tarefas

- [ ] **TF3.1** — Mapear métodos `ClaudePaths` para: `skills()`, `skill_cache()`, `spec_root()`, `spec(slug)`, `graph()`, `resolve_cache()`. Se algum método estiver faltando em `claude_paths.rs`, adicionar **só** no claude_paths.rs (NÃO fora de escopo da spec) e justificar inline.
- [ ] **TF3.2** — Sweep ordenado por sub-área: skills primeiro (todos 5), depois spec helpers (4), depois scan/ (3), depois wikilink+migrate.
- [ ] **TF3.3** — `rtk cargo check -p mustard-rt` ao final.

## Critérios de Aceitação

- [ ] **AC-W3.1** — Zero `.join(".claude")` em 13 arquivos listados fora de tests gated. Command: `rtk node "C:/Users/ruben/.claude/jobs/3922ef93/ac_tf1.js" 2>&1 | rtk grep "run/\(skill\|spec_children\|spec_draft\|spec_lang\|spec_validate\|scan/\|wikilink\|migrate_to_meta\)"` deve ser vazio.
- [ ] **AC-W3.2** — `rtk cargo check -p mustard-rt` passa.

## Limites

IN: 13 arquivos listados em `apps/rt/src/run/` + `apps/rt/src/run/scan/`.
OUT: hooks/, mcp/, outros arquivos de run/, tests/.

## Role

rt-impl
