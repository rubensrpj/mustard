# wave-1-cli — Purge templates/recipes/

### Parent: [[2026-05-26-template-agnostic-audit]]
### Stage: Plan
### Outcome: Active
### Flags:

## Resumo

Apagar os 5 recipes abstratos (`add-field`, `add-endpoint`, `add-component`, `add-validation`, `null-guard`) de `apps/cli/templates/recipes/`. `mustard init` para de instalar skeletons CRUD. `recipe-match` degrada silencioso (exit 0) quando não acha recipe — comportamento já existente.

## Network

- Parent: [[2026-05-26-template-agnostic-audit]]
- Blocks: [[wave-2-cli]] (CLAUDE.md reescrito pode referenciar a ausência de recipes), [[wave-4-rt]] (recipe_match.rs cleanup pode deletar arquivo se recipes abstratos sumiram)

## Arquivos

- `apps/cli/templates/recipes/add-field.json` (DELETE)
- `apps/cli/templates/recipes/add-endpoint.json` (DELETE)
- `apps/cli/templates/recipes/add-component.json` (DELETE)
- `apps/cli/templates/recipes/add-validation.json` (DELETE)
- `apps/cli/templates/recipes/null-guard.json` (DELETE)
- `apps/cli/templates/recipes/` (RMDIR se vazio)
- `apps/cli/src/commands/init.rs` ou `apps/cli/src/fs_ops.rs` (CHECK — só modify se houver referência hardcoded à pasta `recipes/` no copy loop)

## Tarefas

### CLI Agent
- [ ] Apagar os 5 JSONs em `apps/cli/templates/recipes/`
- [ ] Remover pasta `recipes/` se ficar vazia (rmdir)
- [ ] Grep no `apps/cli/src/` por `recipes` literal — se houver special-casing, decidir: manter copy genérico que ignora pasta inexistente, ou remover branch dedicado
- [ ] `cargo build -p mustard-cli`
- [ ] `cargo test -p mustard-cli`
- [ ] Smoke manual: rodar `cargo run -p mustard-cli -- init` num tmpdir greenfield e confirmar que `.claude/recipes/` não foi criado

## Critérios de Aceitação

- [ ] AC-W1-1: pasta `apps/cli/templates/recipes/` não existe OU está vazia — Command: `node -e "const fs=require('fs');const p='apps/cli/templates/recipes';process.exit(!fs.existsSync(p)||fs.readdirSync(p).filter(f=>f.endsWith('.json')).length===0?0:1)"`
- [ ] AC-W1-2: `cargo build -p mustard-cli` passa — Command: `cargo build -p mustard-cli`
- [ ] AC-W1-3: `cargo test -p mustard-cli` passa — Command: `cargo test -p mustard-cli`

## Limites

- DELETE: 5 JSONs + pasta recipes/
- MODIFY (se necessário): copy loop em `apps/cli/src/commands/init.rs` ou `fs_ops.rs`
- FORA: `apps/rt/src/run/recipe_match.rs` (é W4); `templates/CLAUDE.md` (é W2)
