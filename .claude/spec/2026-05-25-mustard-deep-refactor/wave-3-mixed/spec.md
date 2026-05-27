# W3 — `/scan` Rust-first agnóstico + cobertura completa do entity-registry
### Stage: Plan
### Outcome: Active
### Flags: 

## Contexto

Cobertura atual do `entity-registry.json` no projeto Mustard: ~6%. Princípio formalizado: **estrutural em Rust, IA só interpretação semântica nomeável** ([[feedback_scan_rust_first]]); **zero hardcode de stack** ([[feedback_no_hardcoded_stack_patterns]]); **graph com escopo de pipeline** ([[feedback_graph_pipeline_knowledge]]); **recipes geradas pelo scan** ([[feedback_recipes_from_scan]]).

## Tarefas — 3 etapas

### Etapa 1 — `scan-structural` (Rust puro, agnóstico)

- [x] **T3.1** — `mustard-rt run scan-structural --subproject {path}`. Parser agnóstico de manifests (Cargo.toml/package.json/requirements.txt/pyproject.toml/go.mod/pom.xml/composer.json/Gemfile/*.csproj/pubspec.yaml). Gera `stack.md` cap ≤60 linhas.
- [x] **T3.2** — Corrigir orquestração do `cluster_discovery` (`scan/cluster_discovery.rs`): rodar em **todos** subprojetos retornados por `sync-detect`. Nenhum bypass.
- [ ] **T3.3** — Parser AST-leve agnóstico (`scan/entity_extractor.rs` novo): para cada extensão do `stack.md`, detecta declarações públicas/exportadas via tokens genéricos (`pub`/`export`/`public`/`def`/`class`/`function`/`fn`/`func`/`type`). Reconhece sintaxe, não framework.
- [x] **T3.4** — Gerador de recipes derivado (`scan/recipes_generator.rs` novo): para cada cluster emitido, lê 2-3 amostras, extrai imports comuns + skeleton mínimo, detecta barrel, gera `.claude/recipes/{sub}/add-{clusterLabel}.json` com paths reais. Zero catálogo hardcoded.
- [ ] **T3.5** — Refs stack-aware install (`scan/refs_installer.rs` novo): lê `templates/refs/stack-templates/` com frontmatter `qualifyingSignals`; copia para `.claude/refs/{cmd}/X.md` se signals batem com stack detectada.
- [x] **T3.6** — Graph nodes: emite só `spec.X`/`skill.X`/`command.X`/`ref.X`/`recipe.X`/`conv.X`. Nunca `entity.X`/`enum.X`.
- [ ] **T3.7** — Wirelinks canônicos `[[{sub}.{kind}.{slug}]]` em todo cross-ref.

### Etapa 2 — `scan-interpret` (IA enxuta)

- [x] **T3.8** — Reescrever `apps/rt/scripts/scan/agent-prompt.template.md` ≤80 linhas. Recebe input estruturado do scan-structural. Pede apenas: `patterns.md` (≤150L), `notes.md` (≤80L), `skills/{cluster}/SKILL.md` (≤60L cada). Prompt não menciona stacks específicas.
- [x] **T3.9** — 3 exemplos golden por classe genérica (compiled-strongly-typed / dynamic-scripting / transpiled-typed) em `apps/rt/scripts/scan/examples/{class}/` — não nome de tecnologia.

### Etapa 3 — Validation Rust

- [x] **T3.10** — `mustard-rt run scan-md-validate`: tamanho, refs a paths existentes, wirelinks `[[id]]`, fence `<!-- mustard:generated -->`, duplicação cross-arquivo.
- [x] **T3.11** — `mustard-rt run scan-recipes-validate`: shape, paths existem, sem placeholders literais (`{Entity}`).
- [ ] **T3.12** — `scan_finalize.rs` chama validate; re-dispatch uma vez se falhar; depois fail-open.

## Critérios de Aceitação

- [x] **AC-W3.1** — `cluster_discovery` rodou em **todos** subprojetos detectados. Command: cruzamento `sync-detect.subprojects[]` vs `entity-registry._patterns[stack].discovered[].subprojectName`.
- [x] **AC-W3.2** — Para cada subprojeto ≥5 arquivos código, ≥1 cluster emitido.
- [x] **AC-W3.3** — Para cada cluster, existe receita em `.claude/recipes/{sub}/add-{label}.json` sem placeholders literais. Command: validador.
- [x] **AC-W3.4** — `.claude/graph/` sem `entity.*` nem `enum.*`. Command: `rtk node -e "const fs=require('fs');for(const f of fs.readdirSync('.claude/graph')){if(/\\.(entity|enum)\\./.test(f))process.exit(1)}"`
- [x] **AC-W3.5** — Prompt template ≤80 linhas + nenhuma stack hardcoded. Command: `rtk node -e "const t=require('fs').readFileSync('apps/rt/scripts/scan/agent-prompt.template.md','utf8');if(t.split(String.fromCharCode(10)).length>80)process.exit(1);for(const s of ['Rust','React','Django','Spring','Express','Vue','Angular']){if(new RegExp('\\\\b'+s+'\\\\b').test(t))process.exit(1)}"`
- [x] **AC-W3.6** — `mustard-rt run scan-md-validate --help` + `scan-recipes-validate --help` existem.

## Limites

`apps/rt/src/run/scan_structural.rs` (novo), `apps/rt/src/run/scan/{stack_parser,entity_extractor,recipes_generator,refs_installer}.rs` (novos), `apps/rt/src/run/scan/cluster_discovery.rs` (corrigir cobertura), `apps/rt/src/run/scan_md_validate.rs` (novo), `apps/rt/src/run/scan_recipes_validate.rs` (novo), `apps/rt/src/run/scan_orchestrate.rs`, `apps/rt/src/run/scan_finalize.rs`, `apps/rt/src/run/sync_registry.rs`, `apps/rt/src/run/mod.rs`, `apps/rt/scripts/scan/agent-prompt.template.md`, `apps/rt/scripts/scan/examples/*/*.md`, `apps/cli/templates/refs/scan/scan-protocol.md`.

OUT: cold-path interpret (W2 mega ✅). Hardcode de tecnologia em qualquer lugar.

## Role

mixed (rt majoritário + cli templates encurtados)
