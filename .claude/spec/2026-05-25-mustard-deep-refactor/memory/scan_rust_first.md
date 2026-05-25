---
name: scan-rust-first
description: Tudo estrutural do /scan em Rust; IA só interpretação semântica nomeável
metadata:
  type: principle
  origin_spec: 2026-05-25-mustard-deep-refactor
  origin_wave: wave-3-mixed
---

# Scan Rust-First

O `/scan` do Mustard é Rust-first. Toda funcionalidade que pode ser determinística vive em `apps/rt/src/run/scan/`; IA só é invocada para interpretação semântica nomeável que Rust não consegue.

**IMPORTANTE — "Rust-first" é sobre mecanismo, não conhecimento prévio**: o Rust não conhece nenhuma stack a priori. Padrões emergem do filesystem do projeto-alvo via heurísticas agnósticas (ver [[no_hardcoded_stack_patterns]]). Mustard é ferramenta, não framework — funciona em qualquer projeto sem catálogo hardcoded.

## Estrutural (Rust puro, sem token cost)

- `sync-detect` — descobre subprojetos + stacks
- `cluster_discovery` — sufixos/base-class/decorator/function-prefix clusters
- `entity-registry.json` — struct/trait/enum/component/hook detectados via parser AST-leve
- `stack.md` — parseado de manifests (Cargo.toml, package.json, requirements.txt, etc.)
- `recipes/{subproject}/*.json` — derivadas mecanicamente de registry + paths reais
- Refs stack-aware — copiados de `templates/refs/stack-templates/` conforme stack detectada
- Graph nodes — `spec.X`, `skill.X`, `command.X`, `ref.X`, `recipe.X`, `conv.X` (NUNCA entity/enum — ver [[graph_pipeline_knowledge]])
- `scan-md-validate` — gate pós-IA: tamanho, refs, wirelinks, fences

## Interpretativo (IA, focado, ~80-linha prompt)

O dispatch IA recebe **input estruturado** do scan-structural já pronto e produz APENAS:

- `patterns.md` — descrição humana dos clusters identificados
- `notes.md` — observações qualitativas (convenções não-óbvias, anti-patterns, gotchas)
- `skills/{cluster}/SKILL.md` — descrição humana de cada cluster

**NUNCA mais escrever via IA**: `stack.md`, `recipes.md`, `guards.md`. Rust faz.

## Wirelinks canônicos

Todo cross-ref em `entity-registry.json` ou `.md` gerado usa formato `[[id]]` canônico:
- `[[{sub}.{kind}.{slug}]]` para nós de pipeline (preferido)
- `[[X]]` para nó conceito top-level (legado, mantido para Project/Skill/Bash/Check/etc.)
- Strings sem `[[]]` viram erro do validator (T3.10/T3.11)

## Origem

Esta política nasceu de [[2026-05-25-mustard-deep-refactor]] durante [[wave-3-mixed]] (scan agnóstico Rust-first). Decisão tomada em 2026-05-25 após reanálise crítica que mediu cobertura do entity-registry em ~6% (Mustard era cego ao próprio código) e identificou que 3 dos 5 `.md` gerados pelo scan eram deriváveis mecanicamente sem IA.

## Aplica-se a

- Qualquer wave futura que toque `apps/rt/src/run/scan/`
- Subcomandos novos de geração de artefatos via `/scan`
- Validação pós-IA é obrigatória (gate Rust)

## Status

Active — política vigente.

## Relacionado

- [[no_hardcoded_stack_patterns]] — complementa (sem catálogo prévio)
- [[recipes_from_scan]] — aplicação concreta no fluxo de recipes
- [[graph_pipeline_knowledge]] — aplicação concreta no fluxo do graph
