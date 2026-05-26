# wave-6-mixed — Recipe concept death (matar o conceito por completo)

### Parent: [[2026-05-26-template-agnostic-audit]]
### Stage: Plan
### Outcome: Active
### Flags:

## Resumo

Decisão do usuário durante EXECUTE (post-W4): "recipe não faz mais sentido". A pasta `.claude/recipes/` já foi deletada manualmente como cleanup tactical. Esta wave mata o conceito por completo: deleta `recipe_match.rs` e `scan_recipes_validate.rs` (subcomandos), remove variantes do enum `RunCmd`, remove `SavingsSource::RecipeInjection` do economy ledger (tipo morto), expurga "recipe" do scan taxonomy + seed do resolver, remove chamadas `recipe-match` dos SKILLs feature/task, varre dashboard + core por refs residuais.

## Network

- Parent: [[2026-05-26-template-agnostic-audit]]
- Depends on: [[wave-5-mixed]]

## Justificativa

Recipes existem para entregar skeleton 90%-pronto a agentes de EXECUTE. O conceito assume que existe um arquétipo previsível (CRUD/REST/UI) cujo skeleton vale a pena pré-baker. Mustard descobriu durante este pipeline que isso fere agnosticismo: skeleton pré-baked é hardcode de arquétipo. O `delegate_to_resolver` (W4 project-profiler) já cobre o "trazer convenções vizinhas via grafo" sem precisar de skeleton. Logo: recipe vira código morto.

## Arquivos

### DELETE
- `apps/rt/src/run/recipe_match.rs` (219 linhas pós-W4)
- `apps/rt/src/run/scan_recipes_validate.rs` (228 linhas)

### MODIFY — Rust core/rt
- `apps/rt/src/run/mod.rs` (remover `mod recipe_match;`, `mod scan_recipes_validate;`, variantes do enum `RunCmd`, dispatch arms)
- `packages/core/src/economy/model.rs` (remover variante `SavingsSource::RecipeInjection`)
- `packages/core/src/economy/writer.rs` (remover helper `injection_savings_tokens` se órfão)
- Testes em `packages/core/tests/economy_basic.rs` e similares
- `apps/rt/src/run/scan/interpret.rs` (remover "recipe" do taxonomy — linhas 699, 756, 1095)
- `apps/rt/src/run/scan/graph.rs` (docstring cleanup — linha 664)
- `apps/rt/src/run/scan/resolve.rs` (remover seed `*.recipe.<slug>` — linhas 15, 81, 263-268)
- `apps/rt/src/run/scan_md_validate.rs` (remover validation contract de `.claude/recipes/` — linhas 83, 145, 331)

### MODIFY — Templates + SKILLs
- `apps/cli/templates/commands/mustard/feature/SKILL.md` (remover bloco "4b. Structured Recipe")
- `apps/cli/templates/commands/mustard/task/SKILL.md` (remover chamadas a recipe-match)
- `apps/cli/templates/pipeline-config.md` (varrer menções residuais)
- `apps/cli/templates/refs/stack-templates/browser-debug.md` (já será movido pela W3)
- `.claude/commands/mustard/feature/SKILL.md` + `task/SKILL.md` (espelho local)

### MODIFY — Dashboard
- `apps/dashboard/src/features/economy/SavingsBreakdownCard/index.tsx` (remover fatia `RecipeInjection`)
- `apps/dashboard/src/lib/types/economy.ts` (union type sem `RecipeInjection`)
- `apps/dashboard/src/lib/dashboard.ts` (se referenciar)
- `apps/dashboard/src/data/commands-catalog.ts` (remover `recipe-match` e `scan-recipes-validate`)

### CHECK / SWEEP
- `apps/dashboard/src-tauri/src/lib.rs` e `artifact_update.rs`
- `.claude/.artifacts.json` + `apps/cli/templates/.artifacts.json`

## Tarefas

### RT Agent (Wave 6a — Rust deletes + enum cleanup)
- [ ] DELETE `recipe_match.rs` + `scan_recipes_validate.rs`
- [ ] `mod.rs`: remover declarações + enum variants + dispatch arms
- [ ] `packages/core/src/economy/model.rs`: remover `SavingsSource::RecipeInjection`; fixar matches exhaustivos quebrados
- [ ] `writer.rs`: remover `injection_savings_tokens` se órfão
- [ ] Tests economy: remover asserts do tipo morto
- [ ] `scan/interpret.rs`, `scan/graph.rs`, `scan/resolve.rs`: cleanup
- [ ] `scan_md_validate.rs`: remover contract `.claude/recipes/`
- [ ] `cargo build` (workspace) verde
- [ ] `cargo test -p mustard-rt` e `cargo test -p mustard-core` verdes

### CLI Agent (Wave 6b — SKILLs + templates)
- [ ] `templates/commands/mustard/feature/SKILL.md`: remover bloco "4b. Structured Recipe"
- [ ] `templates/commands/mustard/task/SKILL.md`: limpar
- [ ] `templates/pipeline-config.md`: sweep
- [ ] Espelho local em `.claude/commands/mustard/`
- [ ] `cargo build -p mustard-cli` verde

### Dashboard Agent (Wave 6c — UI cleanup)
- [ ] `SavingsBreakdownCard`: remover fatia
- [ ] `lib/types/economy.ts`: union sem `RecipeInjection`
- [ ] `lib/dashboard.ts`: sweep
- [ ] `data/commands-catalog.ts`: remover entradas
- [ ] Tauri `lib.rs` + `artifact_update.rs`: verificar
- [ ] Build dashboard

### Validação final (qualquer agente)
- [ ] `grep -r "RecipeInjection" packages/ apps/rt/src/` retorna zero
- [ ] `cargo run -p mustard-rt -- run --help` não lista `recipe-match` nem `scan-recipes-validate`

## Critérios de Aceitação

- [ ] AC-W6-1: `apps/rt/src/run/recipe_match.rs` não existe — Command: `node -e "process.exit(require('fs').existsSync('apps/rt/src/run/recipe_match.rs')?1:0)"`
- [ ] AC-W6-2: `apps/rt/src/run/scan_recipes_validate.rs` não existe — Command: `node -e "process.exit(require('fs').existsSync('apps/rt/src/run/scan_recipes_validate.rs')?1:0)"`
- [ ] AC-W6-3: `RecipeInjection` zero hits em `packages/` + `apps/rt/src/` — Command: `bash -c 'count=$(grep -r "RecipeInjection" packages/ apps/rt/src/ 2>/dev/null | wc -l); test "$count" = "0"'`
- [ ] AC-W6-4: `cargo build` workspace passa — Command: `cargo build`
- [ ] AC-W6-5: `cargo test -p mustard-rt` passa — Command: `cargo test -p mustard-rt`
- [ ] AC-W6-6: `mustard-rt run --help` não lista `recipe-match` nem `scan-recipes-validate` — Command: `bash -c 'cargo run -q -p mustard-rt -- run --help 2>&1 | grep -E "recipe-match|scan-recipes-validate" | wc -l | grep -q "^0$"'`
- [ ] AC-W6-7: `templates/commands/mustard/feature/SKILL.md` sem `recipe-match` — Command: `node -e "process.exit(require('fs').readFileSync('apps/cli/templates/commands/mustard/feature/SKILL.md','utf8').includes('recipe-match')?1:0)"`

## Limites

- DELETE: `recipe_match.rs`, `scan_recipes_validate.rs`
- MODIFY (~20 arquivos): listados acima
- BREAKING: enum `SavingsSource` perde variante (aceitar perda em rows antigas, sem migração)
- FORA: `.claude/recipes/` (já deletada); `apps/rt/src/run/scan/` exceto docstring + seed cleanup; geração de recipes (confirmado que NÃO existe)
