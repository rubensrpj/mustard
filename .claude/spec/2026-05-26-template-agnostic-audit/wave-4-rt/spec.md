# wave-4-rt — Limpar recipe_match.rs (purge hardcode arquétipo)

### Parent: [[2026-05-26-template-agnostic-audit]]
### Stage: Plan
### Outcome: Active
### Flags:

## Resumo

`apps/rt/src/run/recipe_match.rs` tem `find_dir_by_convention` (hardcode `backend`/`frontend`/`admin`), `to_pascal_case`, `resolve_pattern` que substitui `{Entity}`/`{entity}`/`{subproject}`/`{backend}`. Hardcode CRUD/web. Recipes derivados do scan já vêm com `files[].path` real. Remover. Avaliar se o arquivo todo vira útil-zero (delete) ou sobra só carga + economy + delegate resolver.

## Network

- Parent: [[2026-05-26-template-agnostic-audit]]
- Depends on: [[wave-1-cli]]
- Parallel-safe com: [[wave-2-cli]], [[wave-3-cli]]

## Arquivos

- `apps/rt/src/run/recipe_match.rs` (MODIFY ou DELETE)
- `apps/rt/src/run/mod.rs` (MODIFY se delete)
- `apps/rt/tests/recipe_match*.rs` + testes inline (MODIFY/DELETE)
- `apps/cli/templates/commands/mustard/feature/SKILL.md`, `task/SKILL.md` (CHECK consumidores)

## Tarefas

### RT Agent
- [ ] Ler `apps/rt/src/run/recipe_match.rs` (277 linhas)
- [ ] Identificar o que agrega valor real: carga JSON do scan, `persist_injection_savings`, `delegate_to_resolver`, print `files[].path`
- [ ] Remover `to_pascal_case`, `find_dir_by_convention`, `resolve_pattern`
- [ ] Simplificar `run()`: emitir `{"resolved_path": files[i].path, ...}` direto, sem `resolve_pattern`
- [ ] Remover testes obsoletos (`pascal_case_uppercases_first`, `resolve_pattern_substitutes_entity`)
- [ ] DECISÃO: se `run()` < 30 linhas E consumidores em SKILL.md não invocarem mais (W5 limpa SKILLs), DELETAR arquivo + remover dispatch em `mod.rs`. Documentar no commit.
- [ ] Se DELETE: remover chamadas `recipe-match` em `templates/commands/mustard/feature/SKILL.md` e `task/SKILL.md`
- [ ] Se MANTER: docstring atualizada
- [ ] `cargo build -p mustard-rt`
- [ ] `cargo test -p mustard-rt`
- [ ] `cargo run -p mustard-rt -- run scan-recipes-validate --strict` no próprio repo

## Critérios de Aceitação

- [ ] AC-W4-1: `recipe_match.rs` sem `find_dir_by_convention`/`resolve_pattern`/`fn to_pascal_case` (ou arquivo deletado) — Command: `node -e "const fs=require('fs');const p='apps/rt/src/run/recipe_match.rs';if(!fs.existsSync(p))process.exit(0);const c=fs.readFileSync(p,'utf8');const hits=['find_dir_by_convention','fn resolve_pattern','fn to_pascal_case'].filter(s=>c.includes(s));process.exit(hits.length===0?0:1)"`
- [ ] AC-W4-2: `cargo build -p mustard-rt` passa — Command: `cargo build -p mustard-rt`
- [ ] AC-W4-3: `cargo test -p mustard-rt` passa — Command: `cargo test -p mustard-rt`
- [ ] AC-W4-4: `scan-recipes-validate --strict` passa — Command: `bash -c 'cargo run -q -p mustard-rt -- run scan-recipes-validate --strict'`

## Limites

- MODIFY ou DELETE: `recipe_match.rs`
- MODIFY se delete: `mod.rs`, testes, consumidores em `templates/commands/mustard/`
- FORA: `apps/rt/src/run/scan/`, `.claude/recipes/{cli,rt,...}/`
